#!/usr/bin/env python3
"""Local-only control panel for the saved Claude pair. Standard library only."""
import argparse
import datetime
import fcntl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import math
from pathlib import Path
import secrets
import subprocess
import sys
import threading
import time
from urllib.parse import unquote, urlparse

import pair
import workflow
import shared_notebook


def running(root):
    with (root / "runner.lock").open("a+") as f:
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(f, fcntl.LOCK_UN)
            return False
        except BlockingIOError:
            return True


def tail(path, limit=180000):
    if not path.exists():
        return ""
    with path.open("rb") as f:
        size = path.stat().st_size
        if size > limit:
            f.seek(size - limit)
        return f.read().decode(errors="replace")


def event_time(event):
    """ISO timestamp on stream events -> epoch seconds (None when absent)."""
    stamp = event.get("timestamp")
    if not isinstance(stamp, str):
        return None
    try:
        return datetime.datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def clip_lines(text, lines=60, chars=6000):
    text = str(text or "")
    kept = text.splitlines()[:lines]
    out = "\n".join(kept)[:chars]
    return out + ("\n…" if len(out) < len(text) else "")


def tool_details(name, args):
    """What a reader needs to see of a tool call, trimmed for the page."""
    if name in ("Edit", "MultiEdit"):
        edits = args.get("edits") or [{"old_string": args.get("old_string"), "new_string": args.get("new_string")}]
        return {"file": args.get("file_path"), "edits": [{"old": clip_lines(e.get("old_string"), 40), "new": clip_lines(e.get("new_string"), 40)}
                                                         for e in edits[:4]]}
    if name == "Write":
        return {"file": args.get("file_path"), "content": clip_lines(args.get("content"), 40),
                "lines": len(str(args.get("content") or "").splitlines())}
    if name == "Bash":
        return {"command": str(args.get("command") or "")[:4000], "description": args.get("description")}
    if name in ("Agent", "Task"):
        return {"subagent_type": args.get("subagent_type") or "general-purpose", "description": args.get("description"),
                "prompt": clip_lines(args.get("prompt"), 80, 8000)}
    keep = {k: (v if isinstance(v, (int, float, bool)) else str(v)[:600]) for k, v in args.items() if k != "content"}
    return keep


def result_text(block):
    content = block.get("content")
    if isinstance(content, list):
        content = "\n".join(c.get("text", "") for c in content if isinstance(c, dict))
    return str(content or "")


_events_cache = {}


def events(path, deep=False):
    """Parsed activity for one call's stream, cached until the file changes.
    Only the live call needs the deep read that keeps its subagents' spawn
    calls in view; finished calls parse a smaller tail."""
    try:
        stat = path.stat()
        key = (stat.st_mtime_ns, stat.st_size)
    except OSError:
        return [], None
    key = key + (deep,)
    hit = _events_cache.get(str(path))
    if hit and hit[0] == key:
        return hit[1]
    parsed = parse_events(path, 4_000_000 if deep else 400_000)
    _events_cache[str(path)] = (key, parsed)
    return parsed


