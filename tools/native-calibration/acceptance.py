#!/usr/bin/env python3
"""Future authorized fresh-binary HW-01–HW-09 verification; never builds.

Screenshots are captured only with the opt-in `--screenshots` flag. The
`receipt` subcommand writes the fresh-build receipt from already-built
binaries and retained build logs; it builds nothing.

Only a bench-owned capability socket can reach the calibration server. The
positive HW-01–HW-09 path connects the native viewer DIRECTLY to the owned
server. The loopback proxy exists only for the separately labelled
physical/unknown identity FIXTURE phase (a second owned viewer); it is not
another calibration engine. All outputs are retained, including failures.
See README.md for the receipt schema and the evidence list.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import re
import socket
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[2]
TERMINAL = {"succeeded", "failed", "cancelled"}
JOB_PATH = re.compile(r"^/v1/jobs/\d+$")
TOKEN = re.compile(r'<meta\s+name="calibration-token"\s+content="([^"]+)"')
# Exact refusal texts (grep the Rust before changing):
# handlers.rs `stepped`; hardware_client/calibration.rs `authorize_virtual`
# and `BINDING_REFUSED`; actions.rs `remote_refusal`.
RANGE = ("must be a number from",)
VIRTUAL_REQUIRED = ("Remote calibration requires a verified virtual",
                    "remote calibration requires a verified fresh virtual")
OUT_OF_SCOPE = ("remote calibration requires a verified fresh virtual",)
BINDING = "Calibration execution binding refused"
BINDING_STATUS = 409
EXPIRY = ("authorization expired",)
# Native STOP_TIMEOUT is 12 s; the fixture proxy must outlast it.
FORWARD_TIMEOUT = 15
CLIENT_DIRECT = "00000000-0000-4000-8000-000000000003"
CLIENT_MISMATCH = "00000000-0000-4000-8000-000000000004"
CLIENT_CLEANUP = "00000000-0000-4000-8000-000000000005"
FOREIGN_SERVER = "00000000-0000-4000-8000-000000000002"
# Screenshot checkpoints, `<step>-<checkpoint>`; README "Screenshot
# checkpoints" lists exactly these (fixtures.py compares them).
CHECKPOINTS = (
    "HW-01-identity", "HW-01-fresh", "HW-01-stale", "HW-01-reconnect",
    "HW-02-disabled",
    "HW-03-held",
    "HW-04-stop-tune", "HW-04-stop-campaign", "HW-04-stop-advanced",
    "HW-06-taught", "HW-06-target", "HW-06-reset",
    "HW-08-stopped", "HW-08-terminal",
    "HW-07-learned", "HW-07-sweep-all",
    "HW-09-stopped", "HW-09-terminal",
)
# Pose capture: the server saves only while holding with velocity < 2
# counts/s, |target - position| < 3 counts and six stable samples, else it
# reports "Still settling" (serve_actuator_calibration.rs, sweep sample
# callback). The viewer polls the server every 150 ms, so six stable samples
# take about 0.75 s of server sampling; the driver waits this long inside those
# bounds before each capture, then retries while the server still settles.
STILL_SECONDS = 0.5
CAPTURE_TRIES = 5
CAPTURE_SECONDS = 5
SETTLING = ("Still settling",)
# Errors that leave a queued viewer job unanswered. On Python 3.9 a read
# timeout is socket.timeout (not yet TimeoutError) and a connect failure is
# URLError; all of them must reach the job-cancel and direct-STOP path.
TRANSPORT_ERRORS = (TimeoutError, socket.timeout, urllib.error.URLError, ConnectionError,
                    http.client.HTTPException)
# Job plus file, per checkpoint (bounded also by the total deadline).
SCREENSHOT_SECONDS = 20
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
# The IEND chunk (length 0, type, CRC) that ends every complete PNG.
PNG_TRAILER = b"\x00\x00\x00\x00IEND\xaeB`\x82"


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


INCLUDE = re.compile(rb'include_(?:str|bytes)!\s*\(\s*"([^"]+)"')


def source_hash():
    """What would compile, including compiled-in assets.

    Hashed: every file `git ls-files` lists under crates/ and web/, the root
    Cargo.toml/Cargo.lock (and rust-toolchain files if tracked), untracked
    (not ignored) files under crates/ and web/, and every literal
    `include_str!`/`include_bytes!` target of a hashed `.rs` file wherever it
    lives (e.g. examples/ JSON, fonts). Tracked-but-deleted files are recorded
    and hashed as a deletion marker. Includes built with `concat!`/`env!` are
    not resolved; they are listed in `unresolved_includes` if seen.
    """
    roots = ["crates", "web", "Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml"]
    tracked = subprocess.check_output(["git", "ls-files", "-z", "--", *roots], cwd=ROOT).split(b"\0")
    untracked = subprocess.check_output(
        ["git", "ls-files", "-z", "--others", "--exclude-standard", "--", "crates", "web"], cwd=ROOT).split(b"\0")
    names = {os.fsdecode(raw): "tracked" for raw in tracked if raw}
    for raw in untracked:
        if raw:
            names.setdefault(os.fsdecode(raw), "untracked")
    unresolved = []
    for name in sorted(n for n in names if n.endswith(".rs")):
        path = ROOT / name
        if not path.is_file():
            continue
        text = path.read_bytes()
        for target in INCLUDE.findall(text):
            resolved = (path.parent / os.fsdecode(target)).resolve()
            try:
                names.setdefault(str(resolved.relative_to(ROOT)), "included")
            except ValueError:
                unresolved.append(f"{name}: {os.fsdecode(target)} (outside repository)")
        if re.search(rb"include_(?:str|bytes)!\s*\(\s*concat!", text):
            unresolved.append(f"{name}: concat!/env! include")
    h = hashlib.sha256()
    deleted, untracked_included, assets = [], [], []
    for name in sorted(names):
        path = ROOT / name
        if not path.is_file():
            deleted.append(name)
            h.update(os.fsencode(name) + b"\0DELETED\0")
            continue
        if names[name] == "untracked":
            untracked_included.append(name)
        elif names[name] == "included":
            assets.append(name)
        h.update(os.fsencode(name) + b"\0" + path.read_bytes() + b"\0")
    return {"sha256": h.hexdigest(), "files": len(names), "deleted_tracked": deleted,
            "untracked_included": untracked_included, "included_assets_outside_roots": assets,
            "unresolved_includes": unresolved}


def request(base, method, path, body=None, headers=None, timeout=3):
    data = None if body is None else json.dumps(body, allow_nan=False).encode()
    req = urllib.request.Request(base + path, data=data, method=method,
                                 headers={"Content-Type": "application/json", **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def decode(raw):
    return json.loads(raw or b"null")


def decode_safe(raw):
    try:
        return decode(raw)
    except ValueError:
        return {"undecodable": (raw or b"")[:2000].decode(errors="replace")}


def satisfied(predicate, value):
    """A wait condition: missing fields, wrong types and None mean "not yet"."""
    if value is None:
        return False
    try:
        return bool(predicate(value))
    except (KeyError, TypeError, IndexError, AttributeError):
        return False


def has_text(message, texts):
    return isinstance(message, str) and any(t in message for t in texts)


def holding_still(state):
    """The motor being taught holds inside the server's capture bounds now."""
    latest = state["server"]["sweep"]["latest"]
    position = latest["position_continuous"]
    if position is None:
        position = latest["position_raw"]
    return (latest["holding"] is True and abs(latest["velocity_counts_s"]) < 2
            and abs(latest["target_raw"] - position) < 3)


def stop_interrupted(state, kind):
    """STOP ended `kind` (tuning/campaign) before it finished: no longer
    running and an error recorded (the server sets `error` only on the
    stopped path). For the campaign, `result` (set only on completion) must
    also be absent. The viewer's tuning JSON (view/status.rs) carries no
    `result` at all, so for tuning the "no result" clause is vacuous: the
    caller must also check that the motor still has no gains."""
    work = state["server"][kind]
    return work["running"] is False and bool(work.get("error")) and not work.get("result")


def stop_reply_latched(code, value):
    return (code == 200 and isinstance(value, dict) and value.get("stop_latched") is True
            and value.get("enabled_id") is None and value.get("busy") is False)


