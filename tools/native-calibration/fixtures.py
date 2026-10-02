"""Written tooling fixtures; do not execute during normal writing turns.

Run separately in an authorized verification pass. Runtime/native fixtures cover
real consumers; these fixtures protect orchestration preflight/evidence retention.
No fixture can launch a calibration process or access a serial device. The
proxy fixture talks only to an in-process loopback stub.
"""
import contextlib
import importlib.util
import io
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading
import unittest
import urllib.request
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("acceptance", Path(__file__).with_name("acceptance.py"))
DRIVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DRIVER)


class FailClosedPreflight(unittest.TestCase):
    def test_real_pty_unknown_and_missing_inputs_refuse_before_any_subprocess(self):
        for serial in ["/dev/cu.usbserial-REAL", "/dev/ttys007", "/dev/pts/4",
                       "http://127.0.0.1:4194", "", None]:
            with self.subTest(serial=serial), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                cfg = root / "input.json"
                cfg.write_text(json.dumps({"serial": serial, "fixture": "SIMULATED forged label"}))
                out = root / "retained"
                argv = ["acceptance", "--config", str(cfg), "--out", str(out),
                        "--fresh-build-receipt", str(root / "absent-receipt"),
                        "--bench", "/never-open", "--server", "/never-open", "--viewer", "/never-open"]
                with patch("sys.argv", argv), patch.object(DRIVER.subprocess, "Popen") as launch, \
                        patch.object(DRIVER.subprocess, "check_output") as git, \
                        contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(DRIVER.main(), 1)
                    launch.assert_not_called()
                    git.assert_not_called()
                result = json.loads((out / "results.json").read_text())
                self.assertFalse(result["ok"])
                self.assertIn("REFUSED physical/PTY/unknown", result["error"]["message"])
                self.assertEqual(json.loads((out / "input-config.json").read_text())["serial"], serial)
                self.assertEqual(result["record_sha256"], {})
                # No server was owned, so no STOP was sent to anything.
                stop = json.loads((out / "cleanup-server-stop.json").read_text())
                self.assertFalse(stop["sent"])

    def test_existing_evidence_directory_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary) / "retained"
            out.mkdir()
            evidence = out / "results.json"
            evidence.write_text("accepted-existing-receipt")
            argv = ["acceptance", "--out", str(out), "--fresh-build-receipt", "absent",
                    "--bench", "absent", "--server", "absent", "--viewer", "absent"]
            with patch("sys.argv", argv), patch.object(DRIVER.subprocess, "Popen") as launch:
                with self.assertRaises(FileExistsError):
                    DRIVER.main()
                launch.assert_not_called()
            self.assertEqual(evidence.read_text(), "accepted-existing-receipt")

    def test_supplied_config_placeholders_fail_closed_if_used_directly(self):
        cfg = json.loads(Path(__file__).with_name("server.virtual.json").read_text())
        self.assertEqual(cfg["serial"], "virtual-capability-only")
        for key in ["output", "viewer"]:
            self.assertTrue(cfg[key].startswith("/nonexistent-"), key)
            self.assertFalse(Path(cfg[key]).exists(), key)


class WaitConditions(unittest.TestCase):
    def test_missing_fields_wrong_types_and_none_are_not_yet(self):
        predicate = lambda s: s["server"]["axes"]["2"]["lower"] is not None
        self.assertFalse(DRIVER.satisfied(predicate, None))
        self.assertFalse(DRIVER.satisfied(predicate, {}))
        self.assertFalse(DRIVER.satisfied(predicate, {"server": None}))
        self.assertFalse(DRIVER.satisfied(predicate, {"server": {"axes": []}}))
        self.assertTrue(DRIVER.satisfied(predicate, {"server": {"axes": {"2": {"lower": 10}}}}))

    def test_stop_reply_must_show_latched_idle_state(self):
        self.assertTrue(DRIVER.stop_reply_latched(200, {"stop_latched": True, "enabled_id": None, "busy": False}))
        self.assertFalse(DRIVER.stop_reply_latched(200, {"stop_latched": True, "enabled_id": 2, "busy": False}))
        self.assertFalse(DRIVER.stop_reply_latched(200, {"ok": True}))
        self.assertFalse(DRIVER.stop_reply_latched(400, {"stop_latched": True, "enabled_id": None, "busy": False}))


class IdentityProxyRobustness(unittest.TestCase):
    def test_non_object_and_null_execution_never_become_virtual(self):
        replies = iter([b"[1, 2]", b'{"execution": null, "connected": true}',
                        b'{"execution": {"schema_version": 1, "kind": "virtual_calibration",'
                        b' "server_instance": "s", "bench_instance": "b"}}', b"not json"])

        class Stub(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                body = next(replies)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        upstream = ThreadingHTTPServer(("127.0.0.1", 0), Stub)
        thread = threading.Thread(target=upstream.serve_forever, daemon=True)
        thread.start()
        with tempfile.TemporaryDirectory() as temporary:
            proxy = DRIVER.IdentityFixture(upstream.server_port, Path(temporary) / "fixture.jsonl")
            url = proxy.start()
            try:
                bodies = []
                for _ in range(4):
                    with urllib.request.urlopen(url + "/calibration/status", timeout=5) as r:
                        bodies.append(r.read())
                self.assertEqual(json.loads(bodies[0]), [1, 2])
                self.assertEqual(json.loads(bodies[1])["execution"]["kind"], "physical")
                self.assertEqual(json.loads(bodies[2])["execution"]["kind"], "physical")
                self.assertEqual(bodies[3], b"not json")
                self.assertEqual(proxy.forwarded_motion, 0)
            finally:
                proxy.close()
                upstream.shutdown()
                upstream.server_close()


if __name__ == "__main__":
    unittest.main()