def parse_events(path, limit):
    raw = tail(path, limit)
    if not raw.strip():
        return [], None
    try:
        whole = json.loads(raw)
        if isinstance(whole, dict) and (whole.get("type") == "result" or whole.get("subtype") in ("success", "error_during_execution")):
            return [], whole
    except json.JSONDecodeError:
        pass
    activity, result, tools = [], None, {}
    for line in raw.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        at = event_time(event)
        parent = event.get("parent_tool_use_id")  # set on a subagent's own events
        if event.get("type") == "result":
            result = event
        elif event.get("type") == "assistant":
            for block in event.get("message", {}).get("content", []):
                if block.get("type") == "text":
                    activity.append({"kind": "message", "text": block.get("text", "")[:6000], "at": at, "parent": parent})
                elif block.get("type") == "tool_use":
                    args = block.get("input", {}) or {}
                    name = block.get("name", "Tool")
                    detail = args.get("file_path") or args.get("path") or args.get("command") or args.get("pattern") or args.get("description") or ""
                    item = {"kind": "tool", "text": name + (": " + str(detail)[:350] if detail else ""), "at": at,
                            "id": block.get("id"), "name": name, "details": tool_details(name, args), "status": "running",
                            "parent": parent}
                    tools[block.get("id")] = item
                    activity.append(item)
        elif event.get("type") == "user":
            for block in event.get("message", {}).get("content", []):
                if isinstance(block, dict) and block.get("type") == "tool_result" and block.get("tool_use_id") in tools:
                    item = tools[block["tool_use_id"]]
                    error = block.get("is_error") in (True, "True", "true")
                    output = result_text(block)
                    item.update(status="error" if error else "done", ended_at=at,
                                output=output[-3000:] if item["name"] == "Bash" else clip_lines(output, 30, 3000))
        elif event.get("type") == "system" and event.get("subtype") == "init":
            activity.append({"kind": "session", "text": "Session started · " + event.get("model", "Claude"), "at": at})
    # Keep recent actions per agent, so one busy subagent can't push the others
    # (or the lead) off the page: the lead's last 80 and each subagent's last 25.
    lanes = {}
    for index, item in enumerate(activity):
        lanes.setdefault(item.get("parent"), []).append(index)
    for item in activity:
        if item.get("name") in ("Agent", "Task"):
            item["total_actions"] = len(lanes.get(item.get("id"), []))
    keep = set()
    for parent, indexes in lanes.items():
        keep.update(indexes[-(80 if parent is None else 25):])
    for item in activity:  # an agent's spawn call always stays, so its lane has a name
        if item.get("name") in ("Agent", "Task"):
            keep.add(activity.index(item))
    return [activity[i] for i in sorted(keep)], result


def subagents(activity):
    """The Agent calls in a turn, with how far each has got."""
    spawned = [a for a in activity if a.get("kind") == "tool" and a.get("name") in ("Agent", "Task")]
    return [{"id": a["id"], "type": a["details"].get("subagent_type"), "description": a["details"].get("description"),
             "status": a["status"], "at": a.get("at"), "ended_at": a.get("ended_at"), "nested": bool(a.get("parent")),
             "prompt": a["details"].get("prompt"), "parent": a.get("parent"), "output": a.get("output"),
             "actions": a.get("total_actions", sum(1 for x in activity if x.get("parent") == a["id"]))} for a in spawned]


_git_cache = {}


def git_summary(config):
    """Commits and uncommitted change counts since the run's baseline (cached briefly)."""
    repo, base = Path(config["worktree"]), config["baseline"]
    hit = _git_cache.get(str(repo))
    if hit and time.monotonic() - hit[0] < 8:
        return hit[1]
    def run(*args):
        try:
            return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, timeout=10).stdout
        except (OSError, subprocess.TimeoutExpired):
            return ""
    commits = []
    for line in run("log", "--format=%h%x1f%s%x1f%ct", f"{base}..HEAD", "-n", "40").splitlines():
        parts = line.split("\x1f")
        if len(parts) == 3:
            commits.append({"hash": parts[0], "subject": parts[1], "at": int(parts[2])})
    stat = run("diff", "--shortstat", base).strip()
    numbers = {k: 0 for k in ("files", "insertions", "deletions")}
    for chunk in stat.split(","):
        words = chunk.split()
        if len(words) >= 2 and words[0].isdigit():
            key = "files" if "file" in words[1] else "insertions" if "insert" in words[1] else "deletions"
            numbers[key] = int(words[0])
    status = run("status", "--porcelain")
    summary = {"commits": commits, **numbers,
               "untracked": sum(1 for l in status.splitlines() if l.startswith("??")),
               "uncommitted": sum(1 for l in status.splitlines() if l and not l.startswith("??"))}
    _git_cache[str(repo)] = (time.monotonic(), summary)
    return summary


def latest_rate_limits(root, state):
    """Claude usage windows from the newest call's stream, else the last saved reading."""
    for output in sorted((root / "logs").glob("*-*.stdout"), reverse=True)[:3]:
        info = pair.rate_limit_info(tail(output, 400000))
        if info:
            return info
    return state.get("rate_limits")


def captures(root, limit=24):
    base = root / "captures"
    if not base.is_dir():
        return []
    files = sorted(base.glob("**/*.png"), key=lambda p: p.stat().st_mtime, reverse=True)[:limit]
    return [{"path": str(p.relative_to(base)), "at": p.stat().st_mtime, "bytes": p.stat().st_size} for p in files]


