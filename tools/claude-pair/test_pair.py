import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import pair


def plan(action="work", review="none"):
    return {"coordination_notes": [], "action": action, "review": review, "summary": "Do one feature",
            "worker_prompt": "Create proof.txt with the assigned exact content.",
            "acceptance_criteria": ["Proof exists with exact contents"], "checks": ["diff"],
            "checklist": [{"id": "one", "workflow": "One native workflow", "status": "pending", "evidence": ""}]}


REPORT = {"coordination_notes": [], "status": "done", "summary": "Created proof", "changed_files": ["proof.txt"],
          "checks": [], "evidence": ["proof.txt"], "blockers": []}


class PairTests(unittest.TestCase):
    def test_reject_accept_without_independent_receipts(self):
        state = {"plan": plan(), "report": REPORT}
        with self.assertRaisesRegex(ValueError, "independent checks"):
            pair.guard_plan(plan(review="accept"), state, {"diff": []})
        state["receipts"] = [{"name": "diff", "exit_code": 1}]
        with self.assertRaises(ValueError):
            pair.guard_plan(plan(review="accept"), state, {"diff": []})

    def test_checklist_cannot_be_dropped_or_falsely_completed(self):
        p = plan("complete", "accept")
        state = {"plan": plan(), "report": REPORT, "receipts": [{"name": "diff", "exit_code": 0}]}
        with self.assertRaises(ValueError):
            pair.guard_plan(p, state, {"diff": []})
        p["checklist"][0].update(status="verified", evidence="Read proof.txt")
        pair.guard_plan(p, state, {"diff": []})
        p["checklist"][0]["id"] = "replacement"
        with self.assertRaisesRegex(ValueError, "drop"):
            pair.guard_plan(p, state, {"diff": []})

    def test_unknown_checks_and_empty_prompt(self):
        p = plan()
        p["checks"] = ["arbitrary shell command"]
        with self.assertRaises(ValueError):
            pair.guard_plan(p, {}, {"diff": []})
        p["checks"] = ["diff"]
        p["worker_prompt"] = ""
        with self.assertRaises(ValueError):
            pair.guard_plan(p, {}, {"diff": []})

    def setup_repo(self, root):
        repo = root / "source"
        repo.mkdir()
        pair.git(repo, "init")
        pair.git(repo, "config", "user.name", "Test")
        pair.git(repo, "config", "user.email", "test@localhost")
        (repo / "tracked.txt").write_text("original\n")
        (repo / "deleted.txt").write_text("delete later\n")
        (repo / ".gitignore").write_text("ignored.txt\n")
        pair.git(repo, "add", ".")
        pair.git(repo, "commit", "-m", "fixture")
        (repo / "tracked.txt").write_text("staged\n")
        pair.git(repo, "add", "tracked.txt")
        (repo / "tracked.txt").write_text("unstaged\n")
        (repo / "new.txt").write_text("untracked\n")
        (repo / "ignored.txt").write_text("not copied\n")
        (repo / "deleted.txt").unlink()
        (repo / "internal-link").symlink_to(repo / "tracked.txt")
        return repo

    def test_snapshot_preserves_source_index_and_working_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            repo = self.setup_repo(root)
            index = (repo / ".git/index").read_bytes()
            status = pair.git(repo, "status", "--porcelain")
            dest = root / "snapshot"
            receipt = pair.snapshot(repo, dest)
            self.assertEqual((dest / "tracked.txt").read_text(), "unstaged\n")
            self.assertEqual((dest / "new.txt").read_text(), "untracked\n")
            self.assertFalse((dest / "deleted.txt").exists())
            self.assertFalse((dest / "ignored.txt").exists())
            self.assertEqual((dest / "internal-link").resolve(), dest / "tracked.txt")
            self.assertEqual(pair.git(dest, "status", "--porcelain"), b"")
            self.assertEqual((repo / ".git/index").read_bytes(), index)
            self.assertEqual(pair.git(repo, "status", "--porcelain"), status)
            self.assertEqual(pair.git(dest, "rev-parse", "HEAD").decode().strip(), receipt["baseline"])
            (dest / "tracked.txt").write_text("worker edit\n")
            self.assertEqual((repo / "tracked.txt").read_text(), "unstaged\n")

    def test_external_symlink_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            repo = self.setup_repo(root)
            (repo / "escape").symlink_to(root / "outside")
            with self.assertRaisesRegex(RuntimeError, "External symlink"):
                pair.snapshot(repo, root / "snapshot")

    def initialize_fixture(self, root):
        repo = self.setup_repo(root)
        args = argparse.Namespace(repo=str(repo), state=str(root / "run"), claude="python3",
             model=None, audit_only=False, max_rounds=1, max_hours=1, turn_minutes=1,
             max_turns=10, budget_usd=10, call_budget_usd=1)
        pair.initialize(args)
        return root / "run"

    def test_end_to_end_phase_resume_worker_prompt_and_completion(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            seen = []
            def fake_call(runner, role, prompt):
                seen.append((role, prompt))
                runner.state["calls"] += 1
                if role == "worker":
                    (runner.repo / "proof.txt").write_text("proof\n")
                    return copy.deepcopy(REPORT)
                p = plan()
                if runner.state.get("report"):
                    p.update(action="complete", review="accept")
                    p["checklist"][0].update(status="verified", evidence="Read proof.txt")
                return p
            with patch.object(pair.Runner, "call", fake_call):
                self.assertEqual(pair.Runner(root).run(steps=1), 0)
                self.assertEqual(pair.read_json(root / "state.json")["phase"], "worker")
                self.assertEqual(pair.Runner(root).run(), 0)
            self.assertEqual([role for role, _ in seen], ["orchestrator", "worker", "orchestrator"])
            self.assertTrue(seen[1][1].startswith(plan()["worker_prompt"]))
            self.assertEqual(pair.read_json(root / "state.json")["status"], "complete")
            self.assertTrue((root / "logs/check-0001-diff.stdout").exists())

    def test_committed_worker_changes_remain_in_review_and_checks(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            runner = pair.Runner(root)
            source_head = pair.git(Path(runner.config["repo"]), "rev-parse", "HEAD")
            (runner.repo / "proof.txt").write_text("proof with trailing whitespace  \n")
            pair.git(runner.repo, "add", "proof.txt")
            pair.git(runner.repo, "-c", "user.name=Test Worker", "-c", "user.email=worker@localhost", "commit", "-m", "Small local task checkpoint")
            self.assertEqual(pair.git(runner.repo, "status", "--porcelain"), b"")
            runner.state.update(plan=plan(), rounds=1)
            runner.deadline = time.monotonic() + 30
            runner.verify()
            self.assertNotEqual(runner.state["receipts"][0]["exit_code"], 0)
            evidence = runner.evidence()
            self.assertIn("Small local task checkpoint", evidence["local_commits"])
            self.assertIn("proof with trailing whitespace", Path(evidence["diff_file"]).read_text())
            self.assertEqual(pair.git(Path(runner.config["repo"]), "rev-parse", "HEAD"), source_head)
            self.assertFalse((Path(runner.config["repo"]) / "proof.txt").exists())

    def test_round_limit_allows_review_then_stops_before_next_worker(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            def fake_call(runner, role, prompt):
                runner.state["calls"] += 1
                if role == "worker":
                    return copy.deepcopy(REPORT)
                return plan(review="accept" if runner.state.get("report") else "none")
            with patch.object(pair.Runner, "call", fake_call):
                pair.Runner(root).run()
            state = pair.read_json(root / "state.json")
            self.assertEqual(state["rounds"], 1)
            self.assertEqual(state["calls"], 3)
            self.assertEqual(state["status"], "paused")

    def test_stop_and_lock(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            (root / "STOP").touch()
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                pair.Runner(root).run()
            with pair.lock(root):
                with self.assertRaisesRegex(RuntimeError, "already running"):
                    with pair.lock(root):
                        pass

    def test_cost_delta_on_resumed_sessions_and_error_reservation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            runner = pair.Runner(root)
            def process(argv, prefix, stdin=None):
                out = prefix.with_suffix(".stdout")
                previous = runner.state["session_costs"].get("orchestrator", 0)
                pair.write_json(out, {"session_id": argv[argv.index("--resume") + 1] if "--resume" in argv else argv[argv.index("--session-id") + 1],
                    "total_cost_usd": previous + .25, "subtype": "success", "structured_output": plan()})
                return 0, out, prefix.with_suffix(".stderr")
            with patch.object(runner, "process", process):
                runner.call("orchestrator", "test")
                sid = runner.state["sessions"]["orchestrator"]
                runner.call("orchestrator", "test")
                self.assertEqual(runner.state["sessions"]["orchestrator"], sid)
                self.assertAlmostEqual(runner.state["cost_usd"], .5)
            with patch.object(runner, "process", side_effect=RuntimeError("crash")):
                with self.assertRaises(RuntimeError):
                    runner.call("worker", "test")
            self.assertAlmostEqual(runner.state["cost_usd"], 1.5)
            self.assertIn("inflight", pair.read_json(root / "state.json"))
            with self.assertRaisesRegex(RuntimeError, "uncertain"):
                pair.Runner(root).run()

    def test_process_timeout_preserves_partial_logs_and_reaps_child(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.initialize_fixture(Path(tmp).resolve())
            runner = pair.Runner(root)
            runner.config["turn_minutes"] = .005
            runner.deadline = time.monotonic() + 10
            prefix = root / "logs" / "timeout"
            with self.assertRaises(InterruptedError):
                runner.process(["python3", "-u", "-c", "import time; print('started'); time.sleep(20)"], prefix)
            self.assertIn("started", prefix.with_suffix(".stdout").read_text())
            self.assertNotIn("child_pid", pair.read_json(root / "state.json"))


if __name__ == "__main__":
    unittest.main()
