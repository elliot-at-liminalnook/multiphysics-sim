"""Commands and the undo stack, and the `Ops` façade every UI action and
script goes through.

A command captures the document state it touches *before* running, so
undo is a swap back: for a body edit that is the previous kernel handle,
for an outliner or material change the previous attribute values, for
add/remove the node objects and their positions. `CommandStack` keeps two
lists; `Ops` builds commands and runs them, and is also the scripting API
(`robocad.commands.Ops(doc)`), so the acceptance macro drives exactly what
the buttons drive.
"""

from __future__ import annotations

import math
from copy import deepcopy
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable, Optional, Sequence

from .document import Material, Document, Measurement, Node, Transform
from .annotations import AnnotationOps, ChangeThreads, stamp
from .references import ReferenceOps
from .saved_views import SavedViewOps
from .kernel import Body, BooleanOp, ChamferSpec, EdgeRef, FaceRef, KernelError, Plane, Sketch, SurfaceKind, SweepOptions, Vec3
from .kernel.base import v_add, v_cross, v_dist, v_dot, v_scale, v_sub, v_unit

# ------------------------------------------------------------- commands


class Command:
    label: str

    def do(self, doc: Document) -> Any: ...
    def undo(self, doc: Document) -> None: ...
    def redo(self, doc: Document) -> Any:
        return self.do(doc)


@dataclass
class EditBodies(Command):
    """Replace the bodies of nodes: `changes` maps node id → new body."""

    label: str
    changes: dict[str, Body]
    previous: dict[str, Body] = field(default_factory=dict)

    def do(self, doc: Document):
        for nid, body in self.changes.items():
            n = doc.nodes[nid]
            if nid not in self.previous:
                self.previous[nid] = n.body
            n.body = body
            n.kind = {"solid": "body", "sheet": "sheet", "wire": "curve"}.get(body.kind, n.kind)
            doc.touch(nid)
        return list(self.changes)

    def undo(self, doc: Document):
        for nid, body in self.previous.items():
            n = doc.nodes[nid]
            n.body = body
            n.kind = {"solid": "body", "sheet": "sheet", "wire": "curve"}.get(body.kind, n.kind) if body else n.kind
            doc.touch(nid)


@dataclass
class AddNodes(Command):
    label: str
    nodes: list[Node]
    parent: Optional[str] = None
    indices: list[int] = field(default_factory=list)

    def do(self, doc: Document):
        for i, n in enumerate(self.nodes):
            idx = self.indices[i] if i < len(self.indices) else None
            doc.add(n, self.parent if n.parent is None else n.parent, idx)
        return [n.id for n in self.nodes]

    def undo(self, doc: Document):
        self.indices = [doc.index_of(n.id) for n in self.nodes]
        for n in self.nodes:
            doc.remove(n.id)


@dataclass
class RemoveNodes(Command):
    label: str
    ids: list[str]
    removed: list[tuple[Node, int]] = field(default_factory=list)

    def do(self, doc: Document):
        self.removed = []
        for i in self.ids:
            if i in doc.nodes:
                idx = doc.index_of(i)
                for n in doc.remove(i):
                    self.removed.append((n, idx))
        return self.ids

    def undo(self, doc: Document):
        # Restore parents before children (they were removed children first).
        for n, idx in reversed(self.removed):
            doc.restore(n, idx if n.id in self.ids else None)


@dataclass
class SetAttributes(Command):
    """Set plain attributes on nodes (name, visible, locked, material, color,
    pivot, transform, tessellation tolerance…); undo restores them."""

    label: str
    changes: dict[str, dict[str, Any]]
    previous: dict[str, dict[str, Any]] = field(default_factory=dict)

    def do(self, doc: Document):
        for nid, attrs in self.changes.items():
            n = doc.nodes[nid]
            if nid not in self.previous:
                self.previous[nid] = {k: getattr(n, k) for k in attrs}
            for k, v in attrs.items():
                setattr(n, k, v)
            doc.touch(nid, geometry=bool(set(attrs) & {'body', 'mesh', 'transform', 'mirror_plane', 'source', 'tessellation_tolerance', 'kind'}))
        return list(self.changes)

    def undo(self, doc: Document):
        for nid, attrs in self.previous.items():
            n = doc.nodes[nid]
            for k, v in attrs.items():
                setattr(n, k, v)
            doc.touch(nid, geometry=bool(set(attrs) & {'body', 'mesh', 'transform', 'mirror_plane', 'source', 'tessellation_tolerance', 'kind'}))


@dataclass
class MoveNode(Command):
    label: str
    node_id: str
    new_parent: Optional[str]
    index: Optional[int] = None
    old_parent: Optional[str] = None
    old_index: int = 0

    def do(self, doc: Document):
        n = doc.nodes[self.node_id]
        self.old_parent, self.old_index = n.parent, doc.index_of(self.node_id)
        doc.move(self.node_id, self.new_parent, self.index)
        return self.node_id

    def undo(self, doc: Document):
        doc.move(self.node_id, self.old_parent, self.old_index)


@dataclass
class SetMaterialDef(Command):
    label: str
    material: Any
    previous: Any = None

    def do(self, doc: Document):
        self.previous = doc.materials.get(self.material.id)
        doc.materials[self.material.id] = self.material
        doc.touch()

    def undo(self, doc: Document):
        if self.previous is None:
            doc.materials.pop(self.material.id, None)
        else:
            doc.materials[self.material.id] = self.previous
        doc.touch()


@dataclass
class Composite(Command):
    label: str
    commands: list[Command]

    def do(self, doc: Document):
        out = None
        for c in self.commands:
            out = c.do(doc)
        return out

    def undo(self, doc: Document):
        for c in reversed(self.commands):
            c.undo(doc)


class CommandStack:
    def __init__(self, doc: Document, limit: int = 200):
        self.doc = doc
        self.undo_stack: list[Command] = []
        self.redo_stack: list[Command] = []
        self.limit = limit
        self.listeners: list[Callable[[], None]] = []

    def push(self, command: Command) -> Any:
        result = command.do(self.doc)
        self.undo_stack.append(command)
        if len(self.undo_stack) > self.limit:
            self.undo_stack.pop(0)
        self.redo_stack.clear()
        self._changed()
        return result

    def undo(self) -> Optional[str]:
        if not self.undo_stack:
            return None
        c = self.undo_stack.pop()
        c.undo(self.doc)
        self.redo_stack.append(c)
        self._changed()
        return c.label

    def redo(self) -> Optional[str]:
        if not self.redo_stack:
            return None
        c = self.redo_stack.pop()
        c.redo(self.doc)
        self.undo_stack.append(c)
        self._changed()
        return c.label

    def can_undo(self) -> bool:
        return bool(self.undo_stack)

    def can_redo(self) -> bool:
        return bool(self.redo_stack)

    def _changed(self):
        for cb in self.listeners:
            cb()


# ----------------------------------------------------------------- ops