def view(root):
    config = pair.read_json(root / "config.json")
    state = pair.read_json(root / "state.json")
    records = []
    session_totals = {}
    active = running(root)
    now = time.time()
    for prompt in sorted((root / "logs").glob("*-*.prompt.md")):
        stem = prompt.name.removesuffix(".prompt.md")
        number, _, role = stem.partition("-")
        if not number.isdigit() or role not in ("director", "orchestrator", "worker"):
            continue
        live = active and state.get("inflight", {}).get("prefix") == str(root / "logs" / stem)
        activity, result = events(root / "logs" / (stem + ".stdout"), deep=live)
        prompt_text = prompt.read_text()
        output = root / "logs" / (stem + ".stdout")
        updated = output.stat().st_mtime if output.exists() else prompt.stat().st_mtime
        duration_ms = (result or {}).get("duration_ms")
        seconds = duration_ms / 1000 if isinstance(duration_ms, (int, float)) else max(0, (now if live else updated) - prompt.stat().st_mtime)
        total = (result or {}).get("total_cost_usd")
        session = (result or {}).get("session_id")
        cost = None
        if isinstance(total, (int, float)):
            cost = max(0.0, total - session_totals.get(session, 0.0))
            session_totals[session] = total
        records.append({"stage": workflow.call_stage(role, prompt_text), "cost_usd": cost, "subagents": subagents(activity),
                        "fast": (result or {}).get("fast_mode_state") == "on",
                        "session_id": session, "num_turns": (result or {}).get("num_turns"),
                        "elapsed_seconds": seconds, "timing_complete": bool(result), "live": live,
                        "last_activity_at": updated,
"id": stem, "number": int(number), "role": role,
                        "prompt": prompt_text, "activity": activity,
                        "result": (result or {}).get("structured_output"),
                        "error": (result or {}).get("result", "") if (result or {}).get("is_error") else "",
                        "stderr": tail(root / "logs" / (stem + ".stderr"), 12000),
                        "model": next(iter((result or {}).get("modelUsage", {})), None),
                        "started_at": prompt.stat().st_mtime,
                        "finished": bool(result), "subtype": (result or {}).get("subtype")})
    inflight = state.get("inflight", {})
    steering = pair.read_json(root / "steering.json") if (root / "steering.json").exists() else None
    elapsed = state["elapsed_seconds"]
    if active and state.get("run_started_at"):
        elapsed += max(0, time.time() - state["run_started_at"])
    checks = []
    for receipt in state.get("receipts", []):
        item = dict(receipt)
        for name in ("stdout", "stderr"):
            if not receipt.get(name):
                item[name + "_text"] = ""
                continue
            p = Path(receipt[name]).resolve()
            item[name + "_text"] = tail(p, 10000) if p.is_relative_to(root) else ""
        checks.append(item)
    flow = workflow.describe(state, active, records, checks, now)
    verification = next(n for n in flow["nodes"] if n["id"] == "verify")
    verification["live_output"] = []
    if state["phase"] in ("verify", "precheck"):
        suffix = "-before" if state["phase"] == "precheck" else ""
        for index, name in enumerate(pair.check_names(state.get("plan"))):
            prefix = pair.check_prefix(root, state["rounds"], index, name)
            prefix = prefix.with_name(prefix.name + suffix)
            out, err = prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
            if out.exists() or err.exists():
                verification["live_output"].append({"name": name, "command": pair.check_argv(config["checks"], name),
                    "text": tail(out, 6000) + tail(err, 6000),
                    "updated_at": max(p.stat().st_mtime for p in (out, err) if p.exists())})
    journal = shared_notebook.entries(root)
    return {"notebook": {"entries": journal[-80:], "total": len(journal), "system": shared_notebook.SYSTEM, "path": str(root / "shared")}, "workflow": flow, "state": state, "active": active, "calls": records,
            "mission": pair.Runner(root).prompt_file("mission.md"),
            "roles": {role: pair.Runner(root).prompt_file(f"{role}.md") for role in ("director", "orchestrator", "worker")},
            "outer_settings": pair.Runner(root).outer_settings(),
            "limits": {k: config.get(k) for k in ("max_rounds", "max_hours", "budget_usd", "call_budget_usd", "turn_minutes", "max_turns")},
            "workspace": config["worktree"], "source": config["repo"], "steering": steering,
            "steering_pending": bool(steering and steering["updated_at"] != state.get("steering_seen")),
            "estimated_spent": max(0, state["cost_usd"] - inflight.get("reserved_usd", 0)),
            "reserved": inflight.get("reserved_usd", 0), "elapsed_seconds": elapsed,
            "checks": checks, "stop_requested": (root / "STOP").exists(), "now": time.time(),
            "fast_roles": config.get("fast_roles", []), "branch": config.get("branch"),
            "unverified_commits": pair.Runner(root).unverified_commits(), "verify_every": config.get("verify_every_commits", 0), "baseline": config.get("baseline"), "state_dir": str(root),
            "git": git_summary(config), "captures": captures(root), "rate_limits": latest_rate_limits(root, state)}


