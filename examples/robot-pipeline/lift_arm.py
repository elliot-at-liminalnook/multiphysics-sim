#!/usr/bin/env python3
"""One robot through the whole pipeline over REST, from a blank window.

    target/debug/sim-spatial                        # a blank window (REST on :8421)
    python3 examples/robot-pipeline/lift_arm.py [PROJECT_DIR]

Steps, as the bottom strip shows them:

1. Design: project_new makes the project and an empty design and opens CAD.
   The arm is then modelled through CAD's own commands, as the design
   assistant would: a grounded PLA base, an MG996R servo mounted on it, a PLA
   arm on the servo's revolute joint (limits -0.5..2.0 rad), a steel payload
   bolted to the arm's tip, an encoder, and an estimated drive backlash.
2. Model: project_step model saves the design and makes the simulation model
   in process; the script prints its assumptions (none may block).
3. Test: project_set_test states what the arm must do (lift the payload to
   60 degrees in a second and hold it, with torque and temperature in
   reserve, within its limits, printed parts at least twice as strong as
   the peak loads need); project_test runs it and prints each criterion's
   outcome.
4. Learn: project_lessons suggest lists lessons for this robot (writing one
   asks the AI: pass --lesson TOPIC to try it).
5. Make: project_make writes the printed parts' STL files and parts.json.

Only the standard library. Writes nothing outside PROJECT_DIR (default
<workspace>/projects/lift-arm, refused if it exists).
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request

BASE = os.environ.get("SIM_REST", "http://127.0.0.1:8421")


def call(command, timeout=600, **args):
    body = json.dumps({"commands": [{"command": command, "args": args}]}).encode()
    req = urllib.request.Request(BASE + "/v1/batch", data=body, headers={"Content-Type": "application/json"}, method="POST")
    job = json.load(urllib.request.urlopen(req, timeout=10))["job_id"]
    t0 = time.time()
    while time.time() - t0 < timeout:
        r = json.load(urllib.request.urlopen(f"{BASE}/v1/jobs/{job}", timeout=10))
        if r.get("status") in ("succeeded", "failed", "cancelled"):
            x = r["results"][0]
            if not x["ok"]:
                raise SystemExit(f"{command}: {x.get('error')}")
            return x["value"]
        time.sleep(0.2)
    raise TimeoutError(command)


def get(resource):
    """A published resource; {} until its mode first publishes it (404)."""
    try:
        return json.load(urllib.request.urlopen(f"{BASE}/v1/{resource}", timeout=10))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return {}
        raise


def op(_op, *args, **kwargs):
    """One CAD operation (one undo step); its result (a new node's id for an add)."""
    v = call("cad_op", name=_op, args=list(args), kwargs=kwargs)
    res = v.get("result", v)
    return res.get("result", res) if isinstance(res, dict) else res


def wait(what, ok, timeout=120):
    t0 = time.time()
    while time.time() - t0 < timeout:
        s = get("project_state")
        if ok(s):
            return s
        time.sleep(0.5)
    raise SystemExit(f"timed out waiting for {what}: {json.dumps(get('project_state').get('steps'), indent=1)}")


def steps(s):
    return "  ".join(f"{x['step']}:{x['state']}" for x in s["steps"])


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    lesson = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--lesson" and i + 1 < len(sys.argv)), None)
    new = {"name": "Lift arm", "description": "A one-joint arm that lifts a 19 g steel weight to 60 degrees in one second and holds it."}
    if args:
        new["dir"] = os.path.abspath(args[0])
    # 1 Design.
    project = call("project_new", **new)["project"]
    # CAD opens the empty design on its own job: wait until it is shown.
    for _ in range(200):
        st = get("cad_state")
        if (st.get("target") or {}).get("path") == project["cad"] and st.get("local_open") is None and (st.get("status") or {}).get("ok"):
            break
        time.sleep(0.3)
    else:
        raise SystemExit("CAD did not open the new design")
    print("1 Design: building the arm in CAD")
    base = op("box", [-40, -40, 0], [80, 40, 80], name="Base")
    op("set_material", [base], "pla")
    op("set_ground", base)
    servo = op("add_motor", "mg996r", [0, 0, 60], [0, 1, 0], mount_on=base, name="Shoulder servo")
    arm = op("box", [-10, 6, 50], [150, 6, 20], name="Arm")
    op("set_material", [arm], "pla")
    payload = op("box", [120, 12, 50], [20, 6, 20], name="Payload")
    op("set_material", [payload], "steel")
    op("connect_fixed", arm, payload, name="payload bolts")
    joint = op("add_joint", "revolute", base, arm, [0, 6, 60], [0, -1, 0], lower=-0.5, upper=2.0, motor=servo, name="Shoulder")
    op("add_sensor", "encoder", arm, [0, 6, 60], joint=joint, name="Shoulder encoder")
    op("set_joint_physics", joint, drive_backlash={"width_rad": 0.017, "provenance": "estimated", "reference": "MG996R spline play, typical value; measure on the bench"})
    # 2 Model.
    print("2 Model: saving the design and making the simulation model")
    call("project_step", step="model")
    s = wait("the model", lambda s: next(x for x in s["steps"] if x["step"] == "model")["state"] in ("done", "attention") and not s["intents"])
    print("  ", steps(s))
    for a in s["model"]["assumptions"]:
        print(f"   {'BLOCKING ' if a['blocking'] else ''}{a['status']}: {a['what']}")
    if s["model"]["blocking"]:
        raise SystemExit("the model has blocking assumptions")
    # 3 Test.
    test = {"name": "lift the payload", "duration_s": 3.0,
            "trajectory": [{"t": 0.0, "targets": {"Shoulder": 0.0}}, {"t": 1.0, "targets": {"Shoulder": 1.047}}],
            "criteria": [{"kind": "reaches", "joint": "Shoulder", "target": 1.047, "tolerance": 0.05, "by_s": 1.5},
                         {"kind": "torque_margin", "min": 0.3}, {"kind": "winding_temperature", "margin_c": 20.0},
                         {"kind": "no_limit_hits"}, {"kind": "part_strength", "min_safety_factor": 2.0}]}
    call("project_set_test", test=test)
    print("3 Test: running it")
    call("project_test")
    s = wait("the test", lambda s: not s["test"]["running"] and s["test"]["latest"] is not None and s["test"]["latest"]["test"]["name"] == test["name"], timeout=300)
    r = s["test"]["latest"]
    for o in r["outcomes"]:
        print(f"   {o['status']:>12}  {o['description']}: {o['detail']}")
    print("  ", r["summary"], "| evidence:", r["evidence"])
    s = wait("the steps to update", lambda s: next(x for x in s["steps"] if x["step"] == "test")["state"] != "ready")
    print("  ", steps(s))
    # 4 Learn.
    print("4 Learn: lessons suggested for this robot")
    for x in call("project_lessons", op="suggest")["suggested"]:
        print(f"   {x['id']}: {x['title']}")
    if lesson:
        call("project_lessons", op="write", topic=lesson)
        s = wait("the lesson", lambda s: s["lessons"]["writer"]["writing"] is None, timeout=900)
        print("  ", s["lessons"]["writer"]["last"])
    # 5 Make.
    if r["verdict"] == "passed":
        print("5 Make: part files")
        call("project_make")
        s = wait("the part files", lambda s: next(x for x in s["steps"] if x["step"] == "make")["state"] == "done", timeout=120)
        for p in s["make"]["latest"]["parts"]:
            print(f"   {p['file']}: {p['name']} ({p['material']}, {p['mass_kg'] * 1000:.1f} g)")
        print("  ", steps(s))
    print("Project:", s["project"]["path"])


if __name__ == "__main__":
    main()
