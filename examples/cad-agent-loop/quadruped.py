#!/usr/bin/env python3
"""A 12-servo quadruped modelled entirely through the CAD editor's REST API.

    target/debug/sim-spatial examples/wheeled-robot/baseline/robot.rcad   # any document; REST on :8421
    python3 examples/cad-agent-loop/quadruped.py /tmp/quadruped.rcad

The script creates the file (cad_file new), then builds, in mm with Z up and
X forward:

- a shelled, filleted PETG chassis with printed board bosses, a vented cover
  with a carry handle, a camera head, battery, controller board, bus adapter
  and power switch, each fixed to the chassis;
- four legs, one batch (one undo step) each: hip roll servo inside the chassis
  (shaft through a cut in the end wall), aluminium hip bracket carrying the hip
  pitch servo, lightened thigh with the knee servo, shank and rubber foot;
  three servo-driven revolute joints with encoders, a foot contact sensor and
  a servo harness cable;
- the body IMU, battery and bus leads, 3S LiPo and 10 ms control settings;
- four saved views and a pinned design-notes comment, then saves.

The result is examples/cad-agent-loop/quadruped.rcad. Masses are provisional
(geometry x catalogue density). Only the standard library.
"""
import json
import os
import sys
import time
import urllib.request

BASE = os.environ.get("SIM_REST", "http://127.0.0.1:8421")


class rest:
    """The viewer's REST: one batch per call, polled until it lands."""

    @staticmethod
    def call(*commands, timeout=300):
        body = json.dumps({"commands": [{"command": c["command"], "args": {k: v for k, v in c.items() if k != "command"}} for c in commands]}).encode()
        req = urllib.request.Request(BASE + "/v1/batch", data=body, headers={"Content-Type": "application/json"}, method="POST")
        job = json.load(urllib.request.urlopen(req, timeout=10))["job_id"]
        t0 = time.time()
        while time.time() - t0 < timeout:
            r = json.load(urllib.request.urlopen(f"{BASE}/v1/jobs/{job}", timeout=10))
            if r.get("status") in ("succeeded", "failed", "cancelled"):
                return r
            time.sleep(0.2)
        raise TimeoutError(job)

    @staticmethod
    def get(resource):
        return json.load(urllib.request.urlopen(f"{BASE}/v1/{resource}", timeout=10))

def check(r, what):
    x = r["results"][0]
    if not x["ok"]:
        raise SystemExit(f"{what}: {x.get('error')}")
    return x["value"]


def op(_op, *args, **kwargs):
    v = check(rest.call({"command": "cad_op", "name": _op, "args": list(args), "kwargs": kwargs}), _op)
    res = v.get("result", v)
    return res.get("result", res) if isinstance(res, dict) else res


def batch(label, operations):
    v = check(rest.call({"command": "cad_model", "op": "batch", "operations": operations}), label)
    print(f"  {label}: {v['message']}")
    return v["result"]


def topology(node):
    v = check(rest.call({"command": "cad_model", "op": "topology", "node": node}), "topology")
    return v.get("result", v)


def box(p, q, name):
    lo = [min(a, b) for a, b in zip(p, q)]
    size = [abs(a - b) for a, b in zip(p, q)]
    return {"op": "box", "args": [lo, size], "kwargs": {"name": name}}


def ref(alias):
    return {"$ref": alias}


def vertical_edges(node):
    return [{"node": node, "edge": e["index"]} for e in topology(node)["edges"]
            if e.get("start") and e.get("end") and abs(e["start"][0] - e["end"][0]) < 1e-6
            and abs(e["start"][1] - e["end"][1]) < 1e-6 and abs(e["start"][2] - e["end"][2]) > 1e-6]


def top_face(node):
    faces = [f for f in topology(node)["faces"] if f.get("normal") and f["normal"][2] > 0.99]
    return max(faces, key=lambda f: f["center"][2])


