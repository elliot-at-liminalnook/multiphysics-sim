"""Full-control sessions, fresh-session scoping, shell checks, before/after receipts and UI capture."""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import pair
import test_pair as fixtures

HERE = Path(__file__).resolve().parent


class ControlTests(unittest.TestCase):
    def runner(self, tmp, **config):
        root = fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
        if config:
            data = pair.read_json(root / "config.json")
            data.update(config)
            pair.write_json(root / "config.json", data)
        runner = pair.Runner(root)
        runner.deadline = time.monotonic() + 60
        return runner

    def fake_process(self, runner, calls, data=None):
        def process(argv, prefix, stdin=None, cwd=None, env=None):
            sid = argv[argv.index("--resume") + 1] if "--resume" in argv else argv[argv.index("--session-id") + 1]
            calls.append(argv)
            role = prefix.name.split("-", 1)[1]
            out = prefix.with_suffix(".stdout")
            result = data or {"orchestrator": fixtures.plan(), "worker": copy.deepcopy(fixtures.REPORT)}[role]
            pair.write_json(out, {"session_id": sid, "total_cost_usd": runner.state["session_costs"].get(sid, 0) + .1,
                                  "subtype": "success", "structured_output": result})
            return 0, out, prefix.with_suffix(".stderr")
        return process

    def test_agents_run_with_full_control_and_project_context(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            calls = []
            with patch.object(runner, "process", self.fake_process(runner, calls)):
                runner.call("orchestrator", "plan", scope="batch:mission")
            argv = calls[0]
            self.assertIn("--dangerously-skip-permissions", argv)
            for flag in ("--safe-mode", "--tools", "--disable-slash-commands", "--permission-prompts"):
                self.assertNotIn(flag, argv)
            self.assertEqual(argv[argv.index("--setting-sources") + 1], "project")
            instructions = argv[argv.index("--append-system-prompt") + 1]
            self.assertIn("# Project handbook", instructions)
            self.assertIn("ui_capture.py", instructions)
            self.assertIn(str(runner.root / "captures"), instructions)
            self.assertIn("Never drive physical hardware", instructions)

    def test_audit_only_stays_read_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp, audit_only=True)
            calls = []
            with patch.object(runner, "process", self.fake_process(runner, calls)):
                runner.call("worker", "inventory", scope="assignment:1")
            self.assertNotIn("--dangerously-skip-permissions", calls[0])
            self.assertEqual(calls[0][calls[0].index("--tools") + 1], "Read,Glob,Grep")

    def test_sessions_are_fresh_per_scope_and_capped(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp, max_session_calls=2)
            calls = []
            resumed = lambda: "--resume" in calls[-1]
            with patch.object(runner, "process", self.fake_process(runner, calls)):
                runner.call("worker", "task 1", scope="assignment:1")
                self.assertFalse(resumed())
                runner.call("worker", "repair 1", scope="assignment:1")
                self.assertTrue(resumed())
                runner.call("worker", "repair again", scope="assignment:1")
                self.assertFalse(resumed(), "a long session is replaced by a fresh one")
                runner.call("worker", "task 2", scope="assignment:2")
                self.assertFalse(resumed())
            # Each session's cumulative usage is reconciled separately.
            self.assertEqual(len(runner.state["session_costs"]), 3)
            self.assertAlmostEqual(runner.state["cost_usd"], .4)

    def test_run_gives_new_assignments_fresh_workers_and_resumes_repairs(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            data = pair.read_json(runner.root / "config.json")
            data["max_rounds"] = 3
            pair.write_json(runner.root / "config.json", data)
            scopes = []
            reviews = iter(["none", "revise", "accept", "accept"])

            def fake_call(runner, role, prompt, scope=None):
                scopes.append((role, scope))
                runner.state["calls"] += 1
                if role == "worker":
                    return copy.deepcopy(fixtures.REPORT)
                review = next(reviews)
                p = fixtures.plan(review=review)
                if review == "accept" and runner.state["rounds"] >= 2:
                    p.update(action="complete")
                    p["checklist"][0].update(status="verified", evidence="checked")
                return p
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            workers = [s for r, s in scopes if r == "worker"]
            self.assertEqual(workers, ["assignment:1", "assignment:1"])
            self.assertTrue(all(s == "batch:mission" for r, s in scopes if r == "orchestrator"))

    def test_orchestrator_prompt_carries_previous_plan(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            prompts = []

            def fake_call(runner, role, prompt, scope=None):
                prompts.append((role, prompt))
                runner.state["calls"] += 1
                return copy.deepcopy(fixtures.REPORT) if role == "worker" else fixtures.plan(
                    review="accept" if runner.state.get("report") else "none")
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            review_prompt = [p for r, p in prompts if r == "orchestrator"][1]
            self.assertIn("Your previous plan", review_prompt)
            self.assertIn(fixtures.plan()["worker_prompt"], review_prompt)
            self.assertIn("diffstat", review_prompt)

    def verify(self, runner, checks):
        p = fixtures.plan()
        p["checks"] = checks
        runner.state.update(plan=p, rounds=1)
        runner.verify()
        return {r["name"]: r for r in runner.state["receipts"]}

    def test_shell_checks_get_cargo_path_and_run_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            receipts = self.verify(runner, ['test -d "$PAIR_CAPTURES" && test "$PWD" = "$PAIR_WORKSPACE" && echo "$PATH"'])
            receipt = next(r for n, r in receipts.items() if n != "diff")
            self.assertEqual(receipt["exit_code"], 0, Path(receipt["stderr"]).read_text())
            self.assertEqual(receipt["command"][:2], ["/bin/bash", "-c"])
            if (Path.home() / ".cargo/bin").is_dir():
                self.assertIn(".cargo/bin", Path(receipt["stdout"]).read_text())

    def test_checks_run_before_the_assignment_and_only_preexisting_failures_are_waivable(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            preexisting, new = "test -f never-existed.txt", "! grep -q bad tracked.txt"
            p = fixtures.plan()
            p["checks"] = [preexisting, new]
            runner.state.update(plan=p, rounds=0)
            runner.precheck()
            self.assertEqual(set(runner.state["prechecks"]), {preexisting, new})
            (runner.repo / "tracked.txt").write_text("bad\n")  # the worker's edit
            runner.state["rounds"] = 1
            runner.verify()
            receipts = {r["name"]: r for r in runner.state["receipts"]}
            self.assertNotEqual(receipts[preexisting]["before"]["exit_code"], 0)
            self.assertEqual(receipts[new]["before"]["exit_code"], 0)

            state = {"plan": p, "report": fixtures.REPORT, "receipts": list(receipts.values())}
            review = fixtures.plan(review="accept")
            review["waived_checks"] = [preexisting, new]
            with self.assertRaisesRegex(ValueError, "grep"):
                pair.guard_plan(review, state, runner.config["checks"])
            (runner.repo / "tracked.txt").write_text("fixed\n")
            runner.verify()
            state["receipts"] = runner.state["receipts"]
            review["waived_checks"] = []
            with self.assertRaisesRegex(ValueError, "never-existed"):
                pair.guard_plan(review, state, runner.config["checks"])
            review["waived_checks"] = [preexisting]
            pair.guard_plan(review, state, runner.config["checks"])
            self.assertFalse((runner.root / "baseline").exists(), "no second checkout")

    def test_new_assignment_runs_prechecks_before_the_worker_when_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp, precheck=True)
            order = []

            def fake_call(runner, role, prompt, scope=None):
                order.append(role)
                runner.state["calls"] += 1
                if role == "worker":
                    order.append("prechecks:" + ",".join(runner.state["prechecks"]))
                    return copy.deepcopy(fixtures.REPORT)
                p = fixtures.plan(review="accept" if runner.state.get("report") else "none")
                p["checks"] = ["true"]
                return p
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            self.assertEqual(order[:3], ["orchestrator", "worker", "prechecks:true"])
            self.assertTrue(list((runner.root / "logs").glob("check-0000-true-before.stdout")))

    def test_history_marks_preexisting_failures_without_rerunning_anything(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            old, new = "test -f never-existed.txt", "! grep -q bad tracked.txt"
            p = fixtures.plan()
            p["checks"] = [old, new]
            runner.state.update(plan=p, rounds=1, assignment=1)
            runner.verify()  # assignment 1: the old check already fails
            self.assertNotIn("before", next(r for r in runner.state["receipts"] if r["name"] == old))
            runner.state.update(rounds=2, assignment=2)
            (runner.repo / "tracked.txt").write_text("bad\n")
            runner.verify()
            receipts = {r["name"]: r for r in runner.state["receipts"]}
            self.assertEqual(receipts[old]["before"]["assignment"], 1)
            self.assertNotEqual(receipts[old]["before"]["exit_code"], 0)
            self.assertNotIn("before", receipts[new], "skipped in assignment 1, so there is no earlier result to excuse it")
            self.assertFalse(list((runner.root / "logs").glob("*-before.*")), "no pre-runs by default")

    def test_checks_run_fastest_first_and_stop_at_the_first_new_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            slow, fast, broken = "sleep 0.3; echo slow", "echo fast", "false"
            runner.state["check_history"] = {slow: {"seconds": 240}, fast: {"seconds": 1}, broken: {"seconds": 5}}
            p = fixtures.plan()
            p["checks"] = [slow, broken, fast]
            runner.state.update(plan=p, rounds=1)
            runner.verify()
            receipts = {r["name"]: r for r in runner.state["receipts"]}
            self.assertEqual([r["name"] for r in runner.state["receipts"]], [slow, broken, fast], "plan order kept")
            self.assertEqual(receipts[fast]["exit_code"], 0)
            self.assertNotEqual(receipts[broken]["exit_code"], 0)
            self.assertTrue(receipts[slow]["skipped"], "the slow check is skipped after the new failure")
            self.assertIsNone(receipts[slow]["exit_code"])
            self.assertIsInstance(receipts[fast]["seconds"], float)
            review = fixtures.plan(review="accept")
            with self.assertRaises(ValueError):
                pair.guard_plan(review, {"plan": p, "report": fixtures.REPORT, "receipts": runner.state["receipts"]}, {})

    def test_without_requested_checks_the_worker_result_goes_straight_to_review(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            phases = []

            def fake_call(runner_, role, prompt, scope=None):
                phases.append(runner_.state["phase"])
                runner_.state["calls"] += 1
                if role == "worker":
                    return copy.deepcopy(fixtures.REPORT)
                p = fixtures.plan("complete" if runner_.state.get("report") else "work",
                                  "accept" if runner_.state.get("report") else "none")
                p["checks"] = []
                if runner_.state.get("report"):
                    p["checklist"][0].update(status="verified", evidence="worker ran cargo test -p x")
                return p
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            state = pair.read_json(runner.root / "state.json")
            self.assertEqual(state["status"], "complete")
            self.assertEqual(phases, ["orchestrator", "worker", "orchestrator"])
            self.assertFalse(list((runner.root / "logs").glob("check-*")), "no coordinator checks ran")

    def test_baseline_is_pinned_and_evidence_summarizes_changes(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
            ref = pair.git(runner.repo, "rev-parse", runner.config["baseline_ref"]).decode().strip()
            self.assertEqual(ref, runner.config["baseline"])
            (runner.repo / "tracked.txt").write_text("changed\n")
            (runner.repo / "brand-new.txt").write_text("new\n")
            (runner.root / "captures").mkdir(exist_ok=True)
            (runner.root / "captures/shot.png").write_bytes(b"\x89PNG")
            evidence = runner.evidence()
            self.assertIn("tracked.txt", evidence["diffstat"])
            self.assertIn("brand-new.txt", evidence["untracked"])
            self.assertEqual(evidence["recent_captures"], [str(runner.root / "captures/shot.png")])


class UsageLimitTests(unittest.TestCase):
    def test_reset_time_comes_from_the_stream_or_the_message(self):
        now = 1_790_000_000.0
        rejected = {"status": "rejected", "resetsAt": now + 3600, "rateLimitType": "five_hour"}
        self.assertEqual(pair.limit_reset(rejected, "", now), now + 3600)
        weekly = {"status": "allowed", "unifiedWindows": {"seven_day": {"utilization": 1.0, "resetsAt": now + 86400}}}
        self.assertEqual(pair.limit_reset(weekly, "You have reached your weekly usage limit", now), now + 86400)
        self.assertEqual(pair.limit_reset(None, "Claude AI usage limit reached|%d" % (now + 50), now), now + 50)
        self.assertEqual(pair.limit_reset(None, "5-hour limit reached · resets in 2 hours", now), now + 7200)
        self.assertEqual(pair.limit_reset(None, "usage limit reached", now, fallback_seconds=900), now + 900)
        self.assertIsNone(pair.limit_reset({"status": "allowed", "resetsAt": now + 60}, "error[E0425]: cannot find value", now))

    def stream(self, sid, status, cost, output=None, error=False):
        events = [{"type": "system", "subtype": "init", "session_id": sid},
                  {"type": "rate_limit_event", "rate_limit_info": {"status": status, "resetsAt": time.time() + 0.5}}]
        if status == "allowed":
            events.append({"type": "assistant", "message": {"content": [{"type": "text", "text": "working"}]}})
        else:
            events.append({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Read", "input": {"file_path": "a.rs"}}]}})
        events.append({"type": "result", "subtype": "error_during_execution" if error else "success", "is_error": error,
                       "result": "usage limit reached" if error else "", "session_id": sid, "total_cost_usd": cost,
                       "structured_output": output})
        return "\n".join(json.dumps(e) for e in events)

    def test_limited_call_waits_then_resumes_the_same_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp, limit_margin_seconds=0)
            calls, reported, totals = [], [], {}

            def process(argv, prefix, stdin=None, cwd=None, env=None):
                if "--print" not in argv:  # a coordinator check
                    prefix.with_suffix(".stdout").write_text("")
                    prefix.with_suffix(".stderr").write_text("")
                    return 0, prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
                sid = argv[argv.index("--resume") + 1] if "--resume" in argv else argv[argv.index("--session-id") + 1]
                calls.append((argv, stdin, sid))
                out = prefix.with_suffix(".stdout")
                role = prefix.name.split("-", 1)[1]
                if role == "worker" and len([c for c in calls if "--resume" not in c[0] and c[1].startswith(fixtures.plan()["worker_prompt"])]) == 1 and "--resume" not in argv:
                    out.write_text(self.stream(sid, "rejected", .05, error=True))
                    return 1, out, prefix.with_suffix(".stderr")
                if role == "worker":
                    data = copy.deepcopy(fixtures.REPORT)
                    reported.append(True)
                elif reported:
                    data = fixtures.plan("complete", "accept")
                    data["checklist"][0].update(status="verified", evidence="checked")
                else:
                    data = fixtures.plan()
                out.write_text(self.stream(sid, "allowed", totals.setdefault(sid, 0) + .1, data))
                totals[sid] += .1
                return 0, out, prefix.with_suffix(".stderr")
            with patch.object(pair.Runner, "process", lambda self_, *a, **k: process(*a, **k)):
                pair.Runner(runner.root).run()
            workers = [(a, stdin, sid) for a, stdin, sid in calls if stdin.startswith(fixtures.plan()["worker_prompt"]) or stdin.startswith(pair.RESUME_NOTE)]
            self.assertEqual(len(workers), 2)
            self.assertNotIn("--resume", workers[0][0])
            self.assertIn("--resume", workers[1][0])
            self.assertEqual(workers[0][2], workers[1][2], "the interrupted session continues")
            self.assertTrue(workers[1][1].startswith(pair.RESUME_NOTE))
            state = pair.read_json(runner.root / "state.json")
            self.assertEqual(state["status"], "complete")
            self.assertNotIn("resume_at", state)
            self.assertEqual(state["rounds"], 1, "the limited attempt is not a worker turn")
            self.assertIn("Usage limit", [e["kind"] for e in __import__("shared_notebook").entries(runner.root)])

    def test_a_restarted_coordinator_waits_out_a_saved_reset(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp, limit_margin_seconds=0)
            runner.state.update(status="waiting", resume_at=time.time() + 2.0)
            runner.save()
            seen = []

            def fake_call(runner_, role, prompt, scope=None):
                seen.append(time.time())
                runner_.state["calls"] += 1
                return fixtures.plan("blocked")
            begun = time.time()
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            self.assertGreaterEqual(seen[0] - begun, 1.9)
            self.assertLess(pair.read_json(runner.root / "state.json")["elapsed_seconds"], 1.0,
                            "waiting for a limit does not use the run's active hours")


class HandoffRuleTests(unittest.TestCase):
    def test_a_done_report_with_blockers_for_later_work_can_be_accepted(self):
        report = dict(fixtures.REPORT, blockers=["T11.2 needs a decision about the stored controller hash"])
        state = {"plan": fixtures.plan(), "report": report, "receipts": [{"name": "diff", "exit_code": 0}]}
        pair.guard_plan(fixtures.plan(review="accept"), state, {})
        state["report"] = dict(report, status="blocked")
        with self.assertRaisesRegex(ValueError, "blocked"):
            pair.guard_plan(fixtures.plan(review="accept"), state, {})

    def test_a_rejected_response_goes_back_to_the_same_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp)
            calls, responses = [], iter([fixtures.plan(review="accept"), fixtures.plan()])

            def process(argv, prefix, stdin=None, cwd=None, env=None):
                sid = argv[argv.index("--resume") + 1] if "--resume" in argv else argv[argv.index("--session-id") + 1]
                calls.append((argv, stdin, sid))
                out = prefix.with_suffix(".stdout")
                pair.write_json(out, {"session_id": sid, "total_cost_usd": .1 * len(calls), "subtype": "success",
                                      "structured_output": next(responses)})
                return 0, out, prefix.with_suffix(".stderr")
            with patch.object(runner, "process", process):
                plan = runner.call_checked("orchestrator", "Plan the work", "batch:mission",
                                           lambda p: pair.guard_plan(p, runner.state, {}))
            self.assertEqual(plan["review"], "none")
            self.assertEqual(len(calls), 2)
            self.assertIn("--resume", calls[1][0])
            self.assertEqual(calls[0][2], calls[1][2])
            self.assertTrue(calls[1][1].startswith("THE COORDINATOR REJECTED YOUR LAST RESPONSE: Cannot review"))
            self.assertIn("Response rejected", [e["kind"] for e in __import__("shared_notebook").entries(runner.root)])

    def test_repeated_violations_still_stop_the_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp, guard_retries=1)
            calls = []
            with patch.object(runner, "process", ControlTests().fake_process(runner, calls, fixtures.plan(review="accept"))):
                with self.assertRaisesRegex(ValueError, "Cannot review"):
                    runner.call_checked("orchestrator", "Plan", "batch:mission", lambda p: pair.guard_plan(p, runner.state, {}))
            self.assertEqual(len(calls), 2)


class NoLimitTests(unittest.TestCase):
    def test_init_defaults_to_no_limits_and_a_director(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = fixtures.PairTests().setup_repo(Path(tmp).resolve())
            args = argparse.Namespace(repo=str(repo), state=None, fresh=False, claude="python3", model=None, audit_only=False,
                                      max_rounds=None, max_hours=None, turn_minutes=None, max_turns=None,
                                      budget_usd=None, call_budget_usd=None, no_director=False)
            pair.initialize(args)
            root = repo / ".claude-pair"
            config = pair.read_json(root / "config.json")
            for key in ("max_rounds", "max_hours", "turn_minutes", "max_turns", "budget_usd", "call_budget_usd"):
                self.assertIsNone(config[key], key)
            self.assertEqual(pair.read_json(root / "outer-settings.json"), {"enabled": True, "max_batches": None})

    def test_unlimited_calls_pass_no_caps_and_runs_do_not_stop_on_turns(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp, max_rounds=None, max_hours=None, turn_minutes=None, max_turns=None,
                                           budget_usd=None, call_budget_usd=None)
            calls = []
            with patch.object(runner, "process", ControlTests().fake_process(runner, calls)):
                runner.call("worker", "task", scope="assignment:1")
            self.assertNotIn("--max-budget-usd", calls[0])
            self.assertNotIn("--max-turns", calls[0])
            self.assertAlmostEqual(runner.state["cost_usd"], .1)
            reviews = iter(range(100))

            def fake_call(runner_, role, prompt, scope=None):
                runner_.state["calls"] += 1
                if role == "worker":
                    return copy.deepcopy(fixtures.REPORT)
                n = next(reviews)
                if n == 6:
                    p = fixtures.plan("complete", "accept")
                    p["checklist"][0].update(status="verified", evidence="done")
                    return p
                return fixtures.plan(review="accept" if n else "none")
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            state = pair.read_json(runner.root / "state.json")
            self.assertEqual(state["rounds"], 6)
            self.assertEqual(state["status"], "complete")

    def test_weekly_limit_stops_the_run_instead_of_waiting_days(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = ControlTests().runner(tmp)
            reset = time.time() + 3 * 86400

            def fake_call(runner_, role, prompt, scope=None):
                raise pair.UsageLimit(reset, "worker stopped", weekly=True)
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(runner.root).run()
            state = pair.read_json(runner.root / "state.json")
            self.assertEqual(state["status"], "paused")
            self.assertEqual(state["weekly_reset_at"], reset)
            self.assertIn("weekly", state["message"])
            now = time.time()
            self.assertTrue(pair.weekly_limit({"rateLimitType": "seven_day"}, "", now + 60, now))
            self.assertTrue(pair.weekly_limit(None, "", now + 2 * 86400, now))
            self.assertFalse(pair.weekly_limit({"rateLimitType": "five_hour", "unifiedWindows": {"seven_day": {"utilization": .4}}}, "", now + 3600, now))


FAKE_VIEWER = r'''
import http.server, json, sys, threading
port = int(sys.argv[sys.argv.index("--api-port") + 1])
jobs = {}
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def reply(self, code, value):
        body = json.dumps(value).encode()
        self.send_response(code); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        if self.path == "/v1/capabilities": return self.reply(200, {"commands": []})
        self.reply(200, jobs[int(self.path.rsplit("/", 1)[1])])
    def do_POST(self):
        batch = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        results = []
        for c in batch["commands"]:
            if c["command"] == "screenshot":
                open(c["args"]["path"], "wb").write(b"\x89PNG\r\n\x1a\nfake")
            results.append({"command": c["command"], "ok": c["command"] != "explode"})
        jobs[len(jobs) + 1] = {"status": "failed" if not all(r["ok"] for r in results) else "succeeded", "results": results}
        self.reply(202, {"url": f"/v1/jobs/{len(jobs)}"})
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
'''


class CaptureTests(unittest.TestCase):
    def capture(self, tmp, steps):
        tmp = str(Path(tmp).resolve())
        viewer = Path(tmp) / "viewer"
        viewer.write_text("#!" + sys.executable + "\n" + FAKE_VIEWER)
        viewer.chmod(0o755)
        out = Path(tmp) / "out"
        result = subprocess.run([sys.executable, str(HERE / "ui_capture.py"), "--out", str(out), "--binary", str(viewer),
                                 "--settle", "0", "--steps", json.dumps(steps), "--", "--system", "x.json"],
                                capture_output=True, text=True, timeout=60)
        return result, out, json.loads((out / "capture.json").read_text())

    def test_script_runs_commands_and_saves_screenshots(self):
        with tempfile.TemporaryDirectory() as tmp:
            result, out, receipt = self.capture(tmp, [{"command": "fit"}, {"screenshot": "after-fit"}])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(receipt["ok"])
            self.assertEqual(receipt["screenshots"], [str(out / "after-fit.png")])
            self.assertEqual(receipt["argv"][-2:], ["--system", "x.json"])

    def test_failed_command_fails_the_check_and_stops_the_viewer(self):
        with tempfile.TemporaryDirectory() as tmp:
            result, out, receipt = self.capture(tmp, [{"command": "explode"}])
            self.assertEqual(result.returncode, 1)
            self.assertFalse(receipt["ok"])
            self.assertEqual(receipt["screenshots"], [])
            pid_free = subprocess.run(["pgrep", "-f", str(Path(tmp) / "viewer")], capture_output=True)
            self.assertNotEqual(pid_free.returncode, 0, "viewer process must be stopped")


if __name__ == "__main__":
    unittest.main()