class Dashboard(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, root):
        self.root = root
        self.token = secrets.token_urlsafe(32)
        self.control_lock = threading.Lock()
        self.child = None
        super().__init__(address, Handler)
        threading.Thread(target=self.watch_limits, daemon=True).start()

    def busy(self):
        return running(self.root) or bool(self.child and self.child.poll() is None)

    def launch(self, retry=False):
        argv = [sys.executable, str(Path(__file__).parent / "pair.py"), "resume", "--state", str(self.root)]
        if retry:
            argv.append("--retry-interrupted")
        with (self.root / "coordinator.log").open("ab") as log:
            self.child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)

    def watch_limits(self):
        """If the coordinator exited while waiting out a usage limit (terminal
        closed, crash), relaunch it once the limit has reset."""
        while True:
            time.sleep(15)
            try:
                state = pair.read_json(self.root / "state.json")
                due = state.get("status") == "waiting" and time.time() >= state.get("resume_at", float("inf"))
                if due and not (self.root / "STOP").exists():
                    with self.control_lock:
                        if not self.busy():
                            self.launch(retry=bool(state.get("inflight")))
            except Exception:
                pass


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def send(self, code, data, content_type="application/json"):
        body = (data if isinstance(data, bytes) else
                json.dumps(data).encode() if content_type == "application/json" else data.encode())
        self.send_response(code)
        self.send_header("Content-Type", content_type if content_type.startswith("image/") else content_type + "; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'")
        self.end_headers()
        self.wfile.write(body)

    def trusted_host(self):
        port = self.server.server_port
        return self.headers.get("Host") in (f"127.0.0.1:{port}", f"localhost:{port}")

    def do_GET(self):
        if not self.trusted_host():
            return self.send(403, {"error": "Local host required"})
        path = urlparse(self.path).path
        try:
            if path == "/":
                html = (Path(__file__).parent / "dashboard.html").read_text().replace("__PAIR_TOKEN__", self.server.token)
                return self.send(200, html, "text/html")
            if path == "/api/ping":
                return self.send(200, {"workspace": pair.read_json(self.server.root / "config.json")["worktree"]})
            if path == "/api/journal":
                return self.send(200, shared_notebook.render(shared_notebook.entries(self.server.root)), "text/plain")
            if path in ("/dashboard.js", "/dashboard.css"):
                kind = "text/javascript" if path.endswith(".js") else "text/css"
                return self.send(200, (Path(__file__).parent / path[1:]).read_text(), kind)
            if path.startswith("/captures/"):
                base = (self.server.root / "captures").resolve()
                target = (base / unquote(path[len("/captures/"):])).resolve()
                if target.suffix != ".png" or not target.is_relative_to(base) or not target.is_file():
                    return self.send(404, {"error": "Not found"})
                return self.send(200, target.read_bytes(), "image/png")
            if path == "/api/state":
                return self.send(200, view(self.server.root))
            return self.send(404, {"error": "Not found"})
        except Exception as e:
            self.send(500, {"error": str(e)})

    def do_POST(self):
        if not self.trusted_host() or self.headers.get("X-Pair-Token") != self.server.token:
            return self.send(403, {"error": "Reload this local dashboard before using its controls"})
        if self.headers.get("Origin") not in (None, f"http://{self.headers.get('Host')}"):
            return self.send(403, {"error": "Same-origin request required"})
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if size < 0 or size > 20000:
                return self.send(413, {"error": "Request too large"})
            data = json.loads(self.rfile.read(size) or b"{}")
            root = self.server.root
            with self.server.control_lock:
                path = urlparse(self.path).path
                if path == "/api/stop":
                    (root / "STOP").touch()
                    return self.send(200, {"ok": True, "message": "Stop requested; partial work will be preserved."})
                if path == "/api/start":
                    if self.server.busy():
                        return self.send(409, {"error": "The pair is already running"})
                    state = pair.read_json(root / "state.json")
                    config = pair.read_json(root / "config.json")
                    if state["status"] == "complete" and not pair.Runner(root).outer_settings()["enabled"]:
                        return self.send(409, {"error": "This mission is marked complete"})
                    reached = lambda value, cap: cap is not None and value >= cap
                    if (reached(state["rounds"], config.get("max_rounds")) or reached(state["cost_usd"], config.get("budget_usd"))
                            or reached(state["elapsed_seconds"], (config.get("max_hours") or math.inf) * 3600)):
                        return self.send(409, {"error": "A run limit is reached. Increase the limit below before continuing."})
                    interrupted = state.get("inflight")
                    self.server.launch(retry=bool(interrupted))
                    return self.send(200, {"ok": True, "message": (
                        f"Continuing: the interrupted {interrupted['role']} turn resumes in its own session." if interrupted
                        else "The pair is continuing from its saved handoff.")})
                if path == "/api/retry-now":
                    state = pair.read_json(root / "state.json")
                    (root / "RETRY_NOW").touch()
                    if self.server.busy():
                        return self.send(200, {"ok": True, "message": "Retrying now."})
                    if (root / "STOP").exists():
                        return self.send(409, {"error": "The run is stopped. Use Continue."})
                    self.server.launch(retry=bool(state.get("inflight")))
                    return self.send(200, {"ok": True, "message": "Restarting the run and retrying now."})
                if path == "/api/outer":
                    pair.configure_outer(root, data.get("enabled"), data.get("max_batches"))
                    return self.send(200, {"ok": True, "message": "Director settings saved. Active work will finish before any upgrade."})
                if path == "/api/steering":
                    text = data.get("text")
                    if not isinstance(text, str) or len(text) > 10000:
                        return self.send(400, {"error": "Guidance must be at most 10,000 characters"})
                    pair.write_json(root / "steering.json", {"text": text.strip(), "updated_at": time.time()})
                    return self.send(200, {"ok": True, "message": "Guidance saved for the next orchestrator turn."})
                if path == "/api/limits":
                    if running(root) or (self.server.child and self.server.child.poll() is None):
                        return self.send(409, {"error": "Stop the pair before changing its limits"})
                    config = pair.read_json(root / "config.json")
                    keys = ("max_rounds", "max_hours", "budget_usd", "call_budget_usd", "turn_minutes", "max_turns")
                    if set(data) != set(keys):
                        return self.send(400, {"error": "Supply all six limits"})
                    for key in keys:
                        val = data[key]
                        if val is None:
                            continue  # no limit
                        if type(val) not in (int, float) or not math.isfinite(val) or val <= 0:
                            return self.send(400, {"error": "Each limit must be a positive number, or empty for no limit"})
                        if key in ("max_rounds", "max_turns") and int(val) != val:
                            return self.send(400, {"error": "Turn counts must be whole numbers"})
                    config.update(data)
                    for key in ("max_rounds", "max_turns"):
                        if config[key] is not None:
                            config[key] = int(config[key])
                    pair.write_json(root / "config.json", config)
                    return self.send(200, {"ok": True, "message": "Run limits saved."})
                return self.send(404, {"error": "Not found"})
        except (ValueError, TypeError) as e:
            self.send(400, {"error": str(e)})
        except Exception as e:
            self.send(500, {"error": str(e)})


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--state", help="Run state directory (default: <repo>/.claude-pair)")
    p.add_argument("--port", type=int, default=8766)
    args = p.parse_args()
    root = Path(args.state).expanduser().resolve() if args.state else pair.default_state()
    pair.read_json(root / "config.json")
    server = Dashboard(("127.0.0.1", args.port), root)
    url = f"http://127.0.0.1:{server.server_port}"
    pair.write_json(root / "dashboard.json", {"url": url, "started_at": time.time()})
    print(url, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
