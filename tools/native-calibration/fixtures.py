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
import re
import socket
import time
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


class CaptureAndStopPredicates(unittest.TestCase):
    def state(self, **latest):
        sample = {"holding": True, "velocity_counts_s": 0.5, "target_raw": 1000.0,
                  "position_continuous": 1001, "position_raw": 1001}
        sample.update(latest)
        return {"server": {"sweep": {"latest": sample}}}

    def test_holding_still_uses_the_server_capture_bounds(self):
        self.assertTrue(DRIVER.satisfied(DRIVER.holding_still, self.state()))
        self.assertFalse(DRIVER.satisfied(DRIVER.holding_still, self.state(holding=False)))
        self.assertFalse(DRIVER.satisfied(DRIVER.holding_still, self.state(velocity_counts_s=-2.0)))
        self.assertFalse(DRIVER.satisfied(DRIVER.holding_still, self.state(position_continuous=1003)))
        self.assertTrue(DRIVER.satisfied(DRIVER.holding_still, self.state(position_continuous=None, position_raw=998)))
        self.assertFalse(DRIVER.satisfied(DRIVER.holding_still, {"server": {"sweep": None}}))

    def test_stop_interrupted_needs_error_and_no_result(self):
        stopped = {"server": {"tuning": {"running": False, "error": "Operator stop during tuning"}}}
        self.assertTrue(DRIVER.satisfied(lambda s: DRIVER.stop_interrupted(s, "tuning"), stopped))
        # The viewer's tuning JSON has no `result`; a missing tuning entry is "not yet".
        self.assertFalse(DRIVER.satisfied(lambda s: DRIVER.stop_interrupted(s, "tuning"), {"server": {"tuning": None}}))
        finished = {"server": {"campaign": {"running": False, "error": None, "result": {"headline": "done"}}}}
        self.assertFalse(DRIVER.satisfied(lambda s: DRIVER.stop_interrupted(s, "campaign"), finished))
        running = {"server": {"campaign": {"running": True, "error": None, "result": None}}}
        self.assertFalse(DRIVER.satisfied(lambda s: DRIVER.stop_interrupted(s, "campaign"), running))


class PendingJobTransportErrors(unittest.TestCase):
    def test_read_timeout_reaches_job_cancel(self):
        # Python 3.9: a urllib read timeout is socket.timeout, not TimeoutError.
        for error in [socket.timeout("timed out"), DRIVER.urllib.error.URLError("refused"),
                      ConnectionResetError("reset"), TimeoutError("deadline"),
                      # A non-JSON reply while polling: the job is still pending.
                      json.JSONDecodeError("Expecting value", "<html>", 0)]:
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as temporary:
                args = type("Args", (), {"out": Path(temporary), "total_timeout": 60, "screenshots": False})()
                run = DRIVER.Run(args)
                replies = iter([(202, {"url": "/v1/jobs/7"})])

                def http(_base, method, _path, *_rest, **_kw):
                    if method == "POST":
                        return next(replies)
                    raise error
                with patch.object(run, "http", side_effect=http), \
                        patch.object(run, "cancel_job") as cancel, patch.object(run, "direct_stop") as stop:
                    with self.assertRaises(type(error)):
                        run.command("hardware_status", base="http://127.0.0.1:9")
                    cancel.assert_called_once_with("http://127.0.0.1:9", "/v1/jobs/7")
                    stop.assert_not_called()

    def test_submission_failure_sends_direct_stop(self):
        with tempfile.TemporaryDirectory() as temporary:
            args = type("Args", (), {"out": Path(temporary), "total_timeout": 60, "screenshots": False})()
            run = DRIVER.Run(args)
            with patch.object(run, "http", side_effect=socket.timeout("timed out")), \
                    patch.object(run, "cancel_job") as cancel, patch.object(run, "direct_stop") as stop:
                with self.assertRaises(socket.timeout):
                    run.command("hardware_status", base="http://127.0.0.1:9")
                stop.assert_called_once()
                cancel.assert_not_called()


class IdentityProxyRobustness(unittest.TestCase):
    def test_closing_a_proxy_that_never_started_does_not_block(self):
        with tempfile.TemporaryDirectory() as temporary:
            proxy = DRIVER.IdentityFixture(9, Path(temporary) / "fixture.jsonl")
            done = threading.Event()
            closer = threading.Thread(target=lambda: (proxy.close(), done.set()), daemon=True)
            started = time.monotonic()
            closer.start()
            self.assertTrue(done.wait(5), "close() blocked on a proxy whose serve thread never ran")
            self.assertLess(time.monotonic() - started, 5)

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