def settle_step(step, semantic_passed):
    """A step's verdict comes only from its semantic assertions.

    Screenshot entries are evidence and are left untouched: a failed or
    missing screenshot never fails a passing step, and a captured one never
    passes a failing step.
    """
    step["status"] = "passed" if semantic_passed is True else "failed"
    return step


def png_complete(data):
    """A whole PNG: signature first and the IEND trailer last."""
    return len(data) > len(PNG_SIGNATURE) + len(PNG_TRAILER) and data.startswith(PNG_SIGNATURE) \
        and data.endswith(PNG_TRAILER)


def screenshot_summary(enabled, steps):
    """Truthful screenshot reporting for results.json: off, or captured N of M."""
    if not enabled:
        return {"mode": "off", "note": "--screenshots not given; nothing was captured"}
    shots = [s for step in steps for s in step.get("screenshots", [])]
    failed = [{"checkpoint": s["checkpoint"], "error": s["error"]} for s in shots if not s["ok"]]
    attempted = {s["checkpoint"] for s in shots}
    return {"mode": "on", "planned": len(CHECKPOINTS), "attempted": len(shots),
            "captured": len(shots) - len(failed), "failed": failed,
            "not_reached": [c for c in CHECKPOINTS if c not in attempted],
            "note": "screenshots supplement semantic assertions; they never decide a step"}


