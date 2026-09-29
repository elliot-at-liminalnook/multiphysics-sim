#!/usr/bin/env python3
"""Local-only control panel for the saved Claude pair. Standard library only."""
import argparse
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
from urllib.parse import urlparse

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


def events(path):
    raw = tail(path)
    if not raw.strip():
        return [], None
    try:
        whole = json.loads(raw)
        if isinstance(whole, dict) and (whole.get("type") == "result" or whole.get("subtype") in ("success", "error_during_execution")):
            return [], whole
    except json.JSONDecodeError:
        pass
    activity, result = [], None
    for line in raw.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        if event.get("type") == "result":
            result = event
        elif event.get("type") == "assistant":
            for block in event.get("message", {}).get("content", []):
                if block.get("type") == "text":
                    activity.append({"kind": "message", "text": block.get("text", "")[:6000]})
                elif block.get("type") == "tool_use":
                    args = block.get("input", {})
                    detail = args.get("file_path") or args.get("path") or args.get("command") or args.get("pattern") or ""
                    activity.append({"kind": "tool", "text": block.get("name", "Tool") + (": " + str(detail)[:350] if detail else "")})
        elif event.get("type") == "system" and event.get("subtype") == "init":
            activity.append({"kind": "session", "text": "Session started · " + event.get("model", "Claude")})
    return activity[-35:], result


def view(root):
    config = pair.read_json(root / "config.json")
    state = pair.read_json(root / "state.json")
    records = []
    active = running(root)
    now = time.time()
    for prompt in sorted((root / "logs").glob("*-*.prompt.md")):
        stem = prompt.name.removesuffix(".prompt.md")
        number, _, role = stem.partition("-")
        if not number.isdigit() or role not in ("director", "orchestrator", "worker"):
            continue
        activity, result = events(root / "logs" / (stem + ".stdout"))
        prompt_text = prompt.read_text()
        output = root / "logs" / (stem + ".stdout")
        updated = output.stat().st_mtime if output.exists() else prompt.stat().st_mtime
        live = active and state.get("inflight", {}).get("prefix") == str(root / "logs" / stem)
        duration_ms = (result or {}).get("duration_ms")
        seconds = duration_ms / 1000 if isinstance(duration_ms, (int, float)) else max(0, (now if live else updated) - prompt.stat().st_mtime)
        records.append({"stage": workflow.call_stage(role, prompt_text),
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
            p = Path(receipt[name]).resolve()
            item[name + "_text"] = tail(p, 10000) if p.is_relative_to(root) else ""
        checks.append(item)
    flow = workflow.describe(state, active, records, checks, now)
    verification = next(n for n in flow["nodes"] if n["id"] == "verify")
    verification["live_output"] = []
    if state["phase"] == "verify":
        for index, name in enumerate(pair.check_names(state.get("plan"))):
            prefix = pair.check_prefix(root, state["rounds"], index, name)
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
            "limits": {k: config[k] for k in ("max_rounds", "max_hours", "budget_usd", "call_budget_usd", "turn_minutes", "max_turns")},
            "workspace": config["worktree"], "source": config["repo"], "steering": steering,
            "steering_pending": bool(steering and steering["updated_at"] != state.get("steering_seen")),
            "estimated_spent": max(0, state["cost_usd"] - inflight.get("reserved_usd", 0)),
            "reserved": inflight.get("reserved_usd", 0), "elapsed_seconds": elapsed,
            "checks": checks, "stop_requested": (root / "STOP").exists(), "now": time.time()}


class Dashboard(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, root):
        self.root = root
        self.token = secrets.token_urlsafe(32)
        self.control_lock = threading.Lock()
        self.child = None
        super().__init__(address, Handler)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def send(self, code, data, content_type="application/json"):
        body = json.dumps(data).encode() if content_type == "application/json" else data.encode()
        self.send_response(code)
        self.send_header("Content-Type", content_type + "; charset=utf-8")
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
            if path == "/api/journal":
                return self.send(200, shared_notebook.render(shared_notebook.entries(self.server.root)), "text/plain")
            if path in ("/workflow.js", "/workflow.css", "/notebook.js", "/notebook.css"):
                kind = "text/javascript" if path.endswith(".js") else "text/css"
                return self.send(200, (Path(__file__).parent / path[1:]).read_text(), kind)
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
                    if running(root) or (self.server.child and self.server.child.poll() is None):
                        return self.send(409, {"error": "The pair is already running"})
                    state = pair.read_json(root / "state.json")
                    config = pair.read_json(root / "config.json")
                    if state["status"] == "complete" and not pair.Runner(root).outer_settings()["enabled"]:
                        return self.send(409, {"error": "This mission is marked complete"})
                    if state["rounds"] >= config["max_rounds"] or state["elapsed_seconds"] >= config["max_hours"] * 3600 or state["cost_usd"] >= config["budget_usd"]:
                        return self.send(409, {"error": "A run limit is reached. Increase the limit below before continuing."})
                    if state.get("inflight") and not data.get("retry_interrupted"):
                        return self.send(409, {"error": "The last turn was interrupted. Review its work and confirm resume.", "interrupted": True})
                    argv = [sys.executable, str(Path(__file__).parent / "pair.py"), "resume", "--state", str(root)]
                    if state.get("inflight"):
                        argv.append("--retry-interrupted")
                    with (root / "coordinator.log").open("ab") as log:
                        self.server.child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
                    return self.send(200, {"ok": True, "message": "The pair is continuing from its saved handoff."})
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
                        if type(val) not in (int, float) or not math.isfinite(val) or val <= 0:
                            return self.send(400, {"error": "Every limit must be a positive number"})
                        if key in ("max_rounds", "max_turns") and int(val) != val:
                            return self.send(400, {"error": "Turn counts must be whole numbers"})
                    config.update(data)
                    config["max_rounds"] = int(config["max_rounds"])
                    config["max_turns"] = int(config["max_turns"])
                    pair.write_json(root / "config.json", config)
                    return self.send(200, {"ok": True, "message": "Run limits saved."})
                return self.send(404, {"error": "Not found"})
        except (ValueError, TypeError) as e:
            self.send(400, {"error": str(e)})
        except Exception as e:
            self.send(500, {"error": str(e)})


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--state", required=True)
    p.add_argument("--port", type=int, default=8766)
    args = p.parse_args()
    root = Path(args.state).expanduser().resolve()
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
