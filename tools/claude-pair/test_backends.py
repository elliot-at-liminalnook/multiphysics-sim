"""Claude Code and Codex CLI are interchangeable backends, and switching between
them never breaks a run, in either direction.

One fake stands in for both CLIs at the process boundary. It fails the test if
either CLI is ever asked to resume a session it did not create, and records
every call so each scenario can check exactly what each CLI was given.
"""
import contextlib
import copy
import datetime
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch
import uuid

import codex_backend
import dashboard
import pair
import test_pair as fixtures

HERE = Path(__file__).resolve().parent
ROLES = ("orchestrator", "worker")


def iso(t=None):
    return datetime.datetime.fromtimestamp(t or time.time(), datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def report():
    return copy.deepcopy(fixtures.REPORT)


def first_plan():
    p = fixtures.plan()
    p["checks"] = []
    return p


def final_plan():
    p = fixtures.plan("complete", "accept")
    p["checks"] = []
    p["checklist"][0].update(status="verified", evidence="Read proof.txt")
    return p


class FakeCLIs:
    """Both agent CLIs. Behaviours queue per call: ok, interrupt (stopped
    mid-turn), interrupt-early (stopped before the session started), limit
    (usage limit), garbage (a final message that isn't the schema)."""

    def __init__(self, runner, home):
        self.runner, self.home = runner, Path(home)
        self.calls, self.violations = [], []
        self.created = {"claude": set(), "codex": set()}
        self.script, self.results = [], {}
        self.spawns = []  # per Codex call: subagent names it spawns

    def result_for(self, role):
        if role in self.results:
            value = self.results[role]
            return value() if callable(value) else copy.deepcopy(value)
        if role == "worker":
            return report()
        return final_plan() if self.runner.state.get("report") else first_plan()

    def violation(self, message):
        self.violations.append(message)
        raise AssertionError(message)

    def __call__(self, argv, prefix, stdin=None, cwd=None, env=None):
        role = prefix.name.split("-", 1)[1]
        out, err = prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
        err.write_text("")
        behaviour = self.script.pop(0) if self.script else "ok"
        if Path(argv[0]).name == "codex" or argv[1:2] == ["exec"]:
            return self.codex(argv, role, out, err, stdin, env, behaviour)
        return self.claude(argv, role, out, err, stdin, env, behaviour)

    def claude(self, argv, role, out, err, stdin, env, behaviour):
        if "--resume" in argv:
            sid, resumed = argv[argv.index("--resume") + 1], True
            if sid not in self.created["claude"]:
                self.violation(f"claude was asked to resume {sid}, a session it never created")
        else:
            sid, resumed = argv[argv.index("--session-id") + 1], False
            if sid in self.created["codex"] or sid in self.created["claude"]:
                self.violation(f"claude was given an existing session ID {sid} as new")
        self.calls.append({"backend": "claude", "role": role, "session": sid, "resumed": resumed, "argv": argv,
                           "prompt": stdin, "env": env})
        assistant = json.dumps({"type": "assistant", "message": {"content": [{"type": "text", "text": "working"}]}})
        if behaviour.startswith("interrupt"):
            if behaviour == "interrupt":
                self.created["claude"].add(sid)
                out.write_text(assistant + "\n")
            raise InterruptedError("Stop requested")
        self.created["claude"].add(sid)
        if behaviour == "limit":
            reset = int(time.time()) + 3600
            info = {"status": "rejected", "resetsAt": reset, "rateLimitType": "five_hour"}
            lines = [json.dumps({"type": "rate_limit_event", "rate_limit_info": info}), assistant,
                     json.dumps({"type": "result", "subtype": "error_during_execution", "is_error": True, "session_id": sid,
                                 "result": "You've hit your usage limit"})]
            out.write_text("\n".join(lines) + "\n")
            return 1, out, err
        data = {"garbage": None}.get(behaviour, self.result_for(role))
        out.write_text(json.dumps({"type": "result", "session_id": sid, "subtype": "success", "is_error": False,
                                   "total_cost_usd": self.runner.state["session_costs"].get(sid, 0) + .25,
                                   "structured_output": data}) + "\n")
        return 0, out, err

    def codex(self, argv, role, out, err, stdin, env, behaviour):
        if argv[-1] != "-" or not stdin:
            self.violation("codex must get its prompt on stdin, with `-`")
        resumed = argv[2] == "resume"
        if resumed:
            tid = argv[-2]
            if tid not in self.created["codex"]:
                self.violation(f"codex was asked to resume {tid}, a session it never created")
        else:
            tid = str(uuid.uuid4())
            if any(a.startswith("developer_instructions=") for a in argv) is False:
                self.violation("a new codex session must carry the role instructions")
        self.calls.append({"backend": "codex", "role": role, "session": tid, "resumed": resumed, "argv": argv,
                           "prompt": stdin, "env": env})
        started = [json.dumps({"type": "thread.started", "thread_id": tid}), json.dumps({"type": "turn.started"}),
                   json.dumps({"type": "item.completed", "item": {"id": "item_0", "type": "agent_message", "text": "working"}})]
        if behaviour == "interrupt-early":
            raise InterruptedError("Stop requested")
        self.created["codex"].add(tid)
        if behaviour == "interrupt":
            out.write_text("\n".join(started) + "\n")
            raise InterruptedError("Stop requested")
        if behaviour == "limit":
            self.rollout(tid, used=100, reached="codex")
            message = json.dumps({"type": "error", "status": 429, "error": {"message": "You've hit your usage limit."}})
            out.write_text("\n".join(started + [json.dumps({"type": "turn.failed", "error": {"message": message}})]) + "\n")
            return 1, out, err
        self.rollout(tid, used=40)
        for name in (self.spawns.pop(0) if self.spawns else ()):
            self.child(tid, name)
        final = "not json at all" if behaviour == "garbage" else json.dumps(self.result_for(role))
        lines = started + [json.dumps({"type": "item.completed", "item": {"id": "item_1", "type": "agent_message", "text": final}}),
                           json.dumps({"type": "turn.completed", "usage": {"input_tokens": 1000, "cached_input_tokens": 500,
                                                                           "output_tokens": 50, "reasoning_output_tokens": 0}})]
        out.write_text("\n".join(lines) + "\n")
        return 0, out, err

    def folder(self):
        folder = self.home / "sessions" / "2026" / "10" / "01"
        folder.mkdir(parents=True, exist_ok=True)
        return folder

    def rollout(self, tid, used, reached=None):
        """Like Codex: one file per session, growing with each call, real times."""
        time.sleep(0.01)
        path = self.folder() / f"rollout-2026-10-01T12-00-00-{tid}.jsonl"
        rows = [] if path.exists() else [{"timestamp": iso(), "type": "session_meta", "payload": {"id": tid, "source": "exec", "cli_version": "test"}}]
        rows += [{"timestamp": iso(), "type": "event_msg", "payload": {"type": "item_completed", "item": {
                    "type": "CommandExecution", "id": f"exec-{uuid.uuid4()}", "command": ["/bin/zsh", "-c", "git status"],
                    "aggregated_output": "clean\n", "exit_code": 0, "status": "completed"}}},
                 {"timestamp": iso(), "type": "event_msg", "payload": {"type": "token_count", "rate_limits": {
                    "primary": {"used_percent": used, "window_minutes": 10080, "resets_at": int(time.time()) + 3 * 86400},
                    "secondary": {"used_percent": 10, "window_minutes": 300, "resets_at": int(time.time()) + 3600},
                    "rate_limit_reached_type": reached, "plan_type": "pro"}}}]
        with path.open("a") as f:
            f.write("\n".join(json.dumps(r) for r in rows) + "\n")

    def child(self, parent, name, role="pair_implementer", diff="@@\n-old\n+new\n", reply=None):
        """A subagent's own rollout, linked to its parent like Codex's."""
        tid = f"{name}-{uuid.uuid4().hex[:8]}"
        rows = [{"timestamp": iso(), "type": "session_meta", "payload": {"id": tid, "agent_nickname": name.title(),
                 "source": {"subagent": {"thread_spawn": {"parent_thread_id": parent, "agent_path": f"/root/{name}", "agent_role": role}}}}},
                {"timestamp": iso(), "type": "event_msg", "payload": {"type": "task_started"}},
                {"timestamp": iso(), "type": "event_msg", "payload": {"type": "item_completed", "item": {
                 "type": "FileChange", "id": f"fc-{tid}", "status": "completed", "changes": {f"/x/{name}.rs": {"type": "update", "unified_diff": diff}}}}},
                {"timestamp": iso(), "type": "event_msg", "payload": {"type": "task_complete", "last_agent_message": reply or f"{name} done"}}]
        (self.folder() / f"rollout-2026-10-01T12-00-05-{tid}.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n")
        return tid


class Base(unittest.TestCase):
    def setUp(self):
        self.__dict__.pop("fake", None)  # subtests call setUp again for a clean run
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        base = Path(self.tmp.name).resolve()
        self.home = base / "codex-home"
        self.home.mkdir()
        env = patch.dict(os.environ, {"CODEX_HOME": str(self.home)})
        env.start()
        self.addCleanup(env.stop)
        self.root = fixtures.PairTests().initialize_fixture(base)
        config = pair.read_json(self.root / "config.json")
        config.update(max_rounds=None, budget_usd=None, call_budget_usd=None, max_turns=None, turn_minutes=None, max_hours=None)
        pair.write_json(self.root / "config.json", config)
        self.runner = self.fresh_runner()

    def fresh_runner(self):
        """A coordinator as a restart would build it: from what's on disk."""
        runner = pair.Runner(self.root)
        runner.deadline = time.monotonic() + 600
        if hasattr(self, "fake"):
            self.fake.runner = runner
        else:
            self.fake = FakeCLIs(runner, self.home)
        return runner

    def use(self, backend=None, **roles):
        """Switch exactly as the CLI and dashboard do: through config.json."""
        if backend:
            pair.switch_backend(self.root, backend)
        for role, name in roles.items():
            pair.switch_backend(self.root, name, role)

    def call(self, role="worker", scope="assignment:1", runner=None):
        runner = runner or self.runner
        with patch.object(runner, "process", self.fake):
            return runner.call(role, f"task for {role}", scope=scope)

    def run_steps(self, steps=0, runner=None):
        runner = runner or self.fresh_runner()
        with patch.object(runner, "process", self.fake):
            code = runner.run(steps=steps)
        self.runner = runner
        return code

    def last(self, role=None):
        calls = [c for c in self.fake.calls if role is None or c["role"] == role]
        return calls[-1]

    def tearDown(self):
        self.assertEqual(self.fake.violations, [], "a CLI was given a session from the other backend")


class SwitchBetweenTurns(Base):
    def test_every_transition_keeps_sessions_on_their_own_backend(self):
        """Same backend twice resumes; a change always starts fresh on the new
        one; the session map always names the backend that owns each session."""
        sequence = ["claude", "claude", "codex", "codex", "claude", "codex", "claude", "claude", "codex"]
        previous = None
        for backend in sequence:
            self.use(backend)
            self.call("worker")
            call = self.last()
            self.assertEqual(call["backend"], backend)
            self.assertEqual(call["resumed"], backend == previous, f"{previous} -> {backend}")
            sid = self.runner.state["sessions"]["worker"]
            self.assertEqual(sid, call["session"])
            self.assertEqual(self.runner.state["session_backends"][sid], backend)
            previous = backend

    def test_each_role_tracks_its_own_backend(self):
        self.use("claude", worker="codex")
        self.call("orchestrator", scope="batch:mission")
        self.call("worker")
        self.assertEqual([c["backend"] for c in self.fake.calls], ["claude", "codex"])
        self.use(worker="claude", orchestrator="codex")
        self.call("orchestrator", scope="batch:mission")
        self.call("worker")
        self.assertEqual([(c["backend"], c["resumed"]) for c in self.fake.calls[2:]], [("codex", False), ("claude", False)])
        self.call("orchestrator", scope="batch:mission")
        self.call("worker")
        self.assertEqual([(c["backend"], c["resumed"]) for c in self.fake.calls[4:]], [("codex", True), ("claude", True)])

    def test_a_switch_applies_at_the_next_call_without_a_restart(self):
        self.call("worker")
        pair.switch_backend(self.root, "codex")  # the running coordinator's Runner is unchanged
        self.call("worker")
        self.assertEqual(self.last()["backend"], "codex")

    def test_clearing_a_role_override_follows_the_default_again(self):
        self.use("codex", worker="claude")
        self.call("worker")
        self.assertEqual(self.last()["backend"], "claude")
        pair.switch_backend(self.root, clear_role="worker")
        self.call("worker")
        self.assertEqual((self.last()["backend"], self.last()["resumed"]), ("codex", False))

    def test_an_unknown_backend_value_falls_back_to_claude(self):
        config = pair.read_json(self.root / "config.json")
        config.update(backend="gemini", role_backends={"worker": "nonsense"})
        pair.write_json(self.root / "config.json", config)
        self.call("worker")
        self.assertEqual(self.last()["backend"], "claude")

    def test_the_same_result_makes_the_same_handoff_on_either_backend(self):
        handoffs = {}
        for backend in ("claude", "codex"):
            self.use(backend)
            data = self.call("worker", scope=f"assignment:{backend}")
            entry = pair.shared_notebook.entries(self.root)[-1]
            handoffs[backend] = (data, entry["author"], entry["kind"], entry["summary"], entry["notes"])
        self.assertEqual(handoffs["claude"], handoffs["codex"])


class SwitchMidTurn(Base):
    def interrupt_then_switch(self, first, second, behaviour="interrupt"):
        self.use(first)
        self.runner.state.update(phase="worker", plan=first_plan(), assignment=1)
        self.runner.save()
        self.fake.script = [behaviour]
        self.run_steps(steps=1)
        state = pair.read_json(self.root / "state.json")
        self.assertEqual(state["status"], "paused")
        self.assertEqual(state["inflight"]["backend"], first)
        self.use(second)
        self.run_steps(steps=1)
        return [c for c in self.fake.calls if c["role"] == "worker"]

    def test_a_stopped_turn_continues_on_the_other_backend_in_both_directions(self):
        for first, second in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{first} -> {second}"):
                self.setUp()
                calls = self.interrupt_then_switch(first, second)
                self.assertEqual([(c["backend"], c["resumed"]) for c in calls], [(first, False), (second, False)])
                self.assertIn(f"ran on {first} and was cut short", calls[1]["prompt"])
                self.assertIn(f"continuing it on {second}", calls[1]["prompt"])
                state = pair.read_json(self.root / "state.json")
                self.assertNotIn("retry_session", state)
                self.assertNotIn("inflight", state)
                self.assertEqual(state["session_backends"][state["sessions"]["worker"]], second)
                self.assertEqual(state["phase"], "orchestrator", "the worker's report was taken")
                self.tearDown()

    def test_a_stopped_turn_on_the_same_backend_resumes_its_session(self):
        for backend in ("claude", "codex"):
            with self.subTest(backend):
                self.setUp()
                calls = self.interrupt_then_switch(backend, backend)
                self.assertEqual([c["resumed"] for c in calls], [False, True])
                self.assertEqual(calls[0]["session"], calls[1]["session"])
                self.tearDown()

    def test_a_codex_turn_stopped_before_its_session_started_starts_fresh(self):
        for second in ("codex", "claude"):
            with self.subTest(second):
                self.setUp()
                calls = self.interrupt_then_switch("codex", second, behaviour="interrupt-early")
                self.assertFalse(calls[1]["resumed"])
                self.assertIn("interrupted before its session started", calls[1]["prompt"])
                self.tearDown()

    def test_a_codex_session_is_recovered_from_partial_output_after_a_crash(self):
        calls = self.interrupt_then_switch("codex", "codex")
        self.assertTrue(calls[1]["resumed"])
        self.assertEqual(calls[1]["session"], calls[0]["session"])


class SwitchAfterTrouble(Base):
    def test_a_usage_limit_pause_then_a_switch_starts_fresh_without_stale_resume_notes(self):
        for first, second in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{first} -> {second}"):
                self.setUp()
                self.use(first)
                self.fake.script = ["limit"]
                with self.assertRaises(pair.UsageLimit) as raised:
                    self.call("worker")
                self.assertEqual(raised.exception.weekly, first == "codex", "Codex's reached window here is the weekly one")
                self.assertEqual(self.runner.state.get("limit_resume"), self.runner.state["sessions"]["worker"])
                self.use(second)
                self.call("worker")
                call = self.last()
                self.assertEqual((call["backend"], call["resumed"]), (second, False))
                self.assertNotIn("usage-limit pause", call["prompt"])
                self.assertIn("was cut short", call["prompt"])
                self.assertNotIn("limit_resume", self.runner.state)
                self.assertNotIn("retry_session", self.runner.state)
                self.tearDown()

    def test_a_usage_limit_on_the_same_backend_resumes_with_the_resume_note(self):
        for backend in ("claude", "codex"):
            with self.subTest(backend):
                self.setUp()
                self.use(backend)
                pair.switch_backend(self.root, auto_switch=False)  # with it on, the limit moves the role (AutoSwitch tests)
                self.fake.script = ["limit"]
                with self.assertRaises(pair.UsageLimit):
                    self.call("worker")
                self.call("worker")
                self.assertTrue(self.last()["resumed"])
                self.assertIn("usage-limit pause", self.last()["prompt"])
                self.tearDown()

    def test_a_failed_call_then_a_switch_keeps_the_error_note(self):
        for first, second in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{first} -> {second}"):
                self.setUp()
                self.use(first)
                self.fake.script = ["garbage"]
                with self.assertRaises(pair.CallFailed):
                    self.call("worker")
                self.use(second)
                self.call("worker")
                call = self.last()
                self.assertEqual((call["backend"], call["resumed"]), (second, False))
                self.assertIn("previous attempt ended with an error", call["prompt"])
                self.assertIn("did not match the schema", call["prompt"])
                self.assertNotIn("retry_note", self.runner.state)
                self.tearDown()


class WholeRuns(Base):
    def assert_cycle(self, expect):
        self.assertEqual(self.run_steps(), 0)
        state = pair.read_json(self.root / "state.json")
        self.assertEqual(state["status"], "complete")
        self.assertEqual(state["plan"]["action"], "complete")
        self.assertEqual(state["report"]["summary"], "Created proof")
        self.assertEqual([(c["role"], c["backend"]) for c in self.fake.calls], expect)
        roles = [e["author"] for e in pair.shared_notebook.entries(self.root) if e["kind"] == "Agent report / proposal"]
        self.assertEqual(roles, ["orchestrator", "worker", "orchestrator"])

    def test_a_full_assignment_cycle_on_each_backend_and_each_mix(self):
        for default, overrides in (("claude", {}), ("codex", {}), ("claude", {"worker": "codex"}), ("codex", {"worker": "claude"})):
            with self.subTest(default=default, overrides=overrides):
                self.setUp()
                self.use(default, **overrides)
                worker = overrides.get("worker", default)
                self.assert_cycle([("orchestrator", default), ("worker", worker), ("orchestrator", default)])
                self.tearDown()

    def test_switching_mid_run_between_steps_finishes_the_cycle(self):
        self.use("claude")
        self.run_steps(steps=1)        # orchestrator assigns on Claude
        self.use("codex")
        self.run_steps(steps=1)        # worker runs on Codex
        self.use("claude")
        self.assertEqual(self.run_steps(), 0)  # orchestrator reviews on Claude
        self.assertEqual([(c["role"], c["backend"], c["resumed"]) for c in self.fake.calls],
                         [("orchestrator", "claude", False), ("worker", "codex", False), ("orchestrator", "claude", True)])
        self.assertEqual(pair.read_json(self.root / "state.json")["status"], "complete")


class StateCompatibility(Base):
    def test_state_written_before_backends_existed_is_all_claude(self):
        sid = str(uuid.uuid4())
        self.fake.created["claude"].add(sid)
        self.runner.state["sessions"]["worker"] = sid
        self.runner.state["session_scopes"] = {"worker": "assignment:1"}
        self.runner.state.pop("session_backends", None)
        self.runner.save()
        runner = self.fresh_runner()
        self.call("worker", runner=runner)
        self.assertEqual((self.last()["backend"], self.last()["resumed"], self.last()["session"]), ("claude", True, sid))
        self.use("codex")
        self.call("worker", runner=runner)
        self.assertEqual((self.last()["backend"], self.last()["resumed"]), ("codex", False))

    def test_config_written_before_backends_existed_runs_claude(self):
        config = pair.read_json(self.root / "config.json")
        for key in ("backend", "role_backends", "codex"):
            config.pop(key, None)
        pair.write_json(self.root / "config.json", config)
        self.call("worker")
        self.assertEqual(self.last()["backend"], "claude")

    def test_a_restart_between_every_call_keeps_the_invariant(self):
        for backend in ("codex", "codex", "claude", "codex", "claude", "claude"):
            self.use(backend)
            runner = self.fresh_runner()
            self.call("worker", runner=runner)
            self.assertEqual(self.last()["backend"], backend)
        self.assertEqual([c["resumed"] for c in self.fake.calls], [False, True, False, False, False, True])


class LedgerAndInstructions(Base):
    def test_codex_calls_never_move_the_dollar_ledger_and_claude_accounting_is_unchanged(self):
        self.use("claude")
        self.call("worker")
        after_claude = self.runner.state["cost_usd"]
        self.assertAlmostEqual(after_claude, .25)
        self.use("codex")
        self.call("worker")
        self.call("worker")
        self.assertAlmostEqual(self.runner.state["cost_usd"], after_claude)
        self.assertEqual(self.runner.state["tokens"]["codex"]["input_tokens"], 2000)
        codex_sids = {s for s, b in self.runner.state["session_backends"].items() if b == "codex"}
        self.assertFalse(codex_sids & set(self.runner.state["session_costs"]))
        self.use("claude")
        self.call("worker")
        self.assertAlmostEqual(self.runner.state["cost_usd"], .5)

    def test_a_dollar_cap_reservation_is_returned_after_a_codex_call_and_noted(self):
        config = pair.read_json(self.root / "config.json")
        config.update(budget_usd=5, call_budget_usd=1)
        pair.write_json(self.root / "config.json", config)
        runner = self.fresh_runner()
        self.use("codex")
        self.call("worker", runner=runner)
        self.assertAlmostEqual(runner.state["cost_usd"], 0.0)
        self.assertNotIn("--max-budget-usd", self.last()["argv"])
        kinds = [e["kind"] for e in pair.shared_notebook.entries(self.root)]
        self.assertIn("Limit not available on Codex", kinds)

    def test_changed_instructions_reach_a_resumed_codex_session_once(self):
        self.use("codex")
        self.call("worker")
        self.assertNotIn("STANDING INSTRUCTIONS UPDATED", self.last()["prompt"])
        self.call("worker")
        self.assertNotIn("STANDING INSTRUCTIONS UPDATED", self.last()["prompt"], "unchanged instructions are not repeated")
        local = self.root / "prompts"
        local.mkdir(exist_ok=True)
        (local / "worker.md").write_text((HERE / "prompts" / "worker.md").read_text() + "\nNEW RULE: always say pineapple.\n")
        self.call("worker")
        self.assertTrue(self.last()["resumed"])
        self.assertTrue(self.last()["prompt"].startswith("STANDING INSTRUCTIONS UPDATED"))
        self.assertIn("NEW RULE: always say pineapple.", self.last()["prompt"])
        self.call("worker")
        self.assertNotIn("STANDING INSTRUCTIONS UPDATED", self.last()["prompt"])

    def test_a_fresh_session_on_either_backend_carries_the_current_instructions(self):
        local = self.root / "prompts"
        local.mkdir(exist_ok=True)
        (local / "worker.md").write_text("WORKER RULES V2")
        for backend in ("codex", "claude"):
            self.use(backend)
            self.call("worker", scope=f"assignment:{backend}")
            argv = self.last()["argv"]
            text = (next(a for a in argv if a.startswith("developer_instructions=")) if backend == "codex"
                    else argv[argv.index("--append-system-prompt") + 1])
            self.assertIn("WORKER RULES V2", text)
            self.assertEqual("# This backend: Codex CLI" in text, backend == "codex")


class CommandContracts(Base):
    def test_the_claude_command_is_unchanged_by_the_backend_work(self):
        self.call("worker")
        argv = self.last()["argv"]
        for flag in ("--print", "--output-format", "--json-schema", "--append-system-prompt", "--setting-sources",
                     "--strict-mcp-config", "--no-chrome", "--session-id", "--agents", "--dangerously-skip-permissions"):
            self.assertIn(flag, argv)
        self.assertNotIn("exec", argv)
        self.assertNotIn(str(HERE / "shims"), self.last()["env"]["PATH"])

    def test_the_codex_command_contract(self):
        config = pair.read_json(self.root / "config.json")
        config.update(fast_roles=["worker"], codex={"executable": "codex", "model": "gpt-6-sol", "reasoning_effort": "high", "fast": True})
        pair.write_json(self.root / "config.json", config)
        self.use("codex")
        self.call("worker")
        new = self.last()["argv"]
        self.call("worker")
        resumed = self.last()["argv"]
        for argv in (new, resumed):
            for flag in ("--json", "--ignore-user-config", "--output-schema", "--dangerously-bypass-approvals-and-sandbox",
                         'service_tier="priority"', "allow_login_shell=false", "features.memories=false"):
                self.assertIn(flag, argv)
            self.assertEqual(argv[argv.index("-m") + 1], "gpt-6-sol")
            self.assertIn('model_reasoning_effort="high"', argv)
            self.assertIn("agents.pair_implementer.description=" + json.dumps(json.loads(
                (HERE / "prompts" / "subagents.json").read_text())["pair-implementer"]["description"], ensure_ascii=False), argv)
        self.assertEqual((Path(new[0]).name, new[1]), ("codex", "exec"))
        self.assertTrue(any(a.startswith("developer_instructions=") for a in new))
        self.assertFalse(any(a.startswith("developer_instructions=") for a in resumed))
        self.assertEqual(resumed[2], "resume")
        self.assertEqual(resumed[-2:], [self.runner.state["sessions"]["worker"], "-"])
        schema = json.loads(Path(new[new.index("--output-schema") + 1]).read_text())
        self.assertEqual(schema, pair.REPORT_SCHEMA)
        role_file = self.root / "codex-agents" / "pair_reviewer.toml"
        self.assertIn('sandbox_mode = "read-only"', role_file.read_text())
        self.assertNotIn("sandbox_mode", (self.root / "codex-agents" / "pair_implementer.toml").read_text())

    def test_codex_runs_at_normal_speed_unless_its_fast_mode_is_on(self):
        config = pair.read_json(self.root / "config.json")
        config["fast_roles"] = ["worker"]
        pair.write_json(self.root / "config.json", config)
        self.use("codex")
        self.call("worker")
        self.assertNotIn('service_tier="priority"', self.last()["argv"], "Codex fast mode is off by default")
        self.assertNotIn("Fast mode unavailable", [e["kind"] for e in pair.shared_notebook.entries(self.root)],
                         "not requesting it is not a refusal")
        self.use("claude")
        self.call("worker")
        argv = self.last()["argv"]
        self.assertEqual(json.loads(argv[argv.index("--settings") + 1]), {"fastMode": True}, "Claude keeps fast_roles")
        pair.switch_backend(self.root, "codex", codex_fast=True)  # applies at the next call
        self.call("worker")
        self.assertIn('service_tier="priority"', self.last()["argv"])
        pair.switch_backend(self.root, codex_fast=False)
        self.call("worker")
        self.assertNotIn('service_tier="priority"', self.last()["argv"])

    def test_codex_shell_cap_is_on_the_path_except_in_verification_passes(self):
        self.use("codex")
        self.call("worker")
        env = self.last()["env"]
        self.assertTrue(env["PATH"].startswith(str(HERE / "shims") + os.pathsep))
        self.assertEqual(env["PAIR_SHELL_SECONDS"], "10")
        self.runner.state["verification"] = {"since": "x", "head": "y", "commits": 1, "queued_plan": first_plan(), "reason": "test"}
        self.call("worker", scope="assignment:2")
        env = self.last()["env"]
        self.assertFalse(env["PATH"].startswith(str(HERE / "shims")))
        self.assertIn("This turn has no shell time limit.", next(a for a in self.last()["argv"] if a.startswith("developer_instructions=")))

    def test_audit_only_is_read_only_on_codex_too(self):
        config = pair.read_json(self.root / "config.json")
        config["audit_only"] = True
        pair.write_json(self.root / "config.json", config)
        runner = self.fresh_runner()
        self.use("codex")
        self.call("worker", runner=runner)
        argv = self.last()["argv"]
        self.assertNotIn("--dangerously-bypass-approvals-and-sandbox", argv)
        self.assertIn('sandbox_mode="read-only"', argv)
        self.assertIn("features.multi_agent=false", argv)
        self.assertFalse(any(a.startswith("agents.") for a in argv))

    def test_the_cargo_shim_stops_a_slow_cargo(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "cargo"
            fake.write_text("#!/bin/bash\nsleep 8\necho finished\n")
            fake.chmod(0o755)
            env = dict(os.environ, PATH=f"{HERE / 'shims'}{os.pathsep}{tmp}{os.pathsep}{os.environ['PATH']}", PAIR_SHELL_SECONDS="1")
            started = time.monotonic()
            done = subprocess.run(["cargo", "build"], env=env, capture_output=True, text=True)
            self.assertEqual(done.returncode, 124)
            self.assertLess(time.monotonic() - started, 6)
            self.assertIn("unverified", done.stderr)
            env["PAIR_SHELL_SECONDS"] = "0"
            fake.write_text("#!/bin/bash\necho fast $1\n")
            self.assertEqual(subprocess.run(["cargo", "check"], env=env, capture_output=True, text=True).stdout, "fast check\n")


class ParserContracts(unittest.TestCase):
    def lines(self, *events):
        return "\n".join(json.dumps(e) for e in events) + "\n"

    def test_success_failure_cutoff_and_fast_refusal(self):
        ok = self.lines({"type": "thread.started", "thread_id": "t1"},
                        {"type": "item.completed", "item": {"type": "error", "message": "Configured service tier `priority` is not advertised"}},
                        {"type": "item.completed", "item": {"type": "agent_message", "text": '{"a": 1}'}},
                        {"type": "turn.completed", "usage": {"input_tokens": 5}})
        result = codex_backend.parse(ok, fast_requested=True)
        self.assertEqual((result["subtype"], result["session_id"], result["structured_output"]), ("success", "t1", {"a": 1}))
        self.assertEqual(result["fast_mode_state"], "off")
        nested = json.dumps({"type": "error", "error": {"message": "The model is not supported"}})
        failed = codex_backend.parse(self.lines({"type": "thread.started", "thread_id": "t2"},
                                                {"type": "turn.failed", "error": {"message": nested}}), False)
        self.assertEqual((failed["is_error"], failed["result"]), (True, "The model is not supported"))
        self.assertIsNone(codex_backend.parse(self.lines({"type": "thread.started", "thread_id": "t3"}), False))
        self.assertIsNone(codex_backend.parse(ok.replace("turn.completed", "turn.started"), False))

    def test_rate_limits_take_claudes_shape(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, {"CODEX_HOME": tmp}):
            FakeCLIs(None, tmp).rollout("t9", used=100, reached="codex")
            info = codex_backend.rate_limits("t9")
            self.assertEqual(info["status"], "rejected")
            self.assertEqual(info["rateLimitType"], "seven_day")
            self.assertEqual(info["unifiedWindows"]["seven_day"]["utilization"], 1.0)
            self.assertAlmostEqual(info["unifiedWindows"]["five_hour"]["utilization"], .1)
            self.assertTrue(pair.weekly_limit(info, "", info["resetsAt"], time.time()))
            self.assertEqual(pair.limit_reset(info, "", time.time()), info["resetsAt"])
            self.assertIsNone(codex_backend.rate_limits("missing"))

    def test_toml_strings_survive_quotes_newlines_and_emoji(self):
        try:
            import tomllib
        except ImportError:
            self.skipTest("tomllib needs Python 3.11; checked against codex itself in the README probes")
        text = 'He said "hi"\n\tthen 🤖 and \\ backslash'
        self.assertEqual(tomllib.loads("x = " + codex_backend.toml_string(text))["x"], text)


class DashboardCompatibility(Base):
    def test_the_dashboard_reads_a_history_that_switches_backends(self):
        self.fake.spawns = [(), ("part_a",)]  # the second Codex call spawns one subagent
        for backend in ("claude", "codex", "claude", "codex"):
            self.use(backend)
            self.call("worker")
        dashboard._events_cache.clear()
        view = dashboard.view(self.root)
        workers = [c for c in view["calls"] if c["role"] == "worker"]
        self.assertEqual([c["backend"] for c in workers], ["claude", "codex", "claude", "codex"])
        self.assertTrue(all(c["result"]["summary"] == "Created proof" for c in workers))
        self.assertEqual([c["cost_usd"] is not None for c in workers], [True, False, True, False])
        self.assertTrue(workers[1]["tokens"])
        lanes = workers[-1]["subagents"]
        self.assertEqual([(s["type"], s["status"], s["output"]) for s in lanes], [("pair_implementer", "done", "part_a done")])
        edits = [a for a in workers[-1]["activity"] if a.get("parent") == lanes[0]["id"]]
        self.assertEqual(edits[0]["details"]["edits"], [{"old": "old", "new": "new"}])
        self.assertEqual(view["backend"]["default"], "codex")
        self.assertEqual(view["rate_limits"]["backend"], "codex")

    def test_each_call_of_a_resumed_codex_session_shows_only_its_own_work(self):
        """A Codex session's rollout spans every call that resumed it; each turn on
        the dashboard shows only what happened during that call."""
        self.use("codex")
        self.fake.spawns = [("part_a", "part_b"), (), ("part_c",)]
        for _ in range(3):
            self.call("worker")
        sessions = {c["session"] for c in self.fake.calls}
        self.assertEqual(len(sessions), 1, "all three calls resumed one Codex session")
        # A subagent from the first call gets a follow-up during the third.
        a = next(f for f in (self.home / "sessions" / "2026" / "10" / "01").glob("*part_a-*.jsonl"))
        with a.open("a") as f:
            f.write(json.dumps({"timestamp": iso(), "type": "event_msg", "payload": {"type": "task_started"}}) + "\n")
            f.write(json.dumps({"timestamp": iso(), "type": "event_msg", "payload": {"type": "item_completed", "item": {
                "type": "CommandExecution", "id": "exec-followup", "command": ["/bin/zsh", "-c", "git diff"], "exit_code": 0,
                "status": "completed", "aggregated_output": ""}}}) + "\n")
        # part_c finished during the third call, then got a follow-up in the same call.
        c_file = next(f for f in (self.home / "sessions" / "2026" / "10" / "01").glob("*part_c-*.jsonl"))
        with c_file.open("a") as f:
            f.write(json.dumps({"timestamp": iso(), "type": "event_msg", "payload": {"type": "task_started"}}) + "\n")
        dashboard._events_cache.clear()
        calls = [c for c in dashboard.view(self.root)["calls"] if c["role"] == "worker"]
        self.assertEqual(next(s for s in calls[2]["subagents"] if s["description"].startswith("part_c"))["status"], "running",
                         "a subagent busy with a follow-up isn't shown as done")
        lead = lambda c: [a for a in c["activity"] if not a.get("parent") and a["kind"] == "tool" and a["name"] == "Bash"]
        self.assertEqual([len(lead(c)) for c in calls], [1, 1, 1], "each call's lead shows its own command only")
        names = lambda c: sorted(s["description"].split(" ")[0] for s in c["subagents"])
        self.assertEqual([names(c) for c in calls], [["part_a", "part_b"], [], ["part_a", "part_c"]])
        followup = next(s for s in calls[2]["subagents"] if s["description"].startswith("part_a"))
        self.assertEqual((followup["status"], followup["actions"]), ("running", 1), "only its follow-up, and it is busy again")
        self.assertEqual(next(s for s in calls[0]["subagents"] if s["description"].startswith("part_a"))["status"], "done")
        self.assertEqual(sum(1 for a in calls[0]["activity"] if a["kind"] == "session"), 1)
        self.assertEqual(sum(1 for a in calls[2]["activity"] if a["kind"] == "session"), 0, "a resumed call didn't start the session")

    def test_a_top_level_session_that_mentions_a_thread_is_not_its_subagent(self):
        """Codex writes `source` as a string for top-level sessions. One whose first
        line mentions another turn's thread once took the whole dashboard down."""
        self.use("codex")
        self.fake.spawns = [("part_a",)]
        self.call("worker")
        tid = self.runner.state["sessions"]["worker"]
        for name, source in (("exec", "exec"), ("vscode", "vscode"), ("odd", ["not", "an", "object"]), ("none", None)):
            (self.fake.folder() / f"rollout-2026-10-01T12-00-09-{name}.jsonl").write_text(json.dumps(
                {"timestamp": iso(), "type": "session_meta", "payload": {"id": name, "source": source,
                 "base_instructions": {"text": f"earlier work in thread {tid}"}}}) + "\n")
        dashboard._events_cache.clear()
        call = [c for c in dashboard.view(self.root)["calls"] if c["role"] == "worker"][-1]
        self.assertEqual([s["description"].split(" ")[0] for s in call["subagents"]], ["part_a"])

    def test_one_unreadable_turn_does_not_take_the_page_down(self):
        self.use("codex")
        self.call("worker")
        self.call("worker", scope="assignment:2")
        def broken(path, *args, **kwargs):
            if path.name.startswith("0001"):
                raise RuntimeError("corrupt transcript")
            return original(path, *args, **kwargs)
        original = dashboard.parse_events
        dashboard._events_cache.clear()
        with patch.object(dashboard, "parse_events", broken):
            calls = dashboard.view(self.root)["calls"]
        self.assertIn("corrupt transcript", calls[0]["activity"][0]["text"])
        self.assertEqual(calls[1]["result"]["summary"], "Created proof")

    def test_the_dashboard_switch_endpoint_drives_the_next_call(self):
        server = dashboard.Dashboard(("127.0.0.1", 0), self.root)
        self.addCleanup(server.server_close)
        import threading
        import urllib.request
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        url = f"http://127.0.0.1:{server.server_port}/api/backend"
        def post(payload):
            request = urllib.request.Request(url, json.dumps(payload).encode(), {"X-Pair-Token": server.token,
                                             "Content-Type": "application/json"}, method="POST")
            try:
                with urllib.request.urlopen(request) as r:
                    return r.status
            except urllib.error.HTTPError as e:
                return e.code
        self.assertEqual(post({"backend": "codex", "roles": {"orchestrator": "claude"}, "codex_model": "gpt-6-sol"}), 200)
        self.call("worker")
        self.call("orchestrator", scope="batch:mission")
        self.assertEqual([c["backend"] for c in self.fake.calls], ["codex", "claude"])
        self.assertEqual(pair.read_json(self.root / "config.json")["codex"]["model"], "gpt-6-sol")
        self.assertEqual(post({"backend": "gemini"}), 400)
        self.assertEqual(post({"backend": "claude", "roles": {"janitor": "codex"}}), 400)


class LiveToggle(Base):
    def test_toggling_while_a_turn_is_in_flight_finishes_it_where_it_started(self):
        for first, second in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{first} -> {second}"):
                self.setUp()
                self.use(first)
                inner = self.fake
                def flip_mid_turn(argv, prefix, stdin=None, cwd=None, env=None):
                    pair.switch_backend(self.root, second)  # the user clicks the toggle now
                    return inner(argv, prefix, stdin, cwd, env)
                with patch.object(self.runner, "process", flip_mid_turn):
                    data = self.runner.call("worker", "task", scope="assignment:1")
                self.assertEqual(data["summary"], "Created proof", "the turn was read with the parser of the CLI that ran it")
                sid = self.runner.state["sessions"]["worker"]
                self.assertEqual(self.runner.state["session_backends"][sid], first)
                self.call("worker")
                self.assertEqual((self.last()["backend"], self.last()["resumed"]), (second, False))
                self.tearDown()

    def test_the_dashboard_shows_where_each_role_runs_now_and_next(self):
        self.use("claude")
        self.call("worker")
        self.use("codex")
        self.runner.state["inflight"] = {"role": "worker", "backend": "claude", "prefix": "x", "session_id": None,
                                         "reserved_usd": 0, "started_at": 0}
        self.runner.save()
        roles = dashboard.view(self.root)["role_backends"]
        pick = lambda r: {k: roles[r][k] for k in ("set", "next", "session", "running", "auto")}
        self.assertEqual(pick("worker"), {"set": "codex", "next": "codex", "session": "claude", "running": "claude", "auto": None})
        self.assertEqual(pick("director"), {"set": "codex", "next": "codex", "session": None, "running": None, "auto": None})


class Restarts(Base):
    def test_a_restart_request_stops_between_turns_without_a_call(self):
        self.runner.state.update(phase="worker", plan=first_plan(), assignment=1)
        self.runner.save()
        (self.root / "RESTART").touch()
        self.run_steps()
        state = pair.read_json(self.root / "state.json")
        self.assertEqual((state["status"], state["phase"]), ("restarting", "worker"))
        self.assertNotIn("inflight", state)
        self.assertEqual(self.fake.calls, [])
        self.assertEqual(state["coordinator"]["code"], pair.code_digest())
        self.assertIn("restart", state["coordinator"]["features"])

    def test_a_restart_during_a_usage_limit_wait_keeps_the_saved_reset(self):
        resume_at = time.time() + 3600
        self.runner.state.update(resume_at=resume_at, status="waiting")
        self.runner.save()
        (self.root / "RESTART").touch()
        self.run_steps()
        state = pair.read_json(self.root / "state.json")
        self.assertEqual(state["status"], "restarting")
        self.assertEqual(state["resume_at"], resume_at)

    def test_main_reexecs_itself_on_the_code_on_disk(self):
        def fake_run(runner, steps=0, retry=False):
            runner.state.update(status="restarting")
            runner.save()
            return 0
        (self.root / "RESTART").touch()
        with patch.object(pair.Runner, "run", fake_run), patch.object(pair.os, "execv") as execv, \
                patch.object(pair.sys, "argv", ["pair.py", "resume", "--state", str(self.root)]):
            pair.main()
        execv.assert_called_once()
        self.assertEqual(execv.call_args[0][1][-3:], ["resume", "--state", str(self.root)])
        self.assertFalse((self.root / "RESTART").exists())

    def test_without_a_request_main_does_not_reexec(self):
        with patch.object(pair.Runner, "run", lambda runner, steps=0, retry=False: 0), patch.object(pair.os, "execv") as execv, \
                patch.object(pair.sys, "argv", ["pair.py", "resume", "--state", str(self.root)]):
            pair.main()
        execv.assert_not_called()

    def test_the_dashboard_flags_a_coordinator_running_older_code(self):
        with pair.lock(self.root):  # a coordinator holds the lock
            view = dashboard.view(self.root)["coordinator"]
            self.assertEqual((view["outdated"], view["legacy"]), (True, True), "no stamp: started before stamps existed")
            self.runner.state["coordinator"] = {"code": pair.code_digest(), "features": list(pair.FEATURES)}
            self.runner.save()
            self.assertFalse(dashboard.view(self.root)["coordinator"]["outdated"])
            self.runner.state["coordinator"]["code"] = "0" * 16
            self.runner.save()
            view = dashboard.view(self.root)["coordinator"]
            self.assertEqual((view["outdated"], view["legacy"]), (True, False))
        self.assertFalse(dashboard.view(self.root)["coordinator"]["outdated"], "nothing running: Continue uses the files")

    def test_the_restart_endpoint_for_current_and_legacy_coordinators(self):
        server = dashboard.Dashboard(("127.0.0.1", 0), self.root)
        self.addCleanup(server.server_close)
        import threading
        import urllib.request
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        def post(payload):
            request = urllib.request.Request(f"http://127.0.0.1:{server.server_port}/api/restart", json.dumps(payload).encode(),
                                             {"X-Pair-Token": server.token, "Content-Type": "application/json"}, method="POST")
            try:
                with urllib.request.urlopen(request) as r:
                    return r.status, json.load(r)
            except urllib.error.HTTPError as e:
                return e.code, json.load(e)
        self.assertEqual(post({})[0], 409, "nothing to restart")
        with pair.lock(self.root):
            status, body = post({})
            self.assertEqual((status, body.get("legacy")), (409, True), "a legacy coordinator needs an explicit now")
            self.assertFalse((self.root / "STOP").exists())
            self.assertEqual(post({"now": True})[0], 200)
            self.assertTrue((self.root / "STOP").exists() and server.relaunch)
            (self.root / "STOP").unlink()
            server.relaunch = False
            self.runner.state["coordinator"] = {"code": "old", "features": list(pair.FEATURES)}
            self.runner.save()
            self.assertEqual(post({})[0], 200)
            self.assertTrue((self.root / "RESTART").exists())
            self.assertFalse((self.root / "STOP").exists(), "a current coordinator is never stopped mid-turn")


class AutoSwitch(Base):
    def usage(self, backend, level, resets_in=3600, window="five_hour", runner=None):
        runner = runner or self.runner
        runner.state.setdefault("usage", {})[backend] = {"backend": backend, "unifiedWindows": {
            window: {"utilization": level, "resetsAt": time.time() + resets_in}}}
        runner.save()

    def kinds(self):
        return [e["kind"] for e in pair.shared_notebook.entries(self.root)]

    def test_a_role_moves_to_the_other_backend_at_the_switch_point_in_both_directions(self):
        for preferred, other in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{preferred} nearly used up"):
                self.setUp()
                self.use(preferred)
                self.usage(preferred, 0.96)
                self.call("worker")
                self.call("worker")
                self.assertEqual([(c["backend"], c["resumed"]) for c in self.fake.calls], [(other, False), (other, True)],
                                 "it stays on the other backend for the window, keeping its session there")
                self.assertEqual(pair.Runner(self.root).backend("worker"), preferred, "the user's setting is never rewritten")
                self.assertEqual(self.kinds().count("Backend switched automatically"), 1, "journalled once per window")
                self.tearDown()

    def test_no_switch_below_the_point_after_a_reset_when_off_or_without_room(self):
        cases = {
            "below the switch point": lambda: self.usage("claude", 0.94),
            "the window already reset": lambda: self.usage("claude", 0.99, resets_in=-60),
            "auto-switch is off": lambda: (self.usage("claude", 0.99), pair.switch_backend(self.root, auto_switch=False)),
            "the other backend is nearly used up too": lambda: (self.usage("claude", 0.99), self.usage("codex", 0.97)),
            "codex isn't installed": lambda: (self.usage("claude", 0.99), pair.switch_backend(self.root, codex_executable="no-such-codex-binary")),
        }
        for name, arrange in cases.items():
            with self.subTest(name):
                self.setUp()
                self.use("claude")
                arrange()
                self.call("worker")
                self.assertEqual(self.last()["backend"], "claude")
                self.assertNotIn("Backend switched automatically", self.kinds())
                self.tearDown()

    def test_the_switch_point_is_configurable(self):
        self.use("claude")
        pair.switch_backend(self.root, switch_at=0.8)
        self.usage("claude", 0.85)
        self.call("worker")
        self.assertEqual(self.last()["backend"], "codex")
        with self.assertRaises(ValueError):
            pair.switch_backend(self.root, switch_at=1.5)

    def test_roles_return_to_their_own_backend_after_the_reset(self):
        self.use("claude")
        self.usage("claude", 0.97)
        self.call("worker")
        self.usage("claude", 0.97, resets_in=-1)  # the window reset
        self.call("worker")
        self.assertEqual([(c["backend"], c["resumed"]) for c in self.fake.calls], [("codex", False), ("claude", False)])
        self.assertEqual(self.kinds().count("Backend switched back"), 1)
        self.assertNotIn("worker", self.runner.state.get("auto_switched", {}))

    def test_a_usage_limit_continues_on_the_other_backend_instead_of_waiting(self):
        for limited, other in (("claude", "codex"), ("codex", "claude")):
            with self.subTest(f"{limited} limit"):
                self.setUp()
                self.use(limited)
                self.runner.state.update(phase="worker", plan=first_plan(), assignment=1)
                self.runner.save()
                self.fake.script = ["limit"]
                self.run_steps(steps=1)
                state = pair.read_json(self.root / "state.json")
                self.assertEqual([(c["backend"], c["role"]) for c in self.fake.calls], [(limited, "worker"), (other, "worker")])
                self.assertIn("was cut short", self.fake.calls[1]["prompt"])
                self.assertNotIn("resume_at", state, "nothing waited")
                self.assertEqual(state["phase"], "orchestrator", "the worker's report was taken on the other backend")
                self.assertGreater(state["backend_limits"][limited]["until"], time.time())
                self.assertIn("Usage limit: switched backend", self.kinds())
                self.tearDown()

    def test_with_both_backends_out_the_run_pauses_as_before(self):
        self.use("codex")
        self.usage("claude", 0.99, resets_in=86400)
        self.runner.state.update(phase="worker", plan=first_plan(), assignment=1)
        self.runner.save()
        self.fake.script = ["limit"]  # Codex's weekly window runs out
        self.run_steps(steps=1)
        state = pair.read_json(self.root / "state.json")
        self.assertEqual([c["backend"] for c in self.fake.calls], ["codex"])
        self.assertEqual(state["status"], "paused")
        self.assertIn("weekly", state["message"].lower())
        self.assertNotIn("Usage limit: switched backend", self.kinds())

    def test_a_limit_that_follows_the_switch_waits_instead_of_looping(self):
        """However a CLI behaves, a run never switches back and forth forever."""
        self.use("claude")
        self.runner.state.update(phase="worker", plan=first_plan(), assignment=1)
        self.runner.save()
        reset = time.time() + 3 * 86400
        calls = []
        def always_limited(runner_, role, prompt, scope=None):
            calls.append(role)
            raise pair.UsageLimit(reset, "worker stopped", weekly=True, backend="claude")
        with patch.object(pair.Runner, "call", always_limited):
            self.run_steps()
        state = pair.read_json(self.root / "state.json")
        self.assertEqual(len(calls), 2, "one switch, then the old weekly pause")
        self.assertEqual((state["status"], state["weekly_reset_at"]), ("paused", reset))

    def test_the_dashboard_shows_the_switch_and_both_backends_usage(self):
        self.use("claude")
        self.usage("claude", 0.96)
        self.usage("codex", 0.30, window="seven_day", resets_in=86400)
        view = dashboard.view(self.root)
        worker = view["role_backends"]["worker"]
        self.assertEqual((worker["set"], worker["next"], worker["auto"]["from"]), ("claude", "codex", "claude"))
        self.assertAlmostEqual(view["usage"]["backends"]["claude"]["level"], .96)
        self.assertAlmostEqual(view["usage"]["backends"]["codex"]["level"], .30)
        self.assertEqual(view["usage"]["auto_switch"], {"enabled": True, "at": 0.95})


if __name__ == "__main__":
    unittest.main()
