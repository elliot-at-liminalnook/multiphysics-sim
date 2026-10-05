"""Build a two-wheel differential-drive rover through the native viewer's REST API, then drive it.

STATUS: this script was committed UNEXECUTED. No run evidence exists for this
flow in the commit that added it; its request shapes were checked by reading
the viewer's source only. rest_log.json records that status on every run.

It never touches hardware: it talks only to the viewer at
http://127.0.0.1:<port> and asks for nothing but CAD edits, file exports,
Build-mode simulation and Robot-mode simulation.

Launch (the viewer opens the seed .rcad in CAD mode; the seed is only used to
bring CAD mode up and is never edited or saved), then run this script:

    cargo run -p sim-spatial -- examples/wheeled-robot/baseline/robot.rcad
    python3 clients/python/examples/build_rover_over_rest.py --out /tmp/rover-<date>

The flow (each REST call is one small function so its shape can be changed in
one place; every call is tagged with its RV step id in DIR/rest_log.json):

  RV-01  CAD mode: a new document DIR/robot.rcad, then RoboCAD ops reproducing
         cad/scripts/wheeled_learning_fixture.py (geometry and materials).
  RV-02  joints, motor attachment, joint physics.
  RV-03  motors, encoders, IMU, battery, control, settings; later the save and
         the physical export DIR/robot.simrobot.json, whose cad_sha256 must
         equal the sha256 of DIR/robot.rcad.
  RV-04  four comment threads, pinned to the chassis, the left axle joint, the
         right axle joint and the passive wheel, each saying what the part is
         for; a reply on the left axle thread carrying a [left wheel](part:ID)
         part link; a link op adding the drive motors and IMU to the chassis
         thread.
  RV-06  DIR/robot.drive.json (sim.drive/1, numbers derived from the export)
         and DIR/robot.controller.json (sim.controller-binding/1).
  RV-05  Build mode on DIR/rover.system.json: rover, external controller and
         drive limiter, linked to the files above and wired.
  RV-07  Build live run: start, drive forward ~1 s, stop, pause.
  RV-40  Robot mode on the export (the script as a whole is checklist RV-40;
         its requests carry the docs/rover-checklist.md ids): RV-17 open the
         model with its binding, RV-18 run, RV-29 robot_drive forward 2 s,
         yaw 2 s, stop, RV-32 the robot_state drive fields, RV-33 save the
         drive recording into DIR; then pause.

The rover has two driven wheels (left and right, each an N20 motor on a
continuous axle with an encoder) and one unpowered passive wheel.

On any failure it sends a best-effort stop (Robot mode: robot_drive stop then
robot_run pause; Build mode: system_drive stop then system_run pause; nothing
else; a step that timed out first has its job cancelled with DELETE
/v1/jobs/{id}), writes the log, prints the failing step id and response and
exits 1. --out must not exist and must not lie under the repository's
examples/, cad/ or web/ (the viewer's protected set).
Stdlib only, Python 3.9+.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
import sys
import time
import urllib.error
import urllib.request

EXPORT_KIND = "rigid"  # cad_results export kind: flex=0, no planar hint (results/mod.rs ExportKind::Rigid), as the fixture exports
FORWARD_FRACTION = 0.6  # operating fraction of the free-running wheel speed
FORWARD_ACCEL = 0.5  # m/s^2, chosen as in the baseline profile
DEADMAN_S = 0.5
HTTP_TIMEOUT_S = 5.0
SCRIPT_STATUS = ("committed unexecuted: no run evidence existed for this flow when the script was "
                 "committed; this log is the record of one run of it")
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # loopback: never a proxy


class StepFailed(Exception):
    def __init__(self, step, name, message, response=None):
        super().__init__(f"{step} {name}: {message}")
        self.step, self.name, self.message, self.response = step, name, message, response


class Rest:
    """The loopback REST client and the request log."""

    def __init__(self, port):
        self.base = f"http://127.0.0.1:{port}"
        self.log = []
        self.started = time.monotonic()
        self.mode = None  # "cad" | "build" | "robot": picks the best-effort stop
        self.step, self.step_name = "RV-01", "start"  # the step in progress, for unexpected errors

    def at(self, step, name):
        """Mark the step in progress (every request marks it; local steps call it directly)."""
        self.step, self.step_name = step, name

    def record(self, step, name, **entry):
        self.log.append(dict(step=step, name=name, t_s=round(time.monotonic() - self.started, 3), **entry))

    def http(self, step, name, method, path, body=None):
        """One HTTP request; returns (status, parsed JSON). Transport errors fail the step."""
        self.at(step, name)
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, method=method,
                                     headers={"Content-Type": "application/json"} if data else {})
        try:
            with OPENER.open(req, timeout=HTTP_TIMEOUT_S) as r:
                return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            text = e.read().decode(errors="replace")
            try:
                return e.code, json.loads(text)
            except ValueError:
                return e.code, text
        except (urllib.error.URLError, http.client.HTTPException, OSError, ValueError) as e:
            raise StepFailed(step, name, f"{method} {path}: {e}")

    def batch(self, step, name, commands, deadline_s, log_full=True):
        """POST /v1/batch, then poll its job until succeeded, failed or cancelled (sim-api lib.rs route)."""
        body = {"commands": commands}
        status, accepted = self.http(step, name, "POST", "/v1/batch", body)
        if status != 202 or not isinstance(accepted, dict) or "url" not in accepted:
            self.record(step, name, method="POST", path="/v1/batch", request=body, status=status, response=accepted)
            raise StepFailed(step, name, f"batch not accepted (HTTP {status})", accepted)
        end, polls = time.monotonic() + deadline_s, 0
        while True:
            status, job = self.http(step, name, "GET", accepted["url"])
            polls += 1
            if status != 200:
                break
            if job.get("status") in ("succeeded", "failed", "cancelled"):
                break
            if time.monotonic() > end:
                self.record(step, name, method="POST", path="/v1/batch", request=body, accepted=accepted, polls=polls, response=job)
                cancel = self.cancel_job(step, name, accepted["url"])
                raise StepFailed(step, name, f"no answer within {deadline_s} s; cancel requested with DELETE {accepted['url']} "
                                 f"(not resubmitted); the job's state after the cancel: {cancel}", job)
            time.sleep(0.05)
        shown = job if log_full or not isinstance(job, dict) else {
            "status": job.get("status"),
            "results": [{k: r.get(k) for k in ("index", "command", "ok", "error")} for r in job.get("results", [])],
            "trimmed": "state answers are logged by their observed fields only"}
        self.record(step, name, method="POST", path="/v1/batch", request=body, accepted=accepted, polls=polls, status=status, response=shown)
        if status != 200 or job.get("status") != "succeeded":
            raise StepFailed(step, name, f"job {job.get('status') if isinstance(job, dict) else status}", job)
        return [r.get("value") for r in job["results"]]

    def cancel_job(self, step, name, url):
        """DELETE /v1/jobs/{id}: sets cancel_requested and answers the job (sim-api lib.rs route). Unstarted
        commands are skipped; a pending one (e.g. a RoboCAD edit already sent) must settle first."""
        try:
            status, job = self.http(step, name + " (cancel)", "DELETE", url)
        except StepFailed as e:
            status, job = None, e.message
        self.record(step, name + " (cancel)", method="DELETE", path=url, status=status, response=job)
        return job.get("status") if isinstance(job, dict) else job

    def command(self, step, name, command, args, deadline_s=30.0, log_full=True):
        """One command; returns its answer (the job result's `value`)."""
        return self.batch(step, name, [{"command": command, "args": args}], deadline_s, log_full)[0]

    def poll(self, step, name, command, observe, ready, deadline_s, interval_s=0.25, failed=None):
        """Read a state command until ready(state); observe(state) is what the log keeps per poll.
        A refused read (e.g. the mode is still switching) counts as not ready."""
        end, last = time.monotonic() + deadline_s, None
        while True:
            try:
                state = unwrap(self.command(step, name, command, {}, 10.0, log_full=False), command)
            except StepFailed as e:
                last = {"refused": e.message, "response": e.response}
            else:
                last = observe(state)
                self.record(step, name, observed=last)
                why = failed(state) if failed else None
                if why:
                    raise StepFailed(step, name, why, last)
                if ready(state):
                    self.record(step, name, ready_state=state)
                    return state
            if time.monotonic() > end:
                raise StepFailed(step, name, f"{command} not ready within {deadline_s} s", last)
            time.sleep(interval_s)

    def best_effort_stop(self):
        """Stop motion in the active mode; outcomes are recorded and never raised. Never anything else.
        The viewer runs one command at a time in queue order (sim-api Server::poll), so a stop queued
        behind a long job (a cancelled one still settling) may run late: hence its own longer deadline."""
        calls = {"robot": [("robot_drive", {"stop": True}), ("robot_run", {"action": "pause"})],
                 "build": [("system_drive", {"stop": True}), ("system_run", {"action": "pause"})]}.get(self.mode, [])
        outcomes = []
        for command, args in calls:
            try:
                self.command("STOP", f"best-effort {command}", command, args, 60.0, log_full=False)
                outcomes.append({"command": command, "args": args, "ok": True})
            except StepFailed as e:
                outcomes.append({"command": command, "args": args, "ok": False, "error": e.message})
        self.record("STOP", "best-effort stop", mode=self.mode, outcomes=outcomes,
                    note="a stop queued behind a long job may have run late (one command at a time, in queue order)")
        return outcomes


def unwrap(answer, key):
    """robot_state (and robot_drive) answer {"robot_state": {...}}; others answer the state itself."""
    return answer[key] if isinstance(answer, dict) and isinstance(answer.get(key), dict) else answer


def get(value, *path):
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def same_path(a, b):
    return bool(a) and bool(b) and os.path.realpath(a) == os.path.realpath(b)


# ---------------------------------------------------------------- CAD (RV-01..RV-04)

def cad_state_paths(state):
    """The cad_state fields this script reads, in one place (cad/snapshot.rs state_json, results/export.rs)."""
    return {"connection": get(state, "connection", "state"), "path": get(state, "health", "path"),
            "dirty": get(state, "health", "dirty"), "unsaved": state.get("unsaved"), "edit": state.get("edit"),
            "stale": state.get("stale"), "revision": state.get("revision"),
            "export_running": get(state, "results", "exports", "running"),
            "export_last": get(state, "results", "exports", "last"),
            "export_recent": get(state, "results", "exports", "recent")}


def export_outcome(state, seq):
    """The finished export with this seq: in results.exports.recent (Part A's bounded history of the
    same objects as last, keyed by seq) or in results.exports.last; None while it has not finished."""
    paths = cad_state_paths(state)
    for entry in list(paths["export_recent"] or []) + [paths["export_last"]]:
        if isinstance(entry, dict) and entry.get("seq") == seq:
            return entry
    return None


def open_new_document(rest, seed, rcad):
    """RV-01: bring CAD mode up on the seed (read-only, never edited or saved), then cad_file new at rcad."""
    active = get(rest.command("RV-01", "viewer mode", "viewer_mode", {}), "active")
    if active != "cad":
        rest.command("RV-01", "enter CAD mode on the seed", "viewer_mode", {"mode": "cad", "path": seed}, 120.0)
    rest.mode = "cad"
    rest.poll("RV-01", "RoboCAD connected", "cad_state", cad_state_paths,
              lambda s: cad_state_paths(s)["connection"] == "connected", 180.0)
    rest.command("RV-01", "new document", "cad_file", {"op": "new", "path": rcad}, 120.0)
    rest.poll("RV-01", "new document open", "cad_state", cad_state_paths,
              lambda s: cad_state_paths(s)["connection"] == "connected" and same_path(cad_state_paths(s)["path"], rcad), 180.0)


def op_result_id(step, name, answer):
    """The node id RoboCAD returned: cad_op answers {message, result: OpResult {result, history, job}}
    (cad/actions.rs CadOp, sync/mod.rs edit_results, cad_client types.rs OpResult)."""
    for candidate in (get(answer, "result", "result"), get(answer, "result")):
        if isinstance(candidate, str) and candidate:
            return candidate
    raise StepFailed(step, name, "the cad_op answer carries no node id at result.result", answer)


def cad_op(rest, step, name, op, args, kwargs=None, want_id=False):
    """One RoboCAD op: {"command":"cad_op","args":{"name","args","kwargs"}}."""
    answer = rest.command(step, name, "cad_op", {"name": op, "args": args, "kwargs": kwargs or {}}, 120.0)
    return op_result_id(step, name, answer) if want_id else answer


def build_fixture(rest):
    """cad/scripts/wheeled_learning_fixture.py:18-61, the same values, through cad_op. Returns node ids."""
    ids = {}
    friction = {"coulomb": 0.0001, "viscous": 0.00001, "stribeck": 0., "stribeck_speed": 0.1, "static_ratio": 1.}
    ids["chassis"] = cad_op(rest, "RV-01", "chassis box", "box", [[-70., -40., 35.], [120., 80., 10.]], {"name": "chassis"}, True)
    cad_op(rest, "RV-01", "chassis material", "set_material", [[ids["chassis"]], "petg"])
    for side, y in [("left", 60.), ("right", -60.)]:
        sign = 1. if y > 0. else -1.
        wheel = cad_op(rest, "RV-01", f"{side} wheel", "cylinder", [[25., y - 6., 30.], [0., 1., 0.], 30., 12.], {"name": side + " wheel"}, True)
        cad_op(rest, "RV-01", f"{side} wheel material", "set_material", [[wheel], "petg"])
        motor = cad_op(rest, "RV-03", f"{side} drive motor", "add_motor", ["n20_100", [25., sign * 45., 30.], [0., sign, 0.]],
                       {"mount_on": ids["chassis"], "name": side + " drive"}, True)
        joint = cad_op(rest, "RV-02", f"{side} axle", "add_joint", ["continuous", ids["chassis"], wheel, [25., y, 30.], [0., 1., 0.]],
                       {"name": side + " axle"}, True)
        cad_op(rest, "RV-02", f"{side} attach motor", "attach_motor", [joint, motor])
        cad_op(rest, "RV-02", f"{side} axle physics", "set_joint_physics", [joint], {
            "drive_backlash": {"width_rad": 0., "provenance": "estimated",
                               "reference": "fixture assumes a rigid shaft-wheel coupling; motor gearbox backlash remains separate",
                               "uncertainty_rad": None},
            "friction": dict(friction)})
        encoder = cad_op(rest, "RV-03", f"{side} encoder", "add_sensor", ["encoder", wheel, [25., y, 30.]],
                         {"joint": joint, "name": side + " encoder"}, True)
        ids.update({side + " wheel": wheel, side + " drive": motor, side + " axle": joint, side + " encoder": encoder})
    ids["passive wheel"] = cad_op(rest, "RV-01", "passive wheel", "cylinder", [[-55., -5., 20.], [0., 1., 0.], 20., 10.], {"name": "passive wheel"}, True)
    cad_op(rest, "RV-01", "passive wheel material", "set_material", [[ids["passive wheel"]], "petg"])
    ids["passive axle"] = cad_op(rest, "RV-02", "passive axle", "add_joint", ["continuous", ids["chassis"], ids["passive wheel"], [-55., 0., 20.], [0., 1., 0.]],
                                 {"name": "passive axle"}, True)
    cad_op(rest, "RV-02", "passive axle physics", "set_joint_physics", [ids["passive axle"]], {"friction": dict(friction)})
    ids["body imu"] = cad_op(rest, "RV-03", "body imu", "add_sensor", ["imu", ids["chassis"], [0., 0., 40.]], {"name": "body imu"}, True)
    cad_op(rest, "RV-03", "battery", "set_battery", [], {"cells": 5, "chemistry": "nimh", "capacity_ah": 0.5})
    cad_op(rest, "RV-03", "control", "set_control", [], {"period_s": 0.02, "latency_s": 0.001, "targets": {"left axle": 0., "right axle": 0.}})
    cad_op(rest, "RV-03", "world setting", "set_robot_setting", ["world", {"floor_z": 0., "floor_material": "world",
           "floor_stiffness": 2e5, "floor_damping": 2e3, "terrain": None}])
    assumptions = {
        "status": "uncalibrated synthetic CAD benchmark",
        "geometry": "three solid PETG wheels, rectangular PETG chassis, library N20 motor bodies",
        "mass": "CAD volume and material density, plus library motor mass; no separate battery body modeled",
        "joint_friction": "estimated 0.0001 N m Coulomb and 0.00001 N m s/rad viscous per axle",
        "actuators": "library N20 equivalent electrical and gearbox estimates; position firmware is a simulated external controller",
        "topology": "two powered continuous axles and a passive continuous rear axle; no steering caster",
        "purpose": "exercise shared contracts and locomotion APIs on different morphology; not a manufactured robot specification",
    }
    cad_op(rest, "RV-03", "benchmark assumptions", "set_robot_setting", ["benchmark_assumptions", assumptions])
    return ids


def wait_cad_settled(rest, step, name, expect_path=None):
    """No edit in flight and the shown document not behind RoboCAD's (thread commits are refused otherwise).
    With expect_path, RoboCAD's document (health.path) must be that file, else the step fails naming it."""
    def wrong(s):
        path = cad_state_paths(s)["path"]
        if expect_path and path and not same_path(path, expect_path):
            return f"cad_state.health.path is {path!r}, not the new document {expect_path!r}"
        return None
    rest.poll(step, name, "cad_state", cad_state_paths,
              lambda s: s.get("edit") is None and s.get("stale") is None and get(s, "connection", "state") == "connected"
              and (expect_path is None or same_path(cad_state_paths(s)["path"], expect_path)), 60.0, failed=wrong)


def thread_id(step, name, answer):
    """A created thread's id: RoboCAD's thread at answer.result.id (threads/source.rs), else answer.id."""
    for candidate in (get(answer, "result", "id"), get(answer, "id")):
        if isinstance(candidate, str) and candidate:
            return candidate
    raise StepFailed(step, name, "the cad_threads answer carries no thread id at result.id or id", answer)


def threads_call(rest, name, args):
    """cad_threads op after {"op":"list"} in one ordered batch (list waits for the threads at the current revision)."""
    wait_cad_settled(rest, "RV-04", name + " (settled)")
    results = rest.batch("RV-04", name, [{"command": "cad_threads", "args": {"op": "list"}},
                                        {"command": "cad_threads", "args": args}], 60.0)
    return results[1]


def annotate(rest, ids):
    """RV-04: one thread per part explaining its purpose, one reply, one link."""
    author = "rover script"
    # Pins: the chassis body, each driven axle JOINT (RoboCAD anchors a thread to any node that
    # exists; a joint has no geometry stamp, so its pin reads as attached: annotations.py anchor)
    # and the passive wheel body. Points are in mm, at the joint origin or on the part.
    pins = [("chassis", "chassis", [0., 0., 45.], "The chassis is the rover's body: it carries the two N20 drive motors (left drive, right drive), "
             "the body IMU and the battery (5-cell NiMH, a robot setting with no body of its own)."),
            ("left axle", "left axle", [25., 60., 30.], "The left axle is the continuous joint between the chassis and the left wheel. "
             "The left N20 motor (left drive) turns it, and the left encoder reads its angle for the controller."),
            ("right axle", "right axle", [25., -60., 30.], "The right axle is the continuous joint between the chassis and the right wheel. "
             "The right N20 motor (right drive) turns it, and the right encoder reads its angle for the controller."),
            ("passive wheel", "passive wheel", [-55., 0., 40.], "The passive wheel is unpowered: it turns freely on the continuous joint 'passive axle' "
             "and keeps the chassis level behind the two driven wheels.")]
    threads = {}
    for key, part, point, body in pins:
        answer = threads_call(rest, f"thread on {part}", {"op": "create", "node": ids[part], "point": point, "body": body, "author": author})
        threads[key] = thread_id("RV-04", f"thread on {part}", answer)
    # A part link in the dock's own text form, [label](part:ID) (threads.rs insert_link); RoboCAD
    # adds a link written that way to the thread's linked parts (annotations.py).
    threads_call(rest, "reply on the left axle", {"op": "reply", "thread": threads["left axle"], "author": author,
                 "body": f"The encoder counts axle turns, not ground travel: wheel slip on the floor does not show in it. "
                         f"The wheel it turns is [left wheel](part:{ids['left wheel']})."})
    threads_call(rest, "link the chassis thread", {"op": "link", "thread": threads["chassis"],
                 "ids": [ids["left drive"], ids["right drive"], ids["body imu"]]})
    return threads


def save_and_export(rest, rcad, model):
    """RV-03: cad_save to rcad, then the physical export; its cad_sha256 must be the saved file's."""
    wait_cad_settled(rest, "RV-03", "settled before save, on the new document", expect_path=rcad)
    rest.command("RV-03", "save", "cad_save", {}, 120.0)  # the document was created at rcad (cad_file new)
    rest.poll("RV-03", "saved", "cad_state", cad_state_paths, lambda s: s.get("unsaved") is False, 60.0)
    started = rest.command("RV-03", "export", "cad_results", {"op": "export", "path": model, "kind": EXPORT_KIND}, 30.0)
    seq = started.get("seq") if isinstance(started, dict) else None
    if not isinstance(seq, int):
        raise StepFailed("RV-03", "export", "the export answer has no seq (expected {started, path, seq, message})", started)
    # The finished export's object carries its seq (cad/results/export.rs Exports::json: last, recent).
    state = rest.poll("RV-03", "export finished", "cad_state", cad_state_paths,
                      lambda s: export_outcome(s, seq) is not None, 900.0, 1.0)
    last = export_outcome(state, seq)
    if last.get("ok") is not True:
        raise StepFailed("RV-03", "export finished", "the export failed", last)
    digest = last.get("cad_sha256")
    with open(rcad, "rb") as f:
        actual = hashlib.sha256(f.read()).hexdigest()
    if not digest or digest != actual:
        raise StepFailed("RV-03", "export hash", f"export cad_sha256 {digest!r} is not the sha256 of {rcad} ({actual}); "
                         f"reason: {last.get('cad_sha256_reason')!r}", last)
    return {"cad_sha256": digest, "cad_sha256_reason": last.get("cad_sha256_reason"), "rcad_sha256": actual}


# ---------------------------------------------------------------- profile and binding (RV-06)

def floor_to(x, step):
    return round(math.floor(x / step + 1e-9) * step, 6)


def wheel_reading(model, joint_name):
    """Mirror sim-domain-robot drive_geometry.rs wheel(): radius is the largest distance of the child
    link's collision vertices (link frame) from the joint axis line through origin - child.com;
    free speed is the one motor on the joint: gearbox.max_output_speed / gear_ratio."""
    joint = next((j for j in model["joints"] if j.get("name") == joint_name), None)
    if joint is None or joint.get("type") != "continuous":
        raise StepFailed("RV-06", "derive profile", f"the export has no continuous joint {joint_name!r}")
    link = next((l for l in model["links"] if l.get("name") == joint["child"]), None)
    vertices = get(link, "collision", "vertices") or []
    if not vertices:
        raise StepFailed("RV-06", "derive profile", f"link {joint['child']!r} of {joint_name!r} has no collision vertices")
    length = math.sqrt(sum(c * c for c in joint["axis"]))
    axis = [c / length for c in joint["axis"]]
    centre = [o - c for o, c in zip(joint["origin"], link["com"])]
    radius = 0.0
    for v in vertices:
        d = [a - b for a, b in zip(v, centre)]
        along = sum(a * b for a, b in zip(d, axis))
        radius = max(radius, math.sqrt(sum((a - b * along) ** 2 for a, b in zip(d, axis))))
    motors = [m for m in model.get("motors", []) if m.get("joint") == joint_name]
    if len(motors) != 1:
        raise StepFailed("RV-06", "derive profile", f"joint {joint_name!r} has {len(motors)} motors; one is needed")
    out, ratio = motors[0]["gearbox"]["max_output_speed"], motors[0].get("gear_ratio", 1.0)
    return {"joint": joint_name, "link": joint["child"], "radius_m": radius, "origin_y_m": joint["origin"][1],
            "motor": motors[0]["name"], "max_output_speed": out, "gear_ratio": ratio, "free_speed_rad_s": out / ratio}


def write_profile_and_binding(out_dir, model_path):
    """RV-06: DIR/robot.drive.json from the export's numbers and DIR/robot.controller.json."""
    with open(model_path) as f:
        model = json.load(f)
    left, right = wheel_reading(model, "left axle"), wheel_reading(model, "right axle")
    if abs(left["radius_m"] - right["radius_m"]) > 1e-6:
        raise StepFailed("RV-06", "derive profile", "the wheel radii differ", [left, right])
    radius = (left["radius_m"] + right["radius_m"]) / 2
    track = left["origin_y_m"] - right["origin_y_m"]
    if not track > 0:
        raise StepFailed("RV-06", "derive profile", f"track {track} m is not positive", [left, right])
    free = min(left["free_speed_rad_s"], right["free_speed_rad_s"])
    period = get(model, "control", "period_s")
    if not (isinstance(period, (int, float)) and DEADMAN_S > period):
        raise StepFailed("RV-06", "derive profile", f"deadman {DEADMAN_S} s must exceed control.period_s {period!r}")
    forward = floor_to(FORWARD_FRACTION * free * radius, 0.01)
    yaw = floor_to(2 * forward / track, 0.1)
    yaw_accel = floor_to(2 * FORWARD_ACCEL / track, 0.1)
    read = (f"robot.simrobot.json motors[{left['motor']}, {right['motor']}].gearbox.max_output_speed "
            f"{left['max_output_speed']}, {right['max_output_speed']} rad/s / gear_ratio {left['gear_ratio']}, {right['gear_ratio']} "
            f"(free wheel speed {free} rad/s); wheel radius {radius} m (links[{left['link']}, {right['link']}].collision.vertices "
            f"about joints[left axle, right axle].axis); track {track} m (joints[left axle].origin y {left['origin_y_m']} - "
            f"joints[right axle].origin y {right['origin_y_m']})")
    uncal = "Estimated, uncalibrated; written by clients/python/examples/build_rover_over_rest.py."
    profile = {
        "schema": "sim.drive/1",
        "description": "Teleoperation drive for the REST-built rover: two N20-driven wheels mixed as a differential drive, "
                       "no lateral axis. Limits are a chosen 60% operating fraction of the motors' free-running wheel speed, "
                       "derived from robot.simrobot.json; geometry is derived from the model.",
        "kinematics": {"type": "differential", "left": "left axle", "right": "right axle"},
        "geometry": {"source": "model"},
        "axes": {
            "forward": {"max_speed": {"value": forward, "unit": "m/s"}, "max_accel": {"value": FORWARD_ACCEL, "unit": "m/s^2"},
                        "stop_decel": {"value": 2 * FORWARD_ACCEL, "unit": "m/s^2"},
                        "provenance": {"kind": "estimated", "source": f"max_speed: floor to 0.01 of {FORWARD_FRACTION} x {free} rad/s x {radius} m "
                                       f"= {forward} m/s; read: {read}. max_accel chosen as the baseline profile's 0.5 m/s^2; "
                                       f"stop_decel twice that. {uncal}"}},
            "yaw": {"max_speed": {"value": yaw, "unit": "rad/s"}, "max_accel": {"value": yaw_accel, "unit": "rad/s^2"},
                    "stop_decel": {"value": round(2 * yaw_accel, 6), "unit": "rad/s^2"},
                    "provenance": {"kind": "estimated", "source": f"max_speed: floor to 0.1 of 2 x {forward} m/s / track {track} m = {yaw} rad/s; "
                                   f"max_accel: floor to 0.1 of 2 x {FORWARD_ACCEL} / {track} = {yaw_accel} rad/s^2; stop_decel twice that; "
                                   f"read: {read}. The {FORWARD_FRACTION} fraction is a chosen margin. {uncal}"}},
        },
        "actions": [{"name": "stop", "request": "stop", "description": "Bring the rover to rest under the profile's acceleration limits."},
                    {"name": "halt", "request": "halt", "description": "Zero the twist at once."}],
        "deadman": {"timeout_s": DEADMAN_S, "on_loss": "ramp",
                    "provenance": {"kind": "estimated", "source": f"0.5 s: {DEADMAN_S / period:.0f} control periods of {period} s "
                                   f"(robot.simrobot.json control.period_s); ramp uses each axis's stop_decel. Chosen, not measured. {uncal}"}},
    }
    script = os.path.join(REPO, "clients", "python", "examples", "diff_drive_rover.py")
    try:
        # The viewer canonicalizes binding_dir/script (controller_binding.rs load), resolving symlinks such as
        # /tmp -> /private/tmp before `..`; a relative path from the resolved directory stays correct.
        script_ref = os.path.relpath(os.path.realpath(script), os.path.realpath(out_dir))
    except ValueError:
        script_ref = os.path.realpath(script)
    binding = {"schema": "sim.controller-binding/1",
               "description": "The REST-built rover's teleoperation controller: the shared diff_drive_rover.py simloop program.",
               "controller": {"language": "python", "script": script_ref, "args": []},
               "drive_profile": "robot.drive.json"}
    for name, doc in (("robot.drive.json", profile), ("robot.controller.json", binding)):
        with open(os.path.join(out_dir, name), "x") as f:
            json.dump(doc, f, indent=2)
            f.write("\n")
    return {"forward_m_s": forward, "yaw_rad_s": yaw, "yaw_accel": yaw_accel, "radius_m": radius, "track_m": track,
            "free_speed_rad_s": free, "script": script_ref, "period_s": period}


# ---------------------------------------------------------------- Build (RV-05, RV-07)

def system_wiring_commands():
    """sim_system::Command list (commands.rs; tag "command", snake_case). The three instances are
    hosted: link_file names the file each one stands for (relative to the system file), and their
    implementation parameters (model, seam period, limits) come from those files at run time, never
    from the document (system_robot::resolve refuses any other parameter on them). The controller's
    sense.command.* parameters only declare its port members (their value is ignored, external.rs);
    the wiring is limiter twist -> controller command channels. No lateral: the profile has no lateral axis."""
    def element(component_type):
        return {"kind": {"kind": "element", "component_type": component_type}}
    commands = [{"command": "add_instance", "at": "", "name": n, "instance": element(t)}
                for n, t in (("rover", "robot.articulated"), ("controller", "control.external"), ("limiter", "control.drive_limiter"))]
    commands += [{"command": "link_file", "instance": "rover", "path": "robot.simrobot.json"},
                 {"command": "link_file", "instance": "controller", "path": "robot.controller.json"},
                 {"command": "link_file", "instance": "limiter", "path": "robot.drive.json"}]
    for axis in ("forward", "yaw"):
        commands.append({"command": "set_parameter", "at": "", "name": "controller", "parameter": f"sense.command.{axis}", "binding": {"value": 1}})
        commands.append({"command": "connect", "at": "", "label": f"twist {axis}", "terminals": [
            {"instance": "limiter", "port": f"twist.{axis}"}, {"instance": "controller", "port": f"sense.command.{axis}"}]})
    return commands


def build_live_run(state):
    """The live-run JSON in system_state (builder.rs state_json "live_run"; Part B adds its "drive")."""
    return state.get("live_run") if isinstance(state, dict) else None


def wire_in_build(rest, system_path):
    """RV-05: an empty sim.system/2 file, Build mode on it, one `system` edit (the file store saves each apply)."""
    doc = {"schema": "sim.system/2", "title": "Two-wheel rover", "revision": 0, "root": "root",
           "definitions": {"root": {"label": "Two-wheel rover"}}}
    with open(system_path, "x") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    rest.command("RV-05", "enter Build mode", "viewer_mode", {"mode": "build", "path": system_path}, 120.0)
    rest.mode = "build"
    observe = lambda s: {k: s.get(k) for k in ("path", "revision", "compiling", "compile_error", "status")}
    rest.poll("RV-05", "system open", "system_state", observe, lambda s: same_path(s.get("path"), system_path), 120.0)
    rest.command("RV-05", "wire the rover", "system", {"label": "Wire the rover", "commands": system_wiring_commands()}, 60.0)
    rest.poll("RV-05", "compiled", "system_state", observe, lambda s: not s.get("compiling"), 180.0,
              failed=lambda s: s.get("compile_error") and f"compile_error: {s.get('compile_error')}")


def drive_steadily(rest, step, name, command, args, seconds, rate_hz=10.0):
    """Re-send one drive request at a steady rate (faster than the 0.5 s deadman). Each answer is a
    whole robot_state, so the log keeps each request's outcome only; the final state is logged in full."""
    start = time.monotonic()
    for i in range(int(round(seconds * rate_hz))):
        delay = start + i / rate_hz - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        rest.command(step, name, command, args, 2.0, log_full=False)


def live_run_in_build(rest):
    """RV-07: start the live run, drive forward ~1 s, stop, read the drive fields, pause."""
    rest.command("RV-07", "start live run", "system_run", {"action": "start"}, 30.0)
    observe = lambda s: build_live_run(s)
    rest.poll("RV-07", "live run running", "system_state", observe,
              lambda s: get(build_live_run(s), "phase") == "running" and get(build_live_run(s), "error") is None, 120.0,
              failed=lambda s: get(build_live_run(s), "error") and f"live_run.error: {get(build_live_run(s), 'error')}")
    drive_steadily(rest, "RV-07", "drive forward", "system_drive", {"forward": 0.5}, 1.0)
    rest.command("RV-07", "stop", "system_drive", {"stop": True}, 5.0)
    state = rest.command("RV-07", "read drive", "system_state", {}, 10.0)
    rest.command("RV-07", "pause live run", "system_run", {"action": "pause"}, 10.0)
    return get(build_live_run(state), "drive")


# ---------------------------------------------------------------- Robot mode (RV-40)

def robot_observe(s):
    d = s.get("drive") or {}
    return {"status": s.get("status"), "error": s.get("error"), "run_phase": get(s, "run", "phase"), "run_error": get(s, "run", "error"),
            "drive_bound": d.get("bound"), "binding_error": d.get("binding_error"), "profile": get(d, "profile", "path"),
            "drive_status": d.get("status"), "recording_pending": get(s, "recording", "pending"),
            "recording_error": get(s, "recording", "error"), "last_saved": get(s, "recording", "last_saved", "path")}


def robot_failed(s):
    """A load or binding failure, naming robot_state's field (drive is {bound: false, binding_error}
    when the binding beside the model failed to load: robot/state.rs drive)."""
    o = robot_observe(s)
    if o["status"] == "error":
        return f"robot_state.error: {o['error']}"
    if o["drive_bound"] is False:
        return f"robot_state.drive.binding_error: {o['binding_error']}"
    if o["run_phase"] == "failed" or o["run_error"]:
        return f"robot_state.run.error (binding or run failure): {o['run_error']}"
    return None


def drive_in_robot_mode(rest, model, profile_path, recording):
    """RV-40 (the script as a whole): RV-17 open the export with its binding, RV-18 run, RV-29 drive
    forward and yaw and stop, RV-32 read the drive fields, RV-33 save the recording; then pause (RV-40)."""
    rest.command("RV-17", "enter Robot mode", "viewer_mode", {"mode": "robot", "path": model}, 120.0)
    rest.mode = "robot"
    state = rest.poll("RV-17", "loaded and bound", "robot_state", robot_observe,
                      lambda s: s.get("status") == "loaded" and get(s, "drive", "bound") is True, 180.0, failed=robot_failed)
    if not same_path(get(state, "drive", "profile", "path"), profile_path):
        raise StepFailed("RV-17", "loaded and bound", f"the bound profile is not {profile_path}", robot_observe(state))
    rest.command("RV-18", "start run", "robot_run", {"action": "start"}, 30.0)
    rest.poll("RV-18", "run running", "robot_state", robot_observe, lambda s: get(s, "run", "phase") == "running", 120.0, failed=robot_failed)
    drive_steadily(rest, "RV-29", "drive forward", "robot_drive", {"forward": 0.6}, 2.0)
    drive_steadily(rest, "RV-29", "drive yaw", "robot_drive", {"yaw": 0.5}, 2.0)
    rest.command("RV-29", "stop", "robot_drive", {"stop": True}, 5.0)
    final = unwrap(rest.command("RV-32", "final drive fields", "robot_state", {}, 10.0), "robot_state")
    drive = final.get("drive") or {}
    fields = {k: get(drive, "status", k) for k in ("request", "commanded", "heartbeat", "age_s", "expired")}
    fields.update({"limits": drive.get("limits"), "geometry": drive.get("geometry")})
    rest.command("RV-33", "save recording", "robot_save_recording", {"path": recording, "note": "build_rover_over_rest.py RV-40"}, 10.0)
    saved = rest.poll("RV-33", "recording written", "robot_state", robot_observe,
                      lambda s: get(s, "recording", "pending") in (False, None) and get(s, "recording", "last_saved", "path") is not None, 60.0,
                      failed=lambda s: get(s, "recording", "error") and f"robot_state.recording.error: {get(s, 'recording', 'error')}")
    rest.command("RV-40", "pause run", "robot_run", {"action": "pause"}, 10.0)
    return fields, get(saved, "recording", "last_saved", "path")


# ---------------------------------------------------------------- main

def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--port", type=int, default=8421)
    parser.add_argument("--out", required=True, help="output directory; must not exist")
    parser.add_argument("--cad-seed", default=os.path.join(REPO, "examples", "wheeled-robot", "baseline", "robot.rcad"),
                        help="an existing .rcad used only to bring CAD mode up (never edited or saved)")
    args = parser.parse_args()
    out_dir = os.path.abspath(args.out)
    if os.path.exists(out_dir):
        print(f"refusing: {out_dir} exists; give a new directory", file=sys.stderr)
        return 2
    for protected in ("examples", "cad", "web"):  # the viewer's PROTECTED set (robot/recording.rs:32)
        root = os.path.realpath(os.path.join(REPO, protected))
        real = os.path.realpath(out_dir)
        if real == root or real.startswith(root + os.sep):
            print(f"refusing: {out_dir} is under {protected}/ of the repository; run outputs never go there", file=sys.stderr)
            return 2
    seed = os.path.abspath(args.cad_seed)
    if not os.path.isfile(seed):
        print(f"refusing: the CAD seed {seed} is not a file", file=sys.stderr)
        return 2
    os.makedirs(out_dir, exist_ok=False)
    rcad, model = os.path.join(out_dir, "robot.rcad"), os.path.join(out_dir, "robot.simrobot.json")
    rest = Rest(args.port)
    summary, failure = {}, None
    try:
        # /v1/capabilities lists every mode's commands whatever mode is active: it is fixed when the
        # server binds (sim-spatial rest.rs:17) from app::actions::capabilities(), which flattens every
        # registered feature, each command with its `modes` (app/actions.rs:390). So a missing name is
        # a viewer without that command, not a mode not yet entered.
        caps = rest.http("RV-01", "capabilities", "GET", "/v1/capabilities")
        names = {c.get("command") for c in get(caps[1], "commands") or [] if isinstance(c, dict)}
        missing = sorted({"viewer_mode", "cad_state", "cad_file", "cad_op", "cad_threads", "cad_save", "cad_results", "system",
                          "system_state", "system_run", "system_drive", "robot_state", "robot_run", "robot_drive",
                          "robot_save_recording"} - names)
        rest.record("RV-01", "capabilities", method="GET", path="/v1/capabilities", status=caps[0],
                    response={"listed": len(names), "missing": missing})
        if caps[0] != 200 or missing:
            raise StepFailed("RV-01", "capabilities", f"the viewer does not offer {missing or 'its capabilities'} (HTTP {caps[0]})",
                             {"missing": missing})
        open_new_document(rest, seed, rcad)
        ids = build_fixture(rest)
        summary["threads"] = annotate(rest, ids)
        summary["export"] = save_and_export(rest, rcad, model)
        rest.at("RV-06", "write profile and binding")
        summary["profile"] = write_profile_and_binding(out_dir, model)
        wire_in_build(rest, os.path.join(out_dir, "rover.system.json"))
        summary["build_drive"] = live_run_in_build(rest)
        summary["robot_drive"], summary["recording"] = drive_in_robot_mode(
            rest, model, os.path.join(out_dir, "robot.drive.json"), os.path.join(out_dir, "drive.recording.json"))
    except StepFailed as e:
        failure = e
        rest.best_effort_stop()
    except BaseException as e:  # a bug or Ctrl-C: still stop and write the log
        failure = StepFailed(rest.step, f"{rest.step_name} (unexpected)", repr(e))
        rest.best_effort_stop()
    log = {"schema": "build_rover_over_rest/1", "script": "RV-40", "script_status": SCRIPT_STATUS, "viewer": rest.base, "out": out_dir,
           "ok": failure is None,
           "failure": None if failure is None else {"step": failure.step, "name": failure.name, "message": failure.message, "response": failure.response},
           "summary": summary, "requests": rest.log}
    with open(os.path.join(out_dir, "rest_log.json"), "x") as f:
        json.dump(log, f, indent=1, default=str)
    if failure is not None:
        print(f"FAILED at {failure.step} ({failure.name}): {failure.message}", file=sys.stderr)
        print(json.dumps(failure.response, default=str)[:4000], file=sys.stderr)
        print(f"log: {os.path.join(out_dir, 'rest_log.json')}", file=sys.stderr)
        return 1
    p, x = summary["profile"], summary["export"]
    print(f"rover built and driven over REST in {out_dir}")
    print(f"  export cad_sha256 {x['cad_sha256']} (matches robot.rcad; reason {x['cad_sha256_reason']!r})")
    print(f"  profile: forward {p['forward_m_s']} m/s, yaw {p['yaw_rad_s']} rad/s (radius {p['radius_m']:.4f} m, track {p['track_m']:.3f} m)")
    print(f"  threads: {', '.join(f'{k}={v}' for k, v in summary['threads'].items())}")
    print(f"  Build live-run drive: {json.dumps(summary['build_drive'])[:300]}")
    print(f"  Robot drive (final): {json.dumps(summary['robot_drive'])[:600]}")
    print(f"  recording: {summary['recording']}")
    print(f"  log: {os.path.join(out_dir, 'rest_log.json')} ({len(rest.log)} entries; {SCRIPT_STATUS.split(':')[0]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
