#!/usr/bin/env python3
"""Two serial, persistent Claude Code sessions with durable reviewed handoffs.

Python standard library only; macOS/Linux. No model calls until `run`.
"""
import argparse
import contextlib
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid

import outer_loop
import shared_notebook
import disk_preflight

HERE = Path(__file__).resolve().parent
TEXT = {"type": "string"}
STRINGS = {"type": "array", "items": TEXT}


def obj(properties):
    return {"type": "object", "properties": properties,
            "required": list(properties), "additionalProperties": False}


DECISIONS = {"type": "array", "items": obj({"decision": TEXT, "why": TEXT, "alternatives": TEXT, "revisit_if": TEXT})}
PLAN_SCHEMA = obj({
    "decisions": DECISIONS,
    "action": {"enum": ["work", "complete", "blocked"]},
    "review": {"enum": ["none", "accept", "revise"]},
    "coordination_notes": STRINGS, "summary": TEXT, "worker_prompt": TEXT, "acceptance_criteria": STRINGS,
    "checks": STRINGS, "waived_checks": STRINGS,
    "checklist": {"type": "array", "items": obj({
        "id": TEXT, "workflow": TEXT,
        "status": {"enum": ["pending", "in_progress", "verified", "blocked"]},
        "evidence": TEXT})}})
REPORT_SCHEMA = obj({"decisions": DECISIONS, "coordination_notes": STRINGS, "status": {"enum": ["done", "blocked"]}, "summary": TEXT,
                     "changed_files": STRINGS, "checks": STRINGS,
                     "evidence": STRINGS, "blockers": STRINGS})


def write_json(path, data):
    path = Path(path)
    tmp = path.with_name(path.name + ".tmp")
    with tmp.open("w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
        f.flush()
        os.fsync(f.fileno())
    tmp.replace(path)


def read_json(path):
    return json.loads(Path(path).read_text())


def clip(text, limit):
    return text if len(text) <= limit else text[:limit] + f"\n… {len(text) - limit} more characters omitted"


def git(repo, *args, env=None):
    return subprocess.run(["git", "-C", str(repo), *args], check=True,
                          capture_output=True, env=env).stdout


def baseline_ref(stamp):
    return "refs/claude-pair/" + re.sub(r"[^A-Za-z0-9_-]", "-", stamp) + "/baseline"


def check_argv(catalogue, name):
    """A catalogue name runs its saved argv; any other text is a shell command."""
    return catalogue[name] if name in catalogue else ["/bin/bash", "-c", name]


def check_names(plan):
    """Checks the orchestrator explicitly asked the coordinator to run. There is
    no fixed suite: the worker verifies its own work with tests it chooses."""
    return list(dict.fromkeys((plan or {}).get("checks", [])))


def check_prefix(root, rounds, index, name):
    slug = name if re.fullmatch(r"[A-Za-z0-9_.-]{1,40}", name) else f"cmd{index:02d}"
    return Path(root) / "logs" / f"check-{rounds:04d}-{slug}"


check_passed = outer_loop.check_passed


class CallFailed(RuntimeError):
    """An agent call failed in a way worth retrying (API error, crash, malformed
    result). The run backs off and continues the same session."""


class UsageLimit(Exception):
    """A Claude subscription or spend limit stopped a call; resume after reset_at."""
    def __init__(self, reset_at, detail, weekly=False):
        super().__init__(detail)
        self.reset_at, self.detail, self.weekly = reset_at, detail, weekly


def weekly_limit(info, text, reset_at, now):
    """The weekly allowance (not the 5-hour window) is what ran out."""
    info = info or {}
    windows = info.get("unifiedWindows") or {}
    week = windows.get("seven_day") if isinstance(windows.get("seven_day"), dict) else {}
    return ("seven_day" in str(info.get("rateLimitType", "")) or (week.get("utilization") or 0) >= 1
            or bool(re.search(r"weekly", text or "", re.I)) or reset_at - now > 24 * 3600)


LIMIT_TEXT = re.compile(r"usage limit|limit reached|hit your limit|weekly limit|spend limit|rate.?limit|limit resets|resets in", re.I)
RESUME_NOTE = ("(The coordinator resumed this session after a Claude usage-limit pause. Your earlier "
               "progress in this conversation and in the project folder is intact: continue where you "
               "left off rather than starting over, then return the structured result.)\n\n")


def stream_events(raw):
    for line in raw.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict):
            yield event


def rate_limit_info(raw):
    """The last rate_limit_event Claude Code reported in a stream-json transcript."""
    info = None
    for event in stream_events(raw):
        if event.get("type") == "rate_limit_event" and isinstance(event.get("rate_limit_info"), dict):
            info = event["rate_limit_info"]
    return info


def limit_reset(info, text, now, fallback_seconds=900):
    """When a failed call was stopped by a usage limit, the time it resets; else None."""
    rejected = bool(info) and info.get("status") not in (None, "allowed", "allowed_warning")
    if not rejected and not LIMIT_TEXT.search(text or ""):
        return None
    times = []
    if info:
        if rejected and isinstance(info.get("resetsAt"), (int, float)):
            times.append(info["resetsAt"])
        for window in (info.get("unifiedWindows") or {}).values():
            if isinstance(window, dict) and (window.get("utilization") or 0) >= 1 and isinstance(window.get("resetsAt"), (int, float)):
                times.append(window["resetsAt"])
    for match in re.finditer(r"limit reached\|(\d{9,})", text or ""):
        times.append(int(match.group(1)))
    match = re.search(r"resets? in (\d+)\s*(s|sec|second|m|min|minute|h|hr|hour)", text or "", re.I)
    if match:
        unit = match.group(2).lower()[0]
        times.append(now + int(match.group(1)) * {"s": 1, "m": 60, "h": 3600}[unit])
    match = re.search(r"resets? (?:at )?(\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2})\s*UTC", text or "")
    if match:
        import datetime
        stamp = datetime.datetime.fromisoformat(match.group(1).replace(" ", "T")).replace(tzinfo=datetime.timezone.utc)
        times.append(stamp.timestamp())
    future = [t for t in times if t > now]
    return max(future) if future else now + fallback_seconds


def cargo_path(path):
    cargo = Path.home() / ".cargo" / "bin"
    parts = [p for p in (path or "").split(os.pathsep) if p]
    return os.pathsep.join(([str(cargo)] if cargo.is_dir() and str(cargo) not in parts else []) + parts)


