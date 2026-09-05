"""The document: a scene graph of bodies, sheets, sketches, reference
meshes and images, construction planes, measurements, groups and live
instances, with materials, persistence and autosave.

Direct modeling means a body *is* its geometry: there is no history to
replay, so a node holds its kernel `Body` and the command stack keeps the
previous `Body` handle for undo (kernel shapes are never mutated in place,
so an old handle stays valid and costs nothing to keep).

File format: `.rcad` is a zip with `manifest.json` (every node's metadata,
materials, sketches, planes, measurements, view state), `brep/<id>.brep`
per body (the kernel's binary B-rep), `mesh/<id>.npz` per reference mesh,
`image/<id>` per reference image, and `thumbnail.png`.
"""

from __future__ import annotations

import io
import json
import os
import threading
import tempfile
import time
import uuid
import zipfile
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable, Optional

from .kernel import Body, GeometryKernel, Plane, Sketch, Vec3, default_kernel
from .kernel.base import Mesh, v_add

FORMAT_VERSION = 1


def write_archive(path, entries):
    """Write an immutable archive snapshot; safe off the document/UI thread."""
    path = os.fspath(path)
    fd, tmp = tempfile.mkstemp(prefix='.robocad-save-', suffix='.tmp', dir=os.path.dirname(os.path.abspath(path)))
    os.close(fd)
    try:
        with zipfile.ZipFile(tmp, 'w', zipfile.ZIP_DEFLATED) as archive:
            for name, data in entries:
                archive.writestr(name, data)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


# Engineering properties a material carries for the physical model (SI):
# Young's modulus and strengths in Pa, conductivity W/m·K, specific heat
# J/kg·K, expansion 1/K, glass transition °C, kinetic/static friction against
# other material ids ("world" = floor/table, "steel" = pins and shafts), and
# how a printed part's modulus and strength drop across layers.
ENGINEERING_KEYS = ("youngs_modulus", "poisson", "yield_strength", "ultimate_strength", "glass_transition_c", "thermal_conductivity", "specific_heat", "thermal_expansion", "friction", "print", "bearing_pressure")

# (E, ν, σ_y, σ_u, Tg, k, cp, α, µ_k self, µ_k vs steel, allowable bearing MPa, anisotropy, layer adhesion)
_ENG = {
    "pla": (3.5e9, 0.36, 50e6, 60e6, 60.0, 0.13, 1800.0, 68e-6, 0.35, 0.30, 15e6, 0.6, 0.7),
    "petg": (2.1e9, 0.38, 45e6, 50e6, 80.0, 0.20, 1200.0, 60e-6, 0.40, 0.32, 20e6, 0.7, 0.8),
    "abs": (2.0e9, 0.37, 35e6, 40e6, 105.0, 0.17, 1400.0, 90e-6, 0.35, 0.30, 15e6, 0.6, 0.7),
    "asa": (2.0e9, 0.37, 38e6, 45e6, 100.0, 0.17, 1400.0, 90e-6, 0.35, 0.30, 15e6, 0.6, 0.7),
    "tpu": (0.03e9, 0.48, 6e6, 30e6, -30.0, 0.19, 1500.0, 150e-6, 0.8, 0.6, 3e6, 0.9, 0.9),
    "nylon": (1.6e9, 0.40, 45e6, 50e6, 50.0, 0.25, 1700.0, 100e-6, 0.25, 0.20, 25e6, 0.7, 0.8),
    "resin": (2.2e9, 0.38, 40e6, 50e6, 70.0, 0.18, 1500.0, 90e-6, 0.4, 0.3, 12e6, 1.0, 1.0),
    "al": (69e9, 0.33, 275e6, 310e6, 500.0, 167.0, 896.0, 23e-6, 0.45, 0.45, 100e6, 1.0, 1.0),
    "steel": (200e9, 0.29, 500e6, 600e6, 1000.0, 45.0, 470.0, 12e-6, 0.4, 0.4, 250e6, 1.0, 1.0),
    "brass": (100e9, 0.34, 200e6, 350e6, 800.0, 110.0, 380.0, 19e-6, 0.35, 0.35, 80e6, 1.0, 1.0),
    "pcb": (20e9, 0.14, 150e6, 300e6, 130.0, 0.3, 1100.0, 15e-6, 0.4, 0.4, 40e6, 1.0, 1.0),
    "rubber": (0.005e9, 0.49, 3e6, 15e6, -50.0, 0.16, 2000.0, 200e-6, 1.0, 0.9, 2e6, 1.0, 1.0),
    "glass": (3.0e9, 0.37, 60e6, 70e6, 105.0, 0.19, 1470.0, 70e-6, 0.4, 0.35, 20e6, 1.0, 1.0),
}
_ENG_DEFAULT = _ENG["pla"]