ALU, BLUE, DARK, ORANGE = [0.75, 0.77, 0.8], [0.16, 0.36, 0.72], [0.13, 0.13, 0.15], [0.95, 0.47, 0.12]

# ---------------------------------------------------------------- document
if len(sys.argv) != 2 or not sys.argv[1].endswith(".rcad"):
    raise SystemExit("usage: quadruped.py /abs/path/new.rcad")
path = os.path.abspath(sys.argv[1])
check(rest.call({"command": "cad_file", "op": "new", "path": path}), "new document")
print("created", path)

# ---------------------------------------------------------------- materials
print("materials")
for step in [{"op": "new"}, {"op": "form_set", "field": "name", "text": "LiPo pack"}, {"op": "form_set", "field": "density", "text": "1.25"}, {"op": "form_submit"}]:
    r = rest.call({"command": "cad_materials", **step})["results"][0]
    if not r["ok"]:
        print("  (material)", r.get("error"))
        break

# ---------------------------------------------------------------- chassis
print("chassis")
chassis = op("box", [-130, -70, 125], [260, 140, 50], name="Chassis")
op("fillet", chassis, vertical_edges(chassis), 14)
op("shell", chassis, 2.5, [{"node": chassis, "face": top_face(chassis)["index"]}])
op("set_material", [chassis], "petg")
op("set_color", [chassis], ORANGE)

# Bosses for the controller board, printed with the chassis.
r = batch("board bosses", [
    {"op": "cylinder", "args": [[x, y, 127], [0, 0, 1], 3.0, 31], "kwargs": {"name": "boss"}, "as": f"b{i}"}
    for i, (x, y) in enumerate([(-40, -25), (40, -25), (-40, 25), (40, 25)])
] + [{"op": "boolean", "args": [chassis, [ref(f"b{i}") for i in range(4)]], "kwargs": {"op": "union"}}])

cover = op("box", [-130, -70, 175], [260, 140, 3], name="Top cover")
op("fillet", cover, vertical_edges(cover), 14)
batch("cover vents and handle", [
    *[dict(box([x, -40, 174], [x + 6, 40, 179], "vent"), **{"as": f"v{i}"}) for i, x in enumerate(range(-75, 80, 25))],
    {"op": "boolean", "args": [cover, [ref(f"v{i}") for i in range(7)]], "kwargs": {"op": "subtract"}},
    dict(box([-40, -6, 178], [-32, 6, 203], "Carry handle"), **{"as": "h"}),
    dict(box([32, -6, 178], [40, 6, 203], "post"), **{"as": "p2"}),
    dict(box([-40, -6, 195], [40, 6, 203], "bar"), **{"as": "p3"}),
    {"op": "boolean", "args": [ref("h"), [ref("p2"), ref("p3")]], "kwargs": {"op": "union"}},
    {"op": "fillet_all", "args": [ref("h"), 2.0]},
    {"op": "set_material", "args": [[cover, ref("h")], "petg"]},
    {"op": "set_color", "args": [[cover, ref("h")], DARK]},
])
handle = [n["id"] for n in rest.get("cad_state")["nodes"] if n["name"] == "Carry handle"][0]

head = batch("sensor head", [
    dict(box([130, -22, 138], [150, 22, 168], "Sensor head"), **{"as": "head"}),
    {"op": "fillet_all", "args": [ref("head"), 3.0]},
    {"op": "cylinder", "args": [[150, 0, 153], [1, 0, 0], 9.0, 5.0], "kwargs": {"name": "Camera lens"}, "as": "lens"},
    {"op": "set_material", "args": [[ref("head")], "asa"]},
    {"op": "set_color", "args": [[ref("head")], DARK]},
    {"op": "set_material", "args": [[ref("lens")], "glass"]},
])