def folder_tree(repo):
    """Tree object for everything in the folder now (tracked edits and
    non-ignored untracked files), built with a temporary index so HEAD, the
    real index and every file stay untouched."""
    with tempfile.TemporaryDirectory() as tmp:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(tmp) / "index"))
        git(repo, "read-tree", "HEAD", env=env)
        git(repo, "add", "-A", "--", ".", env=env)
        return git(repo, "write-tree", env=env).decode().strip()


def working_baseline(repo, message):
    """A commit for the folder exactly as it is when the run starts, including
    the user's uncommitted work. Reviews diff against it, so work that existed
    before the run is never mistaken for agent work."""
    head = git(repo, "rev-parse", "HEAD").decode().strip()
    if not git(repo, "status", "--porcelain").strip():
        return head, head
    env = dict(os.environ, GIT_AUTHOR_NAME="Claude Pair Baseline", GIT_AUTHOR_EMAIL="local-baseline@localhost",
               GIT_COMMITTER_NAME="Claude Pair Baseline", GIT_COMMITTER_EMAIL="local-baseline@localhost")
    return head, git(repo, "commit-tree", folder_tree(repo), "-p", head, "-m", message, env=env).decode().strip()


def exclude_state(repo, root):
    """Keep run state out of git status, diffs and commits."""
    try:
        relative = root.relative_to(repo)
    except ValueError:
        return
    exclude = Path(git(repo, "rev-parse", "--git-path", "info/exclude").decode().strip())
    exclude = exclude if exclude.is_absolute() else repo / exclude
    pattern = "/" + relative.parts[0] + "*/"
    lines = exclude.read_text().splitlines() if exclude.exists() else []
    if pattern not in lines:
        exclude.parent.mkdir(parents=True, exist_ok=True)
        exclude.write_text("\n".join(lines + ["# claude-pair run state", pattern]) + "\n")


@contextlib.contextmanager
def lock(root):
    with (root / "runner.lock").open("a+") as f:
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError("A coordinator is already running for this state directory")
        try:
            yield
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def validate(value, schema):
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"Invalid enum: {value!r}")
    typ = schema.get("type")
    if typ == "object":
        if not isinstance(value, dict) or set(value) != set(schema["properties"]):
            raise ValueError("Structured response has missing/extra fields")
        for key, spec in schema["properties"].items():
            validate(value[key], spec)
    elif typ == "array":
        if not isinstance(value, list):
            raise ValueError("Expected array")
        for item in value:
            validate(item, schema["items"])
    elif typ == "string" and not isinstance(value, str):
        raise ValueError("Expected string")


def guard_plan(plan, state, checks):
    validate(plan, PLAN_SCHEMA)
    previous = state.get("plan")
    if state.get("report"):
        if plan["review"] == "none":
            raise ValueError("Worker result requires an explicit review")
        if plan["review"] == "accept":
            # A "done" report may still list blockers for later work; the
            # orchestrator judges those. Only a blocked assignment can't be accepted.
            if state["report"]["status"] != "done":
                raise ValueError("Cannot accept a worker report whose status is blocked; use review=revise or action=blocked")
            receipts = state.get("receipts", [])
            expected = set(check_names(previous))
            if {r["name"] for r in receipts} != expected:
                raise ValueError("Cannot accept without passing independent checks")
            failing = [r["name"] for r in receipts if not check_passed(r, plan["waived_checks"])]
            if failing:
                raise ValueError("Cannot accept without passing independent checks: " + ", ".join(failing) +
                                 " (a failure can be waived only if it was already failing before the assignment)")
    elif plan["review"] != "none":
        raise ValueError("Cannot review a worker that has not run")
    items = plan["checklist"]
    ids = [i["id"] for i in items]
    if not items or any(not x.strip() for x in ids) or len(ids) != len(set(ids)):
        raise ValueError("Checklist must have nonempty unique IDs")
    if previous and not {i["id"] for i in previous["checklist"]}.issubset(ids):
        raise ValueError("Cannot silently drop checklist items")
    if any(i["status"] == "verified" and not i["evidence"].strip() for i in items):
        raise ValueError("Verified items require evidence")
    if any(not c.strip() for c in plan["checks"]):
        raise ValueError("Checks must be catalogue names or nonempty shell commands")
    if plan["action"] == "work" and (not plan["worker_prompt"].strip() or not plan["acceptance_criteria"]):
        raise ValueError("Assignment requires a prompt and acceptance criteria")
    if plan["action"] == "complete":
        if not state.get("report") or plan["review"] != "accept" or any(i["status"] != "verified" for i in items):
            raise ValueError("Completion requires accepted work and all checklist items verified")