def default_engineering(material_id: str, tags: Optional[list] = None) -> dict:
    """Engineering defaults for a material id (unknown ids get PLA-like values
    if tagged as prints, steel-like if tagged as metal)."""
    e = _ENG.get(material_id)
    if e is None:
        e = _ENG["steel"] if tags and "metal" in tags else _ENG_DEFAULT
    E, nu, sy, su, tg, k, cp, alpha, mu_self, mu_steel, bearing, aniso, adhesion = e
    friction = {"self": {"static": round(mu_self * 1.2, 3), "kinetic": mu_self}, "steel": {"static": round(mu_steel * 1.2, 3), "kinetic": mu_steel}, "world": {"static": round(mu_self * 1.2, 3), "kinetic": mu_self}}
    is_print = material_id in ("pla", "petg", "abs", "asa", "tpu", "nylon") or bool(tags and "print" in tags and "resin" not in tags)
    return {"youngs_modulus": E, "poisson": nu, "yield_strength": sy, "ultimate_strength": su, "glass_transition_c": tg, "thermal_conductivity": k, "specific_heat": cp, "thermal_expansion": alpha, "friction": friction, "bearing_pressure": bearing, "print": {"anisotropy_z": aniso, "layer_adhesion_factor": adhesion} if is_print else None}


@dataclass
class Material:
    id: str
    name: str
    density: float  # g/cm³
    color: tuple[float, float, float] = (0.7, 0.7, 0.72)
    roughness: float = 0.5
    metallic: float = 0.0
    tags: list[str] = field(default_factory=list)
    engineering: dict = field(default_factory=dict)  # see ENGINEERING_KEYS; missing keys fall back to defaults

    def props(self) -> dict:
        """Engineering properties, defaults filled in."""
        d = default_engineering(self.id, self.tags)
        for k, v in self.engineering.items():
            if k in ("friction", "print") and isinstance(v, dict) and isinstance(d.get(k), dict):
                d[k] = {**d[k], **v}
            else:
                d[k] = v
        return d

    def to_json(self) -> dict:
        return {"id": self.id, "name": self.name, "density": self.density, "color": list(self.color), "roughness": self.roughness, "metallic": self.metallic, "tags": self.tags, "engineering": self.engineering}

    @staticmethod
    def from_json(d: dict) -> "Material":
        return Material(d["id"], d["name"], d["density"], tuple(d.get("color", (0.7, 0.7, 0.72))), d.get("roughness", 0.5), d.get("metallic", 0.0), d.get("tags", []), dict(d.get("engineering", {})))