inside = batch("battery and electronics", [
    dict(box([-60, -22, 127.5], [60, 22, 155.5], "Battery 3S 2200 mAh"), **{"as": "bat"}),
    dict(box([-45, -30, 158], [45, 30, 159.6], "Controller board"), **{"as": "board"}),
    dict(box([65, -15, 127.5], [87, 15, 129.1], "Servo bus adapter"), **{"as": "bus"}),
    {"op": "cylinder", "args": [[-127.5, 35, 160], [-1, 0, 0], 6.0, 8.0], "kwargs": {"name": "Power switch"}, "as": "sw"},
    {"op": "set_material", "args": [[ref("board"), ref("bus")], "pcb"]},
    {"op": "set_color", "args": [[ref("board"), ref("bus")], [0.1, 0.45, 0.2]]},
    {"op": "set_material", "args": [[ref("sw")], "abs"]},
])
battery, board, bus, switch = inside["bat"], inside["board"], inside["bus"], inside["sw"]
try:
    op("set_material", [battery], "lipo_pack")
except SystemExit as e:
    print("  battery material:", e)
op("set_color", [battery], [0.2, 0.25, 0.6])

for child, at, name in [(cover, [0, 0, 176], "cover screws"), (handle, [0, 0, 190], "handle screws"), (head["head"], [140, 0, 153], "head mount"),
                        (battery, [0, 0, 140], "battery strap"), (board, [0, 0, 159], "board standoffs"), (bus, [76, 0, 128], "adapter tape"),
                        (switch, [-130, 35, 160], "switch nut")]:
    parent = cover if child == handle else chassis
    op("connect_fixed", parent, child, at, name=name)
op("connect_fixed", head["head"], head["lens"], [152, 0, 153], name="lens bezel")