def vacant_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class IdentityFixture:
    """FIXTURE ONLY: present the owned virtual server as physical/unknown.

    Used only by the second owned viewer in the labelled fixture phase. It
    never presents a virtual identity, so native motion authorization cannot
    pass through it; every non-STOP command POST is counted as a crossing.
    """
    def __init__(self, port, evidence):
        self.port, self.evidence = port, evidence
        self.mode = "physical"
        self.forwarded_motion = 0
        self.lock = threading.Lock()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                self.forward()

            def do_POST(self):
                self.forward()

            def forward(self):
                action, status, sent = None, None, False
                try:
                    try:
                        n = int(self.headers.get("Content-Length") or 0)
                    except ValueError:
                        n = -1
                    if n < 0 or n > 8192:
                        status = 413
                        self.send_error(413)
                        sent = True
                        return
                    body = self.rfile.read(n) if n else None
                    try:
                        parsed = json.loads(body) if body else None
                        if isinstance(parsed, dict) and isinstance(parsed.get("action"), str):
                            action = parsed["action"]
                    except ValueError:
                        action = "<undecodable>"
                    headers = {k: v for k, v in self.headers.items()
                               if k.lower() not in {"host", "connection", "content-length"}}
                    conn = http.client.HTTPConnection("127.0.0.1", owner.port, timeout=FORWARD_TIMEOUT)
                    try:
                        conn.request(self.command, self.path, body, headers)
                        response = conn.getresponse()
                        raw = response.read()
                        status = response.status
                        content_type = response.getheader("Content-Type", "application/json")
                        if content_type.startswith("application/json") and status == 200:
                            try:
                                value = json.loads(raw or b"null")
                                if isinstance(value, dict) and "execution" in value:
                                    execution = value["execution"]
                                    instance = execution.get("server_instance", "") if isinstance(execution, dict) else ""
                                    if owner.mode == "physical":
                                        value["execution"] = {"schema_version": 1, "kind": "physical",
                                                              "server_instance": instance, "bench_instance": ""}
                                    else:
                                        value.pop("execution")
                                    raw = json.dumps(value).encode()
                            except (ValueError, TypeError, AttributeError):
                                pass  # forwarded unchanged; never upgrades identity
                        self.send_response(status)
                        sent = True
                        self.send_header("Content-Type", content_type)
                        self.send_header("Content-Length", str(len(raw)))
                        self.end_headers()
                        self.wfile.write(raw)
                    finally:
                        conn.close()
                except (OSError, ValueError, http.client.HTTPException) as e:
                    status = status or 502
                    if not sent:
                        try:
                            self.send_error(502, str(e)[:200])
                        except OSError:
                            pass
                finally:
                    with owner.lock:
                        if self.command == "POST" and action != "stop":
                            owner.forwarded_motion += 1
                        with owner.evidence.open("a") as log:
                            log.write(json.dumps({"time": time.time(), "method": self.command,
                                "path": self.path, "action": action, "mode": owner.mode,
                                "status": status}) + "\n")
        self.http = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.http.daemon_threads = True
        self.thread = threading.Thread(target=self.http.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return f"http://127.0.0.1:{self.http.server_port}"

    def close(self):
        # shutdown() waits for serve_forever to exit: never call it when the
        # serve thread never started (or already ended), or it blocks forever.
        try:
            if self.thread.is_alive():
                self.http.shutdown()
        finally:
            self.http.server_close()
            if self.thread.is_alive():
                self.thread.join(timeout=3)


class Run:
    def __init__(self, args):
        self.args = args
        self.out = args.out.resolve()
        self.processes, self.logs, self.proxy = [], [], None
        self.viewers = {}
        self.base = self.server = None
        self.server_proc = self.server_log = None
        self.server_paused = False
        self.server_token = None
        self.identity = None
        self.deadline = time.monotonic() + args.total_timeout
        self.step = "preflight"
        self.screenshots = bool(getattr(args, "screenshots", False))
        self.results = {"schema_version": 2, "fidelity": "SIMULATED virtual bench; uncalibrated model",
                        "ok": False, "steps": [], "screenshots": screenshot_summary(self.screenshots, [])}

    # ---- evidence -------------------------------------------------------
    def retain(self, name, value):
        (self.out / name).write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")

    def append(self, name, value):
        with (self.out / name).open("a") as log:
            log.write(json.dumps(value, allow_nan=False) + "\n")

    def record_hashes(self):
        records = self.out / "records"
        return {str(p.relative_to(self.out)): sha(p) for p in records.rglob("*.json")} if records.is_dir() else {}

    # ---- owned processes ------------------------------------------------
    def check_deadline(self):
        if time.monotonic() >= self.deadline:
            raise TimeoutError("total verification deadline")
        for label, viewer in self.viewers.items():
            if viewer["required"] and viewer["proc"].poll() is not None:
                raise RuntimeError(f"owned viewer {label} exited; refusing unrelated listener")

    def spawn(self, name, argv, env=None):
        log = (self.out / f"{name}.log").open("xb")
        self.logs.append(log)
        proc = subprocess.Popen([str(x) for x in argv], cwd=ROOT, stdout=log, env=env,
                                stderr=subprocess.STDOUT, start_new_session=True)
        self.processes.append((name, proc))
        self.retain("processes.json", [{"name": n, "pid": p.pid, "owned": True} for n, p in self.processes])
        return proc

    def server_owned(self):
        """Ownership, not identity: our live child printed that it bound this port."""
        if self.server_proc is None or self.server_proc.poll() is not None or not self.server:
            return False
        try:
            text = (self.out / f"{self.server_log}.log").read_text(errors="replace")
        except OSError:
            return False
        return f"Calibration and robot viewer: {self.server}" in text

    def pause_server(self):
        if not self.server_owned():
            raise RuntimeError("refusing to pause an unowned server")
        # Flag first: cleanup must resume even if we are interrupted here.
        self.server_paused = True
        os.kill(self.server_proc.pid, signal.SIGSTOP)

    def resume_server(self):
        if not self.server_paused:
            return
        try:
            if self.server_proc is not None and self.server_proc.poll() is None:
                os.kill(self.server_proc.pid, signal.SIGCONT)
        finally:
            self.server_paused = False

    def server_token_value(self):
        code, page = request(self.server, "GET", "/", timeout=2)
        match = TOKEN.search(page.decode(errors="replace"))
        if code != 200 or not match:
            raise ConnectionError("owned server token discovery")
        return match[1]

    def direct_stop(self, reason):
        """Direct server STOP without identity headers, decided by ownership."""
        record = {"reason": reason, "time": time.time(), "sent": False}
        try:
            self.resume_server()
            if not self.server_owned():
                record["why"] = "no live owned server bound to the recorded port"
                return record
            headers = {"X-Control-Token": self.server_token_value(), "X-Client-Id": CLIENT_CLEANUP}
            code, raw = request(self.server, "POST", "/calibration/command",
                                {"action": "stop", "sequence": 999999999}, headers, 3)
            value = decode_safe(raw)
            record.update(sent=True, http=code, response=value, stop_latched=stop_reply_latched(code, value))
        except Exception as e:  # retained; cleanup continues
            record["error"] = f"{type(e).__name__}: {e}"
        finally:
            self.append("server-stops.jsonl", record)
        return record

    # ---- REST -----------------------------------------------------------
    def http(self, base, method, path, body=None, headers=None, timeout=3):
        self.check_deadline()
        if base == self.server and path.startswith("/calibration/") and headers is None:
            if self.server_token is None:
                self.server_token = self.server_token_value()
            headers = {"X-Control-Token": self.server_token, "X-Client-Id": CLIENT_DIRECT}
        status, raw = request(base, method, path, body, headers,
                              timeout=max(.1, min(timeout, self.deadline - time.monotonic())))
        try:
            value = decode(raw)
        except ValueError:
            # Retain the undecodable reply before the caller sees the error.
            self.append("http.jsonl", {"step": self.step, "base": base, "method": method, "path": path,
                                       "body": body, "http": status, "response": decode_safe(raw)})
            raise
        self.append("http.jsonl", {"step": self.step, "base": base, "method": method, "path": path,
                                   "body": body, "http": status, "response": value})
        return status, value

    def cancel_job(self, base, path):
        """DELETE the timed-out job (cooperative: a pending hardware ticket
        observes `call.cancelled` and requests STOP) so later STOPs do not
        queue behind it, then STOP directly on the server."""
        record = {"step": self.step, "base": base, "job": path}
        try:
            code, raw = request(base, "DELETE", path, timeout=2)
            record["delete"] = {"http": code, "response": decode_safe(raw)}
            end = time.monotonic() + 5
            while time.monotonic() < end:
                code, raw = request(base, "GET", path, timeout=2)
                job = decode_safe(raw)
                if isinstance(job, dict) and job.get("status") in TERMINAL:
                    record["terminal"] = job
                    break
                time.sleep(.2)
        except Exception as e:
            record["error"] = f"{type(e).__name__}: {e}"
        finally:
            record["server_stop"] = self.direct_stop(f"job timeout {path}")
            self.append("cancelled-jobs.jsonl", record)

    def command(self, command, args=None, refusal=False, base=None, seconds=25):
        base = base or self.base
        try:
            code, accepted = self.http(base, "POST", "/v1/batch",
                {"commands": [{"command": command, "args": args or {}}], "stop_on_error": True})
        except TRANSPORT_ERRORS + (ValueError,):
            # The job may be queued with no URL to cancel (or an unreadable
            # reply hid it): STOP directly.
            self.direct_stop(f"{command} submission failed")
            raise
        if code != 202 or not isinstance(accepted, dict):
            raise AssertionError(f"command not queued: {code} {accepted}")
        path = accepted.get("url")
        if not isinstance(path, str) or not JOB_PATH.match(path):
            raise AssertionError("untrusted job URL")
        try:
            job = self.poll(lambda: self.http(base, "GET", path)[1],
                            lambda j: j["status"] in TERMINAL, seconds, f"{command} terminal")
        except TRANSPORT_ERRORS + (ValueError,):
            # ValueError: a non-JSON poll reply; the job is still pending.
            self.cancel_job(base, path)
            raise
        result = job["results"][0]
        if refusal:
            assert job["status"] == "failed" and result["ok"] is False and result.get("error"), job
            return result
        assert job["status"] == "succeeded" and result["ok"] is True, job
        return result.get("value")

    def poll(self, read, predicate, seconds, label):
        end = min(self.deadline, time.monotonic() + seconds)
        last = None
        while True:
            self.check_deadline()
            last = read()
            if satisfied(predicate, last):
                return last
            if time.monotonic() >= end:
                break
            time.sleep(.12)
        self.retain(f"timeout-state-{int(time.time() * 1000)}.json", {"step": self.step, "label": label, "last": last})
        raise TimeoutError(label)

    def state(self, base=None):
        return self.command("hardware_status", base=base)

    def wait(self, predicate, seconds=20, label="state assertion", base=None):
        return self.poll(lambda: self.state(base), predicate, seconds, label)

    def action(self, name, value=None, expect=None, base=None):
        result = self.command("hardware", {"action": name if value is None else {name: value}},
                              refusal=expect is not None, base=base)
        if expect is not None:
            assert has_text(result.get("error"), expect), (name, expect, result)
        return result

    def click(self, name, expect=None, base=None, seconds=25, activates=None):
        """Activate a listed control. `activates` names the hardware action
        the control must currently stand for (e.g. a jog toggle's
        `jog_release` while held); `seconds` bounds the activation job."""
        listing = self.command("system_ui", {"action": {"operation": "controls"}}, base=base)
        identity = name if name.startswith("mode:") else "hardware:" + name
        controls = [c for c in listing["controls"] if c.get("id") == identity]
        assert len(controls) == 1, (identity, controls)
        if expect is None:
            assert controls[0]["enabled"], controls[0]
        else:
            assert not controls[0]["enabled"] and has_text(controls[0].get("disabled_reason"), expect), controls[0]
        if activates is not None:
            action = (controls[0].get("action") or {}).get("hardware")
            assert isinstance(action, dict) and list(action) == [activates], (identity, activates, controls[0])
        result = self.command("system_ui", {"action": {"operation": "activate",
            "id": identity, "ui_revision": listing["ui_revision"]}}, refusal=expect is not None, base=base,
            seconds=seconds)
        if expect is not None:
            assert has_text(result.get("error"), expect), (identity, expect, result)
        return result

    def launch_viewer(self, label, url):
        """An owned viewer isolated from the operator's preferences and recents."""
        config = self.out / f"viewer-config-{label}"
        config.mkdir()
        env = dict(os.environ)
        removed = [k for k in ("PHENOMENA_EXHIBIT",) if env.pop(k, None) is not None]
        overrides = {
            # recent.rs config_dir: unified preferences and recent documents.
            "SIM_SPATIAL_CONFIG_DIR": str(config),
            # robot/hardware/settings.rs PATH_VARIABLE: legacy hardware preferences.
            "SIM_SPATIAL_PREFERENCES": str(config / "hardware-preferences.json"),
            "SIM_LESSON_SETTINGS": str(config / "lesson-settings.json"),
        }
        env.update(overrides)
        port = vacant_port()
        base = f"http://127.0.0.1:{port}"
        argv = [self.args.viewer, "--robot-preset", "robot-measured-400hz", "--hardware", url, "--api-port", port]
        self.retain(f"viewer-{label}-launch.json", {"argv": [str(x) for x in argv], "environment_overrides": overrides,
                                                     "environment_removed": removed})
        proc = self.spawn(f"viewer-{label}", argv, env=env)
        self.viewers[label] = {"proc": proc, "base": base, "required": True, "verified": False}

        def ready():
            try:
                code, root = self.http(base, "GET", "/")
            except (OSError, ValueError):
                return None
            return root if code == 200 else None
        root = self.poll(ready, lambda r: r["pid"] == proc.pid and r["service"] == "sim-spatial", 90, f"viewer {label} API")
        self.retain(f"viewer-{label}-root.json", root)
        self.viewers[label]["verified"] = True
        return base

    def stop_viewer(self, label):
        viewer = self.viewers[label]
        viewer["required"] = False
        proc = viewer["proc"]
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3)

    # ---- shared step helpers -------------------------------------------
    def begin(self, identity):
        self.step = identity
        self.results["steps"].append({"id": identity, "status": "running"})
        self.retain("results.json", self.results)

    def done(self, **extra):
        step = self.results["steps"][-1]
        step.update(state=self.state(), **extra)
        settle_step(step, True)
        self.retain("results.json", self.results)

    def screenshot(self, checkpoint):
        """Opt-in evidence: the viewer's own REST `screenshot` of the main window.

        Never raises (except KeyboardInterrupt) and never decides the step:
        every outcome is recorded in the step's `screenshots` list. The
        command answers when the capture is QUEUED (switch/mod.rs
        `screenshot`); Bevy's `save_to_disk` writes the PNG after the next
        frame, so the file is polled until it is a complete PNG. A stuck job
        is cancelled with DELETE only: it holds no hardware ticket, and a
        direct server STOP here would change the semantic sequence. A real
        stall then surfaces in the next semantic command's own deadline.
        """
        if not self.screenshots:
            return None
        step = self.results["steps"][-1]
        name = f"{step['id']}-{checkpoint}"
        path = self.out / "screenshots" / f"{name}.png"
        entry = {"checkpoint": name, "path": str(path), "ok": False, "error": None}
        step.setdefault("screenshots", []).append(entry)
        job_path = None
        end = min(self.deadline, time.monotonic() + SCREENSHOT_SECONDS)
        try:
            if name not in CHECKPOINTS:
                raise ValueError(f"unlisted checkpoint {name}")
            path.parent.mkdir(exist_ok=True)
            if path.exists() or path.is_symlink():
                raise FileExistsError(f"{path} exists; never overwritten")
            code, accepted = self.http(self.base, "POST", "/v1/batch",
                {"commands": [{"command": "screenshot", "args": {"path": str(path)}}], "stop_on_error": True})
            job_path = accepted.get("url") if code == 202 and isinstance(accepted, dict) else None
            if not isinstance(job_path, str) or not JOB_PATH.match(job_path):
                job_path = None
                raise RuntimeError(f"screenshot not queued: {code} {accepted}")
            job = None
            while True:
                code, job = self.http(self.base, "GET", job_path, timeout=2)
                if isinstance(job, dict) and job.get("status") in TERMINAL:
                    break
                if time.monotonic() >= end:
                    raise TimeoutError(f"screenshot job not terminal within {SCREENSHOT_SECONDS} s")
                time.sleep(.12)
            finished, job_path = job_path, None
            results = job.get("results") or [{}]
            result = results[0] if isinstance(results[0], dict) else {}
            if job["status"] != "succeeded" or result.get("ok") is not True:
                raise RuntimeError(f"screenshot refused: {result.get('error') or job}")
            value = result.get("value")
            if not isinstance(value, dict) or value.get("path") != str(path):
                raise RuntimeError(f"screenshot answered another path: {value} ({finished})")
            while True:
                data = path.read_bytes() if path.is_file() else b""
                if png_complete(data):
                    break
                if time.monotonic() >= end:
                    raise TimeoutError(f"no complete PNG at {path} within {SCREENSHOT_SECONDS} s "
                                       f"({len(data)} bytes, signature {data.startswith(PNG_SIGNATURE)})")
                time.sleep(.12)
            entry.update(ok=True, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
        except Exception as e:  # recorded; semantic assertions continue
            entry["error"] = f"{type(e).__name__}: {e}"
            if job_path is not None:
                try:
                    code, raw = request(self.base, "DELETE", job_path, timeout=2)
                    entry["cancel"] = {"job": job_path, "http": code, "response": decode_safe(raw)}
                except Exception as cancel:
                    entry["cancel"] = {"job": job_path, "error": f"{type(cancel).__name__}: {cancel}"}
        finally:
            self.results["screenshots"] = screenshot_summary(self.screenshots, self.results["steps"])
            try:
                self.retain("results.json", self.results)
            except Exception as e:  # results.json is rewritten by the next step and by cleanup
                entry["retain_error"] = f"{type(e).__name__}: {e}"
        return entry

    def position(self, state, motor=None):
        motor = motor or state["session"]["id"] or 2
        sample = state["server"]["samples"][str(motor)]
        return sample["position_continuous"] if sample["position_continuous"] is not None else sample["position_raw"]

    def explicit_form(self):
        """Never rely on remembered choices: set and observe the drive form."""
        self.action("hold_others", {"on": True})
        self.action("drive_mode", {"mode": "pwm"})
        return self.wait(lambda s: s["form"]["hold_others"] is True and s["form"]["drive_mode"] == "pwm",
                         10, "explicit hold_others/pwm form")

    def selected(self, motor=2):
        self.click(f"select_{motor}")
        return self.wait(lambda s: s["session"]["id"] == motor and s["session"]["ready"]
                         and not s["session"]["busy"])

    def held(self):
        return self.wait(lambda s: s["session"]["intent"] == "hold" and
                         ((s["server"].get("sweep") or {}).get("latest") or {}).get("holding") is True)

    def stopped(self):
        return self.wait(lambda s: not s["session"]["ready"] and not s["session"]["run"]
                         and s["server"]["enabled_id"] is None and not s["server"]["busy"]
                         and not (s["server"].get("sweep") or {}).get("running")
                         and not (s["server"].get("tuning") or {}).get("running")
                         and not (s["server"].get("campaign") or {}).get("running"))

    def server_status(self):
        return self.http(self.server, "GET", "/calibration/status")[1]

    def held_inputs(self):
        """Which jog inputs the viewer holds: `hardware_status.form`
        `held_upper`/`held_lower` when exposed, else the jog toggles'
        listed action (`jog_release` while that direction is held)."""
        form = self.state()["form"]
        if "held_upper" in form and "held_lower" in form:
            return {"upper": form["held_upper"] is True, "lower": form["held_lower"] is True, "source": "form"}
        listing = self.command("system_ui", {"action": {"operation": "controls"}})
        held = {"source": "controls"}
        for direction in ("upper", "lower"):
            control = [c for c in listing["controls"] if c.get("id") == "hardware:jog_" + direction]
            assert len(control) == 1, control
            held[direction] = list((control[0].get("action") or {}).get("hardware") or {}) == ["jog_release"]
        return held

    def jog_toggle(self, direction, release):
        """Press or release a jog with its own toggle control: the same id
        activates `jog_press`, then `jog_release` while held."""
        self.click("jog_" + direction, activates="jog_release" if release else "jog_press")
        return self.poll(self.held_inputs, lambda h: h[direction] is (not release), 10,
                         f"jog {direction} {'released' if release else 'held'}")

    def jog(self, direction, counts=700):
        before = self.position(self.state())
        self.jog_toggle(direction, release=False)
        try:
            changed = self.wait(lambda s: abs(self.position(s) - before) >= counts, 30, "encoder moved")
            assert changed["session"]["intent"] != "hold"
        except BaseException:
            # Best-effort release; the original failure is what propagates.
            try:
                self.jog_toggle(direction, release=True)
            except Exception as e:
                try:
                    self.append("jog-release-failures.jsonl",
                                {"step": self.step, "direction": direction, "error": f"{type(e).__name__}: {e}"})
                except Exception:
                    pass
            raise
        self.jog_toggle(direction, release=True)
        return self.held()

    def still(self, label="holding still for capture"):
        """`holding_still` continuously for STILL_SECONDS."""
        end = min(self.deadline, time.monotonic() + 20)
        since, last = None, None
        while True:
            last = self.state()
            if satisfied(holding_still, last):
                since = time.monotonic() if since is None else since
                if time.monotonic() - since >= STILL_SECONDS:
                    return last
            else:
                since = None
            if time.monotonic() >= end:
                break
            time.sleep(.05)
        self.retain(f"timeout-state-{int(time.time() * 1000)}.json", {"step": self.step, "label": label, "last": last})
        raise TimeoutError(label)

    def calibration_saves(self):
        """Timestamped calibration files the server writes on each saved pose."""
        return len(list((self.out / "records").glob("calibration-*.json")))

    def capture(self, boundary, axis):
        """Save a pose and prove it was saved, retrying while it settles.

        A capture the server judges unsettled is dropped with "Still
        settling", and an earlier value (e.g. the reference after
        reset_poses) can already be non-null, so success is a NEW
        calibration file plus `Saved <boundary> pose` plus the value.
        """
        attempts = []
        for attempt in range(1, CAPTURE_TRIES + 1):
            self.still()
            before = self.calibration_saves()
            self.click("capture_" + boundary)
            end = min(self.deadline, time.monotonic() + CAPTURE_SECONDS)
            while True:
                s = self.state()
                # hardware_status may report "server": null or empty axes:
                # missing fields mean "not saved yet".
                server = s.get("server") if isinstance(s, dict) else None
                message = server.get("capture_message") if isinstance(server, dict) else None
                if (self.calibration_saves() > before and message == f"Saved {boundary} pose"
                        and satisfied(lambda v: v["server"]["axes"][axis][boundary] is not None, s)):
                    self.append("captures.jsonl", {"step": self.step, "axis": axis, "boundary": boundary,
                                                   "attempts": attempts + [{"attempt": attempt, "saved": True}]})
                    return s
                if time.monotonic() >= end:
                    break
                time.sleep(.12)
            attempts.append({"attempt": attempt, "saved": False, "capture_message": message,
                             "settling": has_text(message, SETTLING)})
        self.append("captures.jsonl", {"step": self.step, "axis": axis, "boundary": boundary, "attempts": attempts})
        raise AssertionError(f"{boundary} pose of motor {axis} not saved after {CAPTURE_TRIES} tries: {attempts}")

    def teaching(self, motor=2):
        axis = str(motor)
        self.jog("lower")
        lower = self.capture("lower", axis)["server"]["axes"][axis]["lower"]
        self.jog("upper", 1500)
        taught = self.capture("upper", axis)
        assert abs(taught["server"]["axes"][axis]["upper"] - lower) >= 1200
        self.capture("reference", axis)
        return taught

    def settled(self, seconds=30, label="bounded target reached"):
        return self.wait(lambda s: abs(self.position(s) - s["session"]["target_raw"]) <= 32, seconds, label)

    # ---- the verification ----------------------------------------------
    def verify(self):
        self.begin("HW-01")
        connected = self.wait(lambda s: s["connected"] and not s["stale"] and s["age_ms"] is not None
                              and s["server"]["execution"] == self.identity, 90, "direct virtual connection")
        assert connected["url"] == self.server, "positive path must connect directly to the owned server"
        assert connected["server"]["fidelity"] == "virtual_simulated"
        assert connected["session"]["id"] is None and connected["server"]["enabled_id"] is None
        self.screenshot("identity")
        assert self.server_status()["execution"] == self.identity
        self.retain("execution.json", self.identity)
        generation = connected["generation"]
        self.screenshot("fresh")
        # Simulated disconnection: the owned server process is stalled
        # (SIGSTOP) and stays stalled until the native link has REVOKED this
        # generation. Stale alone is not enough: an in-flight status request
        # keeps `awaiting` set (request timeout plus margin, actions.rs
        # poll_jobs), and resuming before revocation would let the link turn
        # fresh again and a later select would really enable motor 2.
        self.pause_server()
        try:
            self.wait(lambda s: s["stale"], 15, "stale while server stalled")
            self.screenshot("stale")
            self.wait(lambda s: s["authorization_revoked"] is True, 25, "revoked while server stalled")
        finally:
            self.resume_server()
        resumed = self.wait(lambda s: s["authorization_revoked"] is True and s["generation"] == generation,
                            10, "revocation persists after resume")
        self.retain("hw01-revoked.json", resumed)
        self.action("select", {"id": 2}, expect=EXPIRY)
        assert self.server_status()["enabled_id"] is None, "refused select must not enable a motor"
        self.click("connect")
        fresh = self.wait(lambda s: s["connected"] and not s["stale"] and not s["authorization_revoked"]
                          and s["generation"] != generation and s["server"]["execution"] == self.identity)
        assert fresh["server"]["enabled_id"] is None and not fresh["session"]["ready"]
        # Authorization is restored only for the new generation: select_2 is
        # listed enabled again (the listing applies the same `authorize`).
        listing = self.command("system_ui", {"action": {"operation": "controls"}})
        chip = [c for c in listing["controls"] if c.get("id") == "hardware:select_2"]
        assert len(chip) == 1 and chip[0]["enabled"], chip
        self.screenshot("reconnect")
        self.done(revoked_generation=generation, restored_generation=fresh["generation"])

        self.begin("HW-02")
        self.explicit_form()
        for motor in [1, 2, 3, 2]:
            self.selected(motor)
        self.click("set_disabled")
        self.wait(lambda s: s["server"]["axes"]["2"]["disabled"] and not s["session"]["ready"])
        self.screenshot("disabled")
        self.click("select_2")
        assert not self.state()["session"]["ready"]
        self.click("set_disabled")
        self.wait(lambda s: not s["server"]["axes"]["2"]["disabled"])
        self.selected()
        self.done()

        self.begin("HW-03")
        self.action("speed", {"percent": 25})
        self.wait(lambda s: s["form"]["speed_percent"] == 25)
        self.jog("upper", 60)
        self.jog("lower", 60)
        # Opposing inputs through the toggles: both pressed holds.
        self.jog_toggle("upper", release=False)
        both = self.jog_toggle("lower", release=False)
        assert both["upper"] and both["lower"], both
        held = self.held()
        before = self.position(held)
        # Release each by activating its toggle again.
        self.jog_toggle("upper", release=True)
        released = self.jog_toggle("lower", release=True)
        assert not released["upper"] and not released["lower"], released
        self.held()
        end = time.monotonic() + 1
        while time.monotonic() < end:
            assert abs(self.position(self.state()) - before) <= 32, "opposed/released input failed to hold"
        self.screenshot("held")
        self.done()

        self.begin("HW-04")
        for section in ["tune", "campaign", "mirror", "advanced"]:
            self.click("section_" + section)
            listing = self.command("system_ui", {"action": {"operation": "controls"}})
            assert any(c.get("id") == "hardware:stop" and c.get("enabled") for c in listing["controls"])
            if section != "mirror":
                self.screenshot("stop-" + section)
            self.selected()
            self.click("jog_upper")
            self.wait(lambda s: s["session"]["run"] is not None)
            self.click("stop")
            self.stopped()
        self.done()

        self.begin("HW-05")
        for interrupt in ["focus", "close", "toggle", "mode"]:
            self.selected()
            self.click("jog_upper")
            self.wait(lambda s: s["session"]["run"] is not None)
            if interrupt == "focus":
                self.action("loss", {"reason": "focus_lost"})
                self.stopped()
            elif interrupt in {"close", "toggle"}:
                self.click("close" if interrupt == "close" else "toggle_panel")
                state = self.stopped()
                assert not state["open"]
                self.click("toggle_panel")
            else:
                previous = self.state()["generation"]
                # Build needs a document (prepare.rs `needs`); Phenomena opens
                # without one, so it exercises Robot-scope exit (actions.rs `leave`).
                self.click("mode:phenomena")
                self.poll(self.server_status,
                          lambda s: s["enabled_id"] is None and not s["busy"]
                          and not (s.get("sweep") or {}).get("running"), 15, "mode exit STOP")
                absent = self.command("hardware_status", refusal=True)
                assert has_text(absent.get("error"), ("the active mode is phenomena",)), absent
                # Robot re-entry rebuilds the scene: allow it the connect budget.
                self.click("mode:robot", seconds=90)
                self.wait(lambda s: s["connected"] and not s["stale"] and s["generation"] != previous
                          and not s["session"]["ready"], 90, "robot re-entry reconnects idle")
                self.explicit_form()
        self.done()

        self.begin("HW-06")
        self.selected()
        self.action("speed", {"percent": 80})
        self.teaching()
        self.screenshot("taught")
        # Range validation after poses exist, so `stepped` refuses rather
        # than a disabled slider (handlers.rs remote_check/target_enabled).
        for bad in [-1, 101]:
            self.action("speed", {"percent": bad}, expect=RANGE)
            self.action("target", {"percent": bad}, expect=RANGE)
        for percent in [25, 75]:
            self.action("target", {"percent": percent})
            self.action("target_commit")
            reached = self.settled()
            a = reached["server"]["axes"]["2"]
            assert min(a["lower"], a["upper"]) + 4 <= reached["session"]["target_raw"] <= max(a["lower"], a["upper"]) - 4
        self.screenshot("target")
        self.click("reset_poses")
        self.wait(lambda s: s["server"]["axes"]["2"]["lower"] is None and s["server"]["axes"]["2"]["upper"] is None)
        self.screenshot("reset")
        self.wait(lambda s: s["session"]["ready"])
        self.teaching()
        self.selected(3)
        self.action("speed", {"percent": 80})
        self.teaching(3)
        self.click("stop")
        self.stopped()
        self.selected()
        self.done()

        self.begin("HW-08")
        self.selected()
        self.action("target", {"percent": 50})
        self.action("target_commit")
        self.settled()
        # Checked before confirming, so no status job runs between Tune and STOP.
        assert satisfied(lambda s: s["server"]["axes"]["2"]["tuning"] is None, self.state()), \
            "motor 2 must start untuned"
        self.click("tune_confirm")
        self.click("tune")
        self.wait(lambda s: (s["server"].get("tuning") or {}).get("running"), 15, "tune running")
        # Nothing (no screenshot) runs between seeing the tune run and STOP.
        self.click("stop")
        stopped = self.stopped()
        # STOP really interrupted it: the stopped-path error and still no
        # gains on motor 2 (the tuning JSON has no `result` field to check).
        assert satisfied(lambda s: stop_interrupted(s, "tuning") and s["server"]["axes"]["2"]["tuning"] is None,
                         stopped), \
            ("tune finished before STOP", stopped["server"]["tuning"])
        self.retain("tune-interrupted.json", stopped["server"]["tuning"])
        self.screenshot("stopped")
        self.selected()
        self.click("tune_confirm")
        self.click("tune")
        # Stages of the FRESH tune only: polled ones while it runs (the
        # interrupted tune is no longer running), and the session's
        # `tune_stages`, which a tune start clears (session/sequences.rs).
        polled = []

        def tune_terminal(s):
            t = s["server"].get("tuning") or {}
            if t.get("running") is True and t.get("stage") and (not polled or polled[-1] != t["stage"]):
                polled.append(t["stage"])
            return not t.get("running") and bool(s["server"]["axes"]["2"].get("tuning")) and not s["form"]["tune_ok"]
        terminal = self.wait(tune_terminal, 120, "terminal tuning gains and record")
        tuning = terminal["server"]["axes"]["2"]["tuning"]
        assert all(math.isfinite(tuning[g]) for g in ["kp", "ki", "kd", "friction_duty"])
        stages = terminal["session"]["tune_stages"]
        assert isinstance(stages, list) and len(set(stages)) >= 2, stages
        record = (self.out / "records" / tuning["record"]).resolve()
        assert record.is_relative_to(self.out / "records") and record.is_file()
        self.retain("tune-stages.json", {"session": stages, "polled_while_running": polled})
        self.screenshot("terminal")
        self.selected(3)
        self.action("target", {"percent": 50})
        self.action("target_commit")
        self.settled()
        self.click("tune_confirm")
        self.click("tune")
        tuned_third = self.wait(lambda s: not (s["server"].get("tuning") or {}).get("running") and
            bool(s["server"]["axes"]["3"].get("tuning")) and not s["form"]["tune_ok"], 120, "second motor terminal tuning")
        third_record = (self.out / "records" / tuned_third["server"]["axes"]["3"]["tuning"]["record"]).resolve()
        assert third_record.is_relative_to(self.out / "records") and third_record.is_file()
        self.selected()
        self.done()

        self.begin("HW-07")
        self.click("sweep")
        self.wait(lambda s: s["session"]["sweeping"] and s["form"]["speed_percent"] == 0)
        self.action("speed", {"percent": 80})
        start_position = self.position(self.state())
        travel = self.wait(lambda s: abs(self.position(s) - start_position) >= 60, 20, "saved-range encoder travel")
        bounds = travel["server"]["axes"]["2"]
        assert min(bounds["lower"], bounds["upper"]) <= self.position(travel) <= max(bounds["lower"], bounds["upper"])
        self.click("sweep")
        self.held()
        self.click("sweep")
        self.wait(lambda s: s["session"]["sweeping"])
        self.action("loss", {"reason": "focus_lost"})
        self.stopped()
        assert self.state()["server"]["axes"]["2"]["lower"] is not None
        self.selected()
        self.action("speed", {"percent": 80})
        self.click("learn")
        self.wait(lambda s: s["session"]["learning"])

        def learned(s):
            # session.rs `render`: on completion the session keeps the run's
            # terminal adaptation in `learning_terminal`, sets learning false
            # and intent hold. sweep.latest.adaptation is transient.
            terminal = s["session"]["learning_terminal"]
            return (terminal["learning_complete"] is True and terminal["decreasing_stops"] >= 3
                    and terminal["increasing_stops"] >= 3 and not s["session"]["learning"]
                    and s["session"]["intent"] == "hold")
        learned_state = self.wait(learned, 150, "terminal learned stopping evidence")
        self.retain("learning-terminal.json", learned_state)
        self.screenshot("learned")
        # Learning already ended in hold; pressing Learn again would START a
        # new learning run (session/buttons.rs `learn`). Go straight to STOP.
        self.click("stop")
        self.stopped()
        self.selected()
        self.action("speed", {"percent": 80})
        self.click("sweep_all")
        self.wait(lambda s: s["session"]["sweep_all"] and (s["server"].get("sweep") or {}).get("all"))

        def all_ends(s):
            axes = (s["server"].get("sweep") or {}).get("axes") or {}
            return all((axes.get(str(motor)) or {}).get("half_cycles", 0) >= 2 for motor in [2, 3])
        ends = self.wait(all_ends, 150, "two taught motors completed both sweep ends")
        self.retain("sweep-all-ends.json", ends)
        self.wait(lambda s: not s["session"]["sweep_all"] and not (s["server"].get("sweep") or {}).get("running"),
                  20, "sweep-all terminal")
        self.screenshot("sweep-all")
        self.selected()
        self.click("sweep_all")
        self.wait(lambda s: s["session"]["sweep_all"])
        self.click("stop")
        self.stopped()
        self.done()

        self.begin("HW-09")
        self.selected()
        self.click("campaign_confirm")
        self.click("campaign")
        saved = self.wait(lambda s: s["server"]["campaign"]["running"] and s["server"]["campaign"]["completed"] >= 1,
                          180, "campaign saved stage")
        # Nothing (no screenshot, no record hashing) runs between seeing a
        # saved stage and STOP.
        self.click("stop")
        stopped = self.stopped()
        assert satisfied(lambda s: stop_interrupted(s, "campaign"), stopped), ("campaign finished before STOP", stopped["server"]["campaign"])
        completed = stopped["server"]["campaign"]["completed"]
        assert completed >= saved["server"]["campaign"]["completed"]
        receipts = {k: v for k, v in self.record_hashes().items()
                    if "/receipts/" in k and not k.endswith(".execution.json")}
        assert len(receipts) >= completed, receipts
        self.screenshot("stopped")
        self.selected()
        self.click("campaign_confirm")
        self.click("campaign_resume")
        self.wait(lambda s: s["server"]["campaign"]["running"] and s["server"]["campaign"]["completed"] >= completed)
        finish = self.wait(lambda s: s["server"]["campaign"]["result"] and not s["server"]["campaign"]["running"]
                           and not s["form"]["campaign_ok"], 300, "campaign terminal")
        directory = Path(finish["server"]["campaign"]["result"]["directory"]).resolve()
        assert directory.is_relative_to(self.out / "records") and (directory / "report.json").is_file()
        now = self.record_hashes()
        assert all(now.get(k) == v for k, v in receipts.items()), "completed stage rewritten during resume"
        self.screenshot("terminal")
        self.done()

        self.begin("LC1-real-server-refusal")
        self.click("stop")
        self.stopped()
        for name in ["raw_step", "gait_play", "sync_start"]:
            self.action(name, None, expect=OUT_OF_SCOPE)
        token = self.server_token_value()
        # No identity headers: the virtual server refuses execution as a binding.
        code, result = self.http(self.server, "POST", "/calibration/command", {"action": "inspect", "sequence": 1},
                                 {"X-Control-Token": token, "X-Client-Id": CLIENT_MISMATCH})
        assert code == BINDING_STATUS and str(result.get("error", "")).startswith(BINDING), (code, result)
        mismatch = {"X-Control-Token": token, "X-Client-Id": CLIENT_MISMATCH,
                    "X-Calibration-Server": FOREIGN_SERVER,
                    "X-Calibration-Bench": self.identity["bench_instance"], "X-Calibration-Generation": "1"}
        code, result = self.http(self.server, "POST", "/calibration/command", {"action": "select", "id": 2, "sequence": 2}, mismatch)
        assert code == BINDING_STATUS and str(result.get("error", "")).startswith(BINDING), (code, result)
        code, value = self.http(self.server, "POST", "/calibration/command", {"action": "stop", "sequence": 3}, mismatch)
        assert stop_reply_latched(code, value), ("STOP must bypass mismatched identity", code, value)
        self.stopped()
        old_identity = dict(self.identity)
        old_generation = self.state()["generation"]
        self.server_proc.terminate()
        self.server_proc.wait(timeout=3)
        self.wait(lambda s: s["stale"] or not s["connected"], 10, "server loss revokes")
        self.action("select", {"id": 2}, expect=EXPIRY)
        self.server_token = None
        self.server_log = "server-replacement"
        self.server_proc = self.spawn(self.server_log, self.server_argv)

        def replacement_ready():
            if self.server_proc.poll() is not None:
                raise RuntimeError("owned replacement server exited during readiness")
            try:
                return self.server_status()
            except (OSError, ValueError):
                return None
        replacement = self.poll(replacement_ready,
            lambda s: s["execution"]["kind"] == "virtual_calibration"
            and s["execution"]["bench_instance"] == old_identity["bench_instance"]
            and s["execution"]["server_instance"] != old_identity["server_instance"],
            20, "new server instance")
        assert self.server_owned(), "replacement listener is not our child"
        self.retain("replacement-execution.json", replacement["execution"])
        self.identity = replacement["execution"]
        old_headers = {"X-Control-Token": self.server_token_value(), "X-Client-Id": CLIENT_MISMATCH,
            "X-Calibration-Server": old_identity["server_instance"],
            "X-Calibration-Bench": old_identity["bench_instance"], "X-Calibration-Generation": str(old_generation)}
        code, error = self.http(self.server, "POST", "/calibration/command",
                                {"action": "select", "id": 2, "sequence": 4}, old_headers)
        assert code == BINDING_STATUS and str(error.get("error", "")).startswith(BINDING), (code, error)
        code, value = self.http(self.server, "POST", "/calibration/command", {"action": "stop", "sequence": 5}, old_headers)
        assert stop_reply_latched(code, value), ("STOP rejected for a replaced instance", code, value)
        self.action("select", {"id": 2}, expect=EXPIRY)
        self.click("connect")
        fresh = self.wait(lambda s: s["connected"] and not s["stale"] and s["generation"] != old_generation
                          and s["server"]["execution"] == self.identity)
        assert not fresh["session"]["ready"] and fresh["server"]["enabled_id"] is None
        self.done()

        self.begin("LC1-FIXTURE-physical-unknown")
        # FIXTURE phase: a second owned viewer reaches the owned server only
        # through the identity proxy, which presents it as physical/unknown.
        direct = self.server_status()
        assert direct["connected"] is True and direct["enabled_id"] is None, "fixture needs the idle connected bus"
        self.proxy = IdentityFixture(int(self.server.rsplit(":", 1)[1]), self.out / "identity-fixture.jsonl")
        proxy_url = self.proxy.start()
        self.retain("identity-fixture.json", {"url": proxy_url, "label": "FIXTURE ONLY; never virtual",
                                              "forward_timeout_s": FORWARD_TIMEOUT})
        base = self.launch_viewer("fixture", proxy_url)
        fixture_generation = None
        observations = []
        for mode in ["physical", "unknown"]:
            self.proxy.mode = mode
            if fixture_generation is not None:
                self.click("connect", base=base)
            state = self.wait(lambda s: s["connected"] and not s["stale"] and s["generation"] != fixture_generation
                              and s["server"]["fidelity"] == "physical_or_unknown", 90, f"fixture {mode} connection", base=base)
            assert state["url"] == proxy_url
            fixture_generation = state["generation"]
            before = self.proxy.forwarded_motion
            self.action("select", {"id": 2}, expect=VIRTUAL_REQUIRED, base=base)
            self.click("select_2", expect=VIRTUAL_REQUIRED, base=base)
            assert self.proxy.forwarded_motion == before, "refused action reached the executor"
            self.command("hardware_stop", base=base)
            observations.append({"mode": mode, "generation": fixture_generation, "crossings": self.proxy.forwarded_motion})
        self.stop_viewer("fixture")
        self.done(fixture=observations)
        self.results["ok"] = True

    # ---- cleanup --------------------------------------------------------
    def cleanup_server_stop(self):
        record = self.direct_stop("cleanup")
        self.retain("cleanup-server-stop.json", record)
        if record.get("sent") and not record.get("stop_latched"):
            self.results["ok"] = False

    def cleanup_viewer_stops(self):
        outcomes = {}
        for label, viewer in self.viewers.items():
            entry = outcomes[label] = {"sent": False}
            try:
                proc, base = viewer["proc"], viewer["base"]
                if not viewer["verified"] or proc.poll() is not None:
                    entry["why"] = "viewer not verified or not running"
                    continue
                code, raw = request(base, "GET", "/", timeout=1)
                root = decode_safe(raw)
                if code != 200 or not isinstance(root, dict) or root.get("pid") != proc.pid:
                    entry["why"] = "listener is not our viewer"
                    continue
                code, raw = request(base, "POST", "/v1/batch", {"commands": [{"command": "hardware_stop", "args": {}}],
                                                                "stop_on_error": True}, timeout=2)
                accepted = decode_safe(raw)
                entry.update(sent=True, http=code, accepted=accepted)
                path = accepted.get("url") if isinstance(accepted, dict) else None
                if isinstance(path, str) and JOB_PATH.match(path):
                    end = time.monotonic() + 5
                    while time.monotonic() < end:
                        code, raw = request(base, "GET", path, timeout=1)
                        job = decode_safe(raw)
                        entry["job"] = job
                        if isinstance(job, dict) and job.get("status") in TERMINAL:
                            break
                        time.sleep(.2)
            except Exception as e:
                entry["error"] = f"{type(e).__name__}: {e}"
        self.retain("cleanup-viewer-stops.json", outcomes)

    def terminate_processes(self):
        cleanup = []
        for name, proc in reversed(self.processes):
            # Popen retains unreaped ownership; no pgrep/global/group termination.
            try:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=3)
                cleanup.append({"name": name, "pid": proc.pid, "returncode": proc.returncode})
            except (OSError, subprocess.TimeoutExpired) as e:
                cleanup.append({"name": name, "pid": proc.pid, "error": str(e)})
                self.results["ok"] = False
        self.retain("cleanup-processes.json", cleanup)

    def close_proxy(self):
        if self.proxy:
            self.proxy.close()

    def close_logs(self):
        for log in self.logs:
            log.close()

    def cleanup(self):
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            try:
                signal.signal(sig, signal.SIG_IGN)
            except (OSError, ValueError):
                pass
        stages = []
        try:
            # Direct server STOP first: never wait behind the viewer's job queue.
            for name, stage in [("resume-server", self.resume_server), ("server-stop", self.cleanup_server_stop),
                                ("viewer-stop", self.cleanup_viewer_stops), ("processes", self.terminate_processes),
                                ("proxy", self.close_proxy), ("logs", self.close_logs)]:
                entry = {"stage": name, "ok": False}
                try:
                    stage()
                    entry["ok"] = True
                except BaseException as e:
                    entry["error"] = f"{type(e).__name__}: {e}"
                    self.results["ok"] = False
                finally:
                    stages.append(entry)
        finally:
            self.results["cleanup"] = stages
            self.results["screenshots"] = screenshot_summary(self.screenshots, self.results["steps"])
            try:
                self.results["record_sha256"] = self.record_hashes()
            finally:
                self.results["finished_at"] = time.time()
                self.retain("results.json", self.results)


