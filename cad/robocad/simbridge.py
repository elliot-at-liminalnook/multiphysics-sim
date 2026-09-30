"""The CAD ↔ simulation loop.

`export_sim_model` writes a `*.simrobot.json` the Rust side (the native
viewer `sim-spatial --robot file`, `sim-cad` for headless runs) turns into
a multibody model: every body carries its mass, centre of mass,
planar inertia and section outline computed from the solid and its
material density; joints are declared in the document as revolute axes
between two bodies (a `plane`-kind node named `joint:<child>` whose
origin is the pivot and whose normal is the axis, or a matching
cylindrical hole pair), and a body named `ground` (or with the tag) is
fixed. `SimLink` watches the saved file: every save re-exports the model,
and the running simulator viewer reloads it, so the loop is
edit → Ctrl+S → watch it move.

The live-loop viewer is sim-spatial robot mode (`viewer_command`): it
watches the model file and reloads in place, and a model that does not
exist yet (a first background export) loads when it appears. The legacy
`sim-app --scene cad --model file` window is only a labelled fallback when
sim-spatial is not built.
"""

from __future__ import annotations

import json
import math
import os
import subprocess
import sys
import threading
import time
from typing import Optional

from .document import Document, Node
from .kernel import Plane, SurfaceKind, Vec3
from .kernel.base import v_dist, v_dot, v_scale, v_sub, v_unit

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def _planar_inertia(doc: Document, body, centroid: Vec3, normal: Vec3, mass_g: float, volume: float) -> float:
    """Moment of inertia about the plane normal through the centroid, in
    kg·m²: the kernel's volume moment (mm⁵) times the density."""
    if volume <= 0:
        return 0.0
    density_kg_mm3 = (mass_g / 1000.0) / volume
    return doc.kernel.moment_of_inertia(body, centroid, normal) * density_kg_mm3 * 1.0e-6


def joints_of(doc: Document) -> list[dict]:
    """Joints: `joint` nodes first (type, limits, motor), then the legacy
    `joint:<child>[:<parent>]` planes (revolute, parent = nearest body)."""
    from .robotics import MOTOR_LIBRARY

    joints = []
    bodies = {n.name: n for n in doc.bodies()}
    by_id = {n.id: n for n in doc.bodies()}
    for n in doc.walk():
        if n.kind == "joint" and n.joint is not None:
            j = n.joint
            child = by_id.get(j.child)
            parent = by_id.get(j.parent) if j.parent else None
            if child is None:
                continue
            motor = None
            if j.motor and j.motor in doc.nodes and doc.nodes[j.motor].robot:
                meta = doc.nodes[j.motor].robot
                spec = MOTOR_LIBRARY.get(meta.get("spec", ""))
                if spec:
                    motor = {"id": j.motor, "name": doc.nodes[j.motor].name, "spec": spec.id, "stall_torque": spec.stall_torque * j.gear_ratio, "no_load_speed": spec.no_load_speed / max(j.gear_ratio, 1e-9), "gear_ratio": spec.gear_ratio * j.gear_ratio, "rotor_inertia": spec.rotor_inertia * (spec.gear_ratio * j.gear_ratio) ** 2, "kind": spec.kind}
            joints.append({"name": n.name, "id": n.id, "type": j.type, "child": child.name, "parent": parent.name if parent else None, "pivot": list(j.pivot), "axis": list(v_unit(j.axis)), "limits": [j.lower, j.upper] if j.lower is not None or j.upper is not None else None, "motor": motor, "damping": j.damping, "friction": j.friction, "home": j.home, "stroke": j.stroke})
    for n in doc.walk():
        if n.kind != "plane" or n.plane is None or not n.name.startswith("joint:"):
            continue
        parts = n.name.split(":")
        child = parts[1] if len(parts) > 1 else ""
        parent = parts[2] if len(parts) > 2 else None
        if parent is None:
            best, best_d = None, math.inf
            for name, b in bodies.items():
                if name == child or b.body is None:
                    continue
                d = doc.kernel.mass_properties(b.body)
                dd = v_dist(d.centroid, n.plane.origin)
                if dd < best_d:
                    best, best_d = name, dd
            parent = best
        joints.append({"name": n.name, "id": n.id, "type": "revolute", "child": child, "parent": parent, "pivot": list(n.plane.origin), "axis": list(v_unit(n.plane.normal)), "limits": None, "motor": None, "damping": 0.0, "friction": 0.0, "home": 0.0, "stroke": 0.0})
    return joints