DEFAULT_MATERIALS = [
    Material("pla", "PLA", 1.24, (0.85, 0.85, 0.87), 0.6, 0.0, ["print", "plastic"]),
    Material("petg", "PETG", 1.27, (0.75, 0.82, 0.9), 0.35, 0.0, ["print", "plastic"]),
    Material("abs", "ABS", 1.04, (0.2, 0.2, 0.22), 0.55, 0.0, ["print", "plastic"]),
    Material("asa", "ASA", 1.07, (0.9, 0.55, 0.2), 0.5, 0.0, ["print", "plastic"]),
    Material("tpu", "TPU 95A", 1.21, (0.3, 0.3, 0.3), 0.9, 0.0, ["print", "flexible"]),
    Material("nylon", "Nylon PA12", 1.01, (0.92, 0.92, 0.88), 0.7, 0.0, ["print", "plastic"]),
    Material("resin", "Resin (standard)", 1.15, (0.6, 0.6, 0.6), 0.2, 0.0, ["print", "resin"]),
    Material("al", "Aluminium 6061", 2.70, (0.8, 0.82, 0.85), 0.35, 1.0, ["metal"]),
    Material("steel", "Steel", 7.85, (0.55, 0.56, 0.58), 0.4, 1.0, ["metal"]),
    Material("brass", "Brass", 8.5, (0.85, 0.7, 0.35), 0.3, 1.0, ["metal"]),
    Material("pcb", "PCB FR4", 1.85, (0.1, 0.45, 0.25), 0.6, 0.0, ["electronics"]),
    Material("rubber", "Rubber", 1.1, (0.15, 0.15, 0.15), 0.95, 0.0, ["flexible"]),
    Material("glass", "Acrylic", 1.18, (0.8, 0.9, 1.0), 0.05, 0.0, ["clear"]),
]