BINARIES = ("bench", "server", "viewer")


def nonempty_file(path, label, executable=False):
    """Refusal text for a missing/empty (or non-executable) input, else None."""
    path = Path(path)
    if not path.is_file():
        return f"{label} {path} is missing or not a regular file"
    if path.stat().st_size == 0:
        return f"{label} {path} is empty"
    if executable and not os.access(path, os.X_OK):
        return f"{label} {path} is not executable"
    return None


def build_receipt(binaries, build_logs):
    """The fresh-build receipt `run` validates (source_sha256, binaries.<label>.sha256).

    Hashes bind already-built binaries to the current source tree; they do
    not prove compilation happened. The verifier inspects the build logs.
    """
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", commit):
        raise ValueError(f"unexpected git rev-parse HEAD output {commit!r}")
    status = subprocess.check_output(["git", "status", "--porcelain=v1", "-z", "--untracked-files=normal"], cwd=ROOT)
    provenance = source_hash()
    return {
        "source_sha256": provenance["sha256"],
        "source_commit": commit,
        "source_dirty": bool(status.strip(b"\0")),
        "source_provenance": provenance,
        "build_logs": [{"path": str(log), "bytes": log.stat().st_size, "sha256": sha(log)} for log in build_logs],
        "binaries": {label: {"path": str(path), "bytes": path.stat().st_size, "sha256": sha(path)}
                     for label, path in binaries.items()},
        "written_by": "tools/native-calibration/acceptance.py receipt (builds nothing)",
        "written_at": time.time(),
    }