# ---------------------------------------------------------------- legs
def leg(fx, sy, label):
    X = lambda a: fx * a
    Y = lambda b: sy * b
    knee = (-2.4, 0.15) if fx > 0 else (-0.15, 2.4)
    print(label)
    r = batch(f"{label} leg", [
        # Hip roll servo inside the chassis, shaft out through the end wall.
        {"op": "add_motor", "args": ["hx30hm", [X(127), Y(40), 150], [fx, 0, 0]], "kwargs": {"mount_on": chassis, "cut_mount": True, "name": f"{label} hip servo"}, "as": "m_hip"},
        # Hip bracket: an end plate on the hip horn and a side plate carrying the pitch servo.
        dict(box([X(131), Y(25), 135], [X(136), Y(100), 165], f"{label} hip bracket"), **{"as": "bracket"}),
        dict(box([X(131), Y(95), 125], [X(185), Y(100), 175], "side plate"), **{"as": "side"}),
        {"op": "fillet_all", "args": [ref("bracket"), 1.5]},
        {"op": "fillet_all", "args": [ref("side"), 1.5]},
        {"op": "boolean", "args": [ref("bracket"), [ref("side")]], "kwargs": {"op": "union"}},
        {"op": "add_motor", "args": ["hx30hm", [X(160), Y(95), 150], [0, sy, 0]], "kwargs": {"mount_on": ref("bracket"), "cut_mount": True, "name": f"{label} hip pitch servo"}, "as": "m_pitch"},
        # Thigh: a lightened aluminium link from the hip pitch horn to the knee servo.
        dict(box([X(148), Y(100), 63], [X(172), Y(105), 162], f"{label} thigh"), **{"as": "thigh"}),
        {"op": "fillet_all", "args": [ref("thigh"), 1.5]},
        dict(box([X(154), Y(99), 95], [X(166), Y(106), 130], "thigh slot"), **{"as": "tslot"}),
        {"op": "boolean", "args": [ref("thigh"), [ref("tslot")]], "kwargs": {"op": "subtract"}},
        {"op": "add_motor", "args": ["hx30hm", [X(160), Y(105), 75], [0, -sy, 0]], "kwargs": {"mount_on": ref("thigh"), "cut_mount": True, "name": f"{label} knee servo"}, "as": "m_knee"},
        # Shank and rubber foot.
        dict(box([X(150), Y(95), 12], [X(170), Y(100), 85], f"{label} shank"), **{"as": "shank"}),
        {"op": "fillet_all", "args": [ref("shank"), 1.5]},
        dict(box([X(156), Y(94), 30], [X(164), Y(101), 65], "shank slot"), **{"as": "sslot"}),
        {"op": "boolean", "args": [ref("shank"), [ref("sslot")]], "kwargs": {"op": "subtract"}},
        {"op": "sphere", "args": [[X(160), Y(97.5), 12], 12.0], "kwargs": {"name": f"{label} foot"}, "as": "foot"},
        {"op": "set_material", "args": [[ref("bracket"), ref("thigh"), ref("shank")], "al"]},
        {"op": "set_color", "args": [[ref("bracket")], BLUE]},
        {"op": "set_color", "args": [[ref("thigh"), ref("shank")], ALU]},
        {"op": "set_material", "args": [[ref("foot")], "rubber"]},
        {"op": "set_color", "args": [[ref("foot")], DARK]},
        # Joints, each driven by its servo.
        {"op": "add_joint", "args": ["revolute", chassis, ref("bracket"), [X(131), Y(40), 150], [1, 0, 0], -0.6, 0.6], "kwargs": {"name": f"{label} hip roll"}, "as": "j_hip"},
        {"op": "add_joint", "args": ["revolute", ref("bracket"), ref("thigh"), [X(160), Y(100), 150], [0, 1, 0], -1.6, 1.6], "kwargs": {"name": f"{label} hip pitch"}, "as": "j_pitch"},
        {"op": "add_joint", "args": ["revolute", ref("thigh"), ref("shank"), [X(160), Y(101), 75], [0, 1, 0], knee[0], knee[1]], "kwargs": {"name": f"{label} knee"}, "as": "j_knee"},
        {"op": "attach_motor", "args": [ref("j_hip"), ref("m_hip")]},
        {"op": "attach_motor", "args": [ref("j_pitch"), ref("m_pitch")]},
        {"op": "attach_motor", "args": [ref("j_knee"), ref("m_knee")]},
        {"op": "connect_fixed", "args": [ref("shank"), ref("foot"), [X(160), Y(97.5), 12]], "kwargs": {"name": f"{label} foot mount"}, "as": "j_foot"},
        # Servo position feedback and a foot contact sensor.
        {"op": "add_sensor", "args": ["encoder", ref("bracket"), [X(131), Y(40), 150], None, f"{label} hip roll encoder", ref("j_hip")], "as": "e1"},
        {"op": "add_sensor", "args": ["encoder", ref("thigh"), [X(160), Y(100), 150], None, f"{label} hip pitch encoder", ref("j_pitch")], "as": "e2"},
        {"op": "add_sensor", "args": ["encoder", ref("shank"), [X(160), Y(101), 75], None, f"{label} knee encoder", ref("j_knee")], "as": "e3"},
        {"op": "add_sensor", "args": ["force", ref("foot"), [X(160), Y(97.5), 0], None, f"{label} foot contact"], "as": "f1"},
        # The servo harness from the bus adapter down to the knee servo.
        {"op": "add_cable", "args": [chassis, [X(110), Y(60), 165], ref("thigh"), [X(160), Y(110), 75]], "kwargs": {"length": 180, "mass": 0.006, "name": f"{label} servo harness"}, "as": "cable"},
    ])
    ids = [r[k] for k in ["m_hip", "bracket", "m_pitch", "thigh", "m_knee", "shank", "foot", "j_hip", "j_pitch", "j_knee", "j_foot", "e1", "e2", "e3", "f1", "cable"]]
    op("group", ids, f"{label} leg")
    return r


legs = {label: leg(fx, sy, label) for fx, sy, label in [(1, 1, "Front left"), (1, -1, "Front right"), (-1, 1, "Rear left"), (-1, -1, "Rear right")]}