class Runner:
    def __init__(self, root):
        self.root = root
        self.config = read_json(root / "config.json")
        # Local worker commits must remain inside independent whitespace review.
        if self.config["checks"].get("diff") == ["git", "diff", "--check"]:
            self.config["checks"]["diff"] = ["git", "diff", "--check", self.config["baseline"]]
        self.state = read_json(root / "state.json")
        # Usage totals are cumulative per session, and sessions are now fresh
        # per assignment/batch, so the ledger is keyed by session ID.
        costs = self.state.setdefault("session_costs", {})
        for role in ("director", "orchestrator", "worker"):
            if role in costs:
                sid = self.state["sessions"].get(role)
                value = costs.pop(role)
                if sid:
                    costs[sid] = max(costs.get(sid, 0.0), value)
        self.repo = Path(self.config["worktree"])
        self.deadline = 0

    def env(self, **extra):
        """Environment for agents and checks: cargo on PATH and the run's paths."""
        env = dict(os.environ, PATH=cargo_path(os.environ.get("PATH")), CARGO_TERM_COLOR="never",
                   PAIR_STATE=str(self.root), PAIR_WORKSPACE=str(self.repo), PAIR_SOURCE=str(self.config["repo"]),
                   PAIR_TOOLS=str(HERE), PAIR_CAPTURES=str(self.root / "captures"),
                   PAIR_BASELINE=self.config["baseline"])
        env.update(extra)
        return env

    def outer_settings(self):
        path = self.root / "outer-settings.json"
        return read_json(path) if path.exists() else {"enabled": False, "max_batches": None}

    def save(self):
        write_json(self.root / "state.json", self.state)
        s = self.state
        lines = ["# Claude pair status", "", f"Status: **{s['status']}**",
                 f"Phase: {s['phase']} · completed worker turns: {s['rounds']}",
                 f"Usage ledger: ${s['cost_usd']:.4f} including any in-flight reservation (not a subscription bill)",
                 f"Workspace: `{self.repo}`", "", s.get("message", ""), ""]
        if s.get("plan"):
            lines += [s["plan"]["summary"], "", "## Workflow checklist", ""]
            lines += [f"- [{ 'x' if i['status']=='verified' else ' ' }] {i['workflow']} — {i['status']}. {i['evidence']}" for i in s["plan"]["checklist"]]
        (self.root / "STATUS.md").write_text("\n".join(lines) + "\n")
        if (self.root / "shared/SYSTEM.md").exists():
            shared_notebook.current(self.root, self.state, self.config)

    def process(self, argv, prefix, stdin=None, cwd=None, env=None):
        """Persist output before interpretation; stop the process group on limits."""
        out, err = prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
        minutes = self.config.get("turn_minutes")  # None: a call may run as long as it needs
        timeout = min(minutes * 60 if minutes else math.inf, self.deadline - time.monotonic())
        if timeout <= 0 or (self.root / "STOP").exists():
            raise InterruptedError("Stop requested or run time exhausted")
        (self.root / "captures").mkdir(exist_ok=True)
        with out.open("wb") as of, err.open("wb") as ef:
            p = subprocess.Popen(argv, cwd=cwd or self.repo, stdin=subprocess.PIPE if stdin else subprocess.DEVNULL,
                                 stdout=of, stderr=ef, start_new_session=True, env=env or self.env())
            self.state["child_pid"] = p.pid
            self.save()
            try:
                if stdin:
                    p.stdin.write(stdin.encode())
                    p.stdin.close()
                end = time.monotonic() + timeout
                while p.poll() is None:
                    if (self.root / "STOP").exists() or time.monotonic() >= end:
                        raise InterruptedError("Stop requested or per-turn/run time limit reached")
                    if shutil.disk_usage(self.repo).free < 2 * 1024**3:
                        raise InterruptedError("Less than 2 GiB disk space remains; progress saved")
                    time.sleep(0.2)
            finally:
                if p.poll() is None:
                    os.killpg(p.pid, signal.SIGTERM)
                    try:
                        p.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(p.pid, signal.SIGKILL)
                        p.wait()
                self.state.pop("child_pid", None)
                self.save()
        return p.returncode, out, err

    def prompt_file(self, name):
        """Run-local prompt copy when present (edited while stopped), else the installed one."""
        local = self.root / "prompts" / name
        return (local if local.exists() else HERE / "prompts" / name).read_text()

    def instructions(self, role):
        text = "\n\n".join([self.prompt_file("mission.md"), self.prompt_file(f"{role}.md"),
                             self.prompt_file("handbook.md"), shared_notebook.SYSTEM])
        text += ("\n# This run\n\n"
                 f"- Workspace: {self.repo}. This is the user's own project folder and your working "
                 f"directory. Edit here and commit to the current branch ({self.config.get('branch') or 'detached HEAD'}).\n"
                 f"- Run state, logs and receipts: {self.root} (git-ignored; do not edit)\n"
                 f"- Screenshots and UI captures: {self.root / 'captures'} (view PNGs with the Read tool)\n"
                 f"- Native UI capture: python3 {HERE / 'ui_capture.py'} --help\n"
                 f"- Baseline (the folder as it was when the run started, including the user's "
                 f"uncommitted edits): {self.config['baseline']}, pinned as {self.config.get('baseline_ref', 'no ref')}\n"
                 "- Your shell and every check get PAIR_STATE, PAIR_WORKSPACE, PAIR_SOURCE, PAIR_TOOLS, "
                 "PAIR_CAPTURES and PAIR_BASELINE, with ~/.cargo/bin on PATH.\n")
        if self.state.get("outer"):
            text += "\nA Director now selects bounded batches above this pair. The batch contract overrides older whole-mission completion instructions."
            if role != "director":
                text += outer_loop.contract_prompt(self)
        if self.config["audit_only"]:
            text += "\nAUDIT ONLY: do not modify files. The current task is a bounded inventory, not implementation. Report findings via structured output."
        return text

    def call(self, role, prompt, scope=None):
        """One agent turn. A role keeps its session only while `scope` (the
        assignment or batch) is unchanged and the session is short; otherwise it
        starts fresh and relies on the task contract and notebook for context."""
        shared_notebook.setup(self.root, self.state, self.config)
        prompt += shared_notebook.context(self.root, role)
        prompt += disk_preflight.context(self.repo)
        schema = {"director": outer_loop.DIRECTOR_SCHEMA, "orchestrator": PLAN_SCHEMA, "worker": REPORT_SCHEMA}[role]
        scopes = self.state.setdefault("session_scopes", {})
        counts = self.state.setdefault("session_calls", {})
        session = self.state["sessions"].get(role)
        # Resume tokens belong to one session; another role's call must not use them up.
        retrying = bool(session) and self.state.get("retry_session") == session
        if retrying:
            self.state.pop("retry_session")
        if retrying and self.state.get("limit_resume") == session:
            self.state.pop("limit_resume")
            prompt = RESUME_NOTE + prompt
        failure = self.state.pop("retry_note", None) if retrying or self.state.get("retry_note_role") == role else None
        self.state.pop("retry_note_role", None) if failure else None
        if failure:
            prompt = (f"(Your previous attempt ended with an error: {failure}. Anything you already did in this "
                      "conversation and in the project folder is intact. Continue, then return the structured result.)\n\n" + prompt)
        if session and not retrying and (role == "director" or scopes.get(role) != scope or
                                         counts.get(session, 0) >= self.config.get("max_session_calls", 8)):
            session = None
        sid = session or str(uuid.uuid4())
        self.state["calls"] += 1
        prefix = self.root / "logs" / f"{self.state['calls']:04d}-{role}"
        prefix.with_suffix(".prompt.md").write_text(prompt)
        # Optional dollar caps; None means the only ceiling is Claude's own usage limits.
        budget, per_call = self.config.get("budget_usd"), self.config.get("call_budget_usd")
        caps = [c for c in (per_call, None if budget is None else budget - self.state["cost_usd"]) if c is not None]
        cap = min(caps) if caps else None
        if cap is not None and cap < 0.01:
            raise InterruptedError("Estimated usage budget exhausted")
        reservation = cap or 0.0
        # Reserve the entire cap before launch. Crashes cannot reset the ledger.
        self.state["cost_usd"] += reservation
        self.state["inflight"] = {"role": role, "session_id": sid, "prefix": str(prefix), "reserved_usd": reservation,
                                  "started_at": time.time()}
        scopes[role] = scope
        counts[sid] = counts.get(sid, 0) + 1
        self.save()
        argv = [self.config["claude"], "--print", "--output-format", "stream-json", "--verbose",
                "--system-prompt-snapshot", "off",
                "--json-schema", json.dumps(schema), "--append-system-prompt", self.instructions(role),
                # Project settings, AGENTS.md/CLAUDE.md and project skills load; the
                # user's personal settings, hooks and MCP servers do not.
                "--setting-sources", "project",
                "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--no-chrome",
                "--add-dir", str(self.root),
                "--resume" if session else "--session-id", sid]
        if cap is not None:
            argv += ["--max-budget-usd", str(cap)]
        if self.config.get("max_turns"):
            argv += ["--max-turns", str(self.config["max_turns"])]
        if self.config["audit_only"]:
            argv += ["--permission-mode", "dontAsk", "--permission-prompts", "none",
                     "--tools", "Read,Glob,Grep", "--allowedTools", "Read,Glob,Grep"]
        else:
            argv += ["--dangerously-skip-permissions"]
        if not session:
            argv += ["--name", f"Pair {role} ({self.root.name})"]
        if self.config.get("model"):
            argv += ["--model", self.config["model"]]
        print(f"{role}: call {self.state['calls']} ({prefix.name}{', resumed' if session else ', fresh session'})", flush=True)
        code, out, err = self.process(argv, prefix, prompt)
        raw = out.read_text()
        events = list(stream_events(raw))
        try:
            result = json.loads(raw)
        except json.JSONDecodeError:
            results = [event for event in events if event.get("type") == "result"]
            result = results[-1] if results else None
        info = rate_limit_info(raw)
        if info:
            self.state["rate_limits"] = {**info, "observed_at": time.time()}
        if result is None or code or result.get("is_error") or result.get("subtype") != "success":
            text = " ".join([str((result or {}).get("result", "")), (err.read_text()[-4000:] if err.exists() else ""), raw[-12000:]])
            reset = limit_reset(info, text, time.time(), self.config.get("limit_retry_minutes", 15) * 60)
            if reset:
                self.settle_limited_call(role, sid, reservation, result,
                                         any(e.get("type") == "assistant" for e in events))
                raise UsageLimit(reset, f"{role} call {self.state['calls']} stopped by a Claude usage limit",
                                 weekly_limit(info, text, reset, time.time()))
        started = any(e.get("type") == "assistant" for e in events)
        if result is None:
            self.fail_call(role, sid, reservation, started, "Claude's output ended without a final result (crash or cut-off)")
        if result.get("session_id") != sid:
            self.fail_call(role, sid, reservation, False, "Claude returned an unexpected session ID")
        self.state["sessions"][role] = sid
        total = result.get("total_cost_usd")
        prior = self.state["session_costs"].get(sid, 0.0)
        spent = total - prior if isinstance(total, (float, int)) and math.isfinite(total) and total >= prior else 0.0
        self.state["cost_usd"] += spent - reservation
        if spent:
            self.state["session_costs"][sid] = total
        self.state.pop("inflight")
        self.save()
        if code or result.get("is_error") or result.get("subtype") != "success":
            self.fail_call(role, sid, 0.0, started, f"Claude reported {result.get('subtype') or 'an error'}: "
                           + str(result.get("result") or "")[:300])
        if result.get("permission_denials"):
            # Not a boundary any more (agents run without permission prompts);
            # recorded so a refused interactive tool is visible in the journal.
            shared_notebook.append(self.root, {"id": f"denials-{self.state['calls']:04d}", "author": "coordinator",
                "kind": "Permission denials", "summary": f"{role} had {len(result['permission_denials'])} tool call(s) refused.",
                "notes": [json.dumps(d)[:600] for d in result["permission_denials"][:8]], "source": str(out)})
        data = result.get("structured_output")
        try:
            validate(data, schema)
        except ValueError as error:
            self.fail_call(role, sid, 0.0, True, f"The structured response did not match the schema ({error})")
        self.record_decisions(role, data)
        shared_notebook.response(self.root, role, self.state["calls"], data, out)
        return data

    def fail_call(self, role, sid, reserved, started, reason):
        """Settle a failed call and arrange a retry that continues its session."""
        self.state["cost_usd"] -= reserved
        self.state.pop("inflight", None)
        if started:
            self.state["sessions"][role] = sid
            self.state["retry_session"] = sid
        self.state["retry_note"] = reason
        self.state["retry_note_role"] = role
        self.save()
        raise CallFailed(f"{role} call {self.state['calls']}: {reason}")

    def record_decisions(self, role, data):
        """Keep every decision an agent made on the user's behalf in one log."""
        made = data.get("decisions") or []
        if not made:
            return
        log = self.state.setdefault("decisions", [])
        context = {"role": role, "call": self.state["calls"], "at": time.time(),
                   "batch": (self.state.get("outer", {}).get("current_batch") or {}).get("id"),
                   "assignment": self.state.get("assignment")}
        log.extend({**context, **d} for d in made)
        lines = ["# Decisions the agents made", "", "Newest last. Each was made instead of stopping to ask; revisit any of them.", ""]
        for d in log:
            lines += [f"## {d['decision']}", "",
                      f"{time.strftime('%Y-%m-%d %H:%M', time.localtime(d['at']))} · {d['role']} · call {d['call']}"
                      + (f" · batch {d['batch']}" if d.get("batch") else ""), "",
                      f"- **Why:** {d['why']}", f"- **Alternatives:** {d['alternatives']}", f"- **Revisit if:** {d['revisit_if']}", ""]
        (self.root / "shared").mkdir(exist_ok=True)
        shared_notebook.atomic_text(self.root / "shared" / "DECISIONS.md", "\n".join(lines))

    def call_checked(self, role, prompt, scope, guard):
        """Call a planner and apply the coordinator's handoff rules. A rejected
        response goes back to the same session with the reason, instead of
        halting an unattended run on a fixable mistake."""
        note = ""
        for attempt in range(self.config.get("guard_retries", 2) + 1):
            data = self.call(role, note + prompt, scope=scope)
            try:
                guard(data)
                return data
            except ValueError as error:
                if attempt == self.config.get("guard_retries", 2):
                    raise
                shared_notebook.append(self.root, {"id": f"rejected-{self.state['calls']:04d}", "author": "coordinator",
                    "kind": "Response rejected", "summary": f"The {role}'s response broke a handoff rule and was not applied: {error}",
                    "notes": ["The same session was asked to correct it."], "source": str(self.root / "logs")})
                self.state["retry_session"] = self.state["sessions"].get(role)
                note = (f"THE COORDINATOR REJECTED YOUR LAST RESPONSE: {error}\n"
                        "Nothing from it was applied. Return a corrected structured response for the same situation.\n\n")

    def settle_limited_call(self, role, sid, reservation, result, started):
        """Reconcile a call a usage limit cut short, and arrange for the same
        session to continue after the reset if it had already begun work."""
        total = (result or {}).get("total_cost_usd")
        prior = self.state["session_costs"].get(sid, 0.0)
        spent = total - prior if isinstance(total, (int, float)) and math.isfinite(total) and total >= prior else 0.0
        self.state["cost_usd"] += spent - reservation
        if spent:
            self.state["session_costs"][sid] = total
        self.state.pop("inflight", None)
        if started:
            self.state["sessions"][role] = sid
            self.state["retry_session"] = sid
            self.state["limit_resume"] = sid
        self.save()

    def wait_until(self, reset_at, reason, kind="limit"):
        """Sleep through a usage-limit window without counting it as active time.
        STOP still works; the saved resume time survives a coordinator restart."""
        resume_at = reset_at + self.config.get("limit_margin_seconds", 60)
        clock = time.strftime("%a %H:%M", time.localtime(resume_at))
        self.state.update(status="waiting", resume_at=resume_at, wait_kind=kind,
                          message=f"{reason}. Resuming automatically at {clock}.")
        self.save()
        shared_notebook.append(self.root, {"id": f"limit-{self.state['calls']:04d}-{int(resume_at)}",
            "author": "coordinator", "kind": "Usage limit" if kind == "limit" else "Retry after a failed call",
            "summary": f"{reason}. The run waits and resumes at {clock}, continuing the interrupted session.",
            "notes": [], "source": str(self.root / "state.json")})
        print(f"waiting: {reason}; resuming at {clock}", flush=True)
        started = time.monotonic()
        try:
            while time.time() < resume_at:
                if (self.root / "STOP").exists():
                    self.state.pop("resume_at", None)
                    raise InterruptedError("Stopped while waiting for the Claude usage limit to reset")
                time.sleep(max(0.01, min(5, resume_at - time.time())))
        finally:
            waited = time.monotonic() - started
            self.deadline += waited
            self.waited += waited
        self.state.pop("resume_at", None)
        self.state.pop("wait_kind", None)
        self.state.update(status="running", message="Continuing where the run left off.")
        self.save()

    def evidence(self):
        # The orchestrator can open all logs with Read; full diffs stay on disk.
        prefix = self.root / "logs" / f"review-{self.state['rounds']:04d}"
        diff = prefix.with_suffix(".diff")
        # Compare whole-folder trees so untracked files (the user's and the
        # agents') are neither hidden nor shown as deletions.
        base, now = self.config["baseline"], folder_tree(self.repo)
        with diff.open("wb") as f:
            subprocess.run(["git", "diff", "--no-ext-diff", base, now], cwd=self.repo, stdout=f, check=True)
        status = git(self.repo, "status", "--short").decode(errors="replace")
        prefix.with_suffix(".status.txt").write_text(status)
        stat = git(self.repo, "diff", "--stat=160", base, now).decode(errors="replace")
        commits = git(self.repo, "log", "--stat=160", "--format=%n%h %s", base + "..HEAD").decode(errors="replace")
        captures = sorted((self.root / "captures").glob("**/*.png"), key=lambda p: p.stat().st_mtime, reverse=True)
        return {"local_commits": git(self.repo, "log", "--format=%h %s", base + "..HEAD").decode(errors="replace"),
                "diffstat": clip(stat, 6000), "commits_with_stats": clip(commits, 6000),
                "untracked": [line[3:] for line in status.splitlines() if line.startswith("??")][:200],
                "diff_file": str(diff), "status_file": str(prefix.with_suffix('.status.txt')),
                "recent_captures": [str(p) for p in captures[:20]],
                "note": "diff_file compares the run's baseline with everything in the folder now, including new untracked files. Use the diffstat to choose what to read. View captures with Read.",
                "worker_report": self.state.get("report"), "independent_checks": self.state.get("receipts", [])}

    def ordered_checks(self):
        """Plan order, but cheapest first by measured duration so a quick failure
        short-circuits the slow builds; unmeasured checks run in the middle."""
        timings = self.state.get("check_history", {})
        names = check_names(self.state["plan"])
        seconds = lambda n: (timings.get(n) or {}).get("seconds")
        known = sorted(seconds(n) for n in names if seconds(n) is not None)
        middle = known[len(known) // 2] if known else 0
        cost = lambda n: -1 if n == "diff" else seconds(n) if seconds(n) is not None else middle
        return sorted(enumerate(names), key=lambda item: (cost(item[1]), item[0]))

    def run_check(self, index, name, suffix=""):
        prefix = check_prefix(self.root, self.state["rounds"], index, name)
        argv = check_argv(self.config["checks"], name)
        started = time.monotonic()
        code, out, err = self.process(argv, prefix.with_name(prefix.name + suffix))
        return {"name": name, "command": argv, "exit_code": code, "stdout": str(out), "stderr": str(err),
                "seconds": round(time.monotonic() - started, 1)}

    def precheck(self):
        """Optional (config precheck): run a new assignment's checks before the
        worker edits anything. Doubles check time; history usually suffices."""
        self.state["prechecks"] = {name: self.run_check(i, name, "-before")
                                   for i, name in self.ordered_checks() if name != "diff"}

    def earlier_result(self, name):
        """This check's result before the current assignment: a pre-run if one was
        made, else its latest result from an earlier assignment."""
        if name in self.state.get("prechecks", {}):
            return self.state["prechecks"][name]
        seen = self.state.get("check_history", {}).get(name)
        if seen and seen.get("assignment", 0) < self.state.get("assignment", 0):
            return seen
        return None

    def verify(self):
        history = self.state.setdefault("check_history", {})
        receipts, stop = [], None
        for index, name in self.ordered_checks():
            if stop:
                receipts.append({"name": name, "command": check_argv(self.config["checks"], name), "exit_code": None,
                                 "skipped": True, "stdout": "", "stderr": "", "seconds": 0,
                                 "note": f"Skipped: {stop} failed first and the work needs changes anyway."})
                continue
            receipt = self.run_check(index, name)
            before = self.earlier_result(name)
            if receipt["exit_code"] != 0 and before:
                receipt["before"] = {k: before.get(k) for k in ("exit_code", "stdout", "stderr", "assignment")}
            if receipt["exit_code"] != 0 and not (before and before.get("exit_code")):
                stop = name  # a new failure: the remaining checks can wait for the repair
            history[name] = {"exit_code": receipt["exit_code"], "seconds": receipt["seconds"],
                             "stdout": receipt["stdout"], "stderr": receipt["stderr"],
                             "assignment": self.state.get("assignment", 0), "round": self.state["rounds"]}
            receipts.append(receipt)
            self.save()
        order = {n: i for i, n in enumerate(check_names(self.state["plan"]))}
        self.state["receipts"] = receipts = sorted(receipts, key=lambda r: order[r["name"]])

        def verdict(r):
            if r.get("skipped"):
                return " skipped"
            if r["exit_code"] == 0:
                return f" passed ({r['seconds']:.0f}s)"
            b = r.get("before")
            return f" failed ({r['seconds']:.0f}s)" + ("" if not b else " (already failing before this assignment)" if b["exit_code"]
                                else " (passed before this assignment: new failure)")
        shared_notebook.append(self.root, {"id": f"checks-{self.state['rounds']:04d}-{self.state['calls']:04d}",
            "author": "coordinator", "kind": "Independent verification",
            "summary": "Independent checks finished: " + "; ".join(r["name"][:120] + verdict(r) for r in receipts),
            "notes": [r["stdout"] + " | " + r["stderr"] for r in receipts], "source": str(self.root / "logs")})

    def run(self, steps=0, retry=False):
        outer_loop.prepare(self)
        shared_notebook.setup(self.root, self.state, self.config)
        if self.state["status"] == "complete":
            print("Mission already complete")
            return
        if self.state.get("inflight"):
            # A turn cut short by Stop, a crash or a closed terminal continues in
            # its own session; its edits in the folder are intact.
            interrupted = self.state.pop("inflight")
            pending = git(self.repo, "status", "--short").decode(errors="replace").splitlines()
            shared_notebook.append(self.root, {"id": f"resume-{self.state['calls']:04d}-{int(time.time())}",
                "author": "coordinator", "kind": "Resumed interrupted turn",
                "summary": f"The {interrupted['role']} turn in {Path(interrupted['prefix']).name} was interrupted; its session "
                           "continues when that role runs next. Uncommitted changes listed here are most likely that turn's "
                           "partial work, not the user's: check them against its log before treating them as the user's.",
                "notes": pending[:40], "source": interrupted["prefix"]})
            self.state["sessions"][interrupted["role"]] = interrupted["session_id"]
            self.state["retry_session"] = interrupted["session_id"]
        if (self.root / "STOP").exists():
            raise RuntimeError("STOP is present. Use the resume command to clear it deliberately.")
        if self.state.get("run_started_at"):
            self.state["elapsed_seconds"] += max(0, time.time() - self.state.pop("run_started_at"))
        hours = self.config.get("max_hours")
        remaining = hours * 3600 - self.state["elapsed_seconds"] if hours else math.inf
        self.deadline = time.monotonic() + remaining
        started = time.monotonic()
        self.waited = 0.0
        self.state["status"] = "running"
        self.state["message"] = "Coordinator running; consult logs for the active assignment."
        self.state["run_started_at"] = time.time()
        self.state["pid"] = os.getpid()
        self.save()
        count = 0
        try:
            if self.state.get("resume_at") and time.time() < self.state["resume_at"]:
                self.wait_until(self.state["resume_at"] - self.config.get("limit_margin_seconds", 60),
                                "Waiting for the Claude usage limit to reset")
            while True:
                try:
                    if time.monotonic() >= self.deadline or (self.root / "STOP").exists():
                        raise InterruptedError("Stopped at the saved phase")
                    phase = self.state["phase"]
                    steering_path = self.root / "steering.json"
                    if phase == "worker" and steering_path.exists():
                        guidance = read_json(steering_path)
                        if guidance["updated_at"] != self.state.get("steering_seen"):
                            self.state["operator_replan"] = {"queued_assignment": self.state.get("plan"),
                                                            "previous_report": self.state.get("report")}
                            self.state["report"] = None
                            self.state["receipts"] = []
                            self.state["phase"] = phase = "orchestrator"
                            self.save()
                    if phase == "director":
                        settings = self.outer_settings()
                        if not settings["enabled"]:
                            raise InterruptedError("Batch finished; automatic next-batch planning is off")
                        completed = sum(not b.get("legacy") for b in self.state["outer"]["history"])
                        if settings.get("max_batches") and completed >= settings["max_batches"]:
                            raise InterruptedError("Completed-batch ceiling reached; increase it to continue")
                        if self.config.get("max_rounds") and self.state["rounds"] >= self.config["max_rounds"]:
                            raise InterruptedError("Worker-turn ceiling reached before selecting another batch")
                        prompt = outer_loop.director_prompt(self)
                        if steering_path.exists():
                            prompt += "\nCURRENT OPERATOR GUIDANCE:\n" + read_json(steering_path)["text"]
                        decision = self.call_checked("director", prompt, "director",
                            lambda d: outer_loop.guard_decision(d, self.state, self.config["checks"]))
                        write_json(self.root / "logs" / f"director-{self.state['calls']:04d}.json", decision)
                        if not outer_loop.dispatch(self, decision):
                            break
                    elif phase == "orchestrator":
                        prompt = ("Read the mission and inspect the repository. Maintain your checklist. "
                                  "The worker chooses and runs the tests that verify its work. `checks` is optional and "
                                  "normally []; list a command (or a name from this catalogue) only when you want the "
                                  "coordinator to rerun something specific and cheap:\n" + json.dumps(self.config["checks"]))
                        timings = {n: h["seconds"] for n, h in self.state.get("check_history", {}).items() if h.get("seconds") is not None}
                        if timings:
                            prompt += ("\nMeasured duration of each check's latest run, in seconds. The coordinator reruns "
                                       "your checks after every worker turn, so this is a recurring cost:\n" + json.dumps(timings))
                        if self.state.get("plan"):
                            prompt += ("\nYour previous plan (you may be in a fresh session: carry its checklist IDs forward "
                                       "and treat it as your own earlier decision):\n" + json.dumps(self.state["plan"]))
                        if self.state.get("report"):
                            prompt += "\nReview the worker using this evidence:\n" + json.dumps(self.evidence())
                        elif self.state.get("operator_replan"):
                            prompt += "\nNew operator guidance arrived before the queued worker assignment started. Reconsider that assignment now. There is no new worker result to review; use review=none. Prior context:\n" + json.dumps(self.state["operator_replan"])
                        elif not self.state.get("outer", {}).get("current_batch"):
                            prompt += ("\nFirst assign a bounded feature inventory and identify the best existing Rust native shell. "
                                       "For this first assignment, request one concise Markdown inventory/decision document, "
                                       "with source pointers and the first proposed implementation slice; do not implement UI changes yet. "
                                       "Inspect representative source paths, not thousands of experiment artifacts. "
                                       "The first assignment needs only the diff check; avoid builds for this documentation-only inventory.")
                        if self.config["audit_only"]:
                            prompt += "\nThis run is audit-only: assign read-only investigation, with diff as the only check."
                        steering = self.root / "steering.json"
                        if steering.exists():
                            guidance = read_json(steering)
                            prompt += "\nCURRENT OPERATOR GUIDANCE (apply within the mission; preserve safety/source constraints):\n" + guidance["text"]
                            self.state["steering_seen"] = guidance["updated_at"]
                            self.save()
                        if self.state.get("outer", {}).get("current_batch"):
                            prompt += outer_loop.contract_prompt(self)
                        batch = (self.state.get("outer", {}).get("current_batch") or {}).get("id", "mission")
                        def guard(plan):
                            guard_plan(plan, self.state, self.config["checks"])
                            outer_loop.guard_contract(self, plan)
                            if self.config["audit_only"] and set(plan["checks"]) - {"diff"}:
                                raise ValueError("Audit-only run cannot request builds")
                        plan = self.call_checked("orchestrator", prompt, "batch:" + batch, guard)
                        shared_notebook.append(self.root, {"id": f"review-{self.state['calls']:04d}",
                            "author": "coordinator", "kind": "Validated handoff",
                            "summary": f"Orchestrator response passed the handoff guards: action={plan['action']}, review={plan['review']}.",
                            "notes": ["Evidence remains scoped to the reviewed assignment; this does not establish whole-project completion."],
                            "source": str(self.root / "logs" / f"plan-{self.state['calls']:04d}.json")})
                        new_assignment = plan["action"] == "work" and plan["review"] != "revise"
                        if new_assignment:
                            # A new assignment gets a fresh worker; repairs resume the same one.
                            self.state["assignment"] = self.state.get("assignment", 0) + 1
                            self.state["prechecks"] = {}
                        self.state["plan"] = plan
                        self.state.pop("operator_replan", None)
                        write_json(self.root / "logs" / f"plan-{self.state['calls']:04d}.json", plan)
                        if plan["action"] == "complete" and self.state.get("outer", {}).get("current_batch"):
                            outer_loop.finish_batch(self, plan)
                        elif plan["action"] == "blocked" and self.state.get("outer", {}).get("current_batch"):
                            outer_loop.set_aside(self, plan)
                        elif plan["action"] != "work":
                            self.state["status"] = plan["action"]
                            self.state["message"] = plan["summary"]
                            break
                        elif new_assignment and self.config.get("precheck") and check_names(plan):
                            self.state["phase"] = "precheck"
                        else:
                            self.state["phase"] = "worker"
                    elif phase == "worker":
                        if self.config.get("max_rounds") and self.state["rounds"] >= self.config["max_rounds"]:
                            raise InterruptedError("Worker-turn ceiling reached; reviewed progress is saved")
                        plan = self.state["plan"]
                        prompt = plan["worker_prompt"] + "\n\nAcceptance criteria:\n" + json.dumps(plan["acceptance_criteria"])
                        self.state["report"] = self.call("worker", prompt, scope=f"assignment:{self.state.get('assignment', 0)}")
                        self.state["rounds"] += 1
                        # Coordinator checks only when the orchestrator asked for some.
                        self.state["phase"] = "verify" if check_names(self.state["plan"]) else "orchestrator"
                        self.state["receipts"] = []
                        write_json(self.root / "logs" / f"worker-{self.state['rounds']:04d}.json", self.state["report"])
                    elif phase == "precheck":
                        self.precheck()
                        self.state["phase"] = "worker"
                    elif phase == "verify":
                        self.verify()
                        self.state["phase"] = "orchestrator"
                    else:
                        raise ValueError(f"Unknown phase: {phase}")
                    self.save()
                    count += 1
                    if steps and count >= steps:
                        raise InterruptedError("Requested number of steps finished; ready to resume")
                    self.state.pop("consecutive_failures", None)
                except CallFailed as failure:
                    count = self.state.get("consecutive_failures", 0) + 1
                    self.state["consecutive_failures"] = count
                    if count > self.config.get("failure_retries", 8):
                        raise RuntimeError(f"{failure} (failed {count} times in a row)")
                    delay = min(30 * 60, self.config.get("failure_backoff_seconds", 30) * 2 ** (count - 1))
                    self.wait_until(time.time() + delay - self.config.get("limit_margin_seconds", 60),
                                    f"An agent call failed ({failure}); retry {count}", kind="retry")
                    continue
                except UsageLimit as limit:
                    if limit.weekly and not self.config.get("wait_for_weekly_limit"):
                        # The run's only ceiling: stop here and continue after the reset.
                        self.state["weekly_reset_at"] = limit.reset_at
                        raise InterruptedError("The weekly Claude usage limit is used up. It resets "
                                               + time.strftime("%a %d %b %H:%M", time.localtime(limit.reset_at))
                                               + "; press Continue after that and the interrupted session picks up where it stopped.")
                    self.wait_until(limit.reset_at, "Claude 5-hour usage limit reached")
        except InterruptedError as e:
            self.state.update(status="paused", message=str(e))
        except (Exception, KeyboardInterrupt) as e:
            self.state.update(status="blocked", message=f"{type(e).__name__}: {e}")
        finally:
            self.state["elapsed_seconds"] += max(0.0, time.monotonic() - started - self.waited)
            self.state.pop("run_started_at", None)
            self.state.pop("pid", None)
            self.save()
            shared_notebook.current(self.root, self.state, self.config)
        print(f"{self.state['status']}: {self.state.get('message', '')}", flush=True)
        print(f"Status: {self.root / 'STATUS.md'}", flush=True)
        return 1 if self.state["status"] == "blocked" else 0


def default_repo():
    return Path(git(Path.cwd(), "rev-parse", "--show-toplevel").decode().strip())


def default_state(repo=None):
    return (repo or default_repo()) / ".claude-pair"


def initialize(args):
    """Set up a run that works directly in the project folder; no model calls."""
    repo = Path(args.repo).expanduser().resolve() if args.repo else default_repo()
    root = Path(args.state).expanduser().resolve() if args.state else default_state(repo)
    if git(repo, "rev-parse", "--show-toplevel").decode().strip() != str(repo):
        raise ValueError("--repo must be the repository root")
    for name in ("max_rounds", "max_hours", "turn_minutes", "max_turns", "budget_usd", "call_budget_usd"):
        value = getattr(args, name, None)
        if value is not None and (not math.isfinite(value) or value <= 0):
            raise ValueError(f"{name} must be positive and finite (omit it for no limit)")
    claude = shutil.which(args.claude)
    if not claude:
        raise ValueError("Claude Code executable not found")
    stamp = time.strftime("%Y%m%d-%H%M%S")
    if root.exists():
        if not args.fresh:
            raise ValueError(f"A run already exists at {root}. Continue it with `run`, or start over with `init --fresh` "
                             "(the old run is kept beside it).")
        with lock(root):
            pass  # refuses while a coordinator is running
        archived = root.with_name(f"{root.name}-{stamp}")
        root.rename(archived)
        print(f"Previous run kept at {archived}", flush=True)
    root.mkdir(parents=True)
    exclude_state(repo, root)
    (root / "logs").mkdir()
    head, baseline = working_baseline(repo, "Working-folder baseline for a claude-pair run")
    ref = baseline_ref(stamp)
    git(repo, "update-ref", ref, baseline)
    branch = git(repo, "rev-parse", "--abbrev-ref", "HEAD").decode().strip()
    config = {"repo": str(repo), "worktree": str(repo), "in_place": True, "source_head": head,
              "baseline": baseline, "baseline_ref": ref, "branch": None if branch == "HEAD" else branch,
              "claude": claude, "model": args.model, "audit_only": args.audit_only,
              "checks": read_json(HERE / "checks.json"), "max_session_calls": 8, "precheck": False}
    for name in ("max_rounds", "max_hours", "turn_minutes", "max_turns", "budget_usd", "call_budget_usd"):
        config[name] = getattr(args, name, None)
    config["checks"]["diff"] = ["git", "diff", "--check", config["baseline"]]
    write_json(root / "config.json", config)
    write_json(root / "state.json", {"version": 1, "status": "ready", "phase": "orchestrator",
        "calls": 0, "rounds": 0, "elapsed_seconds": 0, "cost_usd": 0,
        "sessions": {}, "session_costs": {}, "message": "Ready; no model calls yet."})
    Runner(root).save()
    if not args.audit_only and not getattr(args, "no_director", False):
        # Keep choosing worthwhile batches after the first; the Director may still stop with a reason.
        write_json(root / "outer-settings.json", {"enabled": True, "max_batches": None})
    dirty = "" if head == baseline else " Uncommitted edits were recorded in the baseline, so reviews show only agent work."
    print(f"Ready: agents will work in {repo} on {config['branch'] or 'detached HEAD'}.{dirty}\n"
          f"State: {root}\nRun: python3 {HERE / 'pair.py'} run")


def configure_outer(root, enabled, max_batches):
    if type(enabled) is not bool or (max_batches is not None and (type(max_batches) is not int or max_batches < 1)):
        raise ValueError("Use a boolean enabled flag and a positive whole batch limit (or none)")
    write_json(root / "outer-settings.json", {"enabled": enabled, "max_batches": max_batches})
    # This settings file is independent of the running coordinator's state.
    try:
        with lock(root):
            outer_loop.prepare(Runner(root))
        return
    except RuntimeError as exc:
        if "already running" not in str(exc):
            raise
    with (root / "outer-upgrade.log").open("ab") as log:
        subprocess.Popen([sys.executable, str(HERE / "pair.py"), "watch-outer", "--state", str(root)],
                         stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)


def watch_outer(root):
    # Never interrupt a worker. The old runner keeps its existing code until it
    # exits; takeover only happens under its released exclusive lock.
    with (root / "outer-upgrade.lock").open("a+") as watcher:
        try:
            fcntl.flock(watcher, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return 0
        while True:
            try:
                with lock(root):
                    runner = Runner(root)
                    if "outer" in runner.state or not runner.outer_settings()["enabled"]:
                        return 0
                    continue_completed = runner.state["status"] == "complete" and not (root / "STOP").exists()
                    outer_loop.prepare(runner)
                    if continue_completed:
                        return runner.run()
                    return 0
            except RuntimeError as exc:
                if "already running" not in str(exc):
                    raise
            time.sleep(2)


def main():
    def terminated(_signum, _frame):
        raise KeyboardInterrupt("Coordinator terminated")
    signal.signal(signal.SIGTERM, terminated)
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    init = sub.add_parser("init", help="Set up a run in this project folder; no model calls")
    init.add_argument("--repo", help="Project folder (default: the git repository containing the current directory)")
    init.add_argument("--state", help="Run state directory (default: <repo>/.claude-pair, git-ignored)")
    init.add_argument("--fresh", action="store_true", help="Start a new run, keeping the previous one beside it")
    init.add_argument("--claude", default="claude")
    init.add_argument("--model", help="Omit to use Claude Code's default model")
    init.add_argument("--audit-only", action="store_true")
    # Every limit is off unless given: the run works until Claude's weekly limit is used up.
    init.add_argument("--max-rounds", type=int, help="Stop after this many worker turns (default: no limit)")
    init.add_argument("--max-hours", type=float, help="Stop after this many active hours (default: no limit)")
    init.add_argument("--turn-minutes", type=float, help="Time limit for one agent call (default: none)")
    init.add_argument("--max-turns", type=int, help="Model turns allowed in one call (default: no limit)")
    init.add_argument("--budget-usd", type=float, help="Estimated-usage ceiling for the run (default: none)")
    init.add_argument("--call-budget-usd", type=float, help="Estimated-usage ceiling per call (default: none)")
    init.add_argument("--no-director", action="store_true", help="Stop when the mission is done instead of choosing more batches")
    for name in ("run", "resume", "status", "stop", "enable-outer", "watch-outer"):
        p = sub.add_parser(name)
        p.add_argument("--state", help="Run state directory (default: <repo>/.claude-pair)")
        if name == "enable-outer":
            p.add_argument("--max-batches", type=int, help="Stop after this many accepted batches (default: no limit)")
        if name in ("run", "resume"):
            p.add_argument("--steps", type=int, default=0, help="Pause after N state transitions (0 = until limit)")
            p.add_argument("--retry-interrupted", action="store_true", help="After inspecting logs, resume an uncertain interrupted session")
    args = parser.parse_args()
    try:
        if args.command == "init":
            initialize(args)
            return 0
        root = Path(args.state).expanduser().resolve() if args.state else default_state()
        if args.command == "enable-outer":
            configure_outer(root, True, args.max_batches)
            print("Director enabled; active work is preserved and upgrades at the next safe restart.")
            return 0
        if args.command == "watch-outer":
            return watch_outer(root)
        if args.command == "status":
            print((root / "STATUS.md").read_text())
            return 0
        if args.command == "stop":
            (root / "STOP").touch()
            print("Stop requested. The coordinator will terminate its active process group and preserve progress.")
            return 0
        if args.steps < 0:
            raise ValueError("--steps must be nonnegative")
        with lock(root):
            if args.command == "resume":
                (root / "STOP").unlink(missing_ok=True)
            return Runner(root).run(args.steps, args.retry_interrupted)
    except (Exception, KeyboardInterrupt) as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