class Ops(AnnotationOps, ReferenceOps, SavedViewOps):
    """Every modeling action. Methods return node ids (or values) and push
    exactly one undoable command each."""

    def configure_robot(self, expected_revision: int, updates=None, joints=None, groups=None, moves=None):
        """Apply validated assembly metadata and connectors as one undoable edit."""
        from .assembly import configure_robot
        return configure_robot(self, expected_revision, updates, joints, groups, moves)

    def set_component_graph(self, graph: dict) -> dict:
        from .component_graph import ChangeGraph
        self.stack.push(ChangeGraph(self.doc, graph))
        return deepcopy(self.doc.component_graph)

    def __init__(self, doc: Document, stack: Optional[CommandStack] = None):
        self.doc = doc
        self.stack = stack or CommandStack(doc)
        self.k = doc.kernel
        self.last_clearance = 0.2
        self.last_polygon_sides = 6

    # ---- helpers -------------------------------------------------------
    def body_of(self, node_id: str) -> Body:
        b = self.doc.resolved_body(node_id)
        if b is None:
            raise KernelError(f"node {node_id} has no geometry")
        return b

    def _edit(self, label: str, node_id: str, fn: Callable[[Body], Body]) -> str:
        node = self.doc.nodes[node_id]
        if node.locked:
            raise KernelError(f"{node.name} is locked")
        if node.kind == "instance":
            raise KernelError("edit the source body; an instance follows it")
        new = fn(node.body)
        self.stack.push(EditBodies(label, {node_id: new}))
        return node_id

    def _new(self, label: str, body: Body, name: str, material: Optional[str] = None) -> str:
        kind = {"solid": "body", "sheet": "sheet", "wire": "curve"}.get(body.kind, "body")
        node = Node(self.doc.new_id(), kind, self.doc.unique_name(name), body=body, material=material or ("pla" if kind == "body" else None))
        self.stack.push(AddNodes(label, [node]))
        return node.id

    def undo(self) -> Optional[str]:
        return self.stack.undo()

    def redo(self) -> Optional[str]:
        return self.stack.redo()

    # ---- nodes / outliner ---------------------------------------------
    def delete(self, ids: Sequence[str]):
        self.stack.push(RemoveNodes("Delete", list(ids)))

    def rename(self, node_id: str, name: str):
        self.stack.push(SetAttributes("Rename", {node_id: {"name": name}}))

    def set_visible(self, ids: Sequence[str], visible: bool):
        self.stack.push(SetAttributes("Show" if visible else "Hide", {i: {"visible": visible} for i in ids}))

    def set_locked(self, ids: Sequence[str], locked: bool):
        self.stack.push(SetAttributes("Lock" if locked else "Unlock", {i: {"locked": locked} for i in ids}))

    def set_disabled(self, ids: Sequence[str], disabled: bool):
        self.stack.push(SetAttributes("Disable" if disabled else "Enable", {i: {"disabled": disabled} for i in ids}))

    def set_material(self, ids: Sequence[str], material_id: str):
        self.stack.push(SetAttributes("Material", {i: {"material": material_id} for i in ids}))

    def set_color(self, ids: Sequence[str], color: Optional[tuple[float, float, float]]):
        self.stack.push(SetAttributes("Color", {i: {"color": color} for i in ids}))

    def set_pivot(self, node_id: str, pivot: Optional[Vec3]):
        self.stack.push(SetAttributes("Pivot", {node_id: {"pivot": pivot}}))

    def group(self, ids: Sequence[str], name: str = "Group") -> str:
        ids = self._selection_roots(ids)
        parent = self.doc.nodes[ids[0]].parent if ids else None
        g = Node(self.doc.new_id(), "group", self.doc.unique_name(name), parent=parent)
        cmds: list[Command] = [AddNodes("Group", [g])]
        for i in ids:
            cmds.append(MoveNode("Group", i, g.id))
        self.stack.push(Composite("Group", cmds))
        return g.id

    def _selection_roots(self, ids):
        selected = set(ids)
        roots = []
        for i in dict.fromkeys(ids):
            p = self.doc.nodes[i].parent
            while p is not None and p not in selected:
                p = self.doc.nodes[p].parent
            if p is None:
                roots.append(i)
        return roots

    def move_nodes(self, ids: Sequence[str], new_parent: Optional[str], index: Optional[int] = None):
        """Reorganize a selection in one undo step, preserving nested groups."""
        ids = self._selection_roots(ids)
        if new_parent is not None:
            if self.doc.nodes[new_parent].kind != 'group':
                raise KernelError('Move target must be a group')
            p = new_parent
            while p is not None:
                if p in ids:
                    raise KernelError('Cannot move a group into itself or its descendants')
                p = self.doc.nodes[p].parent
        if ids:
            self.stack.push(Composite('Move in outliner', [
                MoveNode('Move in outliner', nid, new_parent, None if index is None else index + offset)
                for offset, nid in enumerate(ids)]))

    def move_node(self, node_id: str, new_parent: Optional[str], index: Optional[int] = None):
        self.stack.push(MoveNode("Move in outliner", node_id, new_parent, index))

    def set_active_group(self, group_id: Optional[str]):
        self.doc.active_group = group_id
        self.doc.notify("active_group", group_id)

    def isolate(self, ids: Sequence[str]):
        """Hide everything but `ids` (and their ancestors); undoable as one step."""
        keep = set(ids)
        for i in ids:
            keep.update(n.id for n in self.doc.walk(i))
        for i in list(ids):
            p = self.doc.nodes[i].parent
            while p:
                keep.add(p)
                p = self.doc.nodes[p].parent
        changes = {n.id: {"visible": n.id in keep} for n in self.doc.walk() if n.kind != "group" or n.id in keep or True}
        self.stack.push(SetAttributes("Isolate", changes))

    def show_all(self):
        self.stack.push(SetAttributes("Show all", {n.id: {"visible": True} for n in self.doc.walk()}))

    # ---- primitives -----------------------------------------------------
    def box(self, corner: Vec3, size: Vec3, name: str = "Box") -> str:
        return self._new("Box", self.k.box(corner, size), name)

    def box_center(self, center: Vec3, size: Vec3, name: str = "Box") -> str:
        corner = v_sub(center, v_scale(size, 0.5))
        return self.box(corner, size, name)

    def box_three_point(self, a: Vec3, b: Vec3, c: Vec3, height: float, name: str = "Box") -> str:
        """A box from two base corners along its x edge, a third point for its width, and a height."""
        x = v_unit(v_sub(b, a))
        w = v_sub(c, a)
        w = v_sub(w, v_scale(x, v_dot(w, x)))
        y = v_unit(w)
        z = v_cross(x, y)
        sk = Sketch(Plane(a, z, x))
        sk.rectangle((0, 0), (v_dist(a, b), v_dot(v_sub(c, a), y)))
        return self._new("Box", self.k.extrude(sk.to_body(), z, height), name)

    def cylinder(self, base: Vec3, axis: Vec3, radius: float, height: float, name: str = "Cylinder") -> str:
        return self._new("Cylinder", self.k.cylinder(base, axis, radius, height), name)

    def sphere(self, center: Vec3, radius: float, name: str = "Sphere") -> str:
        return self._new("Sphere", self.k.sphere(center, radius), name)

    # ---- sketches -------------------------------------------------------
    def new_sketch(self, plane: Plane, name: str = "Sketch") -> str:
        node = Node(self.doc.new_id(), "sketch", self.doc.unique_name(name), sketch=Sketch(plane, [], name))
        self.stack.push(AddNodes("Sketch", [node]))
        return node.id

    def edit_sketch(self, sketch_id: str, fn: Callable[[Sketch], Any], label: str = "Edit sketch") -> Any:
        """Apply `fn` to a copy of the sketch and swap it in (undoable)."""
        node = self.doc.nodes[sketch_id]
        new = Sketch.from_json(node.sketch.to_json())
        result = fn(new)
        self.stack.push(SetAttributes(label, {sketch_id: {"sketch": new}}))
        return result

    # ---- solids from sketches --------------------------------------------
    def _profile(self, source: str | Body, curves: Optional[Sequence[int]] = None) -> tuple[Body, Optional[Plane]]:
        if isinstance(source, Body):
            return source, None
        node = self.doc.nodes[source]
        if node.sketch is not None:
            sk = node.sketch
            sel = [sk.curves[i] for i in curves] if curves else sk.curves
            closed = [c for c in sel if c.closed or c.kind in ("circle", "ellipse", "slot")]
            if len(closed) >= 2:
                # Outer + holes: the largest closed loop is the outer.
                closed.sort(key=lambda c: -_loop_area(c))
                return sk.to_face(closed[0], closed[1:]), sk.plane
            if len(closed) == 1:
                return sk.to_face(closed[0]), sk.plane
            return sk.to_body(sel), sk.plane
        return self.body_of(source), None

    def _apply_boolean(self, body: Body, op: BooleanOp, target: Optional[str], name: str, label: str) -> str:
        if op == BooleanOp.NEW or target is None:
            return self._new(label, body, name)
        return self._edit(label, target, lambda t: self.k.boolean(t, body, op))

    def extrude(self, source: str | Body, distance: float, direction: Optional[Vec3] = None, taper_deg: float = 0.0, symmetric: bool = False, op: BooleanOp = BooleanOp.NEW, target: Optional[str] = None, up_to: Optional[str] = None, name: str = "Extrude") -> str:
        prof, plane = self._profile(source)
        d = direction or (plane.normal if plane else (0.0, 0.0, 1.0))
        if up_to:
            body = self.k.extrude_up_to(prof, d, self.body_of(up_to))
        else:
            body = self.k.extrude(prof, d, distance, taper_deg, symmetric)
        return self._apply_boolean(body, op, target, name, "Extrude")

    def revolve(self, source: str | Body, axis_point: Vec3, axis_dir: Vec3, angle_deg: float = 360.0, op: BooleanOp = BooleanOp.NEW, target: Optional[str] = None, name: str = "Revolve") -> str:
        prof, _ = self._profile(source)
        return self._apply_boolean(self.k.revolve(prof, axis_point, axis_dir, angle_deg), op, target, name, "Revolve")

    def sweep(self, profile: str | Body, path: str | Body, options: SweepOptions = SweepOptions(), name: str = "Sweep") -> str:
        prof, _ = self._profile(profile)
        p, _ = self._profile(path)
        return self._new("Sweep", self.k.sweep(prof, p, options), name)

    def pipe(self, path: str | Body, diameter: float, name: str = "Pipe") -> str:
        p, _ = self._profile(path)
        return self._new("Pipe", self.k.pipe(p, diameter), name)

    def loft(self, profiles: Sequence[str | Body], guides: Sequence[str | Body] = (), solid: bool = True, ruled: bool = False, name: str = "Loft") -> str:
        ps = [self._profile(p)[0] for p in profiles]
        gs = [self._profile(g)[0] for g in guides]
        return self._new("Loft", self.k.loft(ps, gs, solid, ruled), name)

    def fill(self, edges: str | Body, name: str = "Patch") -> str:
        b, _ = self._profile(edges)
        return self._new("Fill", self.k.fill_hole(b), name)

    def bridge(self, a: str | Body, b: str | Body, name: str = "Bridge") -> str:
        return self._new("Bridge", self.k.bridge(self._profile(a)[0], self._profile(b)[0]), name)

    # ---- direct editing ---------------------------------------------------
    def push_pull(self, node_id: str, face: FaceRef, distance: float) -> str:
        return self._edit("Push/Pull", node_id, lambda b: self.k.push_pull(b, face, distance))

    def offset_faces(self, node_id: str, faces: Sequence[FaceRef], distance: float) -> str:
        return self._edit("Offset face", node_id, lambda b: self.k.offset_faces(b, faces, distance))

    def offset_face_to(self, node_id: str, face: FaceRef, target: str, clearance: float = 0.0) -> str:
        t = self.body_of(target)
        return self._edit("Dependent offset", node_id, lambda b: self.k.offset_face_to_body(b, face, t, clearance))

    def move_faces(self, node_id: str, faces: Sequence[FaceRef], translation: Vec3) -> str:
        return self._edit("Move face", node_id, lambda b: self.k.move_faces(b, faces, translation))

    def rotate_faces(self, node_id: str, faces: Sequence[FaceRef], axis_point: Vec3, axis_dir: Vec3, angle_deg: float) -> str:
        return self._edit("Rotate face", node_id, lambda b: self.k.rotate_faces(b, faces, axis_point, axis_dir, angle_deg))

    def set_radius(self, node_id: str, face: FaceRef, radius: float) -> str:
        return self._edit("Set radius", node_id, lambda b: self.k.set_cylinder_radius(b, face, radius))

    def set_diameter(self, node_id: str, face: FaceRef, diameter: float) -> str:
        return self.set_radius(node_id, face, diameter / 2.0)

    def set_distance(self, node_id: str, face_a: FaceRef, face_b: FaceRef, distance: float, move: str = "b") -> str:
        """Live dimension between two parallel planar faces: push the chosen face."""
        current = abs(v_dot(v_sub(face_b.centroid, face_a.centroid), v_unit(face_a.normal)))
        delta = distance - current
        f = face_b if move == "b" else face_a
        return self._edit("Set distance", node_id, lambda b: self.k.push_pull(b, f, delta))

    def set_angle(self, node_id: str, face_a: FaceRef, face_b: FaceRef, angle_deg: float) -> str:
        """Rotate face_b about the edge it shares with face_a to the angle."""
        current = math.degrees(math.acos(max(-1.0, min(1.0, v_dot(v_unit(face_a.normal), v_unit(face_b.normal))))))
        axis = v_unit(v_cross(face_a.normal, face_b.normal))
        b = self.body_of(node_id)
        shared = [e for e in self.k.edges_of_face(b, face_b) if any(v_dist(e.midpoint, f.midpoint) < 1e-6 for f in self.k.edges_of_face(b, face_a))]
        point = shared[0].midpoint if shared else face_b.centroid
        return self._edit("Set angle", node_id, lambda body: self.k.rotate_faces(body, [face_b], point, axis, angle_deg - current))

    def draft(self, node_id: str, faces: Sequence[FaceRef], pull_dir: Vec3, angle_deg: float, neutral: Plane) -> str:
        return self._edit("Draft", node_id, lambda b: self.k.draft_faces(b, faces, pull_dir, angle_deg, neutral))

    def delete_faces(self, node_id: str, faces: Sequence[FaceRef]) -> str:
        return self._edit("Delete face", node_id, lambda b: self.k.delete_faces(b, faces))

    def untrim(self, node_id: str, faces: Sequence[FaceRef]) -> str:
        return self.delete_faces(node_id, faces)

    def imprint(self, node_id: str, tool: str | Body) -> str:
        t = self._profile(tool)[0]
        return self._edit("Imprint", node_id, lambda b: self.k.imprint(b, t))

    def split_face(self, node_id: str, plane: Plane) -> str:
        from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
        from .kernel.occt import _plane_of

        half = Body(BRepBuilderAPI_MakeFace(_plane_of(plane), -1e4, 1e4, -1e4, 1e4).Face(), "sheet")
        return self._edit("Split face", node_id, lambda b: self.k.imprint(b, half))

    def boolean(self, target: str, tools: Sequence[str], op: BooleanOp, keep_tools: bool = False) -> str:
        bodies = [self.body_of(t) for t in tools]

        def fn(b: Body) -> Body:
            out = b
            for t in bodies:
                out = self.k.boolean(out, t, op)
            return out

        cmds: list[Command] = [EditBodies(op.value.capitalize(), {target: fn(self.doc.nodes[target].body)})]
        if not keep_tools:
            cmds.append(RemoveNodes("Remove tools", [t for t in tools if self.doc.nodes[t].kind != "instance"]))
        self.stack.push(Composite(op.value.capitalize(), cmds))
        return target

    def region(self, a: str, b: str, name: str = "Region") -> str:
        return self._new("Region", self.k.boolean(self.body_of(a), self.body_of(b), BooleanOp.INTERSECT), name)

    def cut(self, node_id: str, cutter: str | Body | Plane, extend: bool = True, keep: str = "both") -> list[str]:
        """Cut with a curve, sheet or plane; returns the resulting node ids."""
        node = self.doc.nodes[node_id]
        body = node.body
        if isinstance(cutter, Plane):
            parts = self.k.cut_with_plane(body, cutter, keep)
        else:
            tool, plane = self._profile(cutter)
            if tool.kind == "wire" and plane is not None:
                # A curve cuts by extruding it far in both directions of its plane.
                tool = self.k.extrude(tool, plane.normal, 1.0e4, symmetric=True) if extend else tool
            elif tool.kind == "sheet" and extend:
                tool = self._extend_sheet(tool)
            parts = self.k.split(body, tool)
        if not parts:
            raise KernelError("the cutter does not cross the body")
        cmds: list[Command] = [EditBodies("Cut", {node_id: parts[0]})]
        new_nodes = [Node(self.doc.new_id(), "body", self.doc.unique_name(node.name), body=p, material=node.material, parent=node.parent) for p in parts[1:]]
        if new_nodes:
            cmds.append(AddNodes("Cut", new_nodes))
        self.stack.push(Composite("Cut", cmds))
        return [node_id] + [n.id for n in new_nodes]

    def _extend_sheet(self, sheet: Body) -> Body:
        props = self.k.mass_properties(sheet)
        faces = self.k.faces(sheet)
        if len(faces) == 1 and faces[0].kind == SurfaceKind.PLANE:
            from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
            from .kernel.occt import _plane_of

            pl = Plane.from_normal(faces[0].centroid, faces[0].normal)
            return Body(BRepBuilderAPI_MakeFace(_plane_of(pl), -1e4, 1e4, -1e4, 1e4).Face(), "sheet")
        return sheet

    def shell(self, node_id: str, thickness: float, open_faces: Sequence[FaceRef]) -> str:
        return self._edit("Shell", node_id, lambda b: self.k.shell(b, thickness, open_faces))

    def thicken(self, node_id: str, thickness: float) -> str:
        return self._edit("Thicken", node_id, lambda b: self.k.thicken(b, thickness))

    def fillet(self, node_id: str, edges: Sequence[EdgeRef], radius: float, radius_end: Optional[float] = None) -> str:
        return self._edit("Fillet", node_id, lambda b: self.k.fillet(b, edges, radius, radius_end))

    def fillet_chordal(self, node_id: str, edges: Sequence[EdgeRef], chord: float) -> str:
        return self._edit("Chordal fillet", node_id, lambda b: self.k.fillet_chordal(b, edges, chord))

    def fillet_all(self, node_id: str, radius: float, tension: float = 1.0) -> str:
        return self._edit("Fillet all", node_id, lambda b: self.k.fillet_all(b, radius, tension))

    def full_round(self, node_id: str, edge_a: EdgeRef, edge_b: EdgeRef) -> str:
        return self._edit("Full round", node_id, lambda b: self.k.full_round(b, edge_a, edge_b))

    def remove_fillets(self, node_id: str, faces: Sequence[FaceRef]) -> str:
        return self._edit("Remove fillets", node_id, lambda b: self.k.remove_fillets(b, faces))

    def chamfer(self, node_id: str, edges: Sequence[EdgeRef], spec: ChamferSpec) -> str:
        return self._edit("Chamfer", node_id, lambda b: self.k.chamfer(b, edges, spec))

    def transform(self, ids: Sequence[str], translation: Vec3 = (0.0, 0.0, 0.0), axis: Optional[Vec3] = None, angle_deg: float = 0.0, center: Optional[Vec3] = None, scale: float = 1.0):
        """Move/rotate/scale bodies (baked) and instances/meshes/images (their transform)."""
        edits: dict[str, Body] = {}
        attrs: dict[str, dict[str, Any]] = {}
        pins = {}
        for i in ids:
            n = self.doc.nodes[i]
            if n.locked:
                continue
            c = center or n.pivot or (self.k.mass_properties(n.body).centroid if n.body else (0.0, 0.0, 0.0))
            if n.body is not None:
                edits[i] = self.k.transform(n.body, translation, axis, angle_deg, c, scale, c)
                for tid, thread in self.doc.annotations.items():
                    if thread["anchor"]["node_id"] != i or thread["anchor"]["geometry"] != stamp(self.doc, i):
                        continue
                    t = deepcopy(thread)
                    q = v_scale(v_sub(tuple(t["anchor"]["point"]), c), scale)
                    if axis and angle_deg:
                        a, r = v_unit(axis), math.radians(angle_deg)
                        q = v_add(v_add(v_scale(q, math.cos(r)), v_scale(v_cross(a, q), math.sin(r))), v_scale(a, v_dot(a, q) * (1 - math.cos(r))))
                    t["anchor"]["point"] = list(v_add(v_add(q, c), translation))
                    t["anchor"]["geometry"] = stamp(self.doc, i, edits[i])
                    t["anchor"].pop("face", None)
                    pins[tid] = t
                if n.pivot is not None:
                    attrs[i] = {"pivot": v_add(n.pivot, translation)}
            else:
                t = n.transform
                new_t = Transform(v_add(t.translation, translation), axis or t.axis, t.angle_deg + angle_deg if axis is None or axis == t.axis else angle_deg, t.scale * scale)
                attrs[i] = {"transform": new_t}
        cmds: list[Command] = []
        if edits:
            cmds.append(EditBodies("Transform", edits))
        if attrs:
            cmds.append(SetAttributes("Transform", attrs))
        if pins:
            cmds.append(ChangeThreads("Move annotations", pins))
        if cmds:
            self.stack.push(Composite("Transform", cmds))

    def mirror(self, ids: Sequence[str], plane: Plane, live: bool = False, keep_original: bool = True) -> list[str]:
        out = []
        nodes = []
        for i in ids:
            n = self.doc.nodes[i]
            if live:
                inst = Node(self.doc.new_id(), "instance", self.doc.unique_name(f"{n.name} mirror"), source=i, mirror_plane=plane, material=n.material, parent=n.parent)
                nodes.append(inst)
            else:
                body = self.k.mirror(self.body_of(i), plane)
                nodes.append(Node(self.doc.new_id(), n.kind if n.kind != "instance" else "body", self.doc.unique_name(f"{n.name} mirror"), body=body, material=n.material, parent=n.parent))
        self.stack.push(AddNodes("Mirror", nodes))
        out = [n.id for n in nodes]
        if not keep_original:
            self.stack.push(RemoveNodes("Mirror", list(ids)))
        return out

    def instance(self, source: str, transform: Transform = Transform(), name: Optional[str] = None) -> str:
        n = self.doc.nodes[source]
        inst = Node(self.doc.new_id(), "instance", self.doc.unique_name(name or f"{n.name} instance"), source=source, transform=transform, material=n.material)
        self.stack.push(AddNodes("Instance", [inst]))
        return inst.id

    def make_unique(self, instance_id: str) -> str:
        """Bake an instance into an independent body."""
        n = self.doc.nodes[instance_id]
        body = self.body_of(instance_id)
        new = Node(self.doc.new_id(), "body", self.doc.unique_name(n.name), body=body, material=n.material, parent=n.parent)
        self.stack.push(Composite("Make unique", [AddNodes("Make unique", [new]), RemoveNodes("Make unique", [instance_id])]))
        return new.id

    # ---- arrays ---------------------------------------------------------------
    def array_rect(self, ids: Sequence[str], count: tuple[int, int, int], spacing: Optional[Vec3] = None, extent: Optional[Vec3] = None, as_instances: bool = False, merge: bool = False) -> list[str]:
        """Rectangular array by count + spacing or count + total extent."""
        cx, cy, cz = count
        if spacing is None:
            if extent is None:
                raise KernelError("give a spacing or a total extent")
            spacing = (extent[0] / max(cx - 1, 1), extent[1] / max(cy - 1, 1), extent[2] / max(cz - 1, 1))
        offsets = [(i * spacing[0], j * spacing[1], k * spacing[2]) for i in range(cx) for j in range(cy) for k in range(cz) if (i, j, k) != (0, 0, 0)]
        return self._array(ids, [(o, None, 0.0) for o in offsets], as_instances, merge, "Rectangular array")

    def array_radial(self, ids: Sequence[str], count: int, axis_point: Vec3, axis_dir: Vec3, total_angle: float = 360.0, as_instances: bool = False, merge: bool = False) -> list[str]:
        step = total_angle / count if abs(total_angle - 360.0) < 1e-9 else total_angle / max(count - 1, 1)
        placements = [((0.0, 0.0, 0.0), (axis_point, axis_dir), step * i) for i in range(1, count)]
        return self._array(ids, placements, as_instances, merge, "Radial array")

    def array_curve(self, ids: Sequence[str], path: str | Body, count: int, align: bool = True, as_instances: bool = False, merge: bool = False) -> list[str]:
        p, _ = self._profile(path)
        edges = self.k.edges(p)
        if not edges:
            raise KernelError("the path has no edges")
        # Sample the whole path uniformly by walking its edges.
        pts: list[Vec3] = []
        for e in edges:
            seg = self.k.sample_edge(e, p, 24)
            pts.extend(seg if not pts else seg[1:])
        lens = [v_dist(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
        total = sum(lens) or 1.0
        placements = []
        first = self.k.mass_properties(self.body_of(ids[0])).centroid
        for n in range(1, count):
            target = total * n / (count - 1)
            acc = 0.0
            pos, tangent = pts[-1], v_unit(v_sub(pts[-1], pts[-2]))
            for i, L in enumerate(lens):
                if acc + L >= target:
                    t = (target - acc) / L if L else 0.0
                    pos = v_add(pts[i], v_scale(v_sub(pts[i + 1], pts[i]), t))
                    tangent = v_unit(v_sub(pts[i + 1], pts[i]))
                    break
                acc += L
            offset = v_sub(pos, pts[0])
            if align:
                t0 = v_unit(v_sub(pts[1], pts[0]))
                axis = v_cross(t0, tangent)
                ang = math.degrees(math.acos(max(-1.0, min(1.0, v_dot(t0, tangent)))))
                placements.append((offset, (v_add(first, offset), axis) if ang > 1e-6 else None, ang))
            else:
                placements.append((offset, None, 0.0))
        return self._array(ids, placements, as_instances, merge, "Curve array")

    def _array(self, ids: Sequence[str], placements, as_instances: bool, merge: bool, label: str) -> list[str]:
        nodes: list[Node] = []
        merged: dict[str, Body] = {}
        for i in ids:
            n = self.doc.nodes[i]
            src = self.body_of(i)
            for translation, rotation, angle in placements:
                if as_instances:
                    axis = rotation[1] if rotation else (0.0, 0.0, 1.0)
                    nodes.append(Node(self.doc.new_id(), "instance", self.doc.unique_name(f"{n.name} copy"), source=i, transform=Transform(translation, axis, angle), material=n.material, parent=n.parent))
                else:
                    if rotation:
                        b = self.k.transform(src, translation, rotation[1], angle, rotation[0])
                    else:
                        b = self.k.transform(src, translation)
                    if merge and n.body is not None:
                        merged[i] = self.k.boolean(merged.get(i, n.body), b, BooleanOp.UNION)
                    else:
                        nodes.append(Node(self.doc.new_id(), n.kind if n.kind != "instance" else "body", self.doc.unique_name(f"{n.name} copy"), body=b, material=n.material, parent=n.parent))
        cmds: list[Command] = []
        if merged:
            cmds.append(EditBodies(label, merged))
        if nodes:
            cmds.append(AddNodes(label, nodes))
        self.stack.push(Composite(label, cmds))
        return list(merged) + [n.id for n in nodes]

    # ---- join / unjoin ---------------------------------------------------------
    def join(self, ids: Sequence[str]) -> str:
        bodies = [self.body_of(i) for i in ids]
        joined = self.k.join(bodies)
        first = self.doc.nodes[ids[0]]
        cmds: list[Command] = [EditBodies("Join", {ids[0]: joined}), RemoveNodes("Join", list(ids[1:]))]
        self.stack.push(Composite("Join", cmds))
        return first.id

    def unjoin(self, node_id: str) -> list[str]:
        n = self.doc.nodes[node_id]
        parts = self.k.unjoin(n.body)
        if len(parts) <= 1:
            return [node_id]
        nodes = [Node(self.doc.new_id(), n.kind, self.doc.unique_name(n.name), body=p, material=n.material, parent=n.parent) for p in parts[1:]]
        self.stack.push(Composite("Unjoin", [EditBodies("Unjoin", {node_id: parts[0]}), AddNodes("Unjoin", nodes)]))
        return [node_id] + [x.id for x in nodes]

    def dissolve(self, node_id: str) -> str:
        return self._edit("Dissolve", node_id, lambda b: self.k.dissolve(b))

    def extract_components(self, node_id: str, components: dict[str, list[int]], expected_revision: int) -> dict:
        """Extract named rigid components from an import, keeping the remainder.

        Indices come from GET /nodes/{id}/solids at expected_revision. Each
        list becomes one physical body; its original solids remain intact.
        The entire extraction is undoable as one edit.
        """
        from .candidates import check_revision
        check_revision(self.doc, expected_revision)
        n = self.doc.nodes[node_id]
        if n.kind != 'body' or n.body is None:
            raise KernelError('Select a solid body or compound to extract components')
        if not isinstance(components, dict) or not components or any(not isinstance(name, str) or not name.strip() or not isinstance(indices, list) for name, indices in components.items()):
            raise KernelError('components must map nonempty names to lists of solid indices')
        if any(j.joint and node_id in (j.joint.parent, j.joint.child) for j in self.doc.nodes.values()):
            raise KernelError('Extract components before assigning joints to the source body')
        remainder, bodies = self.k.extract_components(n.body, list(components.values()))
        nodes = []
        used = {node.name for node in self.doc.nodes.values()}
        for name, body in zip(components, bodies):
            base = name.strip(); unique = base; suffix = 2
            while unique in used:
                unique = f'{base} {suffix}'; suffix += 1
            used.add(unique)
            nodes.append(Node(self.doc.new_id(), 'body', unique, parent=n.parent, body=body,
                material=n.material, color=n.color, visible=n.visible, locked=n.locked, disabled=n.disabled,
                tessellation_tolerance=n.tessellation_tolerance))
        self.stack.push(Composite('Extract components', [EditBodies('Extract components', {node_id: remainder}), AddNodes('Extract components', nodes)]))
        return {'remainder': node_id, 'components': {name: node.id for name, node in zip(components, nodes)}, 'revision': self.doc.revision}

    def project_curve(self, sketch_or_curve: str, onto: str, direction: Vec3, name: str = "Projected curve") -> str:
        w, _ = self._profile(sketch_or_curve)
        return self._new("Project", self.k.project_curve(w, self.body_of(onto), direction), name)

    def silhouette(self, node_id: str, plane: Plane, name: str = "Silhouette") -> str:
        return self._new("Silhouette", self.k.silhouette(self.body_of(node_id), plane), name)

    # ---- control points ---------------------------------------------------------
    def set_control_points(self, node_id: str, face: FaceRef, points: list[list[Vec3]]) -> str:
        return self._edit("Move control points", node_id, lambda b: self.k.set_control_points(b, face, points))

    def raise_degree(self, node_id: str, face: FaceRef, du: int, dv: int) -> str:
        return self._edit("Raise degree", node_id, lambda b: self.k.raise_degree(b, face, du, dv))

    def rebuild_face(self, node_id: str, face: FaceRef, su: int, sv: int, degree: int = 3) -> str:
        return self._edit("Rebuild face", node_id, lambda b: self.k.rebuild_face(b, face, su, sv, degree))

    # ---- planes / measurements ---------------------------------------------------
    def plane_from_face(self, node_id: str, face: FaceRef, name: str = "Plane") -> str:
        pl = Plane.from_normal(face.centroid, face.normal)
        return self._add_plane(pl, name)

    def plane_three_points(self, a: Vec3, b: Vec3, c: Vec3, name: str = "Plane") -> str:
        return self._add_plane(Plane.from_three_points(a, b, c), name)

    def plane_two_points_camera(self, a: Vec3, b: Vec3, view_dir: Vec3, name: str = "Plane") -> str:
        x = v_unit(v_sub(b, a))
        n = v_unit(v_cross(x, v_unit(view_dir)))
        n = v_unit(v_cross(n, x))  # normal facing the camera, containing a-b
        return self._add_plane(Plane(a, n, x), name)

    def plane_midplane(self, node_id: str, face_a: FaceRef, face_b: FaceRef, name: str = "Midplane") -> str:
        pa = Plane.from_normal(face_a.centroid, face_a.normal)
        pb = Plane.from_normal(face_b.centroid, face_b.normal)
        return self._add_plane(Plane.midplane(pa, pb), name)

    def _add_plane(self, plane: Plane, name: str) -> str:
        node = Node(self.doc.new_id(), "plane", self.doc.unique_name(name), plane=plane)
        self.stack.push(AddNodes("Plane", [node]))
        return node.id

    def add_measurement(self, m: Measurement, name: str = "Measurement") -> str:
        node = Node(self.doc.new_id(), "measure", self.doc.unique_name(name), measure=m)
        self.stack.push(AddNodes("Measurement", [node]))
        return node.id

    # ---- print helpers ---------------------------------------------------------------
    def clearance(self, node_id: str, faces: Sequence[FaceRef], amount: Optional[float] = None) -> str:
        """Grow holes / shrink bosses (positive) by `amount`, remembering it.
        A hole grows outward, a boss shrinks inward, a planar face offsets inward."""
        amount = self.last_clearance if amount is None else amount
        self.last_clearance = amount

        def fn(b: Body) -> Body:
            out = b
            for f in faces:
                found = self.k.find_face(out, f)
                if found.kind == SurfaceKind.CYLINDER and found.radius is not None:
                    hole = self.k._cylinder_is_hole(out, found) if hasattr(self.k, "_cylinder_is_hole") else True
                    out = self.k.set_cylinder_radius(out, found, found.radius + (amount if hole else -amount))
                else:
                    out = self.k.push_pull(out, found, -amount)
            return out

        return self._edit("Clearance", node_id, fn)

    def fastener_hole(self, node_id: str, face: FaceRef, point: Vec3, spec: "FastenerSpec", depth: Optional[float] = None) -> str:
        from .printing import fastener_tool

        n = v_unit(face.normal)
        tool = fastener_tool(self.k, point, n, spec, depth or 1.0e3)
        node = self.doc.nodes[node_id]
        meta = dict(node.robot or {})
        meta["fasteners"] = list(meta.get("fasteners", [])) + [{"size": spec.size, "kind": spec.kind, "point": list(point), "direction": list(v_scale(n, -1.0)), "depth": depth}]
        self.stack.push(Composite(spec.label, [EditBodies(spec.label, {node_id: self.k.boolean(node.body, tool, BooleanOp.SUBTRACT)}), SetAttributes(spec.label, {node_id: {"robot": meta}})]))
        return node_id


    # ---- robotics ------------------------------------------------------------------
    def add_joint(self, type: str, parent: Optional[str], child: str, pivot: Vec3, axis: Vec3 = (0.0, 0.0, 1.0), lower: Optional[float] = None, upper: Optional[float] = None, motor: Optional[str] = None, gear_ratio: float = 1.0, name: Optional[str] = None) -> str:
        from .robotics import JOINT_TYPES, Joint

        if type not in JOINT_TYPES:
            raise KernelError(f"joint type must be one of {JOINT_TYPES}")
        for bid in (parent, child):
            if bid is not None and (bid not in self.doc.nodes or self.doc.nodes[bid].kind not in ("body", "instance")):
                raise KernelError(f"{bid} is not a body")
        j = Joint(type, parent, child, tuple(pivot), tuple(axis), lower, upper, motor, gear_ratio)
        child_name = self.doc.nodes[child].name
        node = Node(self.doc.new_id(), "joint", self.doc.unique_name(name or f"{type} {child_name}"), joint=j)
        self.stack.push(AddNodes("Joint", [node]))
        return node.id

    def set_joint(self, joint_id: str, **fields) -> str:
        from .robotics import Joint

        n = self.doc.nodes[joint_id]
        if n.kind != "joint" or n.joint is None:
            raise KernelError(f"{n.name} is not a joint")
        d = n.joint.to_json()
        d.update({k: v for k, v in fields.items() if k in d})
        self.stack.push(SetAttributes("Edit joint", {joint_id: {"joint": Joint.from_json(d)}}))
        return joint_id

    def connect_fixed(self, parent: str, child: str, at: Optional[Vec3] = None, name: Optional[str] = None) -> str:
        """Rigidly attach `child` to `parent` (they move as one link)."""
        if at is None:
            at = self.k.mass_properties(self.body_of(child)).centroid
        return self.add_joint("fixed", parent, child, at, (0.0, 0.0, 1.0), name=name)

    def add_motor(self, spec_id: str, mount_point: Vec3, shaft_dir: Vec3, rotation_deg: float = 0.0, mount_on: Optional[str] = None, cut_mount: bool = False, name: Optional[str] = None) -> str:
        from .robotics import MOTOR_LIBRARY, motor_body, motor_mount_holes_tool

        spec = MOTOR_LIBRARY.get(spec_id)
        if spec is None:
            raise KernelError(f"unknown motor {spec_id}; see the library")
        body, meta = motor_body(self.k, spec, tuple(mount_point), tuple(shaft_dir), rotation_deg)
        meta["mounted_on"] = mount_on
        node = Node(self.doc.new_id(), "body", self.doc.unique_name(name or spec.name), body=body, material="steel" if spec.kind != "servo" else "abs", robot=meta, color=spec.color)
        cmds: list[Command] = [AddNodes("Motor", [node])]
        if cut_mount and mount_on and mount_on in self.doc.nodes and self.doc.nodes[mount_on].body is not None:
            tool = motor_mount_holes_tool(self.k, spec, tuple(mount_point), tuple(shaft_dir), rotation_deg=rotation_deg)
            if tool is not None:
                cmds.append(EditBodies("Motor mount holes", {mount_on: self.k.boolean(self.doc.nodes[mount_on].body, tool, BooleanOp.SUBTRACT)}))
        self.stack.push(Composite("Add motor", cmds))
        return node.id

    def mount_motor(self, motor_id: str, body_id: Optional[str]) -> str:
        n = self.doc.nodes[motor_id]
        if not n.robot or n.robot.get("kind") != "motor":
            raise KernelError(f"{n.name} is not a motor")
        meta = dict(n.robot)
        meta["mounted_on"] = body_id
        self.stack.push(SetAttributes("Mount motor", {motor_id: {"robot": meta}}))
        return motor_id

    def attach_motor(self, joint_id: str, motor_id: Optional[str], gear_ratio: float = 1.0) -> str:
        """Make a motor drive a joint (its housing is mounted on the joint's parent)."""
        j = self.doc.nodes[joint_id]
        if j.kind != "joint":
            raise KernelError("not a joint")
        cmds: list[Command] = []
        if motor_id:
            m = self.doc.nodes[motor_id]
            if not m.robot or m.robot.get("kind") != "motor":
                raise KernelError(f"{m.name} is not a motor")
            meta = dict(m.robot)
            meta["drives"] = joint_id
            if meta.get("mounted_on") is None:
                meta["mounted_on"] = j.joint.parent
            cmds.append(SetAttributes("Attach motor", {motor_id: {"robot": meta}}))
        from .robotics import Joint

        d = j.joint.to_json()
        d["motor"] = motor_id
        d["gear_ratio"] = gear_ratio
        cmds.append(SetAttributes("Attach motor", {joint_id: {"joint": Joint.from_json(d)}}))
        self.stack.push(Composite("Attach motor", cmds))
        return joint_id

    def set_ground(self, body_id: str, ground: bool = True) -> str:
        n = self.doc.nodes[body_id]
        meta = dict(n.robot or {})
        meta["ground"] = ground
        self.stack.push(SetAttributes("Ground", {body_id: {"robot": meta}}))
        return body_id

    def infer_joints(self) -> list[str]:
        from .robotics import infer_joints

        existing = {(n.joint.parent, n.joint.child) for n in self.doc.walk() if n.kind == "joint" and n.joint}
        nodes = []
        for j in infer_joints(self.doc):
            if (j.parent, j.child) in existing:
                continue
            nodes.append(Node(self.doc.new_id(), "joint", self.doc.unique_name(f"revolute {self.doc.nodes[j.child].name}"), joint=j))
        if nodes:
            self.stack.push(AddNodes("Infer joints", nodes))
        return [n.id for n in nodes]

    def robot(self, exact: bool = False) -> dict:
        from .robotics import robot_summary

        return robot_summary(self.doc, exact=exact)

    def motor_library(self) -> dict:
        from .robotics import MOTOR_LIBRARY

        return {k: v.to_json() for k, v in MOTOR_LIBRARY.items()}

    # ---- physical model: sensors, cables, battery, control, results -------------------------
    def add_sensor(self, kind: str, body: str, point: Vec3, axes: Optional[list] = None, name: Optional[str] = None, joint: Optional[str] = None, **opts) -> str:
        """An IMU, encoder, current or force sensor on a body at `point`
        (world mm). `axes` rows are the sensor's x, y, z in world; `joint`
        names the joint an encoder/current sensor reads; `opts` override
        rate_hz, noise, bias, quantization, range."""
        if kind not in ("imu", "encoder", "current", "force"):
            raise KernelError("sensor kind must be imu, encoder, current or force")
        if body not in self.doc.nodes or self.doc.nodes[body].kind not in ("body", "instance"):
            raise KernelError(f"{body} is not a body")
        meta = {"kind": kind, "body": body, "point": list(point), "axes": axes, "joint": joint, "joint_name": self.doc.nodes[joint].name if joint in self.doc.nodes else None}
        meta.update({k: v for k, v in opts.items() if k in ("rate_hz", "noise", "bias", "bias_walk", "quantization", "range")})
        node = Node(self.doc.new_id(), "sensor", self.doc.unique_name(name or f"{kind} on {self.doc.nodes[body].name}"), robot=meta)
        self.stack.push(AddNodes("Sensor", [node]))
        return node.id

    def add_cable(self, from_body: str, from_point: Vec3, to_body: str, to_point: Vec3, length: Optional[float] = None, mass: Optional[float] = None, stiffness: Optional[float] = None, name: Optional[str] = None, damping: Optional[float] = None, segments: int = 4) -> str:
        """A cable (wire loom, tube) between two bodies; `length` in mm
        (default 10 % slack), `mass` kg, `stiffness` N (EA), `damping` N·s."""
        for b in (from_body, to_body):
            if b not in self.doc.nodes or self.doc.nodes[b].kind not in ("body", "instance"):
                raise KernelError(f"{b} is not a body")
        meta = {"kind": "cable", "from_body": from_body, "from_point": list(from_point), "to_body": to_body, "to_point": list(to_point), "length": length * 1e-3 if length else None, "mass": mass, "stiffness": stiffness, "damping": damping, "segments": segments}
        node = Node(self.doc.new_id(), "cable", self.doc.unique_name(name or f"cable {self.doc.nodes[from_body].name}-{self.doc.nodes[to_body].name}"), robot=meta)
        self.stack.push(AddNodes("Cable", [node]))
        return node.id

    def set_robot_setting(self, key: str, value) -> dict:
        """Document-level robot settings: battery, control, uncertainty, world, identification."""
        previous = self.doc.robot_settings.get(key)
        doc = self.doc

        class SetSetting(Command):
            label = "Robot setting"

            def do(self, d):
                d.robot_settings[key] = value
                d.touch()

            def undo(self, d):
                if previous is None:
                    d.robot_settings.pop(key, None)
                else:
                    d.robot_settings[key] = previous
                d.touch()

        self.stack.push(SetSetting())
        return doc.robot_settings

    def set_battery(self, cells: int = 2, chemistry: str = "lipo", capacity_ah: float = 1.0, internal_resistance: Optional[float] = None, initial_soc: float = 1.0) -> dict:
        nominal = {"lipo": 3.7, "liion": 3.6, "nimh": 1.2, "alkaline": 1.5, "lifepo4": 3.2}.get(chemistry, 3.7)
        r = internal_resistance if internal_resistance is not None else {"lipo": 0.02, "liion": 0.05, "nimh": 0.03, "alkaline": 0.15, "lifepo4": 0.02}.get(chemistry, 0.03) * cells
        cutoff = {"lipo": 3.0, "liion": 3.0, "nimh": 0.9, "alkaline": 0.9, "lifepo4": 2.5}.get(chemistry, 3.0) * cells
        return self.set_robot_setting("battery", {"cells": cells, "chemistry": chemistry, "nominal_voltage": nominal * cells, "internal_resistance": r, "capacity_ah": capacity_ah, "initial_soc": initial_soc, "cutoff_voltage": cutoff})

    def set_control(self, period_s: float = 0.02, latency_s: float = 0.004, targets: Optional[dict] = None, mode: str = "hold", trajectory: Optional[list] = None) -> dict:
        """Control loop period and latency and the joint targets (rad, by joint name)."""
        return self.set_robot_setting("control", {"period_s": period_s, "latency_s": latency_s, "targets": targets or {}, "mode": mode, "trajectory": trajectory or []})

    def set_uncertainty(self, **sigmas) -> dict:
        """Uncertainty for Monte Carlo: dimension_m, mass, friction, stiffness, backlash, motor_torque, com_m, seed."""
        from .physical import default_settings

        u = dict(self.doc.robot_settings.get("uncertainty") or default_settings()["uncertainty"])
        for k, v in sigmas.items():
            u[k] = v if isinstance(v, dict) or k == "seed" else ({"sigma": v} if k.endswith("_m") else {"sigma_fraction": v})
        return self.set_robot_setting("uncertainty", u)

    def set_material_props(self, material_id: str, **props) -> dict:
        """Engineering properties of a material (Pa, W/m·K, J/kg·K, 1/K, °C, friction table, print anisotropy)."""
        from .document import ENGINEERING_KEYS

        m = self.doc.materials[material_id]
        eng = dict(m.engineering)
        for k, v in props.items():
            if k not in ENGINEERING_KEYS:
                raise KernelError(f"unknown material property {k}; one of {ENGINEERING_KEYS}")
            eng[k] = v
        self.stack.push(SetMaterialDef("Material properties", Material(m.id, m.name, m.density, m.color, m.roughness, m.metallic, list(m.tags), eng)))
        return self.doc.materials[material_id].props()

    def set_joint_physics(self, joint_id: str, **overrides) -> dict:
        """Override inferred joint physics (SI): clearance, backlash, friction,
        stiffness, or flex_patch_radius (m; None restores the inferred patch).
        """
        n = self.doc.nodes[joint_id]
        if n.kind != "joint":
            raise KernelError("not a joint")
        radius = overrides.get('flex_patch_radius')
        if radius is not None and (isinstance(radius, bool) or not isinstance(radius, (int, float))
                                   or not math.isfinite(radius) or radius <= 0):
            raise KernelError('flex_patch_radius must be a finite positive radius in metres, or null for inferred')
        meta = dict(n.robot or {})
        phys = dict(meta.get("physics", {}))
        for k, v in overrides.items():
            if isinstance(v, dict) and isinstance(phys.get(k), dict):
                phys[k] = {**phys[k], **v}
            else:
                phys[k] = v
        meta["physics"] = phys
        self.stack.push(SetAttributes("Joint physics", {joint_id: {"robot": meta}}))
        return phys

    def physical(self, path: Optional[str] = None, flex: bool = True) -> dict:
        """The v3 physical model (written to `path` when given)."""
        from .physical import export_physical_model

        return export_physical_model(self.doc, path, flex=flex)

    def load_results(self, path: str) -> dict:
        from .physical import load_results

        return load_results(self.doc, path)

    def apply_identification(self, path: str) -> dict:
        from .physical import apply_identification

        return apply_identification(self.doc, path)


def _loop_area(c) -> float:
    pts = c.sample(64)
    a = 0.0
    for i in range(len(pts)):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % len(pts)]
        a += x0 * y1 - x1 * y0
    return abs(a) / 2
