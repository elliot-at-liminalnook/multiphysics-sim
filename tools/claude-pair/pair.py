#!/usr/bin/env python3
"""Two serial, persistent Claude Code sessions with durable reviewed handoffs.

Python standard library only; macOS/Linux. No model calls until `run`.
"""
import argparse
import contextlib
import ctypes
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
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


PLAN_SCHEMA = obj({
    "action": {"enum": ["work", "complete", "blocked"]},
    "review": {"enum": ["none", "accept", "revise"]},
    "coordination_notes": STRINGS, "summary": TEXT, "worker_prompt": TEXT, "acceptance_criteria": STRINGS,
    "checks": STRINGS,
    "checklist": {"type": "array", "items": obj({
        "id": TEXT, "workflow": TEXT,
        "status": {"enum": ["pending", "in_progress", "verified", "blocked"]},
        "evidence": TEXT})}})
REPORT_SCHEMA = obj({"coordination_notes": STRINGS, "status": {"enum": ["done", "blocked"]}, "summary": TEXT,
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


def git(repo, *args, env=None):
    return subprocess.run(["git", "-C", str(repo), *args], check=True,
                          capture_output=True, env=env).stdout


def source_files(repo):
    raw = git(repo, "ls-files", "--cached", "--others", "--exclude-standard", "-z")
    return sorted(set(os.fsdecode(p) for p in raw.split(b"\0") if p))


def signatures(repo, files):
    result = {}
    for name in files:
        p = repo / name
        try:
            s = p.lstat()
            result[name] = [s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_mode]
        except FileNotFoundError:
            result[name] = None
    return result


def copy_file(source, dest):
    """APFS clone when available, otherwise ordinary independent copies."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    if sys.platform == "darwin":
        libc = ctypes.CDLL(None, use_errno=True)
        clone = libc.clonefile
        clone.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int]
        clone.restype = ctypes.c_int
        if clone(os.fsencode(source), os.fsencode(dest), 0) == 0:
            return
    if shutil.disk_usage(dest.parent).free < source.stat().st_size + 2 * 1024**3:
        raise RuntimeError("Snapshot copy would leave less than 2 GiB free")
    shutil.copy2(source, dest)


def snapshot(repo, dest):
    """Snapshot working files without changing the source HEAD, index or edits."""
    files = source_files(repo)
    before = signatures(repo, files)
    for name in files:
        p = repo / name
        if p.is_symlink() and not p.resolve().is_relative_to(repo):
            raise RuntimeError(f"External symlink cannot be isolated: {name}")
        if p.is_dir():
            raise RuntimeError(f"Nested repository/submodule needs explicit handling: {name}")
    head = git(repo, "rev-parse", "HEAD").decode().strip()
    git(repo, "worktree", "add", "--detach", "--no-checkout", str(dest), head)
    for name in files:
        src, dst = repo / name, dest / name
        if src.is_symlink():
            dst.parent.mkdir(parents=True, exist_ok=True)
            # Rewrite internal absolute symlinks so they cannot edit the source.
            target = os.readlink(src)
            if os.path.isabs(target):
                target = os.path.relpath(dest / src.resolve().relative_to(repo), dst.parent)
            dst.symlink_to(target)
        elif src.is_file():
            copy_file(src, dst)
    if source_files(repo) != files or signatures(repo, files) != before:
        raise RuntimeError("Source changed during snapshot; incomplete worktree retained. Reinitialize at a new path.")
    git(dest, "read-tree", head)
    git(dest, "add", "-A", "--", ".")
    tree = git(dest, "write-tree").decode().strip()
    env = dict(os.environ, GIT_AUTHOR_NAME="Claude Pair Snapshot",
               GIT_AUTHOR_EMAIL="local-snapshot@localhost",
               GIT_COMMITTER_NAME="Claude Pair Snapshot",
               GIT_COMMITTER_EMAIL="local-snapshot@localhost")
    baseline = git(dest, "commit-tree", tree, "-p", head, "-m",
                   "Local working-copy snapshot for Rust viewer consolidation", env=env).decode().strip()
    git(dest, "update-ref", "HEAD", baseline, head)
    return {"source_head": head, "baseline": baseline, "file_count": len(files),
            "source_signatures_sha256": hashlib.sha256(json.dumps(before, sort_keys=True).encode()).hexdigest()}


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
            if state["report"]["status"] != "done" or state["report"]["blockers"]:
                raise ValueError("Cannot accept a blocked worker")
            receipts = state.get("receipts", [])
            expected = set(previous["checks"]) | {"diff"}
            if {r["name"] for r in receipts} != expected or any(r["exit_code"] != 0 for r in receipts):
                raise ValueError("Cannot accept without passing independent checks")
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
    if not set(plan["checks"]).issubset(checks):
        raise ValueError("Unknown verification check")
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
        self.repo = Path(self.config["worktree"])
        self.deadline = 0

    def outer_settings(self):
        path = self.root / "outer-settings.json"
        return read_json(path) if path.exists() else {"enabled": False, "max_batches": 8}

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

    def process(self, argv, prefix, stdin=None):
        """Persist output before interpretation; stop the process group on limits."""
        out, err = prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
        timeout = min(self.config["turn_minutes"] * 60, self.deadline - time.monotonic())
        if timeout <= 0 or (self.root / "STOP").exists():
            raise InterruptedError("Stop requested or run time exhausted")
        with out.open("wb") as of, err.open("wb") as ef:
            p = subprocess.Popen(argv, cwd=self.repo, stdin=subprocess.PIPE if stdin else subprocess.DEVNULL,
                                 stdout=of, stderr=ef, start_new_session=True)
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

    def call(self, role, prompt):
        shared_notebook.setup(self.root, self.state, self.config)
        prompt += shared_notebook.context(self.root, role)
        prompt += disk_preflight.context(self.repo)
        schema = {"director": outer_loop.DIRECTOR_SCHEMA, "orchestrator": PLAN_SCHEMA, "worker": REPORT_SCHEMA}[role]
        session = self.state["sessions"].get(role)
        sid = session or str(uuid.uuid4())
        self.state["calls"] += 1
        prefix = self.root / "logs" / f"{self.state['calls']:04d}-{role}"
        prefix.with_suffix(".prompt.md").write_text(prompt)
        reservation = min(self.config["call_budget_usd"], self.config["budget_usd"] - self.state["cost_usd"])
        if reservation < 0.01:
            raise InterruptedError("Estimated usage budget exhausted")
        # Reserve the entire cap before launch. Crashes cannot reset the ledger.
        self.state["cost_usd"] += reservation
        self.state["inflight"] = {"role": role, "session_id": sid, "prefix": str(prefix), "reserved_usd": reservation,
                                  "started_at": time.time()}
        self.save()
        instructions = (self.root / "prompts" / "mission.md").read_text() + "\n\n" + (self.root / "prompts" / f"{role}.md").read_text()
        instructions += "\n\n" + shared_notebook.SYSTEM
        read_only = role in ("director", "orchestrator") or self.config["audit_only"]
        if self.state.get("outer"):
            instructions += "\nA read-only Director now selects bounded batches above this pair. The batch contract overrides older whole-mission completion instructions. Only the worker edits code."
            if role != "director":
                instructions += outer_loop.contract_prompt(self)
        if self.config["audit_only"]:
            instructions += "\nAUDIT ONLY: do not modify files. The current task is a bounded inventory, not implementation. Report findings via structured output."
        argv = [self.config["claude"], "--print", "--output-format", "stream-json", "--verbose",
                "--system-prompt-snapshot", "off",
                "--json-schema", json.dumps(schema), "--append-system-prompt", instructions,
                "--safe-mode", "--setting-sources", "", "--settings", '{"disableAllHooks":true}',
                "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}',
                "--no-chrome", "--disable-slash-commands", "--permission-prompts", "none",
                "--permission-mode", "dontAsk" if read_only else "auto",
                "--tools", "Read,Glob,Grep" if read_only else "Read,Glob,Grep,Edit,Write,Bash",
                "--max-budget-usd", str(reservation), "--max-turns", str(self.config["max_turns"]),
                "--resume" if session else "--session-id", sid]
        if read_only:
            argv += ["--allowedTools", "Read,Glob,Grep"]
        if role in ("director", "orchestrator"):
            argv += ["--add-dir", str(self.root)]
        if role == "worker":
            argv += ["--add-dir", str(self.root / "shared")]
        if not session:
            argv += ["--name", f"Rust viewer {role} ({self.root.name})"]
        if self.config.get("model"):
            argv += ["--model", self.config["model"]]
        print(f"{role}: call {self.state['calls']} ({prefix.name})", flush=True)
        code, out, err = self.process(argv, prefix, prompt)
        raw = out.read_text()
        try:
            result = json.loads(raw)
        except json.JSONDecodeError:
            events = [json.loads(line) for line in raw.splitlines() if line.strip()]
            results = [event for event in events if event.get("type") == "result"]
            if not results:
                raise RuntimeError("Claude stream has no final result; inspect logs before retrying")
            result = results[-1]
        if result.get("session_id") != sid:
            raise RuntimeError("Claude returned an unexpected session ID")
        self.state["sessions"][role] = sid
        total = result.get("total_cost_usd")
        prior = self.state["session_costs"].get(role, 0.0)
        if not isinstance(total, (float, int)) or not math.isfinite(total) or total < prior:
            raise RuntimeError("Missing or regressed cumulative usage; reserved cap retained")
        self.state["cost_usd"] += total - prior - reservation
        self.state["session_costs"][role] = total
        self.state.pop("inflight")
        self.save()
        if code or result.get("is_error") or result.get("subtype") != "success":
            raise RuntimeError(f"Claude did not finish successfully; see {out} and {err}")
        if result.get("permission_denials"):
            raise RuntimeError(f"Claude reported permission denials; see {out}. No automatic bypass.")
        data = result.get("structured_output")
        validate(data, schema)
        shared_notebook.response(self.root, role, self.state["calls"], data, out)
        return data

    def evidence(self):
        # The orchestrator can open all logs with Read; full diffs stay on disk.
        prefix = self.root / "logs" / f"review-{self.state['rounds']:04d}"
        diff = prefix.with_suffix(".diff")
        with diff.open("wb") as f:
            subprocess.run(["git", "diff", "--no-ext-diff", self.config["baseline"]], cwd=self.repo, stdout=f, check=True)
        status = git(self.repo, "status", "--short").decode(errors="replace")
        prefix.with_suffix(".status.txt").write_text(status)
        return {"local_commits": git(self.repo, "log", "--format=%h %s", self.config["baseline"] + "..HEAD").decode(errors="replace"),
                "diff_file": str(diff), "status_file": str(prefix.with_suffix('.status.txt')),
                "note": "Untracked files are listed in status, not in diff. Read every relevant new file directly.",
                "worker_report": self.state.get("report"), "independent_checks": self.state.get("receipts", [])}

    def verify(self):
        receipts = []
        names = list(dict.fromkeys(["diff", *self.state["plan"]["checks"]]))
        for name in names:
            prefix = self.root / "logs" / f"check-{self.state['rounds']:04d}-{name}"
            argv = self.config["checks"][name]
            code, out, err = self.process(argv, prefix)
            receipts.append({"name": name, "command": argv, "exit_code": code,
                             "stdout": str(out), "stderr": str(err)})
        self.state["receipts"] = receipts
        shared_notebook.append(self.root, {"id": f"checks-{self.state['rounds']:04d}-{self.state['calls']:04d}",
            "author": "coordinator", "kind": "Independent verification",
            "summary": "Independent checks finished: " + "; ".join(r["name"] + (" passed" if r["exit_code"] == 0 else " failed") for r in receipts),
            "notes": [r["stdout"] + " | " + r["stderr"] for r in receipts], "source": str(self.root / "logs")})

    def run(self, steps=0, retry=False):
        outer_loop.prepare(self)
        shared_notebook.setup(self.root, self.state, self.config)
        if self.state["status"] == "complete":
            print("Mission already complete")
            return
        if self.state.get("inflight") and not retry:
            raise RuntimeError("Interrupted/uncertain call. Inspect its logs and worktree, then use --retry-interrupted. The reserved usage cap remains charged.")
        if retry and self.state.get("inflight"):
            interrupted = self.state.pop("inflight")
            self.state["sessions"][interrupted["role"]] = interrupted["session_id"]
        if (self.root / "STOP").exists():
            raise RuntimeError("STOP is present. Use the resume command to clear it deliberately.")
        if self.state.get("run_started_at"):
            self.state["elapsed_seconds"] += max(0, time.time() - self.state.pop("run_started_at"))
        remaining = self.config["max_hours"] * 3600 - self.state["elapsed_seconds"]
        self.deadline = time.monotonic() + remaining
        started = time.monotonic()
        self.state["status"] = "running"
        self.state["message"] = "Coordinator running; consult logs for the active assignment."
        self.state["run_started_at"] = time.time()
        self.state["pid"] = os.getpid()
        self.save()
        count = 0
        try:
            while True:
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
                    if completed >= settings["max_batches"]:
                        raise InterruptedError("Completed-batch ceiling reached; increase it to continue")
                    if self.state["rounds"] >= self.config["max_rounds"]:
                        raise InterruptedError("Worker-turn ceiling reached before selecting another batch")
                    prompt = outer_loop.director_prompt(self)
                    if steering_path.exists():
                        prompt += "\nCURRENT OPERATOR GUIDANCE:\n" + read_json(steering_path)["text"]
                    decision = self.call("director", prompt)
                    outer_loop.guard_decision(decision, self.state, self.config["checks"])
                    write_json(self.root / "logs" / f"director-{self.state['calls']:04d}.json", decision)
                    if not outer_loop.dispatch(self, decision):
                        break
                elif phase == "orchestrator":
                    prompt = "Read the mission and inspect the repository. Maintain your checklist. Verification catalogue:\n" + json.dumps(self.config["checks"])
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
                    plan = self.call("orchestrator", prompt)
                    guard_plan(plan, self.state, self.config["checks"])
                    outer_loop.guard_contract(self, plan)
                    if self.config["audit_only"] and set(plan["checks"]) - {"diff"}:
                        raise ValueError("Audit-only run cannot request builds")
                    shared_notebook.append(self.root, {"id": f"review-{self.state['calls']:04d}",
                        "author": "coordinator", "kind": "Validated handoff",
                        "summary": f"Orchestrator response passed the handoff guards: action={plan['action']}, review={plan['review']}.",
                        "notes": ["Evidence remains scoped to the reviewed assignment; this does not establish whole-project completion."],
                        "source": str(self.root / "logs" / f"plan-{self.state['calls']:04d}.json")})
                    self.state["plan"] = plan
                    self.state.pop("operator_replan", None)
                    write_json(self.root / "logs" / f"plan-{self.state['calls']:04d}.json", plan)
                    if plan["action"] == "complete" and self.state.get("outer", {}).get("current_batch"):
                        outer_loop.finish_batch(self, plan)
                    elif plan["action"] != "work":
                        self.state["status"] = plan["action"]
                        self.state["message"] = plan["summary"]
                        break
                    else:
                        self.state["phase"] = "worker"
                elif phase == "worker":
                    if self.state["rounds"] >= self.config["max_rounds"]:
                        raise InterruptedError("Worker-turn ceiling reached; reviewed progress is saved")
                    plan = self.state["plan"]
                    prompt = plan["worker_prompt"] + "\n\nAcceptance criteria:\n" + json.dumps(plan["acceptance_criteria"])
                    self.state["report"] = self.call("worker", prompt)
                    self.state["rounds"] += 1
                    self.state["phase"] = "verify"
                    self.state["receipts"] = []
                    write_json(self.root / "logs" / f"worker-{self.state['rounds']:04d}.json", self.state["report"])
                elif phase == "verify":
                    self.verify()
                    self.state["phase"] = "orchestrator"
                else:
                    raise ValueError(f"Unknown phase: {phase}")
                self.save()
                count += 1
                if steps and count >= steps:
                    raise InterruptedError("Requested number of steps finished; ready to resume")
        except InterruptedError as e:
            self.state.update(status="paused", message=str(e))
        except (Exception, KeyboardInterrupt) as e:
            self.state.update(status="blocked", message=f"{type(e).__name__}: {e}")
        finally:
            self.state["elapsed_seconds"] += time.monotonic() - started
            self.state.pop("run_started_at", None)
            self.state.pop("pid", None)
            self.save()
            shared_notebook.current(self.root, self.state, self.config)
        print(f"{self.state['status']}: {self.state.get('message', '')}", flush=True)
        print(f"Status: {self.root / 'STATUS.md'}", flush=True)
        return 1 if self.state["status"] == "blocked" else 0


def initialize(args):
    root = Path(args.state).expanduser().resolve()
    repo = Path(args.repo).expanduser().resolve()
    if root.exists():
        raise ValueError("State directory already exists; use run/resume or choose a new directory")
    if root.is_relative_to(repo):
        raise ValueError("Keep coordinator state/worktree outside the source checkout")
    for name in ("max_rounds", "max_hours", "turn_minutes", "max_turns", "budget_usd", "call_budget_usd"):
        value = getattr(args, name)
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"{name} must be positive and finite")
    if git(repo, "rev-parse", "--show-toplevel").decode().strip() != str(repo):
        raise ValueError("--repo must be the repository root")
    claude = shutil.which(args.claude)
    if not claude:
        raise ValueError("Claude Code executable not found")
    root.mkdir(parents=True)
    (root / "logs").mkdir()
    shutil.copytree(HERE / "prompts", root / "prompts")
    print("Snapshotting tracked and non-ignored working files; source checkout is preserved.", flush=True)
    snap = snapshot(repo, root / "workspace")
    config = {"repo": str(repo), "worktree": str(root / "workspace"), **snap,
              "claude": claude, "model": args.model, "audit_only": args.audit_only,
              "checks": read_json(HERE / "checks.json")}
    for name in ("max_rounds", "max_hours", "turn_minutes", "max_turns", "budget_usd", "call_budget_usd"):
        config[name] = getattr(args, name)
    config["checks"]["diff"] = ["git", "diff", "--check", config["baseline"]]
    write_json(root / "config.json", config)
    write_json(root / "state.json", {"version": 1, "status": "ready", "phase": "orchestrator",
        "calls": 0, "rounds": 0, "elapsed_seconds": 0, "cost_usd": 0,
        "sessions": {}, "session_costs": {}, "message": "Ready; no model calls yet."})
    Runner(root).save()
    print(f"Ready: {root}\nRun: python3 {HERE / 'pair.py'} run --state {root}")


def configure_outer(root, enabled, max_batches):
    if type(enabled) is not bool or type(max_batches) is not int or max_batches < 1:
        raise ValueError("Use a boolean enabled flag and a positive whole batch limit")
    target = root / "prompts/director.md"
    if not target.exists():
        shutil.copyfile(HERE / "prompts/director.md", target)
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
    init = sub.add_parser("init", help="Snapshot the current working tree; no model calls")
    init.add_argument("--repo", required=True)
    init.add_argument("--state", required=True)
    init.add_argument("--claude", default="claude")
    init.add_argument("--model", help="Omit to use Claude Code's default model")
    init.add_argument("--audit-only", action="store_true")
    init.add_argument("--max-rounds", type=int, default=12)
    init.add_argument("--max-hours", type=float, default=8)
    init.add_argument("--turn-minutes", type=float, default=45)
    init.add_argument("--max-turns", type=int, default=100)
    init.add_argument("--budget-usd", type=float, default=100)
    init.add_argument("--call-budget-usd", type=float, default=10)
    for name in ("run", "resume", "status", "stop", "enable-outer", "watch-outer"):
        p = sub.add_parser(name)
        p.add_argument("--state", required=True)
        if name == "enable-outer":
            p.add_argument("--max-batches", type=int, default=8)
        if name in ("run", "resume"):
            p.add_argument("--steps", type=int, default=0, help="Pause after N state transitions (0 = until limit)")
            p.add_argument("--retry-interrupted", action="store_true", help="After inspecting logs, resume an uncertain interrupted session")
    args = parser.parse_args()
    try:
        if args.command == "init":
            initialize(args)
            return 0
        root = Path(args.state).expanduser().resolve()
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