def _link_groups(doc: Document, joints: list[dict]) -> dict[str, str]:
    """Which body each body is rigidly merged into: fixed joints and
    mounted motors collapse into their parent link."""
    by_name = {n.name: n for n in doc.bodies()}
    into: dict[str, str] = {}
    for j in joints:
        if j["type"] == "fixed" and j["parent"] and j["child"] in by_name:
            into[j["child"]] = j["parent"]
    for n in doc.bodies():
        if n.robot and n.robot.get("kind") == "motor" and n.robot.get("mounted_on") in doc.nodes and n.name not in into:
            into[n.name] = doc.nodes[n.robot["mounted_on"]].name

    def resolve(name: str) -> str:
        seen = set()
        while name in into and name not in seen:
            seen.add(name)
            name = into[name]
        return name

    return {n.name: resolve(n.name) for n in doc.bodies()}


def export_sim_model(doc: Document, path: str, plane: Plane = Plane.xz(), section: bool = True, version: int = 3, flex: bool = True) -> dict:
    """The simulation model. `version=3` (default) is the physical assembly
    description (`physical.export_physical_model`, SI, full inertia,
    collision geometry, joint physics, motors, sensors); `version=2` is the
    planar summary in mm: bodies with mass, centre, planar inertia and
    section outline, joints with pivots in the working plane."""
    if version >= 3:
        from .physical import export_physical_model

        return export_physical_model(doc, path, planar=plane, flex=flex)
    joints = joints_of(doc)
    groups = _link_groups(doc, joints)
    raw = {}
    for n in doc.bodies():
        b = n.body
        p = doc.kernel.mass_properties(b)
        mass_g = p.mass(doc.density_of(n.id))
        outline = []
        if section:
            try:
                loops = doc.kernel.section(b, Plane(p.centroid, plane.normal, plane.x_axis))
                for loop in loops:
                    outline.append([[plane.to_local(q)[0], plane.to_local(q)[1]] for q in loop])
            except Exception:
                pass
        raw[n.name] = {"node": n, "mass_kg": mass_g / 1000.0, "centroid": p.centroid, "inertia": _planar_inertia(doc, b, p.centroid, plane.normal, mass_g, p.volume), "bbox": [list(p.bbox_min), list(p.bbox_max)], "outline": outline}
    bodies = []
    for name, r in raw.items():
        if groups[name] != name:
            continue  # merged into another link
        members = [m for m in raw if groups[m] == name]
        mass = sum(raw[m]["mass_kg"] for m in members)
        com = tuple(sum(raw[m]["mass_kg"] * raw[m]["centroid"][i] for m in members) / max(mass, 1e-12) for i in range(3))
        # Parallel axis about the merged centroid, in the plane.
        inertia = 0.0
        for m in members:
            d = v_sub(raw[m]["centroid"], com)
            d_perp = v_sub(d, v_scale(v_unit(plane.normal), v_dot(d, v_unit(plane.normal))))
            inertia += raw[m]["inertia"] + raw[m]["mass_kg"] * (v_dot(d_perp, d_perp) * 1e-6)
        outline = [loop for m in members for loop in raw[m]["outline"]]
        lo = [min(raw[m]["bbox"][0][i] for m in members) for i in range(3)]
        hi = [max(raw[m]["bbox"][1][i] for m in members) for i in range(3)]
        n = r["node"]
        u, v, _ = plane.to_local(com)
        bodies.append({
            "id": n.id, "name": name, "material": n.material, "mass_kg": mass, "com": [u, v], "com_world": list(com), "inertia_zz": inertia,
            "bbox": [lo, hi], "outline": outline, "members": members,
            "ground": name.lower() == "ground" or "ground" in name.lower().split() or bool((n.robot or {}).get("ground")) or any(bool((raw[m]["node"].robot or {}).get("ground")) for m in members),
        })
    sim_joints = []
    for j in joints:
        if j["type"] == "fixed":
            continue
        child = groups.get(j["child"], j["child"])
        parent = groups.get(j["parent"], j["parent"]) if j["parent"] else None
        if child == parent:
            continue
        sim_joints.append(dict(j, child=child, parent=parent, pivot2=list(plane.to_local(tuple(j["pivot"]))[:2]), axis_sign=1.0 if v_dot(v_unit(tuple(j["axis"])), v_unit(plane.normal)) >= 0 else -1.0))
    model = {"format": "simrobot", "version": 2, "unit": "mm", "plane": plane.to_json(), "bodies": bodies, "joints": sim_joints, "source": doc.path}
    # Atomic replace: the running simulator viewer reloads this file on change.
    tmp = f"{path}.{os.getpid()}.tmp"
    with open(tmp, "w") as f:
        json.dump(model, f, indent=1)
    os.replace(tmp, path)
    return model