def receipt_main(argv):
    p = argparse.ArgumentParser(prog="acceptance.py receipt",
        description="Write fresh-build-receipt.json for already-built binaries; builds nothing.")
    for label in BINARIES:
        p.add_argument(f"--{label}", type=Path, required=True)
    p.add_argument("--build-log", type=Path, action="append", default=[],
                   help="retained full build log; repeat for each")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args(argv)
    out = args.out.absolute()
    binaries = {label: getattr(args, label).resolve() for label in BINARIES}
    logs = [log.resolve() for log in args.build_log]
    problems = [] if logs else ["no --build-log given; the verifier must be able to inspect the build"]
    problems += [m for label, path in binaries.items() if (m := nonempty_file(path, label, executable=True))]
    problems += [m for log in logs if (m := nonempty_file(log, "build log"))]
    if out.exists() or out.is_symlink():
        problems.append(f"{out} exists; a receipt is never overwritten")
    if problems:
        for problem in problems:
            print(f"REFUSED: {problem}", file=sys.stderr)
        return 2
    text = json.dumps(build_receipt(binaries, logs), indent=2, allow_nan=False) + "\n"
    try:
        with out.open("x") as f:
            f.write(text)
    except OSError as e:
        print(f"REFUSED: {out}: {e}", file=sys.stderr)
        return 2
    print(json.dumps({"receipt": str(out), "source_sha256": json.loads(text)["source_sha256"]}))
    return 0


