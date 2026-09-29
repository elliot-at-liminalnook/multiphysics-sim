import json
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from unittest.mock import patch

import dashboard
import pair
import test_pair as fixtures
from test_pair import plan, REPORT


class DashboardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = fixtures.PairTests().initialize_fixture(Path(self.temp.name).resolve())
        self.server = dashboard.Dashboard(("127.0.0.1", 0), self.root)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.temp.cleanup()

    def post(self, route, data, token=True, origin=None):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["X-Pair-Token"] = self.server.token
        if origin:
            headers["Origin"] = origin
        request = Request(self.url + "/api/" + route, data=json.dumps(data).encode(), headers=headers)
        try:
            with urlopen(request) as response:
                return response.status, json.load(response)
        except HTTPError as e:
            return e.code, json.load(e)

    def test_host_origin_and_token_protect_controls(self):
        self.assertEqual(self.post("stop", {}, token=False)[0], 403)
        self.assertEqual(self.post("stop", {}, origin="https://other.example")[0], 403)
        self.assertFalse((self.root / "STOP").exists())
        self.assertEqual(self.post("stop", {})[0], 200)
        self.assertTrue((self.root / "STOP").exists())
        with self.assertRaises(HTTPError) as error:
            urlopen(Request(self.url + "/api/state", headers={"Host": "other.example"}))
        self.assertEqual(error.exception.code, 403)

    def test_state_streaming_and_saved_direction(self):
        (self.root / "logs/0001-worker.prompt.md").write_text("Exact worker prompt")
        events = [{"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Read", "input": {"file_path": "src/main.rs"}}]}},
                  {"type": "result", "subtype": "success", "structured_output": REPORT}]
        (self.root / "logs/0001-worker.stdout").write_text("\n".join(json.dumps(x) for x in events))
        code, _ = self.post("steering", {"text": "Prioritize the builder"})
        self.assertEqual(code, 200)
        with urlopen(self.url + "/api/state") as response:
            view = json.load(response)
        self.assertEqual(view["calls"][0]["prompt"], "Exact worker prompt")
        self.assertEqual(view["calls"][0]["activity"][0]["text"], "Read: src/main.rs")
        self.assertEqual(view["calls"][0]["result"], REPORT)
        self.assertTrue(view["steering_pending"])
        self.assertEqual(view["steering"]["text"], "Prioritize the builder")

    def test_limits_reject_invalid_values_and_active_edits(self):
        limits = dashboard.view(self.root)["limits"]
        limits["max_rounds"] = -1
        self.assertEqual(self.post("limits", limits)[0], 400)
        limits["max_rounds"] = 4
        with pair.lock(self.root):
            self.assertEqual(self.post("limits", limits)[0], 409)
        self.assertEqual(self.post("limits", limits)[0], 200)
        self.assertEqual(pair.read_json(self.root / "config.json")["max_rounds"], 4)

    def test_start_uses_saved_runner_and_rejects_duplicate(self):
        with patch.object(dashboard.subprocess, "Popen") as popen:
            popen.return_value.poll.return_value = None
            self.assertEqual(self.post("start", {})[0], 200)
            argv = popen.call_args.args[0]
            self.assertIn("resume", argv)
            self.assertEqual(argv[-1], str(self.root))
            self.assertEqual(self.post("start", {})[0], 409)

    def test_outer_settings_and_director_history_are_visible(self):
        self.assertEqual(self.post("outer", {"enabled": True, "max_batches": 3})[0], 200)
        self.assertEqual(self.post("outer", {"enabled": True, "max_batches": 0})[0], 400)
        (self.root / "logs/0001-director.prompt.md").write_text("Compare next opportunities")
        view = dashboard.view(self.root)
        self.assertTrue(view["outer_settings"]["enabled"])
        self.assertEqual(view["outer_settings"]["max_batches"], 3)
        self.assertEqual(view["calls"][0]["role"], "director")
        self.assertIn("concrete consumer", view["roles"]["director"])
        self.assertEqual(view["state"]["phase"], "director")

    def test_workflow_paused_time_freezes_and_assets_are_served(self):
        (self.root / "logs/0001-worker.prompt.md").write_text("Write a focused test")
        (self.root / "logs/0001-worker.stdout").write_text('{"type":"assistant","message":{"content":[{"type":"text","text":"Checking code"}]}}\n')
        runner = pair.Runner(self.root)
        runner.state.update(status="paused", phase="worker", inflight={"role":"worker", "prefix":str(self.root / "logs/0001-worker")})
        runner.save()
        with patch.object(dashboard.time, "time", return_value=1000):
            first = dashboard.view(self.root)
        with patch.object(dashboard.time, "time", return_value=5000):
            second = dashboard.view(self.root)
        self.assertEqual(first["calls"][0]["elapsed_seconds"], second["calls"][0]["elapsed_seconds"])
        self.assertFalse(second["calls"][0]["live"])
        self.assertEqual(second["workflow"]["current"], "worker")
        for asset in ("workflow.js", "workflow.css"):
            with urlopen(self.url + "/" + asset) as response:
                self.assertEqual(response.status, 200)
        runner.state.update(phase="verify", plan=plan(), rounds=1)
        runner.save()
        (self.root / "logs/check-0001-diff.stdout").write_text("Partial check output")
        checks = dashboard.view(self.root)["workflow"]["nodes"][3]
        self.assertEqual(checks["live_output"][0]["text"], "Partial check output")
        self.assertEqual(checks["checks"], [])

    def test_shared_notebook_visible_without_mutating_history(self):
        import shared_notebook as book
        runner = pair.Runner(self.root)
        book.setup(self.root, runner.state, runner.config)
        book.append(self.root, {"id":"note-example", "author":"worker", "kind":"Agent report / proposal", "summary":"A finding", "notes":["Orchestrator: inspect the evidence"], "source":"test"})
        before = (self.root / "shared/journal.jsonl").read_bytes()
        view = dashboard.view(self.root)
        self.assertEqual(view["notebook"]["entries"][-1]["notes"], ["Orchestrator: inspect the evidence"])
        with urlopen(self.url + "/api/journal") as response:
            self.assertIn("note-example", response.read().decode())
        for asset in ("notebook.js", "notebook.css"):
            with urlopen(self.url + "/" + asset) as response:
                self.assertEqual(response.status, 200)
        self.assertEqual(before, (self.root / "shared/journal.jsonl").read_bytes())

    def test_new_direction_replans_before_queued_worker(self):
        runner = pair.Runner(self.root)
        runner.state.update(phase="worker", plan=plan(), report=REPORT)
        runner.save()
        self.post("steering", {"text": "Start with the builder"})
        calls = []
        def fake_call(runner, role, prompt, scope=None):
            calls.append((role, prompt))
            return plan()
        with patch.object(pair.Runner, "call", fake_call):
            pair.Runner(self.root).run(steps=1)
        self.assertEqual(calls[0][0], "orchestrator")
        self.assertIn("Start with the builder", calls[0][1])
        self.assertIn("Reconsider", calls[0][1])
        self.assertFalse(dashboard.view(self.root)["steering_pending"])


if __name__ == "__main__":
    unittest.main()
