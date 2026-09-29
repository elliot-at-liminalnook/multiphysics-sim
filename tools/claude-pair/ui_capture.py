#!/usr/bin/env python3
"""Drive the native viewer through its REST API and capture what it draws.

Launches the viewer on a free loopback port, runs a script of REST commands,
saves window screenshots (UI, panels and overlays exactly as drawn), writes a
capture.json receipt, and stops the viewer. Exit status is 0 only when every
step succeeded and every screenshot was written, so it works as a pair check.

    python3 ui_capture.py --out $PAIR_CAPTURES/board -- --system examples/systems-builder/motor-driver-board/board.system.json
    python3 ui_capture.py --out DIR --script steps.json -- --lessons lessons --lesson back-driving

A script is a JSON list of steps:
    {"command": "system_ui", "args": {"action": {"operation": "controls"}}}   one REST command
    {"batch": [{"command": "fit"}, {"command": "state"}]}                       several, atomically ordered
    {"get": "/v1/capabilities"}                                                 GET a resource
    {"wait": 1.5}                                                               let frames render
    {"screenshot": "after-click"}                                               save DIR/after-click.png
A final screenshot is added when the script takes none. GET /v1/capabilities
lists every command with an argument example; `system_ui` activates live
controls through the same handlers as a mouse click.

Standard library only. The viewer opens a real window, so it needs a logged-in
desktop session; it runs on this Mac only and never touches hardware.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def request(base, method, path, body=None, timeout=10):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"} if data is not None else {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except ValueError:
            return e.code, {"error": raw.decode(errors="replace")}


def run_batch(base, commands, timeout):
    status, accepted = request(base, "POST", "/v1/batch", {"commands": commands, "stop_on_error": True})
    if status != 202:
        return {"status": "rejected", "http": status, "response": accepted}
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        _, job = request(base, "GET", accepted["url"])
        if job.get("status") in ("succeeded", "failed", "cancelled"):
            return job
        time.sleep(0.05)
    return {"status": "timeout", "job_url": base + accepted["url"],
            "note": "Not resubmitted; the viewer is stopped after the script."}


def wait_for_file(path, timeout):
    end, last = time.monotonic() + timeout, -1
    while time.monotonic() < end:
        size = path.stat().st_size if path.exists() else -1
        if size > 0 and size == last:
            return True
        last = size
        time.sleep(0.25)
    return False


def stop(proc):
    if proc.poll() is None:
        os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, help="Directory for PNGs, capture.json and viewer.log")
    parser.add_argument("--binary", default="target/debug/sim-spatial",
                        help="Viewer executable (default: target/debug/sim-spatial; build it with cargo build -p sim-spatial)")
    parser.add_argument("--script", help="JSON file with a list of steps")
    parser.add_argument("--steps", help="Inline JSON list of steps (instead of --script)")
    parser.add_argument("--ready-timeout", type=float, default=180, help="Seconds to wait for the REST API")
    parser.add_argument("--settle", type=float, default=2, help="Seconds to let first frames render")
    parser.add_argument("--command-timeout", type=float, default=120)
    parser.add_argument("viewer_args", nargs=argparse.REMAINDER, help="Arguments after -- go to the viewer")
    args = parser.parse_args()
    viewer_args = args.viewer_args[1:] if args.viewer_args[:1] == ["--"] else args.viewer_args

    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    binary = Path(args.binary)
    if not binary.exists():
        print(f"Viewer binary not found: {binary}. Build it first (cargo build -p sim-spatial).", file=sys.stderr)
        return 2
    steps = json.loads(Path(args.script).read_text()) if args.script else json.loads(args.steps) if args.steps else []
    if not isinstance(steps, list):
        print("The script must be a JSON list of steps", file=sys.stderr)
        return 2
    if not any("screenshot" in s for s in steps):
        steps = steps + [{"screenshot": "final"}]

    port = free_port()
    base = f"http://127.0.0.1:{port}"
    argv = [str(binary.resolve()), "--api-port", str(port), *viewer_args]
    receipt = {"argv": argv, "cwd": os.getcwd(), "started_at": time.time(), "steps": [], "screenshots": [], "ok": False}
    log = (out / "viewer.log").open("wb")
    proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
    try:
        end = time.monotonic() + args.ready_timeout
        while True:
            if proc.poll() is not None:
                receipt["error"] = f"Viewer exited with {proc.returncode} before its API was ready; see viewer.log"
                return 1
            try:
                if request(base, "GET", "/v1/capabilities", timeout=2)[0] == 200:
                    break
            except (urllib.error.URLError, ConnectionError, socket.timeout):
                pass
            if time.monotonic() > end:
                receipt["error"] = "Viewer API did not become ready in time; see viewer.log"
                return 1
            time.sleep(0.25)
        time.sleep(args.settle)
        ok = True
        for step in steps:
            if proc.poll() is not None:
                receipt["error"] = f"Viewer exited with {proc.returncode} during the script; see viewer.log"
                ok = False
                break
            if "wait" in step:
                time.sleep(float(step["wait"]))
                result = {"status": "succeeded"}
            elif "get" in step:
                status, value = request(base, "GET", step["get"])
                result = {"status": "succeeded" if status == 200 else "failed", "http": status, "response": value}
            elif "screenshot" in step:
                path = out / (Path(step["screenshot"]).stem + ".png")
                path.unlink(missing_ok=True)
                result = run_batch(base, [{"command": "screenshot", "args": {"path": str(path)}}], args.command_timeout)
                if result.get("status") == "succeeded":
                    if wait_for_file(path, 30):
                        receipt["screenshots"].append(str(path))
                    else:
                        result = {**result, "status": "failed", "error": "screenshot file was not written"}
            elif "batch" in step or "command" in step:
                commands = step["batch"] if "batch" in step else [{"command": step["command"], "args": step.get("args", {})}]
                result = run_batch(base, commands, args.command_timeout)
            else:
                result = {"status": "failed", "error": "unknown step; use command, batch, get, wait or screenshot"}
            if "unknown variant" in json.dumps(result):
                result["hint"] = ("The viewer does not know this command. If it exists in the source, the binary "
                                  "is stale: rebuild it (cargo build -p sim-spatial) or pass --binary.")
            receipt["steps"].append({"step": step, "result": result})
            if result.get("status") != "succeeded":
                ok = False
                break
        receipt["ok"] = ok
        return 0 if ok else 1
    finally:
        stop(proc)
        log.close()
        receipt["finished_at"] = time.time()
        (out / "capture.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps({"ok": receipt["ok"], "screenshots": receipt["screenshots"],
                          "error": receipt.get("error"), "receipt": str(out / "capture.json")}, indent=2))


if __name__ == "__main__":
    sys.exit(main())