# ---------------------------------------------------------------- body group, sensors, settings
print("body, sensors and settings")
imu = op("add_sensor", "imu", board, [0, 0, 160], None, "Body IMU", rate_hz=200, noise=0.002)
op("add_cable", battery, [60, 0, 141], board, [40, 0, 159], length=90, mass=0.012, name="Battery lead")
op("add_cable", board, [-40, 20, 159], bus, [70, 0, 129], length=140, mass=0.004, name="Servo bus lead")
body = [chassis, cover, handle, head["head"], head["lens"], battery, board, bus, switch, imu]
body += [n["id"] for n in rest.get("cad_state")["nodes"] if n["kind"] in ("joint", "cable") and not n.get("parent") and n["name"] in ("cover screws", "handle screws", "head mount", "battery strap", "board standoffs", "adapter tape", "switch nut", "lens bezel", "Battery lead", "Servo bus lead")]
op("group", body, "Body")
op("set_battery", 3, "lipo", 2.2)
op("set_control", 0.01, 0.002)
op("set_uncertainty", mass=0.05)

state = rest.get("cad_state")
print("nodes:", len(state["nodes"]))
print("mass:", json.dumps(state.get("local_mass", {}).get("assembly", state.get("local_mass")))[:400])

# ---------------------------------------------------------------- views, notes, save
nodes = rest.get("cad_state")["nodes"]
byname = {n["name"]: n["id"] for n in nodes}
fl = [n["id"] for n in nodes if n.get("parent") == byname["Front left leg"] and n["kind"] == "body"]
for view in [
    {"name": "Quadruped overview", "fit": [], "direction": "iso", "description": "The whole robot: 12 HX-30HM servos, 3 per leg (hip roll, hip pitch, knee)."},
    {"name": "Front-left leg", "fit": fl, "direction": "iso", "parts": fl, "description": "One leg alone: hip servo, hip bracket, pitch servo, slotted thigh, knee servo, shank and rubber foot."},
    {"name": "Hip servos through the chassis wall", "fit": [], "direction": "back", "section": {"axis": "y", "offset": 40}, "description": "Section at y = 40 mm: hip roll servos inside the shelled chassis, shafts through holes cut in the end walls; battery, board on bosses."},
    {"name": "Leg chain section", "fit": fl, "section": {"axis": "x", "offset": 160}, "description": "Section at x = 160 mm through the front legs: pitch servo on the bracket, knee servo on the thigh, shank on the knee horn."},
]:
    check(rest.call({"command": "cad_views", "op": "save", **view}), view["name"])
link = lambda name: f"[{name}](part:{byname[name]})"
mass = state["local_mass"]["assembly"]["mass_kg"]
notes = (f"Design notes (built over REST): a 12-DoF quadruped, {mass:.2f} kg as modelled. "
         f"{link('Chassis')} is a 2.5 mm PETG shell with 14 mm corner fillets and printed bosses for the {link('Controller board')}; the {link('Battery 3S 2200 mAh')} sits on the floor. "
         f"Each leg: hip roll servo inside the chassis (shaft through a cut in the end wall), {link('Front left hip bracket')} carrying the hip pitch servo, "
         f"{link('Front left thigh')} and {link('Front left shank')} (lightened 5 mm aluminium), knee servo on the thigh, rubber foot with a contact sensor. "
         "Every joint has its servo attached and an encoder; the body IMU is on the controller board; 3S LiPo, 10 ms control loop with 2 ms latency. "
         "Provisional: masses are geometry x catalogue density (servo bodies come out near 40 g in ABS against 52 g on the datasheet); the zero pose has the legs straight down.")
check(rest.call({"command": "cad_threads", "op": "create", "node": byname["Chassis"], "point": [0, -70, 150], "body": notes, "author": "Claude", "author_kind": "agent"}), "design notes")
check(rest.call({"command": "cad_save"}), "save")
print("saved", path)