def sim_model_path(doc_path: str) -> str:
    root, _ = os.path.splitext(doc_path)
    return root + ".simrobot.json"


VIEWER_BUILD = "cargo build --release -p sim-spatial"


def viewer_command(model_path: str) -> tuple[Optional[list[str]], str]:
    """The live-loop viewer for `model_path`: `(argv, message)`.

    sim-spatial robot mode, release preferred over debug; else the legacy
    sim-app cad scene with a message naming the fallback; else `(None, …)`
    naming the build command. The message is empty for the normal case.
    sim-spatial tolerates a model that is not written yet and loads it when
    it appears, so callers need not wait for a background export."""
    for profile in ("release", "debug"):
        exe = os.path.join(ROOT, "target", profile, "sim-spatial")
        if os.path.exists(exe):
            return [exe, "--robot", model_path], ""
    legacy = os.path.join(ROOT, "target", "release", "sim-app")
    if os.path.exists(legacy):
        return [legacy, "--scene", "cad", "--model", model_path], f"sim-spatial not built; falling back to legacy sim-app cad scene (build: {VIEWER_BUILD})"
    return None, f"no simulator viewer built: {VIEWER_BUILD}"


def _viewer_env() -> dict:
    env = dict(os.environ)
    env["PATH"] = os.path.expanduser("~/.cargo/bin") + os.pathsep + env.get("PATH", "")
    return env


class SimLink:
    """Watches the document file; on every save re-exports the sim model
    and starts the viewer (`viewer_command`: sim-spatial, else the labelled
    sim-app fallback) when none is running. The viewer itself watches the
    model file, so it reloads in place and is never relaunched per save."""

    def __init__(self, doc: Document, app=None):
        self.doc = doc
        self.app = app
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self.process: Optional[subprocess.Popen] = None

    def start(self):
        self.export()
        self.launch()
        self.doc.listeners.append(self._on_doc)

    def _on_doc(self, event, payload):
        if event == "saved":
            self.export()

    def export(self):
        # v4 without the modal reduction: the live loop wants a save to show up in a second or two.
        if not self.doc.path:
            return
        background = getattr(self.app, "_export_in_background", None)
        if background is not None:
            # Collision grids still take seconds on a full robot: keep the
            # editor responsive and export the latest save when one is running.
            background(sim_model_path(self.doc.path), planar=True, label="live simulation model", flex=False, queue=True)
        else:
            export_sim_model(self.doc, sim_model_path(self.doc.path), flex=False)

    def launch(self):
        if self.process and self.process.poll() is None:
            return
        argv, message = viewer_command(sim_model_path(self.doc.path))
        if message and self.app:
            self.app.status(message)
        if argv is None:
            return
        self.process = subprocess.Popen(argv, cwd=ROOT, env=_viewer_env())

    def stop(self):
        self._stop.set()
        if self.process and self.process.poll() is None:
            self.process.terminate()
        if self._on_doc in self.doc.listeners:
            self.doc.listeners.remove(self._on_doc)


def watch_and_run(doc_path: str, interval: float = 1.0):
    """CLI: `python -m robocad.simbridge robot.rcad` — re-export the sim
    model whenever the CAD file changes, and keep the viewer running
    (started only when none is alive: it reloads the file itself)."""
    last = 0.0
    proc = None
    while True:
        try:
            m = os.path.getmtime(doc_path)
        except OSError:
            time.sleep(interval)
            continue
        if m != last:
            last = m
            doc = Document.load(doc_path)
            export_sim_model(doc, sim_model_path(doc_path), flex=False)
            print(f"exported {sim_model_path(doc_path)}")
            if proc is None or proc.poll() is not None:
                argv, message = viewer_command(sim_model_path(doc_path))
                if message:
                    print(message)
                if argv is not None:
                    proc = subprocess.Popen(argv, cwd=ROOT, env=_viewer_env())
        time.sleep(interval)


if __name__ == "__main__":
    watch_and_run(sys.argv[1])
