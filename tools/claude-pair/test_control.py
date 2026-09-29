"""Full-control sessions, fresh-session scoping, shell checks, before/after receipts and UI capture."""
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

    def test_new_assignment_runs_prechecks_before_the_worker(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = self.runner(tmp)
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