def main():
    argv = sys.argv[1:]
    if argv[:1] == ["receipt"]:
        return receipt_main(argv[1:])
    if argv[:1] == ["run"]:
        argv = argv[1:]
    return run_main(argv)


def run_main(argv):
    p = argparse.ArgumentParser(prog="acceptance.py [run]", description=__doc__)
    p.add_argument("--config", type=Path, default=ROOT / "tools/native-calibration/server.virtual.json")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--fresh-build-receipt", type=Path, required=True)
    p.add_argument("--bench", type=Path, required=True)
    p.add_argument("--server", type=Path, required=True)
    p.add_argument("--viewer", type=Path, required=True)
    p.add_argument("--total-timeout", type=int, default=1800)
    p.add_argument("--screenshots", action="store_true",
                   help="capture the README checkpoints with the viewer's REST screenshot (default off)")
    args = p.parse_args(argv)
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=False)
    run = Run(args)

    def interrupted(_signal, _frame):
        raise KeyboardInterrupt("verification interrupted; retain evidence and attempt STOP")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    try:
        if sys.flags.optimize:
            raise RuntimeError("verification requires Python assertions enabled; do not use -O")
        # No arbitrary endpoint, serial override, environment command or fallback options.
        cfg = json.loads(args.config.read_text())
        run.retain("input-config.json", cfg)
        if not isinstance(cfg, dict) or cfg.get("serial") != "virtual-capability-only":
            raise ValueError("REFUSED physical/PTY/unknown serial input before processes")
        assert str(cfg.get("fixture", "")).startswith("SIMULATED"), "virtual fixture label required (not authorization)"
        assert 60 <= args.total_timeout <= 1800
        receipt = json.loads(args.fresh_build_receipt.read_text())
        provenance = source_hash()
        run.retain("source-provenance.json", provenance)
        run.results["source_provenance"] = provenance
        assert receipt["source_sha256"] == provenance["sha256"], "stale source build receipt"
        for label in BINARIES:
            binary = getattr(args, label).resolve()
            assert binary.is_file() and os.access(binary, os.X_OK)
            assert receipt["binaries"][label]["sha256"] == sha(binary), "stale binary receipt"
            setattr(args, label, binary)
        run.retain("fresh-build-receipt.json", receipt)
        (run.out / "records").mkdir()
        cfg["output"] = str(run.out / "records")
        cfg["viewer"] = str(run.out / "token-page")
        (run.out / "token-page").mkdir()
        (run.out / "token-page/index.html").write_text('<!doctype html><html><body>SIMULATED calibration acceptance token discovery only.</body></html>\n')
        plan_source = ROOT / "examples/actuators/hx30hm/hardware/characterization-campaign/plan.hardware.json"
        plan = json.loads(plan_source.read_text())
        # Explicit bounded fixture: retain limits/gates, choose A+B only, no physical promotion.
        plan.update(a_speeds_counts_s=[40], b_positions=2, c_speeds_counts_s=[], d_duties=[],
                    f_step_counts=0, g_duties=[], h_multi_axis=False, k_repeat=False)
        run.retain("campaign-plan.json", plan)
        cfg["campaign_plan"] = str(run.out / "campaign-plan.json")
        run.retain("server.json", cfg)
        capability = run.out / "bench.sock"
        bench = run.spawn("bench", [args.bench, "--capability-socket", capability,
                                    "--identity-file", run.out / "bench-identity.json"])

        def bench_ready():
            # The bench creates the identity file before writing it: poll
            # until it parses and names our socket.
            if bench.poll() is not None:
                raise RuntimeError("owned bench exited during readiness")
            if not capability.exists():
                return None
            try:
                identity = json.loads((run.out / "bench-identity.json").read_text())
            except (OSError, ValueError):
                return None
            return identity
        bench_identity = run.poll(bench_ready, lambda i: i["capability_socket"] == str(capability)
                                  and i["kind"] == "virtual_calibration" and len(i["bench_instance"]) == 36,
                                  15, "bench capability and identity")
        run.retain("bench-identity-observed.json", bench_identity)
        port = vacant_port()
        run.server = f"http://127.0.0.1:{port}"
        run.server_argv = [args.server, run.out / "server.json", port, "--virtual-bench", capability]
        run.server_log = "server"
        run.server_proc = run.spawn(run.server_log, run.server_argv)

        def ready():
            if run.server_proc.poll() is not None or bench.poll() is not None:
                raise RuntimeError("owned server/bench exited during readiness")
            try:
                return run.server_status()
            except (OSError, ValueError):
                return None
        ready_state = run.poll(ready, lambda s: s["execution"]["schema_version"] == 1
                               and s["execution"]["kind"] == "virtual_calibration"
                               and s["execution"]["bench_instance"] == bench_identity["bench_instance"], 20,
                               "server virtual identity")
        if not run.server_owned():
            raise RuntimeError("calibration listener is not our child")
        run.identity = ready_state["execution"]
        run.base = run.launch_viewer("main", run.server)
        run.verify()
    except (Exception, KeyboardInterrupt) as e:
        run.results["error"] = {"step": run.step, "type": type(e).__name__, "message": str(e)}
        if run.results["steps"]:
            settle_step(run.results["steps"][-1], False)
    finally:
        run.cleanup()
    print(json.dumps({"ok": run.results["ok"], "evidence": str(run.out), "error": run.results.get("error"),
                      "screenshots": run.results.get("screenshots")}))
    return 0 if run.results["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