class ReceiptHelperRefusals(unittest.TestCase):
    """`acceptance.py receipt` refuses, writing nothing and calling no git."""

    def files(self, root):
        binaries = {}
        for label in DRIVER.BINARIES:
            path = root / label
            path.write_bytes(b"#!/bin/sh\n")
            path.chmod(0o755)
            binaries[label] = path
        log = root / "build.log"
        log.write_text("Finished `dev` profile\n")
        return binaries, log

    def refuse(self, argv, out):
        with patch.object(DRIVER.subprocess, "check_output") as git, \
                patch.object(DRIVER.subprocess, "Popen") as launch, \
                contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(DRIVER.receipt_main([str(a) for a in argv]), 2)
            git.assert_not_called()
            launch.assert_not_called()
        self.assertIn("REFUSED", err.getvalue())
        return err.getvalue()

    def argv(self, binaries, logs, out):
        argv = []
        for label, path in binaries.items():
            argv += [f"--{label}", path]
        for log in logs:
            argv += ["--build-log", log]
        return argv + ["--out", out]

    def test_no_build_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, _ = self.files(root)
            out = root / "receipt.json"
            self.assertIn("no --build-log", self.refuse(self.argv(binaries, [], out), out))
            self.assertFalse(out.exists())

    def test_missing_build_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, log = self.files(root)
            out = root / "receipt.json"
            self.assertIn("missing", self.refuse(self.argv(binaries, [log, root / "absent.log"], out), out))
            self.assertFalse(out.exists())

    def test_empty_build_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, log = self.files(root)
            log.write_bytes(b"")
            out = root / "receipt.json"
            self.assertIn("empty", self.refuse(self.argv(binaries, [log], out), out))
            self.assertFalse(out.exists())

    def test_missing_or_empty_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, log = self.files(root)
            binaries["viewer"].write_bytes(b"")
            binaries["server"] = root / "absent-server"
            out = root / "receipt.json"
            err = self.refuse(self.argv(binaries, [log], out), out)
            self.assertIn("viewer", err)
            self.assertIn("server", err)
            self.assertFalse(out.exists())

    def test_existing_output_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, log = self.files(root)
            out = root / "receipt.json"
            out.write_text("accepted-existing-receipt")
            self.assertIn("exists", self.refuse(self.argv(binaries, [log], out), out))
            self.assertEqual(out.read_text(), "accepted-existing-receipt")

    def test_receipt_carries_the_keys_run_validates(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, log = self.files(root)
            provenance = {"sha256": "ab" * 32}
            with patch.object(DRIVER, "source_hash", return_value=provenance), \
                    patch.object(DRIVER.subprocess, "check_output", side_effect=["c" * 40 + "\n", b""]):
                receipt = DRIVER.build_receipt({k: v.resolve() for k, v in binaries.items()}, [log.resolve()])
            self.assertEqual(receipt["source_sha256"], provenance["sha256"])
            self.assertEqual(receipt["source_commit"], "c" * 40)
            self.assertFalse(receipt["source_dirty"])
            for label, path in binaries.items():
                self.assertEqual(receipt["binaries"][label]["sha256"], DRIVER.sha(path))
            self.assertEqual(receipt["build_logs"][0]["path"], str(log.resolve()))
            self.assertEqual(receipt["build_logs"][0]["bytes"], log.stat().st_size)


class ScreenshotsNeverDecide(unittest.TestCase):
    def test_step_verdict_ignores_screenshots(self):
        failed_shot = [{"checkpoint": "HW-02-disabled", "path": "/x.png", "ok": False, "error": "TimeoutError"}]
        good_shot = [{"checkpoint": "HW-02-disabled", "path": "/x.png", "ok": True, "error": None}]
        step = DRIVER.settle_step({"id": "HW-02", "screenshots": failed_shot}, True)
        self.assertEqual(step["status"], "passed")
        self.assertEqual(step["screenshots"], failed_shot)
        step = DRIVER.settle_step({"id": "HW-02", "screenshots": good_shot}, False)
        self.assertEqual(step["status"], "failed")
        self.assertEqual(DRIVER.settle_step({"id": "HW-02"}, None)["status"], "failed")

    def test_summary_reports_off_captured_failed_and_not_reached(self):
        self.assertEqual(DRIVER.screenshot_summary(False, [])["mode"], "off")
        steps = [{"id": "HW-01", "status": "passed", "screenshots": [
                     {"checkpoint": "HW-01-identity", "path": "/a.png", "ok": True, "error": None},
                     {"checkpoint": "HW-01-fresh", "path": "/b.png", "ok": False, "error": "refused"}]}]
        summary = DRIVER.screenshot_summary(True, steps)
        self.assertEqual((summary["planned"], summary["attempted"], summary["captured"]),
                         (len(DRIVER.CHECKPOINTS), 2, 1))
        self.assertEqual(summary["failed"], [{"checkpoint": "HW-01-fresh", "error": "refused"}])
        self.assertNotIn("HW-01-identity", summary["not_reached"])
        self.assertIn("HW-09-terminal", summary["not_reached"])

    def test_png_completeness(self):
        whole = DRIVER.PNG_SIGNATURE + b"\x00" * 20 + DRIVER.PNG_TRAILER
        self.assertTrue(DRIVER.png_complete(whole))
        self.assertFalse(DRIVER.png_complete(b""))
        self.assertFalse(DRIVER.png_complete(whole[:-1]))
        self.assertFalse(DRIVER.png_complete(b"GIF89a" + whole[8:]))

    def test_disabled_run_never_requests_a_screenshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            args = type("Args", (), {"out": Path(temporary), "total_timeout": 60, "screenshots": False})()
            run = DRIVER.Run(args)
            run.results["steps"].append({"id": "HW-02", "status": "running"})
            with patch.object(DRIVER, "request") as http:
                self.assertIsNone(run.screenshot("disabled"))
                http.assert_not_called()
            self.assertNotIn("screenshots", run.results["steps"][0])
            self.assertFalse((Path(temporary) / "screenshots").exists())

    def test_readme_lists_exactly_the_driver_checkpoints(self):
        text = Path(__file__).with_name("README.md").read_text()
        section = text.split("## Screenshot checkpoints", 1)[1]
        listed = tuple(re.findall(r"^- `(HW-0\d-[a-z-]+)`", section, re.MULTILINE))
        self.assertEqual(listed, DRIVER.CHECKPOINTS)


if __name__ == "__main__":
    unittest.main()