@dataclass
class Transform:
    """Translation + rotation (axis-angle, degrees) + uniform scale, for
    instances and reference meshes/images (bodies are baked in world)."""

    translation: Vec3 = (0.0, 0.0, 0.0)
    axis: Vec3 = (0.0, 0.0, 1.0)
    angle_deg: float = 0.0
    scale: float = 1.0

    def to_json(self) -> dict:
        return {"translation": list(self.translation), "axis": list(self.axis), "angle_deg": self.angle_deg, "scale": self.scale}

    @staticmethod
    def from_json(d: dict) -> "Transform":
        return Transform(tuple(d.get("translation", (0, 0, 0))), tuple(d.get("axis", (0, 0, 1))), d.get("angle_deg", 0.0), d.get("scale", 1.0))

    def apply(self, kernel: GeometryKernel, body: Body) -> Body:
        return kernel.transform(body, self.translation, self.axis, self.angle_deg, (0.0, 0.0, 0.0), self.scale)

    def matrix(self) -> list[list[float]]:
        import math

        ax = self.axis
        n = math.sqrt(sum(c * c for c in ax)) or 1.0
        x, y, z = ax[0] / n, ax[1] / n, ax[2] / n
        a = math.radians(self.angle_deg)
        c, s, t = math.cos(a), math.sin(a), 1 - math.cos(a)
        S = self.scale
        return [
            [S * (t * x * x + c), S * (t * x * y - s * z), S * (t * x * z + s * y), self.translation[0]],
            [S * (t * x * y + s * z), S * (t * y * y + c), S * (t * y * z - s * x), self.translation[1]],
            [S * (t * x * z - s * y), S * (t * y * z + s * x), S * (t * z * z + c), self.translation[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]


@dataclass
class Measurement:
    kind: str  # distance | radius | angle | dimension
    points: list[Vec3]
    value: float
    label: str = ""
    # For live dimensions: the body and the face(s) it is attached to.
    body_id: Optional[str] = None
    faces: list[dict] = field(default_factory=list)

    def to_json(self) -> dict:
        return {"kind": self.kind, "points": [list(p) for p in self.points], "value": self.value, "label": self.label, "body_id": self.body_id, "faces": self.faces}

    @staticmethod
    def from_json(d: dict) -> "Measurement":
        return Measurement(d["kind"], [tuple(p) for p in d["points"]], d["value"], d.get("label", ""), d.get("body_id"), d.get("faces", []))


@dataclass
class Node:
    id: str
    kind: str  # body | sheet | curve | sketch | mesh | image | plane | measure | group | instance
    name: str
    parent: Optional[str] = None
    children: list[str] = field(default_factory=list)
    visible: bool = True
    locked: bool = False
    disabled: bool = False
    material: Optional[str] = None
    color: Optional[tuple[float, float, float]] = None
    pivot: Optional[Vec3] = None
    # payloads
    body: Optional[Body] = None
    sketch: Optional[Sketch] = None
    plane: Optional[Plane] = None
    measure: Optional[Measurement] = None
    mesh: Optional[Mesh] = None
    image: Optional[dict] = None  # {path, plane, width, height, opacity, data(bytes)}
    source: Optional[str] = None  # instance: the node it mirrors
    transform: Transform = field(default_factory=Transform)
    mirror_plane: Optional[Plane] = None  # a live mirror instance
    tessellation_tolerance: float = 0.05
    # Robotics: a `joint` node carries a Joint; a motor body carries robot metadata.
    joint: Optional[Any] = None
    robot: Optional[dict] = None
    # Simulation results for this node (link/joint/motor block of the results file).
    results: Optional[dict] = None

    def to_json(self) -> dict:
        d = {"id": self.id, "kind": self.kind, "name": self.name, "parent": self.parent, "children": self.children, "visible": self.visible, "locked": self.locked, "disabled": self.disabled, "material": self.material, "color": list(self.color) if self.color else None, "pivot": list(self.pivot) if self.pivot else None, "transform": self.transform.to_json(), "source": self.source, "tessellation_tolerance": self.tessellation_tolerance}
        if self.body is not None:
            d["body_kind"] = self.body.kind
        if self.sketch is not None:
            d["sketch"] = self.sketch.to_json()
        if self.plane is not None:
            d["plane"] = self.plane.to_json()
        if self.measure is not None:
            d["measure"] = self.measure.to_json()
        if self.mirror_plane is not None:
            d["mirror_plane"] = self.mirror_plane.to_json()
        if self.joint is not None:
            d["joint"] = self.joint.to_json()
        if self.robot is not None:
            d["robot"] = self.robot
        if self.results is not None:
            d["results"] = self.results
        if self.image is not None:
            d["image"] = {k: v for k, v in self.image.items() if k != "data" and not k.startswith('_')}
            d["image"]["plane"] = self.image["plane"].to_json() if isinstance(self.image.get("plane"), Plane) else self.image.get("plane")
        return d


class Document:
    def __init__(self, kernel: Optional[GeometryKernel] = None):
        self.kernel = kernel or default_kernel()
        self.nodes: dict[str, Node] = {}
        self.roots: list[str] = []
        self.materials: dict[str, Material] = {m.id: m for m in DEFAULT_MATERIALS}
        self.active_group: Optional[str] = None
        self.path: Optional[str] = None
        self.dirty = False
        self.view: dict[str, Any] = {}
        self.saved_views: dict[str, Any] = {}
        # Robot-level settings: battery, control, uncertainty, world, identification.
        self.robot_settings: dict[str, Any] = {}
        from .component_graph import empty_graph
        self.component_graph = empty_graph()
        # The last simulation results file loaded (see physical.load_results).
        self.results: Optional[dict] = None
        self.annotations: dict[str, dict] = {}
        self.document_id = uuid.uuid4().hex
        self.revision = 0
        self._snapshot_body_cache = {}
        self.listeners: list[Callable[[str, Any], None]] = []
        self.autosave_interval: float = 120.0
        self._autosave_thread: Optional[threading.Thread] = None
        self._autosave_stop = threading.Event()
        self._lock = threading.RLock()
        self.mesh_cache: dict[tuple[str, float], Mesh] = {}

    # ---- events -------------------------------------------------------
    def notify(self, event: str, payload: Any = None):
        if event in ('changed', 'annotations', 'saved_views'):
            self.revision += 1
        if event == 'changed' and self.results:
            self.results['stale'] = True
        for cb in list(self.listeners):
            try:
                cb(event, payload)
            except Exception:
                pass

    def touch(self, node_id: Optional[str] = None, *, geometry: bool = True):
        self.dirty = True
        if node_id and geometry:
            self.mesh_cache = {k: v for k, v in self.mesh_cache.items() if k[0] != node_id}
            # Instances of this node are stale too.
            for n in self.nodes.values():
                if n.kind == "instance" and n.source == node_id:
                    self.mesh_cache = {k: v for k, v in self.mesh_cache.items() if k[0] != n.id}
        self.notify("changed", node_id)

    # ---- nodes ---------------------------------------------------------
    def new_id(self) -> str:
        return uuid.uuid4().hex[:12]

    def unique_name(self, base: str) -> str:
        names = {n.name for n in self.nodes.values()}
        if base not in names:
            return base
        k = 2
        while f"{base} {k}" in names:
            k += 1
        return f"{base} {k}"

    def add(self, node: Node, parent: Optional[str] = None, index: Optional[int] = None) -> Node:
        with self._lock:
            parent = parent if parent is not None else self.active_group
            if parent is not None and parent not in self.nodes:
                parent = None
            node.parent = parent
            self.nodes[node.id] = node
            siblings = self.nodes[parent].children if parent else self.roots
            if index is None:
                siblings.append(node.id)
            else:
                siblings.insert(index, node.id)
        self.touch(node.id)
        self.notify("added", node.id)
        return node

    def add_body(self, body: Body, name: str = "Body", material: Optional[str] = None, parent: Optional[str] = None) -> Node:
        kind = {"solid": "body", "sheet": "sheet", "wire": "curve"}.get(body.kind, "body")
        node = Node(self.new_id(), kind, self.unique_name(name), body=body, material=material or ("pla" if kind == "body" else None))
        return self.add(node, parent)

    def add_sketch(self, sketch: Sketch, name: str = "Sketch", parent: Optional[str] = None) -> Node:
        return self.add(Node(self.new_id(), "sketch", self.unique_name(name), sketch=sketch), parent)

    def add_plane(self, plane: Plane, name: str = "Plane", parent: Optional[str] = None) -> Node:
        return self.add(Node(self.new_id(), "plane", self.unique_name(name), plane=plane), parent)

    def add_group(self, name: str = "Group", parent: Optional[str] = None) -> Node:
        return self.add(Node(self.new_id(), "group", self.unique_name(name)), parent)

    def add_measure(self, m: Measurement, name: str = "Measurement", parent: Optional[str] = None) -> Node:
        return self.add(Node(self.new_id(), "measure", self.unique_name(name), measure=m), parent)

    def add_instance(self, source: str, transform: Transform = Transform(), name: Optional[str] = None, mirror: Optional[Plane] = None, parent: Optional[str] = None) -> Node:
        src = self.nodes[source]
        node = Node(self.new_id(), "instance", self.unique_name(name or f"{src.name} instance"), source=source, transform=transform, mirror_plane=mirror, material=src.material)
        return self.add(node, parent)

    def add_mesh(self, mesh: Mesh, name: str = "Mesh", transform: Transform = Transform(), parent: Optional[str] = None) -> Node:
        return self.add(Node(self.new_id(), "mesh", self.unique_name(name), mesh=mesh, transform=transform), parent)

    def add_image(self, path: str, plane: Plane, width: float, height: float, opacity: float = 0.6, data: Optional[bytes] = None, parent: Optional[str] = None) -> Node:
        if data is None:
            with open(path, "rb") as f:
                data = f.read()
        img = {"path": path, "plane": plane, "width": width, "height": height, "opacity": opacity, "data": data}
        return self.add(Node(self.new_id(), "image", self.unique_name(os.path.basename(path) or "Image"), image=img), parent)

    def remove(self, node_id: str) -> list[Node]:
        """Remove a node and its subtree; returns the removed nodes (for undo)."""
        removed = []
        with self._lock:
            node = self.nodes.get(node_id)
            if node is None:
                return removed
            for child in list(node.children):
                removed.extend(self.remove(child))
            siblings = self.nodes[node.parent].children if node.parent and node.parent in self.nodes else self.roots
            if node_id in siblings:
                siblings.remove(node_id)
            del self.nodes[node_id]
            removed.append(node)
            if self.active_group == node_id:
                self.active_group = None
        self.touch(node_id)
        self.notify("removed", node_id)
        return removed

    def restore(self, node: Node, index: Optional[int] = None):
        self.add(node, node.parent, index)

    def move(self, node_id: str, new_parent: Optional[str], index: Optional[int] = None):
        with self._lock:
            node = self.nodes[node_id]
            if new_parent is not None:
                # No cycles.
                p = new_parent
                while p is not None:
                    if p == node_id:
                        return
                    p = self.nodes[p].parent
            old = self.nodes[node.parent].children if node.parent else self.roots
            old.remove(node_id)
            node.parent = new_parent
            new = self.nodes[new_parent].children if new_parent else self.roots
            if index is None:
                new.append(node_id)
            else:
                new.insert(min(index, len(new)), node_id)
        self.touch()
        self.notify("moved", node_id)

    def index_of(self, node_id: str) -> int:
        node = self.nodes[node_id]
        siblings = self.nodes[node.parent].children if node.parent else self.roots
        return siblings.index(node_id)

    def walk(self, parent: Optional[str] = None) -> Iterable[Node]:
        ids = self.roots if parent is None else self.nodes[parent].children
        for i in ids:
            n = self.nodes.get(i)
            if n is None:
                continue
            yield n
            if n.children:
                yield from self.walk(i)

    def bodies(self, visible_only: bool = False) -> list[Node]:
        return [n for n in self.walk() if n.kind in ("body", "sheet") and n.body is not None and (not visible_only or self.is_visible(n.id))]

    def is_visible(self, node_id: str) -> bool:
        n = self.nodes.get(node_id)
        while n is not None:
            if not n.visible or n.disabled:
                return False
            n = self.nodes.get(n.parent) if n.parent else None
        return True

    def find(self, name: str) -> Optional[Node]:
        return next((n for n in self.nodes.values() if n.name == name), None)

    def search(self, text: str) -> list[Node]:
        t = text.lower()
        return [n for n in self.walk() if t in n.name.lower()]

    def same_material(self, node_id: str) -> list[str]:
        m = self.nodes[node_id].material
        return [n.id for n in self.nodes.values() if n.kind in ("body", "instance") and n.material == m]

    # ---- geometry access --------------------------------------------------
    def resolved_body(self, node_id: str) -> Optional[Body]:
        """A body in world coordinates: an instance is its source transformed
        (and mirrored) on demand; a body is itself."""
        n = self.nodes.get(node_id)
        if n is None:
            return None
        if n.kind == "instance":
            src = self.resolved_body(n.source) if n.source else None
            if src is None:
                return None
            b = src
            if n.mirror_plane is not None:
                b = self.kernel.mirror(b, n.mirror_plane)
            if n.transform.angle_deg or any(n.transform.translation) or n.transform.scale != 1.0:
                b = n.transform.apply(self.kernel, b)
            return b
        return n.body

    def mesh_of(self, node_id: str, tolerance: Optional[float] = None) -> Optional[Mesh]:
        n = self.nodes.get(node_id)
        if n is None:
            return None
        tol = tolerance or n.tessellation_tolerance
        key = (node_id, tol)
        if key in self.mesh_cache:
            return self.mesh_cache[key]
        if n.kind == "mesh":
            m = n.mesh
            if n.transform.angle_deg or any(n.transform.translation) or n.transform.scale != 1.0:
                M = n.transform.matrix()
                m = Mesh([tuple(M[r][0] * v[0] + M[r][1] * v[1] + M[r][2] * v[2] + M[r][3] for r in range(3)) for v in n.mesh.vertices], n.mesh.normals, n.mesh.triangles, n.mesh.triangle_face, n.mesh.face_count)
            self.mesh_cache[key] = m
            return m
        body = self.resolved_body(node_id)
        if body is None:
            return None
        m = self.kernel.tessellate(body, tol)
        self.mesh_cache[key] = m
        return m

    def density_of(self, node_id: str) -> float:
        n = self.nodes[node_id]
        mat = self.materials.get(n.material or "")
        return mat.density if mat else 1.0

    # ---- persistence -------------------------------------------------------
    def to_manifest(self) -> dict:
        return {
            "format": "robocad", "version": FORMAT_VERSION, "saved": time.time(),
            "document_id": self.document_id, "revision": self.revision,
            "roots": self.roots, "active_group": self.active_group, "view": self.view,
            "saved_views": self.saved_views,
            "materials": [m.to_json() for m in self.materials.values()],
            "nodes": [n.to_json() for n in self.nodes.values()],
            "annotations": self.annotations,
            "robot_settings": self.robot_settings,
            "component_graph": self.component_graph,
            "results": self.results,
        }

    def save(self, path: Optional[str] = None, thumbnail: Optional[bytes] = None, *, mark_saved: bool = True):
        path = path or self.path
        if not path:
            raise ValueError("no path to save to")
        revision, entries = self.archive_snapshot(thumbnail)
        write_archive(path, entries)
        if mark_saved:
            self.path = path
            self.dirty = self.revision != revision
            self.notify("saved", path)

    def archive_snapshot(self, thumbnail=None):
        """Capture metadata and immutable geometry bytes on the owning thread.

        Loaded/saved B-reps are reused by body-handle identity. Renaming,
        grouping, selection and material edits never reserialize geometry.
        Compression and file IO are deliberately outside this method.
        """
        with self._lock:
            entries = [('manifest.json', json.dumps(self.to_manifest(), indent=1).encode())]
            for n in self.nodes.values():
                if n.body is not None:
                    cached = self._snapshot_body_cache.get(n.id)
                    if cached is None or cached[0] is not n.body:
                        cached = (n.body, self.kernel.serialize(n.body))
                        self._snapshot_body_cache[n.id] = cached
                    entries.append((f'brep/{n.id}.brep', cached[1]))
                if n.mesh is not None:
                    import numpy as np
                    buf = io.BytesIO()
                    # The outer archive handles compression in the worker.
                    np.savez(buf, vertices=np.array(n.mesh.vertices, dtype=np.float32), normals=np.array(n.mesh.normals, dtype=np.float32), triangles=np.array(n.mesh.triangles, dtype=np.int32), triangle_face=np.array(n.mesh.triangle_face, dtype=np.int32))
                    entries.append((f'mesh/{n.id}.npz', buf.getvalue()))
                if n.image is not None and n.image.get('data'):
                    entries.append((f'image/{n.id}', bytes(n.image['data'])))
            if thumbnail:
                entries.append(('thumbnail.png', bytes(thumbnail)))
            return self.revision, tuple(entries)

    @classmethod
    def load(cls, path: str, kernel: Optional[GeometryKernel] = None) -> "Document":
        doc = cls(kernel)
        with zipfile.ZipFile(path) as z:
            manifest = json.loads(z.read("manifest.json"))
            doc.document_id = manifest.get('document_id', doc.document_id)
            doc.revision = int(manifest.get('revision', 0))
            doc.materials = {m["id"]: Material.from_json(m) for m in manifest.get("materials", [])} or doc.materials
            doc.view = manifest.get("view", {})
            doc.saved_views = manifest.get("saved_views", {})
            doc.robot_settings = manifest.get("robot_settings", {}) or {}
            doc.component_graph = manifest.get('component_graph', doc.component_graph)
            doc.results = manifest.get("results")
            doc.annotations = manifest.get("annotations", {})
            names = set(z.namelist())
            for d in manifest["nodes"]:
                node = Node(d["id"], d["kind"], d["name"], d.get("parent"), list(d.get("children", [])), d.get("visible", True), d.get("locked", False), d.get("disabled", False), d.get("material"), tuple(d["color"]) if d.get("color") else None, tuple(d["pivot"]) if d.get("pivot") else None)
                node.transform = Transform.from_json(d.get("transform", {}))
                node.source = d.get("source")
                node.tessellation_tolerance = d.get("tessellation_tolerance", 0.05)
                if f"brep/{node.id}.brep" in names:
                    captured_brep = z.read(f"brep/{node.id}.brep")
                    node.body = doc.kernel.deserialize(captured_brep, d.get("body_kind", "solid"))
                    # OCCT mutates internal query flags while loading/tessellating.
                    # Preserve the bytes of this immutable handle for identity;
                    # replacing the body naturally invalidates this cache.
                    doc._snapshot_body_cache[node.id] = (node.body, captured_brep)
                if "sketch" in d:
                    node.sketch = Sketch.from_json(d["sketch"])
                if "plane" in d:
                    node.plane = Plane.from_json(d["plane"])
                if "measure" in d:
                    node.measure = Measurement.from_json(d["measure"])
                if "mirror_plane" in d:
                    node.mirror_plane = Plane.from_json(d["mirror_plane"])
                if "joint" in d:
                    from .robotics import Joint

                    node.joint = Joint.from_json(d["joint"])
                if "robot" in d:
                    node.robot = d["robot"]
                if "results" in d:
                    node.results = d["results"]
                if f"mesh/{node.id}.npz" in names:
                    import numpy as np

                    data = np.load(io.BytesIO(z.read(f"mesh/{node.id}.npz")))
                    node.mesh = Mesh([tuple(map(float, v)) for v in data["vertices"]], [tuple(map(float, v)) for v in data["normals"]], [tuple(map(int, t)) for t in data["triangles"]], [int(x) for x in data["triangle_face"]], int(data["triangle_face"].max()) + 1 if len(data["triangle_face"]) else 0)
                if "image" in d:
                    img = {k:v for k,v in d["image"].items() if not k.startswith('_')}
                    img["plane"] = Plane.from_json(img["plane"]) if isinstance(img.get("plane"), dict) else Plane.xy()
                    img["data"] = z.read(f"image/{node.id}") if f"image/{node.id}" in names else None
                    node.image = img
                doc.nodes[node.id] = node
            doc.roots = [i for i in manifest.get("roots", []) if i in doc.nodes]
            doc.active_group = manifest.get("active_group")
        doc.path = path
        doc.dirty = False
        return doc

    @staticmethod
    def read_thumbnail(path: str) -> Optional[bytes]:
        try:
            with zipfile.ZipFile(path) as z:
                return z.read("thumbnail.png") if "thumbnail.png" in z.namelist() else None
        except Exception:
            return None

    # ---- autosave ----------------------------------------------------------
    def autosave_path(self) -> str:
        base = self.path or os.path.join(os.path.expanduser("~"), "untitled.rcad")
        root, _ = os.path.splitext(base)
        return root + ".autosave.rcad"

    def start_autosave(self, interval: Optional[float] = None):
        if interval:
            self.autosave_interval = interval
        self.stop_autosave()
        self._autosave_stop.clear()

        def loop():
            while not self._autosave_stop.wait(self.autosave_interval):
                if self.dirty:
                    try:
                        self.save_autosave()
                    except Exception:
                        pass

        self._autosave_thread = threading.Thread(target=loop, daemon=True)
        self._autosave_thread.start()

    def save_autosave(self):
        p = self.autosave_path()
        self.save(p, mark_saved=False)
        self.notify("autosaved", p)

    def stop_autosave(self):
        self._autosave_stop.set()
        self._autosave_thread = None

    # ---- clipboard with placement ------------------------------------------
    def copy_nodes(self, ids: Iterable[str]) -> dict:
        """A serialisable clipboard: nodes with their B-rep and world placement."""
        items = []
        for i in ids:
            n = self.nodes.get(i)
            if n is None:
                continue
            body = self.resolved_body(i)
            items.append({"node": n.to_json(), "brep": self.kernel.serialize(body).hex() if body is not None else None, "sketch": n.sketch.to_json() if n.sketch else None})
        return {"robocad_clipboard": True, "items": items}

    def paste_nodes(self, clip: dict, keep_placement: bool = True, offset: Vec3 = (0.0, 0.0, 0.0)) -> list[Node]:
        out = []
        for item in clip.get("items", []):
            d = item["node"]
            if item.get("brep"):
                body = self.kernel.deserialize(bytes.fromhex(item["brep"]), d.get("body_kind", "solid"))
                if not keep_placement or any(offset):
                    body = self.kernel.transform(body, offset)
                node = self.add_body(body, d["name"], d.get("material"))
                node.color = tuple(d["color"]) if d.get("color") else None
                out.append(node)
            elif item.get("sketch"):
                out.append(self.add_sketch(Sketch.from_json(item["sketch"]), d["name"]))
        return out
