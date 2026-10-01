"""A local REST API over the document, so a script, an agent and the
person at the GUI can work on the same model at the same time.

Runs inside the app (requests are marshalled onto the Qt thread, so the
viewport, outliner and undo stack stay consistent) or headless over a
`Document` (`python -m robocad.api model.rcad --port 8420`).

Every response is JSON except `/render`, `/screenshot` and `/capture` (PNG). Ids are
node ids from `/nodes`. Faces/edges are addressed by `{"node": id,
"face": index}` / `{"node": id, "edge": index}` as listed by
`/nodes/{id}/faces` and `/edges`, or as full reference dicts.

    GET  /                          health, version, document path
    GET  /doc                       tree, materials, selection, view, undo history
    GET  /nodes[?kind=body]         node summaries
    POST /nodes                     create: {"kind": "box"|"cylinder"|"sphere"|"sketch"|"plane"|"group"|"instance", ...}
    GET  /nodes/{id}                details incl. mass properties and bounds
    PATCH /nodes/{id}               {"name", "visible", "locked", "disabled", "material", "color", "pivot", "transform", "parent"}
    DELETE /nodes/{id}
    GET  /nodes/{id}/faces|edges|vertices|mesh|validate|section?plane=…|sketch|thin?threshold=1.2
    GET  /nodes/{id}/control_points?face=i   {"node", "face", "rows": [[[x,y,z], …], …]}  a face's B-spline poles (read-only)
    GET  /nodes/{id}/curvature_comb[?scale=5&samples=48]   {"node", "lines": [[[x,y,z],[x,y,z]], …]}  curves and sketches (read-only)
    GET  /nodes/{id}/continuity     {"node", "edges": [{"index", "continuity": "G0"|"G1"|"G2"|"boundary", "points"}], "counts"}  (read-only)
    GET  /nodes/{id}/image          {"id", "revision", "format", "width_px", "height_px", "bytes", "data": base64}  a reference image's stored bytes (read-only)
    POST /nodes/{id}/sketch         {"calls": [["rectangle", [[0,0],[20,10]]], ["circle", [[10,5], 2]]]}  edits a sketch
    POST /ops/{name}                any `Ops` method: {"args": [...], "kwargs": {...}} → its return value
    GET  /ops                       the callable Ops methods and their signatures
    POST /undo | /redo              GET /history
    POST /clipboard/copy            {"ids": [...]} → {"robocad_clipboard": true, "items": [...]}  "Copy with Placement" (read-only)
    POST /clipboard/paste           {"clip": {...}} → {"pasted": [ids], "revision", "history"}  one undo step "Paste"
    GET/PUT /selection              {"items": [[node, kind, index], …]}
    GET/PUT /view                   camera and display state (GUI); POST /view/fit
    GET/POST /views                 named views; POST {"name", "state" (optional in GUI)}
    GET/PATCH/DELETE /views/{id}     inspect, rename, replace or delete a saved view
    POST /views/{id}/restore        restore camera, display and section (GUI)
    GET  /render?view=iso&w=1200&h=900&mode=shaded|xray|wireframe&section=x|y|z:value&ids=a,b&highlight=a&labels=1&edges=1&focus=id
    GET  /screenshot                the live viewport (GUI only)
    POST /capture                  {"view": {camera/grid/section}, "focus_ids": []} → PNG, restores live view
    GET/POST /threads               list/create annotation threads (filters: node_id, status)
    GET/PATCH/DELETE /threads/{id}   read, resolve/reopen, reattach, delete
    POST /threads/{id}/comments     reply: {"body", "author"}
    GET/PATCH/DELETE /comments/{id}  read/edit/delete a message
    POST /save {"path"} | /open {"path"} | /export {"format", "path", "settings"} | /import {"path", "unit"}
    POST /save/thumbnail {"path"?}  /save with the thumbnail the desktop's Save writes → {"saved", "thumbnail": bool}
    POST /new {"path"}              write an empty .rcad at path (refused if it exists); the open document is untouched
    GET  /import/units?path=…       a mesh file's unit-prompt guess → {"path", "extent", "guess", "units"} (read-only)
    GET/DELETE /loads/{load_id}    async open status / cancel (desktop)
    GET  /materials | POST /materials {"id","name","density","color"}
    GET  /robot                     joints, motors, DoF, ground, validation issues
    GET  /motors                    the actuator library (POST /ops/add_motor to place one)
    GET  /physical?flex=1[&planar=1]   the physical assembly description (simrobot v4, SI); planar=1 adds the XZ-plane hint
    GET  /results | POST /results/load {"path"}      simulation results (peak stress, margins, temperatures)
    GET  /results/nodes             {"revision", "path", "loaded", "stale", "provenance", "margins": {id: …}, "nodes": {id: {"results", "yield_strength_pa"}}}  (read-only)
    GET  /print/study               {"revision", "study": robot_settings["print_study"] or null, "splits": [split group ids in tree order]}  (read-only)
    POST /identification/apply {"path"}              fitted joint parameters from `sim-cad fit`
    POST /sensors {"kind","body","point",...} | POST /cables {"from_body","from_point","to_body","to_point",...}
    PUT  /battery {"cells","chemistry",...} | PUT /control {"period_s","latency_s","targets"} | PUT /uncertainty {...}
    GET  /commands | POST /commands/{id}   the GUI's command registry
"""

from __future__ import annotations

import base64
import inspect
import io
import json
import os
import queue
import re
import sys
import threading
import time
import traceback
from dataclasses import asdict, is_dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable, Optional
from urllib.parse import parse_qs, urlparse

from . import __version__
from .commands import Ops
from .document import Document, Material, Measurement, Node, Transform
from .kernel import BooleanOp, ChamferSpec, EdgeRef, FaceRef, KernelError, Plane, Sketch, SweepOptions, Vec3
from .kernel.base import v_add, v_scale, v_sub
from .printing import FastenerSpec, wall_thickness

DEFAULT_PORT = 8420


class ApiError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


# ------------------------------------------------------------- serialise


def _json_default(o):
    if is_dataclass(o) and not isinstance(o, type):
        return asdict(o)
    if hasattr(o, "value") and isinstance(o.value, str):
        return o.value
    if isinstance(o, (set, tuple)):
        return list(o)
    if isinstance(o, bytes):
        return base64.b64encode(o).decode()
    return str(o)


def node_summary(doc: Document, n: Node) -> dict:
    return {"id": n.id, "kind": n.kind, "name": n.name, "parent": n.parent, "children": list(n.children), "visible": n.visible, "locked": n.locked, "disabled": n.disabled, "material": n.material, "color": n.color, "pivot": n.pivot, "source": n.source, "transform": n.transform.to_json(), "effective_visible": doc.is_visible(n.id), "component_instance": n.component_instance, "component_member": n.component_member}


def node_detail(doc: Document, n: Node) -> dict:
    d = node_summary(doc, n)
    body = doc.resolved_body(n.id)
    if body is not None:
        p = doc.kernel.mass_properties(body)
        d["body_kind"] = body.kind
        d["mass"] = {"volume_mm3": p.volume, "area_mm2": p.area, "mass_g": p.mass(doc.density_of(n.id)), "centroid": p.centroid, "bbox_min": p.bbox_min, "bbox_max": p.bbox_max, "size": p.size}
        d["face_count"] = len(doc.kernel.faces(body))
        d["edge_count"] = len(doc.kernel.edges(body))
    if n.sketch is not None:
        d["sketch"] = n.sketch.to_json()
    if n.plane is not None:
        d["plane"] = n.plane.to_json()
    if n.measure is not None:
        d["measure"] = n.measure.to_json()
    if n.mirror_plane is not None:
        d["mirror_plane"] = n.mirror_plane.to_json()
    if n.joint is not None:
        d["joint"] = n.joint.to_json()
    if n.robot is not None:
        d["robot"] = n.robot
    if n.component_instance is not None:
        d['component_instance'] = n.component_instance
    if n.component_member is not None:
        d['component_member'] = n.component_member
    if n.mesh is not None:
        d["mesh"] = {"vertices": len(n.mesh.vertices), "triangles": len(n.mesh.triangles)}
    if n.image is not None:
        d["image"] = {k: v for k, v in n.image.items() if k not in ("data", "_tex")}
        d["image"]["plane"] = n.image["plane"].to_json() if isinstance(n.image.get("plane"), Plane) else n.image.get("plane")
    return d


def face_json(f: FaceRef) -> dict:
    d = f.to_json()
    d["index"] = f.index
    return d


def edge_json(e: EdgeRef) -> dict:
    return {"index": e.index, "kind": e.kind.value, "midpoint": e.midpoint, "length": e.length, "start": e.start, "end": e.end, "center": e.center, "radius": e.radius}


# ------------------------------------------------------------- arguments


_MISSING = object()


def _is_number(x) -> bool:
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def _is_point_grid(v) -> bool:
    """A list of rows, each a list of 3-number points (`list[list[Vec3]]`)."""
    return (isinstance(v, list) and bool(v)
            and all(isinstance(row, list) and row and all(isinstance(p, list) and len(p) == 3 and all(_is_number(x) for x in p) for p in row) for row in v))


class ArgConverter:
    """Turns JSON into what `Ops` methods take, by parameter name and shape."""

    def __init__(self, doc: Document):
        self.doc = doc

    def face(self, v) -> FaceRef:
        if isinstance(v, dict) and "node" in v and "face" in v:
            body = self.doc.resolved_body(v["node"])
            if body is None:
                raise ApiError(404, f"node {v['node']} has no geometry")
            faces = self.doc.kernel.faces(body)
            i = int(v["face"])
            if not 0 <= i < len(faces):
                raise ApiError(400, f"face index {i} out of range (0..{len(faces) - 1})")
            return faces[i]
        if isinstance(v, dict) and "kind" in v and "centroid" in v:
            return FaceRef.from_json(v)
        raise ApiError(400, "a face is {node, face} or a face reference")

    def edge(self, v) -> EdgeRef:
        if isinstance(v, dict) and "node" in v and "edge" in v:
            body = self.doc.resolved_body(v["node"])
            if body is None:
                raise ApiError(404, f"node {v['node']} has no geometry")
            edges = self.doc.kernel.edges(body)
            i = int(v["edge"])
            if not 0 <= i < len(edges):
                raise ApiError(400, f"edge index {i} out of range (0..{len(edges) - 1})")
            return edges[i]
        raise ApiError(400, "an edge is {node, edge}")

    def plane(self, v) -> Plane:
        if isinstance(v, str):
            named = {"xy": Plane.xy(), "xz": Plane.xz(), "yz": Plane.yz()}
            if v.lower() in named:
                return named[v.lower()]
            n = self.doc.nodes.get(v)
            if n is not None and n.plane is not None:
                return n.plane
            raise ApiError(400, f"unknown plane {v!r} (xy/xz/yz or a plane node id)")
        if isinstance(v, dict):
            if "normal" in v and "origin" in v:
                return Plane.from_json({"origin": v["origin"], "normal": v["normal"], "x_axis": v.get("x_axis") or Plane.from_normal(tuple(v["origin"]), tuple(v["normal"])).x_axis})
            if "axis" in v and "offset" in v:
                return {"x": Plane.yz, "y": Plane.xz, "z": Plane.xy}[v["axis"]](float(v["offset"]))
        raise ApiError(400, "a plane is 'xy'|'xz'|'yz', a plane node id, {origin, normal[, x_axis]} or {axis, offset}")

    def convert(self, fn: Callable, args: list, kwargs: dict) -> tuple[list, dict]:
        sig = inspect.signature(fn)
        params = list(sig.parameters.values())
        bound_args = []
        for i, a in enumerate(args):
            p = params[i] if i < len(params) else None
            bound_args.append(self._one(p.name if p else "", p.annotation if p else None, a))
        bound_kwargs = {k: self._one(k, sig.parameters[k].annotation if k in sig.parameters else None, v) for k, v in kwargs.items()}
        return bound_args, bound_kwargs

    def _one(self, name: str, annotation, v):
        ann = str(annotation or "")
        if v is None:
            return None
        if "FaceRef" in ann or name in ("face", "face_a", "face_b"):
            if "Sequence" in ann or (isinstance(v, list) and v and isinstance(v[0], dict)):
                return [self.face(x) for x in v]
            return self.face(v)
        if "EdgeRef" in ann or name in ("edge_a", "edge_b", "edges"):
            # `fill(edges: str | Body)`: a node id names the boundary itself.
            if isinstance(v, str) and "EdgeRef" not in ann:
                if v not in self.doc.nodes:
                    raise ApiError(404, f"no node {v}")
                return v
            if isinstance(v, list):
                return [self.edge(x) for x in v]
            return self.edge(v)
        if "Plane" in ann or name in ("plane", "neutral"):
            # `cut(cutter: str | Body | Plane)`: an id of a node that is not a
            # plane node names the cutter itself (a sheet, curve or sketch),
            # as the GUI's "Cut with selection" passes it.
            if isinstance(v, str) and "str" in re.findall(r"\w+", ann) and v in self.doc.nodes and self.doc.nodes[v].plane is None:
                return v
            return self.plane(v)
        if "BooleanOp" in ann or name == "op":
            return BooleanOp(v) if isinstance(v, str) else v
        if "ChamferSpec" in ann or name == "spec" and isinstance(v, dict) and "distance" in v:
            return ChamferSpec(**v)
        if "FastenerSpec" in ann or name == "spec":
            return FastenerSpec(**v) if isinstance(v, dict) else FastenerSpec(str(v))
        if "SweepOptions" in ann or name == "options":
            return SweepOptions(**v) if isinstance(v, dict) else v
        if "Transform" in ann or name == "transform":
            return Transform.from_json(v) if isinstance(v, dict) else v
        if "Measurement" in ann or name == "m":
            return Measurement.from_json(v) if isinstance(v, dict) else v
        if name == "points" and _is_point_grid(v):
            # `set_control_points(points: list[list[Vec3]])`: rows of poles.
            return [[tuple(float(x) for x in p) for p in row] for row in v]
        if "Vec3" in ann and isinstance(v, list):
            return tuple(float(x) for x in v)
        if "Sequence[Vec3]" in ann or name == "points":
            return [tuple(float(x) for x in p) for p in v]
        if isinstance(v, list) and v and isinstance(v[0], list) and len(v[0]) == 3:
            return [tuple(float(x) for x in p) for p in v]
        if isinstance(v, list) and len(v) == 3 and all(isinstance(x, (int, float)) for x in v) and ("Vec3" in ann or name in ("corner", "size", "center", "base", "axis", "translation", "axis_point", "axis_dir", "point", "direction", "pull_dir", "a", "b", "c")):
            return tuple(float(x) for x in v)
        if isinstance(v, list) and name in ("count",):
            return tuple(int(x) for x in v)
        if isinstance(v, list) and name in ("spacing", "extent"):
            return tuple(float(x) for x in v)
        return v


# ------------------------------------------------------------- service


class Service:
    """The operations, independent of transport. `run_on_main` executes a
    callable on the GUI thread when there is one."""

    def __init__(self, doc: Document, ops: Optional[Ops] = None, app=None, run_on_main: Optional[Callable[[Callable], Any]] = None):
        self.doc = doc
        self.ops = ops or Ops(doc)
        self.app = app
        self.run_on_main = run_on_main or self._run_locked
        self.conv = ArgConverter(doc)
        from .print_jobs import PrintJobs
        self.print_jobs = PrintJobs(lambda: self.doc, lambda: self.ops, lambda fn: self.run_on_main(fn), lambda: self._refresh())

    def print_request(self, method, parts, body):
        """/print: registry, split (seconds, synchronous), analyze|plan|assembly|coupons
        (background jobs), jobs (state, cancel)."""
        from .experiments import RevisionConflict
        jobs = self.print_jobs
        try:
            if parts == ['print', 'registry'] and method == 'GET':
                from . import print_registry as pr
                data, sha = pr.load()
                return {'path': pr.default_path(), 'sha256': sha, 'revision': data['revision'],
                        'printers': {k: {'name': v['name'], 'usable_mm': pr.usable_mm(k)} for k, v in data['printers'].items()},
                        'materials': {k: {'name': v['name'], 'cad_material': v['cad_material']} for k, v in data['materials'].items()}}
            if parts == ['print', 'study'] and method == 'GET':
                def study():
                    doc = self.doc
                    with doc._lock:
                        return {'revision': doc.revision, 'study': doc.robot_settings.get('print_study'),
                                'splits': [n.id for n in doc.walk() if (n.robot or {}).get('print_split')]}
                return self.run_on_main(study)
            if parts == ['print', 'split'] and method == 'POST':
                return jobs.split_job(body) if body.get('background') else jobs.split(body)
            if len(parts) == 2 and method == 'POST' and hasattr(jobs, parts[1]) and parts[1] in ('analyze', 'plan', 'assembly', 'coupons', 'strength_split'):
                return getattr(jobs, parts[1])(body)
            if parts == ['print', 'jobs'] and method == 'GET':
                return jobs.list()
            if len(parts) == 3 and parts[1] == 'jobs':
                if method == 'GET':
                    return jobs.wait(parts[2], float(body.get('wait', 0))) if body.get('wait') else jobs.get(parts[2])
                if method == 'DELETE':
                    return jobs.cancel(parts[2])
        except RevisionConflict as error: raise ApiError(409, str(error))
        except KeyError as error: raise ApiError(404, str(error))
        except (KernelError, ValueError, TypeError, FileNotFoundError, RuntimeError) as error: raise ApiError(422, str(error))
        raise ApiError(405, 'Unsupported print operation')

    def _run_locked(self, fn):
        with self.doc._lock:
            return fn()

    def motion_request(self, method, parts, body):
        if parts == ['motion', 'programs']:
            if method == 'GET': return self.doc.robot_settings.get('motion_programs', {})
            if method == 'POST': return self.ops.save_motion(body)
            if method == 'DELETE': self.ops.delete_motion(body['name']); return {'deleted':body['name']}
        if self.app is None: raise ApiError(409, 'Motion playback requires a desktop window')
        panel = self.app.pose_panel
        if parts == ['motion','export']:
            if method == 'GET': return panel.video.state()
            if method == 'DELETE': panel.video.cancel(); return panel.video.state()
            if method == 'POST':
                panel.prepare(body.get('program')) if isinstance(body.get('program'),dict) else None
                if isinstance(body.get('program'),str):
                    program = self.doc.robot_settings.get('motion_programs',{}).get(body['program'])
                    if program is None: raise ApiError(404, 'Motion pattern not found')
                    panel.prepare(program)
                return panel.video.start(body['path'],body.get('fps',24),body.get('width',1280),body.get('height',720))
        if panel.video.running and method != 'GET' and parts != ['motion','stop']: raise ApiError(409, 'Cancel or finish the video export first')
        if parts == ['motion'] and method == 'GET': return panel.state()
        if method != 'POST': raise ApiError(405, 'Use POST for playback controls')
        action = parts[1] if len(parts) == 2 else ''
        if action in ('play','seek'):
            program = body.get('program')
            if isinstance(program,str):
                program = self.doc.robot_settings.get('motion_programs',{}).get(program)
                if program is None: raise ApiError(404, 'Motion pattern not found')
            panel.pause()
            panel.prepare(program)
            panel.seek(float(body.get('time',0.)))
            if action == 'play': panel.toggle_play()
        elif action == 'focus': panel.focus_mechanism()
        elif action == 'pause': panel.pause()
        elif action == 'stop': panel.stop()
        else: raise ApiError(404, 'Unknown motion action')
        return panel.state()

    def annotation_request(self, method, parts, query, body):
        try:
            if len(parts) == 3 and parts[0] == 'threads' and parts[2] == 'show' and method == 'POST':
                tid = parts[1]
                if tid not in self.doc.annotations: raise KeyError(tid)
                if self.app is None: raise ApiError(409, 'Annotation views require a desktop window')
                panel = self.app.comments
                mode = body.get('mode', 'context')
                if mode not in ('context', 'parts', 'highlight', 'back'): raise ApiError(400, 'Use context, parts, highlight or back')
                ids = [body['node_id']] if body.get('node_id') else None
                if ids and ids[0] not in self.doc.nodes: raise ApiError(404, 'Part not found')
                panel.select(tid)
                if panel.current_id() != tid: raise ApiError(409, 'Finish or cancel the current comment draft first')
                if mode == 'context': panel.focus_thread()
                elif mode == 'parts': panel.view_parts(ids)
                elif mode == 'highlight': panel.highlight_parts(ids)
                else: panel.end_inspection()
                return {'thread_id': tid, 'mode': mode, 'view': self.view()}
            if parts == ["threads"]:
                if method == "GET":
                    return self.ops.threads(query.get("node_id"), query.get("status"), query.get('run_id'))
                if method == "POST":
                    tid = self.ops.create_thread(**body)
                    return self.ops.thread(tid)
            if parts[0] == "threads" and len(parts) == 2:
                tid = parts[1]
                if method == "GET": return self.ops.thread(tid)
                if method == "PATCH":
                    self.ops.update_thread(tid, **body)
                    return self.ops.thread(tid)
                if method == "DELETE":
                    self.ops.delete_thread(tid)
                    return {"deleted": tid}
            if parts[0] == "threads" and len(parts) == 3 and parts[2] == "comments" and method == "POST":
                cid = self.ops.add_comment(parts[1], **body)
                return next(c for c in self.doc.annotations[parts[1]]["comments"] if c["id"] == cid)
            if parts[0] == "comments" and len(parts) == 2:
                cid = parts[1]
                if method == "GET":
                    for t in self.doc.annotations.values():
                        for c in t["comments"]:
                            if c["id"] == cid: return dict(c, thread_id=t["id"])
                    raise KeyError(cid)
                if method == "PATCH":
                    tid = self.ops.update_comment(cid, **body)
                    return next(c for c in self.doc.annotations[tid]["comments"] if c["id"] == cid)
                if method == "DELETE":
                    self.ops.delete_comment(cid)
                    return {"deleted": cid}
        except KeyError as e:
            raise ApiError(404, f"annotation or comment not found: {e}")
        except (KernelError, TypeError, ValueError) as e:
            raise ApiError(422, str(e))
        raise ApiError(405, "unsupported annotation operation")

    # -- document ---------------------------------------------------------
    def health(self):
        return {"ok": True, "app": "robocad", "version": __version__, "path": self.doc.path, "dirty": self.doc.dirty, "gui": self.app is not None, "nodes": len(self.doc.nodes), "document_id":self.doc.document_id,"revision":self.doc.revision}

    def autosave(self, start=False):
        if self.app is None:
            raise ApiError(409, 'Background autosave requires a desktop window')
        if start:
            self.app._autosave()
        pending = self.app._autosave_pending
        saved = self.app._autosave_last_revision
        return {'running': pending is not None, 'revision': pending[2] if pending else None,
            'saved_revision': saved[1] if saved and saved[0] == id(self.doc) else None,
            'path': self.doc.autosave_path()}

    def doc_state(self):
        return {"path": self.doc.path, "dirty": self.doc.dirty, "roots": list(self.doc.roots), "active_group": self.doc.active_group, "nodes": [node_summary(self.doc, n) for n in self.doc.walk()], "materials": [m.to_json() for m in self.doc.materials.values()], "selection": self.selection(), "view": self.view(), "history": self.history(),"document_id":self.doc.document_id,"revision":self.doc.revision}

    @property
    def experiments(self):
        from .experiments import Experiments
        if self.app is not None and hasattr(self.app,'experiments'):
            return self.app.experiments
        if not hasattr(self,'_experiments'):
            self._experiments=Experiments(self.doc)
        return self._experiments

    def system_request(self, method, body, parts=None, query=None):
        from copy import deepcopy
        from .candidates import check_revision
        from .experiments import RevisionConflict
        try:
            with self.doc._lock:
                if parts and len(parts) > 1:
                    from .component_graph import edit_graph
                    section = parts[1]
                    if section not in ('components', 'connections'): raise ApiError(404, 'Unknown system collection')
                    if method == 'GET':
                        values = self.doc.component_graph[section]
                        return deepcopy(values[parts[2]] if len(parts) == 3 else values)
                    expected = body.get('expected_revision')
                    if method == 'DELETE':
                        raw = (query or {}).get('expected_revision')
                        try: expected = int(raw)
                        except (ValueError, TypeError): raise ApiError(422, 'DELETE requires expected_revision in the query')
                    check_revision(self.doc, expected)
                    if method == 'POST' and len(parts) == 2:
                        operation = {'action': 'add_component', 'component': body['component']} if section == 'components' else {'action': 'connect', 'ports': body['ports']}
                    elif method == 'PATCH' and len(parts) == 3 and section == 'components':
                        operation = {'action': 'update_component', 'id': parts[2], 'component': body['component']}
                    elif method == 'DELETE' and len(parts) == 3:
                        operation = {'action': 'delete_component' if section == 'components' else 'delete_connection', 'id': parts[2]}
                    else: raise ApiError(405, 'Unsupported system edit')
                    return edit_graph(self.doc, self.ops, operation, expected, self.experiments.catalogue())
                if method == 'PUT':
                    check_revision(self.doc, body.get('expected_revision'))
                    self.ops.set_component_graph(body.get('graph'))
                elif method != 'GET':
                    raise ApiError(405, 'Use GET or PUT for the document system graph')
                return {'revision': self.doc.revision, 'graph': deepcopy(self.doc.component_graph)}
        except RevisionConflict as error: raise ApiError(409, str(error))
        except KeyError as error: raise ApiError(404, str(error))
        except (KernelError, ValueError, TypeError) as error: raise ApiError(422, str(error))

    def experiment_request(self,method,parts,body):
        from .experiments import RevisionConflict
        try:
            if len(parts)==1:
                if method=='GET':return self.experiments.list()
                if method=='POST':return self.experiments.create(body)
            elif len(parts)==2 and method=='GET':
                return self.experiments.catalogue() if parts[1]=='catalogue' else self.experiments.get(parts[1])
            elif len(parts)==3:
                if parts[2]=='cancel' and method=='POST':return self.experiments.cancel(parts[1])
                if parts[2]=='result' and method=='GET':return self.experiments.result(parts[1])
                if parts[2]=='inputs' and method=='GET':return self.experiments.inputs(parts[1])
                if parts[2]=='diagnostics' and method=='GET':return self.experiments.diagnostics(parts[1])
                if parts[2]=='components' and method=='GET':return self.experiments.components(parts[1])
                if parts[2]=='sources' and method=='GET':return self.experiments.source_bundles(parts[1])
                if parts[2]=='partial' and method=='GET':return self.experiments.partial(parts[1])
                if parts[2]=='compare' and method=='POST':return self.experiments.compare(body['baseline_id'],parts[1])
        except RevisionConflict as error:raise ApiError(409,str(error))
        except KeyError as error:raise ApiError(404,str(error))
        except (KernelError,ValueError,TypeError) as error:raise ApiError(422,str(error))
        raise ApiError(405,'Unsupported experiment operation')

    def history(self):
        return {"undo": [c.label for c in self.ops.stack.undo_stack], "redo": [c.label for c in self.ops.stack.redo_stack]}

    @property
    def candidates(self):
        from .candidates import Candidates
        if self.app is not None and hasattr(self.app, 'candidates'): return self.app.candidates
        if not hasattr(self, '_candidates'):
            self._candidates = Candidates(self.doc, self.ops, self.experiments.root/'candidates')
        return self._candidates

    def model_script(self, body):
        """Run a repository model script: snapshot on the UI thread, build on
        this request thread, publish as one undo step (see model_scripts)."""
        from .candidates import PublishState, change_set, check_revision
        from .experiments import RevisionConflict
        from .model_scripts import stage
        from .snapshots import capture

        def snapshot():
            with self.doc._lock:
                if 'expected_revision' in body: check_revision(self.doc, body['expected_revision'])
                return capture(self.doc)

        def publish():
            with self.doc._lock:
                check_revision(self.doc, before.revision)
                self.ops.stack.push(PublishState(self.doc, staged, body.get('label') or f"Run {summary['script']}"))
            self._refresh()
            return self.doc.revision

        try:
            before = self.run_on_main(snapshot)
            staged, summary = stage(before, body.get('path'), body.get('params'), body.get('replace', True))
            changes = change_set(before, capture(staged)) if body.get('changes') else None
            revision = self.run_on_main(publish)
        except RevisionConflict as error: raise ApiError(409, str(error))
        except (KernelError, ValueError, TypeError) as error: raise ApiError(422, str(error))
        return {'revision': revision, **summary, **({'changes': changes} if changes is not None else {})}

    def candidate_request(self, method, parts, body):
        from .experiments import RevisionConflict
        try:
            if parts == ['doc', 'batch'] and method == 'POST': return self.candidates.batch(body)
            if len(parts) == 1:
                if method == 'GET': return self.candidates.list()
                if method == 'POST': return self.candidates.create(body)
            elif len(parts) == 2:
                if method == 'GET': return self.candidates.get(parts[1])
                if method == 'DELETE': return self.candidates.discard(parts[1])
            elif len(parts) == 3 and method == 'POST':
                if parts[2] == 'accept': return self.candidates.accept(parts[1], body.get('expected_revision'))
                if parts[2] == 'experiments':
                    document = self.candidates.document(parts[1])
                    return self.experiments.create({**body, 'candidate_id': parts[1]}, document=document)
        except RevisionConflict as error: raise ApiError(409, str(error))
        except KeyError as error: raise ApiError(404, str(error))
        except (KernelError, ValueError, TypeError) as error: raise ApiError(422, str(error))
        raise ApiError(405, 'Unsupported candidate operation')

    def nodes(self, kind: Optional[str] = None):
        return [node_summary(self.doc, n) for n in self.doc.walk() if kind is None or n.kind == kind]

    def node(self, nid: str) -> Node:
        n = self.doc.nodes.get(nid) or self.doc.find(nid)
        if n is None:
            raise ApiError(404, f"no node {nid}")
        return n

    def create(self, spec: dict) -> dict:
        kind = spec.get("kind")
        name = spec.get("name")
        ops = self.ops
        if kind == "box":
            if "center" in spec:
                nid = ops.box_center(tuple(spec["center"]), tuple(spec["size"]), name or "Box")
            else:
                nid = ops.box(tuple(spec.get("corner", (0, 0, 0))), tuple(spec["size"]), name or "Box")
        elif kind == "cylinder":
            nid = ops.cylinder(tuple(spec.get("base", (0, 0, 0))), tuple(spec.get("axis", (0, 0, 1))), float(spec.get("radius", spec.get("diameter", 10) / 2)), float(spec["height"]), name or "Cylinder")
        elif kind == "sphere":
            nid = ops.sphere(tuple(spec.get("center", (0, 0, 0))), float(spec.get("radius", spec.get("diameter", 10) / 2)), name or "Sphere")
        elif kind == "sketch":
            nid = ops.new_sketch(self.conv.plane(spec.get("plane", "xy")), name or "Sketch")
            if spec.get("calls"):
                try:
                    self.edit_sketch(nid, spec["calls"])
                except (ApiError, KernelError, ValueError, TypeError, IndexError) as error:
                    # A refused call leaves no empty sketch behind: undo the
                    # "Sketch" step and drop it from the redo stack (new_sketch's
                    # push cleared that stack, so the step is all it holds).
                    stack = ops.stack
                    if stack.undo_stack and stack.undo_stack[-1].label == "Sketch":
                        stack.undo()
                        if stack.redo_stack:
                            stack.redo_stack.pop()
                    self._refresh()
                    if isinstance(error, ApiError):
                        raise
                    raise ApiError(422, str(error))
        elif kind == "plane":
            nid = ops._add_plane(self.conv.plane(spec["plane"]), name or "Plane")
        elif kind == "group":
            nid = ops.group(spec.get("children", []), name or "Group") if spec.get("children") else self.doc.add_group(name or "Group").id
        elif kind == "instance":
            nid = ops.instance(spec["source"], Transform.from_json(spec.get("transform", {})), name)
        elif kind == "measure":
            nid = ops.add_measurement(Measurement.from_json(spec["measure"]), name or "Measurement")
        else:
            raise ApiError(400, f"unknown kind {kind!r}: box, cylinder, sphere, sketch, plane, group, instance, measure")
        if spec.get("material"):
            ops.set_material([nid], spec["material"])
        self._refresh()
        return node_detail(self.doc, self.doc.nodes[nid])

    def patch(self, nid: str, attrs: dict) -> dict:
        n = self.node(nid)
        if n.component_member and set(attrs) - {'visible', 'color', 'name'}:
            raise ApiError(409, 'Edit component parameters or detach the occurrence first')
        if n.component_instance and 'transform' in attrs:
            raise ApiError(409, 'Use set_component_overrides with placement for a component occurrence')
        simple = {}
        for k, v in attrs.items():
            if k == "name":
                self.ops.rename(n.id, str(v))
            elif k == "visible":
                self.ops.set_visible([n.id], bool(v))
            elif k == "locked":
                self.ops.set_locked([n.id], bool(v))
            elif k == "disabled":
                self.ops.set_disabled([n.id], bool(v))
            elif k == "material":
                self.ops.set_material([n.id], str(v))
            elif k == "color":
                self.ops.set_color([n.id], tuple(v) if v else None)
            elif k == "pivot":
                self.ops.set_pivot(n.id, tuple(v) if v else None)
            elif k == "transform":
                simple["transform"] = Transform.from_json(v)
            elif k == "parent":
                self.ops.move_node(n.id, v, attrs.get("index"))
            elif k == "tessellation_tolerance":
                simple["tessellation_tolerance"] = float(v)
            elif k == "plane":
                simple["plane"] = self.conv.plane(v)
            elif k == "sketch":
                simple["sketch"] = Sketch.from_json(v)
            elif k != "index":
                raise ApiError(400, f"cannot set {k}")
        if simple:
            from .commands import SetAttributes

            self.ops.stack.push(SetAttributes("Set attributes", {n.id: simple}))
        self._refresh()
        return node_detail(self.doc, n)

    def delete(self, nid: str):
        n = self.node(nid)
        self.ops.delete([n.id])
        self._refresh()
        return {"deleted": n.id}

    # -- geometry queries -------------------------------------------------
    def faces(self, nid: str):
        body = self._body(nid)
        return [face_json(f) for f in self.doc.kernel.faces(body)]

    def edges(self, nid: str, samples=None):
        """Each edge's `edge_json`; with `samples=N` (2..256) also its
        `points`: the polyline the viewport draws and picks
        (`kernel.sample_edges`: N points along curves, 2 for lines)."""
        if samples is None:
            body = self._body(nid)
            return [edge_json(e) for e in self.doc.kernel.edges(body)]
        text = str(samples)
        if isinstance(samples, bool) or not (text.isascii() and text.isdigit() and len(text) <= 3) or not 2 <= int(text) <= 256:
            raise ApiError(400, f"samples must be an integer from 2 to 256, got {samples!r}")
        body = self._body(nid)
        # sample_edges walks the same edge list as kernel.edges (same order,
        # same EdgeRef and index), so each entry is edge_json plus points.
        return [dict(edge_json(e), points=[list(p) for p in pts]) for e, pts in self.doc.kernel.sample_edges(body, count=int(text))]

    def vertices(self, nid: str):
        body = self._body(nid)
        return [{"index": v.index, "point": v.point} for v in self.doc.kernel.vertices(body)]

    def mesh(self, nid: str, tolerance: float = 0.1):
        m = self.doc.mesh_of(nid, tolerance)
        if m is None:
            raise ApiError(404, "no mesh")
        return {"vertices": m.vertices, "triangles": m.triangles, "triangle_face": m.triangle_face, "face_count": m.face_count}

    def validate(self, nid: str):
        body = self._body(nid)
        rep = self.doc.kernel.validate(body)
        return {"valid": rep.valid, "watertight": rep.watertight, "issues": [asdict(i) for i in rep.issues], "summary": rep.summary()}

    def section(self, nid: Optional[str], plane) -> list:
        from .analysis import section_outline

        return section_outline(self.doc, self.conv.plane(plane), [nid] if nid else None)

    def thin(self, nid: str, threshold: float):
        body = self._body(nid)
        return [asdict(r) for r in wall_thickness(self.doc.kernel, body, threshold)]

    def _body(self, nid: str):
        n = self.node(nid)
        body = self.doc.resolved_body(n.id)
        if body is None:
            raise ApiError(404, f"{n.name} has no geometry")
        return body

    # -- clipboard and analysis reads (the GUI's Copy/Paste with Placement,
    # control points, curvature comb and continuity) -------------------------
    def copy(self, ids) -> dict:
        """The GUI's "Copy with Placement" (`Document.copy_nodes`): each
        node's JSON, B-rep (hex) and sketch, in world placement. Changes
        nothing; an unknown id is a 404."""
        if not isinstance(ids, list) or not all(isinstance(i, str) for i in ids):
            raise ApiError(400, "ids must be a list of node ids")
        return self.doc.copy_nodes([self.node(i).id for i in ids])

    def paste(self, clip) -> dict:
        """The GUI's "Paste with Placement": the clip's nodes added as one
        undo step "Paste" (as `MainWindow.paste_with_placement`)."""
        if not isinstance(clip, dict) or not clip.get("robocad_clipboard") or not isinstance(clip.get("items", []), list):
            raise ApiError(400, "Clipboard has no robocad content")
        from .commands import AddNodes

        before = set(self.doc.nodes)
        # Every add/remove bumps `revision`, sets `dirty` and marks loaded
        # results stale (`Document.touch`/`notify`); a failed paste restores
        # all three so it leaves the document exactly as it found it.
        revision, dirty = self.doc.revision, self.doc.dirty
        results = self.doc.results
        stale = results.get("stale", _MISSING) if isinstance(results, dict) else _MISSING
        try:
            nodes = self.doc.paste_nodes(clip, keep_placement=True)
        except Exception as e:
            # Leave no half-pasted nodes outside the undo stack.
            for nid in [i for i in self.doc.nodes if i not in before]:
                self.doc.remove(nid)
            self.doc.revision, self.doc.dirty = revision, dirty
            if isinstance(results, dict) and results is self.doc.results:
                if stale is _MISSING:
                    results.pop("stale", None)
                else:
                    results["stale"] = stale
            if isinstance(e, KernelError):
                # The clip was well formed; the kernel refused it (say, a B-rep it cannot read).
                raise ApiError(422, str(e))
            if isinstance(e, (KeyError, ValueError, TypeError)):
                raise ApiError(400, f"Clipboard has no robocad content: {type(e).__name__}: {e}")
            raise ApiError(400, f"Paste failed: {type(e).__name__}: {e}")
        # Make the paste undoable as one step.
        for n in nodes:
            self.doc.remove(n.id)
        self.ops.stack.push(AddNodes("Paste", nodes))
        self._refresh()
        return {"pasted": [n.id for n in nodes], "revision": self.doc.revision, "history": self.history()}

    def control_points(self, nid: str, face) -> dict:
        """A face's control points (`kernel.control_points`: the B-spline
        poles, rows along u). Read-only."""
        body = self._body(nid)
        faces = self.doc.kernel.faces(body)
        text = str(face) if face is not None else ""
        if not re.fullmatch(r"-?\d{1,9}", text):
            raise ApiError(400, f"face must be a face index, got {face!r}")
        i = int(text)
        if not 0 <= i < len(faces):
            raise ApiError(400, f"face index {i} out of range (0..{len(faces) - 1})")
        try:
            rows = self.doc.kernel.control_points(body, faces[i])
        except KernelError as e:
            raise ApiError(422, str(e))
        return {"node": self.node(nid).id, "face": i, "rows": [[list(p) for p in row] for row in rows]}

    def curvature_comb(self, nid: str, scale=None, samples=None) -> dict:
        """A curve's or sketch's curvature comb (`analysis.curvature_comb`:
        point → point + normal·curvature·scale), as the GUI draws it. A
        sketch node holds no body, so (as in the GUI) it has no comb lines.
        Read-only."""
        import math

        n = self.node(nid)
        if n.kind not in ("curve", "sketch"):
            raise ApiError(400, f"{n.name} is a {n.kind}: the curvature comb is drawn on curves and sketches")
        try:
            s = 5.0 if scale is None else float(scale)
        except ValueError:
            s = math.nan
        if not math.isfinite(s):
            raise ApiError(400, f"scale must be a finite number, got {scale!r}")
        # Bounded so the comb (curvature x scale) stays finite in the JSON.
        if abs(s) > 1e6:
            raise ApiError(400, f"scale must be at most 1e6 in magnitude, got {scale!r}")
        text = "48" if samples is None else str(samples)
        if not (text.isascii() and text.isdigit() and len(text) <= 3) or not 2 <= int(text) <= 512:
            raise ApiError(400, f"samples must be an integer from 2 to 512, got {samples!r}")
        body = self.doc.resolved_body(n.id)
        if body is None:
            return {"node": n.id, "lines": []}
        from .analysis import curvature_comb

        try:
            lines = curvature_comb(self.doc.kernel, body, s, int(text))
        except KernelError as e:
            raise ApiError(422, str(e))
        return {"node": n.id, "lines": [[list(a), list(c)] for a, c in lines]}

    def continuity(self, nid: str) -> dict:
        """Each edge's continuity between its faces (`analysis.continuity_report`)
        with the polyline the GUI colours (`kernel.sample_edge(e, body, 16)`:
        16 points along a curve, a line's two ends), and the counts per grade
        (G0, G1, G2, boundary, as the GUI's status line). Read-only."""
        body = self._body(nid)
        from .analysis import continuity_report

        try:
            rep = continuity_report(self.doc.kernel, body)
            edges = [{"index": e.index, "continuity": g, "points": [list(p) for p in self.doc.kernel.sample_edge(e, body, 16)]} for e, g in rep]
        except KernelError as e:
            raise ApiError(422, str(e))
        counts = {g: sum(1 for _, x in rep if x == g) for g in ("G0", "G1", "G2", "boundary")}
        return {"node": self.node(nid).id, "edges": edges, "counts": counts}

    # -- sketches -----------------------------------------------------------
    def edit_sketch(self, nid: str, calls: list) -> dict:
        n = self.node(nid)
        if n.sketch is None:
            raise ApiError(400, f"{n.name} is not a sketch")

        def fn(sk: Sketch):
            def curve(i: int):
                if not -len(sk.curves) <= i < len(sk.curves):
                    raise ApiError(400, f"curve index {i} out of range ({len(sk.curves)} curves)")
                return sk.curves[i]

            for call in calls:
                method, args = call[0], call[1] if len(call) > 1 else []
                kwargs = call[2] if len(call) > 2 else {}
                if not hasattr(sk, method) or method.startswith("_"):
                    raise ApiError(400, f"no sketch method {method}")
                # Curve indices become curves first, so a list of exactly two
                # indices is not mistaken for a point by the conversion below.
                args, curve_args = list(args), set()
                ints = lambda a: isinstance(a, list) and all(isinstance(i, int) for i in a)
                if method in ("trim", "extend", "split_at", "fillet_corner", "offset", "reverse", "remove", "unjoin", "rebuild", "insert_vertex", "remove_vertex", "arc_tangent") and args and isinstance(args[0], int):
                    args[0] = curve(args[0])
                    curve_args.add(0)
                if method in ("trim", "extend") and len(args) > 1 and ints(args[1]):
                    args[1] = [curve(i) for i in args[1]]
                    curve_args.add(1)
                if method in ("join", "circle_tangent") and args and ints(args[0]):
                    args[0] = [curve(i) for i in args[0]]
                    curve_args.add(0)
                args = [a if i in curve_args else tuple(a) if isinstance(a, list) and len(a) == 2 and all(isinstance(x, (int, float)) for x in a) else ([tuple(p) for p in a] if isinstance(a, list) and a and isinstance(a[0], list) else a) for i, a in enumerate(args)]
                target = sk
                getattr(target, method)(*args, **kwargs)

        try:
            self.ops.edit_sketch(n.id, fn, label="Sketch (API)")
        except (KernelError, ValueError, TypeError) as error:
            raise ApiError(422, str(error))
        self._refresh()
        return node_detail(self.doc, n)

    # -- ops ------------------------------------------------------------------
    def ops_list(self):
        out = {}
        for name, fn in inspect.getmembers(self.ops, predicate=inspect.ismethod):
            if name.startswith("_") or name in ("body_of",):
                continue
            out[name] = str(inspect.signature(fn))
        return out

    def op(self, name: str, args: list, kwargs: dict):
        from .experiments import RevisionConflict
        if name.startswith("_") or not hasattr(self.ops, name):
            raise ApiError(404, f"no op {name}; see GET /ops")
        fn = getattr(self.ops, name)
        a, k = self.conv.convert(fn, args or [], kwargs or {})
        try:
            from .component_jobs import OPERATIONS
            if self.app is not None and name in OPERATIONS:
                return {'job': self.app.components_panel.start(name, a, k)}
            result = fn(*a, **k)
        except RevisionConflict as e:
            raise ApiError(409, str(e))
        except KernelError as e:
            raise ApiError(422, str(e))
        self._refresh()
        return {"result": result, "history": self.history()}

    def component_job_status(self, job_id, cancel=False):
        if self.app is None or job_id not in self.app.components_panel.jobs: raise ApiError(404, 'Component job not found')
        job = self.app.components_panel.jobs[job_id]
        if cancel: job.cancel()
        return job.status()

    def undo(self):
        label = self.ops.undo()
        self._refresh()
        return {"undone": label, "history": self.history()}

    def redo(self):
        label = self.ops.redo()
        self._refresh()
        return {"redone": label, "history": self.history()}

    # -- selection / view ----------------------------------------------------
    def selection(self):
        if self.app is None:
            return {"items": getattr(self, "_headless_selection", [])}
        return {"items": [list(i) for i in self.app.viewport.selection.items], "mode": self.app.viewport.selection_mode}

    def set_selection(self, items: list, mode: Optional[str] = None):
        items = [tuple(i) if isinstance(i, list) else (i, "body", 0) for i in items]
        if self.app is None:
            self._headless_selection = items
            return self.selection()
        vp = self.app.viewport
        vp.selection.items = [(str(a), str(b), int(c)) for a, b, c in items]
        if mode:
            vp.selection_mode = mode
        self.app.selection_changed(None)
        return self.selection()

    def view(self):
        if self.app is None:
            return {}
        vp = self.app.viewport
        cam = vp.camera
        return {"target": cam.target, "distance": cam.distance, "yaw": cam.yaw, "pitch": cam.pitch, "fov": cam.fov, "orthographic": cam.orthographic, "mode": cam.mode, "display_mode": vp.display_mode, "grid": vp.show_grid, "grid_step": vp.grid_step, "selection_mode": vp.selection_mode, "active_plane": vp.active_plane.to_json() if vp.active_plane else None, "section": {"enabled": vp.section_enabled, "plane": vp.section_plane.to_json() if vp.section_plane else None}, "build_plate": vp.build_plate, "high_contrast": self.app.high_contrast, "tool": self.app.tool.name}

    def set_view(self, v: dict):
        if self.app is None:
            raise ApiError(409, "no GUI: /view needs the app")
        vp = self.app.viewport
        cam = vp.camera
        if "preset" in v:
            cam.set_view(v["preset"])
        for k in ("distance", "yaw", "pitch", "fov"):
            if k in v:
                setattr(cam, k, float(v[k]))
        if "target" in v:
            cam.target = tuple(v["target"])
        if "orthographic" in v:
            cam.orthographic = bool(v["orthographic"])
        if "display_mode" in v:
            self.app.set_display_mode(v["display_mode"])
        if "grid" in v:
            vp.show_grid = bool(v["grid"])
        if "grid_step" in v:
            vp.grid_step = float(v["grid_step"])
        if "selection_mode" in v:
            self.app.set_selection_mode(v["selection_mode"])
        if "active_plane" in v:
            vp.active_plane = self.conv.plane(v["active_plane"]) if v["active_plane"] else None
        if "section" in v:
            s = v["section"]
            vp.section_enabled = bool(s.get("enabled", True))
            if s.get("plane"):
                vp.section_plane = self.conv.plane(s["plane"])
        if "build_plate" in v:
            vp.build_plate = tuple(v["build_plate"]) if v["build_plate"] else None
            vp.show_overhangs = vp.build_plate is not None
            vp.dirty_nodes.update(vp.items.keys())
        if v.get("fit"):
            vp.focus_all()
        if v.get("focus"):
            vp.focus_selection()
        vp.update()
        return self.view()

    # -- rendering -------------------------------------------------------------
    def saved_view_request(self, method, parts, body):
        from .saved_views import capture_view, restore_view
        try:
            if len(parts) == 1:
                if method == 'GET': return self.ops.saved_views()
                if method == 'POST':
                    state = body.get('state')
                    if state is None:
                        if self.app is None: raise ApiError(409, 'Headless: provide view state')
                        state = capture_view(self.app.viewport)
                    vid = self.ops.save_view(body.get('name'), state)
                    return self.doc.saved_views[vid]
            elif len(parts) in (2, 3):
                vid = parts[1]
                if vid not in self.doc.saved_views: raise ApiError(404, 'Saved view not found')
                if len(parts) == 3 and parts[2] == 'restore' and method == 'POST':
                    if self.app is None: raise ApiError(409, 'Restore requires a desktop window')
                    self.app.comments.end_inspection()
                    restore_view(self.app, self.doc.saved_views[vid]['state'])
                    self.app.saved_views_panel.select(vid)
                    self.app.status('Restored view: ' + self.doc.saved_views[vid]['name'])
                    return {'restored': vid, 'view': self.view()}
                if len(parts) == 2:
                    if method == 'GET': return self.doc.saved_views[vid]
                    if method == 'PATCH':
                        if not body or set(body) - {'name', 'state'}: raise ApiError(400, 'Patch name or state')
                        self.ops.update_saved_view(vid, **body)
                        return self.doc.saved_views[vid]
                    if method == 'DELETE':
                        self.ops.delete_saved_view(vid)
                        return {'deleted': vid}
            raise ApiError(405, 'Unsupported saved view operation')
        except KernelError as exc:
            raise ApiError(422, str(exc)) from exc

    def render_request(self, q: dict) -> bytes:
        """GET /render without stalling the window: with a GUI, draw through
        the GPU viewport (a temporary camera, like /capture); options only the
        software renderer has (labels, ids, highlight, xray/wireframe) run it
        on a snapshot copy on this request thread."""
        software_only = any(q.get(key) for key in ('ids', 'highlight')) or q.get('labels', '0') not in ('0', 'false') \
            or q.get('mode', 'shaded') not in ('shaded',)
        if self.app is not None and not software_only:
            return self.run_on_main(lambda: self.render_gpu(q))
        if self.app is None:
            return self.run_on_main(lambda: self.render(q))
        import io as _io
        from .snapshots import capture as _capture
        data = self.run_on_main(lambda: _capture(self.doc).data)
        copy_doc = Document.load(_io.BytesIO(data))
        return Service(copy_doc).render(q)

    def render_gpu(self, q: dict) -> bytes:
        import math
        presets = {"iso": (-1.0, -1.4, 0.9), "front": (0.0, -1.0, 0.0), "back": (0.0, 1.0, 0.0), "right": (1.0, 0.0, 0.0), "left": (-1.0, 0.0, 0.0), "top": (0.0, -0.001, 1.0), "bottom": (0.0, -0.001, -1.0), "iso2": (1.0, -1.4, 0.9), "under": (-1.0, -1.4, -0.9)}
        view = q.get("view", "iso")
        d = presets.get(view) or tuple(float(x) for x in view.split(","))
        norm = math.sqrt(sum(x * x for x in d)) or 1.0
        request = {"view": {"yaw": math.degrees(math.atan2(d[1], d[0])),
                            "pitch": max(-89.5, min(89.5, math.degrees(math.asin(d[2] / norm))))}}
        focus = q.get("focus")
        request["focus_ids"] = [focus] if focus else [n.id for n in self.doc.walk() if n.kind in ("body", "instance", "mesh") and self.doc.is_visible(n.id)]
        if q.get("section"):
            axis, _, value = q["section"].partition(":")
            request["view"]["section"] = {"enabled": True, "plane": {"axis": axis, "offset": float(value or 0)}}
        return self.capture(request)

    def render(self, q: dict) -> bytes:
        from .io.snapshot import render

        w = int(q.get("w", 1200))
        h = int(q.get("h", 900))
        view = q.get("view", "iso")
        presets = {"iso": (-1.0, -1.4, 0.9), "front": (0.0, -1.0, 0.0), "back": (0.0, 1.0, 0.0), "right": (1.0, 0.0, 0.0), "left": (-1.0, 0.0, 0.0), "top": (0.0, -0.001, 1.0), "bottom": (0.0, -0.001, -1.0), "iso2": (1.0, -1.4, 0.9), "under": (-1.0, -1.4, -0.9)}
        direction = presets.get(view)
        if direction is None:
            try:
                direction = tuple(float(x) for x in view.split(","))
            except Exception:
                raise ApiError(400, f"view is one of {list(presets)} or 'dx,dy,dz'")
        ids = q.get("ids")
        ids = [self.node(i).id for i in ids.split(",")] if ids else None
        highlight = q.get("highlight")
        highlight = [self.node(i).id for i in highlight.split(",")] if highlight else None
        section = None
        if q.get("section"):
            axis, _, value = q["section"].partition(":")
            section = self.conv.plane({"axis": axis, "offset": float(value or 0)})
        focus = q.get("focus")
        focus_ids = [self.node(focus).id] if focus else None
        import tempfile

        fd, path = tempfile.mkstemp(suffix=".png")
        os.close(fd)
        try:
            render(self.doc, path, (w, h), direction, ids, section, float(q.get("tolerance", 0.15)), highlight, q.get("title", ""), mode=q.get("mode", "shaded"), edges=q.get("edges", "1") not in ("0", "false"), labels=q.get("labels", "0") not in ("0", "false"), focus_ids=focus_ids)
            with open(path, "rb") as f:
                return f.read()
        finally:
            os.unlink(path)

    def capture(self, request: dict) -> bytes:
        """Capture a temporary inspection angle without changing the user's view."""
        if self.app is None:
            raise ApiError(409, "no GUI: use /render")
        import copy
        import math

        view = request.get("view", {})
        allowed = {"target", "distance", "yaw", "pitch", "fov", "orthographic", "preset", "grid", "section"}
        if not isinstance(view, dict) or set(view) - allowed:
            raise ApiError(400, "capture view accepts camera, grid and section settings only")
        try:
            for key in ("distance", "yaw", "pitch", "fov"):
                if key in view and not math.isfinite(float(view[key])):
                    raise ValueError("camera values must be finite")
            if "distance" in view and float(view["distance"]) <= 0:
                raise ValueError("distance must be positive")
            if "fov" in view and not 0 < float(view["fov"]) < 179:
                raise ValueError("fov must be between 0 and 179 degrees")
            if "pitch" in view and not -89.5 <= float(view["pitch"]) <= 89.5:
                raise ValueError("pitch must be between -89.5 and 89.5 degrees")
            if "target" in view and (len(view["target"]) != 3 or not all(math.isfinite(float(x)) for x in view["target"])):
                raise ValueError("target must have three finite coordinates")
            if "section" in view and view["section"].get("plane"):
                plane = self.conv.plane(view["section"]["plane"])
                if not all(math.isfinite(x) for x in (*plane.origin, *plane.normal, *plane.x_axis)) or sum(x*x for x in plane.normal) < 1e-20:
                    raise ValueError("section plane must be finite with a nonzero normal")
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            raise ApiError(400, str(exc)) from exc
        ids = request.get("focus_ids", [])
        if not isinstance(ids, list):
            raise ApiError(400, "focus_ids must be a list")
        ids = [self.node(nid).id for nid in ids]
        vp = self.app.viewport
        camera = vp.camera
        saved = (vp.section_enabled, vp.section_plane, vp.show_grid)
        vp.camera = copy.deepcopy(camera)
        try:
            if ids:
                vp.focus_nodes(ids)
            if any(key in view for key in ("yaw", "pitch", "preset")):
                vp.camera.mode = "turntable"
            self.set_view(view)
            return self.screenshot()
        finally:
            vp.camera = camera
            vp.section_enabled, vp.section_plane, vp.show_grid = saved
            vp.update()

    def screenshot(self) -> bytes:
        if self.app is None:
            raise ApiError(409, "no GUI: use /render")
        img = self.app.viewport.grab().toImage()
        from PySide6.QtCore import QBuffer

        buf = QBuffer()
        buf.open(QBuffer.WriteOnly)
        img.save(buf, "PNG")
        return bytes(buf.data())

    # -- files ---------------------------------------------------------------------
    def save(self, path: Optional[str]):
        p = path or self.doc.path
        if not p:
            raise ApiError(400, "no path")
        self.doc.save(p)
        if self.app:
            self.app.setWindowTitle(self.app._title())
        return {"saved": p}

    # File-workflow gaps (the native viewer's cad-views-export): each calls
    # existing functions only; the .rcad format and the command layer are
    # unchanged.
    def save_with_thumbnail(self, path: Optional[str]):
        """/save with the thumbnail the desktop's Save writes
        (`MainWindow.thumbnail`: the viewport scaled into 256x192). Headless
        it is drawn by the snapshot renderer (`render`) at 256x192 from each
        node's own tessellation (tolerance 0: `mesh_of` reads it as the
        node's `tessellation_tolerance`, the meshes the viewport and the
        native viewer's `/mesh` fetches already cached), so a save adds no
        new tessellation; its cost is the 256x192 painter's pass and the
        edge sampling. A failed thumbnail saves without one, as the
        desktop's does."""
        p = path or self.doc.path
        if not p:
            raise ApiError(400, "no path")
        if self.app is not None:
            thumbnail = self.app.thumbnail()
        else:
            try:
                thumbnail = self.render({"w": "256", "h": "192", "tolerance": "0"})
            except Exception:
                thumbnail = b""
        self.doc.save(p, thumbnail=thumbnail or None)
        if self.app:
            self.app.setWindowTitle(self.app._title())
        return {"saved": p, "thumbnail": bool(thumbnail)}

    def new_file(self, path):
        """Write an empty document (`Document().save`) to a new `.rcad`. The
        desktop's New opens an empty window; a headless service serves one
        document, so a client creates the file here and opens it. The open
        document is not touched; an existing file is never replaced: the
        file is created exclusively first (no window between a check and
        the write), then the empty document is saved over it, and removed
        again if that fails."""
        if not isinstance(path, str) or not path.endswith(".rcad"):
            raise ApiError(400, "path must name a .rcad file")
        try:
            with open(path, "xb"):
                pass
        except FileExistsError:
            raise ApiError(409, f"{path} exists: choose a new file name")
        except OSError as e:
            raise ApiError(422, f"could not write {path}: {e}")
        try:
            Document().save(path)
        except BaseException as e:
            try:
                os.unlink(path)
            except OSError:
                pass
            if isinstance(e, OSError):
                raise ApiError(422, f"could not write {path}: {e}")
            raise
        return {"created": path}

    def mesh_units(self, path):
        """The unit prompt's guess for a mesh file, as `MainWindow.import_path`
        asks it (`importers.load_mesh_file` for the largest raw extent, then
        `importers.mesh_units_guess`). Reads the file; changes nothing."""
        from .io import importers

        if not path:
            raise ApiError(400, "path is required")
        ext = os.path.splitext(path)[1].lower()
        if ext not in (".stl", ".obj", ".3mf", ".fbx", ".ply", ".glb", ".gltf"):
            raise ApiError(400, f"{path}: not a mesh file (.stl, .obj, .3mf, .fbx, .ply, .glb, .gltf); only meshes ask for units")
        if not os.path.isfile(path):
            raise ApiError(404, f"no such file {path}")
        try:
            _, extent = importers.load_mesh_file(path, "mm")
        except Exception as e:
            raise ApiError(422, f"Could not read mesh: {e}")
        return {"path": path, "extent": extent, "guess": importers.mesh_units_guess(extent), "units": ["mm", "cm", "m", "in", "ft"]}

    def open(self, path: str):
        if self.app is not None:
            from .ui.app import MainWindow

            dialog = MainWindow.open_path(path)
            return {"opened": path, "window": True, "loading": True, 'load_id': dialog.load_id}
        raise ApiError(409, "headless: start the server on the file instead")

    def load_status(self, load_id, cancel=False):
        if self.app is None:
            raise ApiError(409, 'Model loading requires a desktop window')
        from .ui.model_loading import LOADERS, LOAD_JOBS
        if load_id not in LOAD_JOBS:
            raise ApiError(404, 'Unknown model load')
        if cancel:
            for dialog in list(LOADERS):
                if dialog.load_id == load_id:
                    dialog.cancel()
                    break
        return dict(LOAD_JOBS[load_id])

    def export(self, fmt: str, path: str, settings: Optional[dict], ids: Optional[list]):
        from .io import exporters
        from .io.drawing import STANDARD_VIEWS, View, export_drawing_svg

        s = settings or {}
        try:
            if fmt == "stl":
                w = exporters.export_stl(self.doc, path, ids, exporters.StlSettings(**s))
            elif fmt == "3mf":
                w = exporters.export_3mf(self.doc, path, ids, exporters.ThreeMfSettings(**s))
            elif fmt == "step":
                exporters.export_step(self.doc, path, ids, exporters.StepSettings(**s))
                w = []
            elif fmt == "iges":
                exporters.export_iges(self.doc, path, ids)
                w = []
            elif fmt == "obj":
                w = exporters.export_obj(self.doc, path, ids, exporters.ObjSettings(**s))
            elif fmt == "svg":
                exporters.export_sketch_svg(self.doc, path, s["sketch"])
                w = []
            elif fmt == "drawing":
                views = [STANDARD_VIEWS[v] for v in s.get("views", ["front", "top", "right", "iso"])]
                if s.get("section"):
                    pl = self.conv.plane(s["section"])
                    views.append(View("Section A-A", pl.normal, section=pl))
                export_drawing_svg(self.doc, path, views, ids, title=s.get("title", ""))
                w = []
            else:
                raise ApiError(400, f"unknown format {fmt}")
        except exporters.ExportError as e:
            raise ApiError(422, str(e))
        return {"exported": path, "warnings": w}

    def import_file(self, path: str, unit: str = "mm"):
        from .io import importers

        ext = os.path.splitext(path)[1].lower()
        if ext in (".step", ".stp"):
            ids = importers.import_step(self.doc, path)
        elif ext in (".iges", ".igs"):
            ids = importers.import_iges(self.doc, path)
        elif ext == ".svg":
            ids = [importers.import_svg(self.doc, path)]
        elif ext in (".png", ".jpg", ".jpeg"):
            ids = [importers.import_image(self.doc, path)]
        else:
            ids = [importers.import_mesh(self.doc, path, unit)]
        self._refresh()
        return {"imported": ids}

    def materials(self):
        return [m.to_json() for m in self.doc.materials.values()]

    def reference_image(self, nid: str) -> dict:
        """`GET /nodes/{id}/image`: a reference image node's stored bytes
        (base64) with its format and pixel size, at the document's revision.
        Read-only; the gap route the native viewer's CAD mode textures
        reference planes from (node_detail strips the bytes)."""
        from PIL import Image

        n = self.node(nid)
        if n.image is None:
            raise ApiError(404, f"{n.name} is not a reference image")
        data = n.image.get("data")
        if not data:
            raise ApiError(404, f"{n.name} has no stored image bytes")
        try:
            with Image.open(io.BytesIO(data)) as image:
                fmt, (width, height) = (image.format or "").lower(), image.size
        except (OSError, ValueError) as e:
            raise ApiError(422, f"{n.name}: the stored image cannot be read: {e}")
        return {"id": n.id, "revision": self.doc.revision, "format": fmt, "width_px": width, "height_px": height,
                "bytes": len(data), "data": base64.b64encode(data).decode("ascii")}

    def results_nodes(self) -> dict:
        """`GET /results/nodes`: the loaded results per node id (the margins
        `results_margins` computes, each node's results block and its
        material's yield strength) at the document's revision. Read-only."""
        from .physical import results_margins

        res = self.doc.results or {}
        nodes = {}
        for n in self.doc.walk():
            if n.results:
                material = self.doc.materials.get(n.material or "")
                nodes[n.id] = {"results": n.results, "yield_strength_pa": material.props().get("yield_strength") if material else None}
        return {"revision": self.doc.revision, "path": res.get("path"), "loaded": res.get("loaded"), "stale": res.get("stale"), "provenance": res.get("provenance"),
                # results_margins files any section but links and joints as a
                # motor; a print study's block (section "print") is no robot margin.
                "margins": {i: m for i, m in results_margins(self.doc).items() if self.doc.nodes[i].results.get("section") in ("links", "joints", "motors")},
                "nodes": nodes}

    def add_material(self, spec: dict):
        from .commands import SetMaterialDef

        m = Material.from_json({"id": spec.get("id") or spec["name"].lower().replace(" ", "_"), "name": spec["name"], "density": float(spec["density"]), "color": spec.get("color", (0.7, 0.7, 0.72)), "roughness": spec.get("roughness", 0.5), "metallic": spec.get("metallic", 0.0), "tags": spec.get("tags", [])})
        self.ops.stack.push(SetMaterialDef("Material", m))
        if self.app:
            self.app.materials.refresh()
        return m.to_json()

    def commands(self):
        if self.app is None:
            return {}
        return {cid: {"label": c["label"], "category": c["category"], "keys": c["keys"]} for cid, c in self.app.commands.items()}

    def run_command(self, cid: str):
        if self.app is None:
            raise ApiError(409, "no GUI")
        if cid not in self.app.commands:
            raise ApiError(404, f"no command {cid}")
        self.app.commands[cid]["run"]()
        return {"ran": cid}

    def _refresh(self):
        if self.app is not None:
            self.app.outliner.refresh()
            self.app.properties.refresh()
            self.app.viewport.update()


# ------------------------------------------------------------- HTTP


def make_handler(service: Service):
    class Handler(BaseHTTPRequestHandler):
        server_version = "robocad/" + __version__

        def log_message(self, fmt, *args):  # quiet
            pass

        def _send(self, status: int, payload, content_type="application/json"):
            body = payload if isinstance(payload, bytes) else json.dumps(payload, default=_json_default).encode()
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(body)

        def _body(self) -> dict:
            n = int(self.headers.get("Content-Length") or 0)
            if n == 0:
                return {}
            try:
                return json.loads(self.rfile.read(n))
            except json.JSONDecodeError as e:
                raise ApiError(400, f"bad JSON: {e}")

        def _route(self, method: str):
            url = urlparse(self.path)
            parts = [p for p in url.path.split("/") if p]
            q = {k: v[-1] for k, v in parse_qs(url.query).items()}
            body = self._body() if method in ("POST", "PUT", "PATCH") else {}
            s = service
            run = s.run_on_main

            def png(data: bytes):
                self._send(200, data, "image/png")

            if not parts:
                return self._send(200, run(s.health))
            head = parts[0]
            if head == 'system':
                return self._send(201 if method == 'POST' else 200, run(lambda: s.system_request(method, body, parts, q)))
            if parts == ['doc', 'script'] and method == 'POST':
                return self._send(200, s.model_script(body))
            if head == 'print':
                # Split works on its own snapshot; analysis runs as a job. Neither holds the UI thread.
                payload = s.print_request(method, parts, body)
                return self._send(202 if method == 'POST' and parts[1] != 'split' else 200, payload)
            if head == 'candidates' or parts == ['doc', 'batch']:
                payload = run(lambda: s.candidate_request(method, parts, body))
                return self._send(202 if parts[-1] == 'experiments' and method == 'POST' else 200, payload)
            if head=='experiments':
                payload=run(lambda:s.experiment_request(method,parts,body))
                return self._send(202 if method=='POST' else 200,payload)
            if head in ("threads", "comments"):
                payload = run(lambda: s.annotation_request(method, parts, q, body))
                return self._send(201 if method == "POST" else 200, payload)
            if head == "doc" and method == "GET":
                return self._send(200, run(s.doc_state))
            if head == "history":
                return self._send(200, run(s.history))
            if head == "autosave" and method in ('GET', 'POST'):
                return self._send(202 if method == 'POST' else 200, run(lambda: s.autosave(method == 'POST')))
            if head == "nodes":
                if len(parts) == 1:
                    if method == "GET":
                        return self._send(200, run(lambda: s.nodes(q.get("kind"))))
                    if method == "POST":
                        return self._send(201, run(lambda: s.create(body)))
                nid = parts[1]
                if len(parts) == 2:
                    if method == "GET":
                        return self._send(200, run(lambda: node_detail(s.doc, s.node(nid))))
                    if method == "PATCH":
                        return self._send(200, run(lambda: s.patch(nid, body)))
                    if method == "DELETE":
                        return self._send(200, run(lambda: s.delete(nid)))
                sub = parts[2]
                if sub == "solids" and method == "GET":
                    def inventory():
                        node = s.node(nid)
                        solid_body = s.doc.resolved_body(nid)
                        return {'node_id': node.id, 'revision': s.doc.revision, 'units': 'mm',
                            'solids': s.doc.kernel.solid_inventory(solid_body) if solid_body is not None else []}
                    return self._send(200, run(inventory))
                if sub == "faces":
                    return self._send(200, run(lambda: s.faces(nid)))
                if sub == "edges":
                    return self._send(200, run(lambda: s.edges(nid, q.get("samples"))))
                if sub == "vertices":
                    return self._send(200, run(lambda: s.vertices(nid)))
                if sub == "mesh":
                    return self._send(200, run(lambda: s.mesh(nid, float(q.get("tolerance", 0.1)))))
                if sub == "validate":
                    return self._send(200, run(lambda: s.validate(nid)))
                if sub == "section":
                    return self._send(200, run(lambda: s.section(nid, q.get("plane", "xz"))))
                if sub == "thin":
                    return self._send(200, run(lambda: s.thin(nid, float(q.get("threshold", 1.2)))))
                if sub == "control_points" and method == "GET":
                    return self._send(200, run(lambda: s.control_points(nid, q.get("face"))))
                if sub == "curvature_comb" and method == "GET":
                    return self._send(200, run(lambda: s.curvature_comb(nid, q.get("scale"), q.get("samples"))))
                if sub == "continuity" and method == "GET":
                    return self._send(200, run(lambda: s.continuity(nid)))
                if sub == "image" and method == "GET":
                    return self._send(200, run(lambda: s.reference_image(nid)))
                if sub == "sketch":
                    if method == "POST":
                        return self._send(200, run(lambda: s.edit_sketch(nid, body.get("calls", []))))
                    return self._send(200, run(lambda: node_detail(s.doc, s.node(nid)).get("sketch")))
            if head == "ops":
                if len(parts) == 1:
                    return self._send(200, run(s.ops_list))
                return self._send(200, run(lambda: s.op(parts[1], body.get("args", []), body.get("kwargs", {}))))
            if head == "clipboard" and len(parts) == 2 and method == "POST":
                if parts[1] == "copy":
                    return self._send(200, run(lambda: s.copy(body.get("ids"))))
                if parts[1] == "paste":
                    return self._send(200, run(lambda: s.paste(body.get("clip"))))
            if head == "undo":
                return self._send(200, run(s.undo))
            if head == "redo":
                return self._send(200, run(s.redo))
            if head == "selection":
                if method == "GET":
                    return self._send(200, run(s.selection))
                return self._send(200, run(lambda: s.set_selection(body.get("items", []), body.get("mode"))))
            if head == "view":
                if len(parts) > 1 and parts[1] == "fit":
                    return self._send(200, run(lambda: s.set_view({"fit": True})))
                if method == "GET":
                    return self._send(200, run(s.view))
                return self._send(200, run(lambda: s.set_view(body)))
            if head == 'components' and method == 'GET':
                return self._send(200, run(s.ops.component_catalogue))
            if head == 'component-jobs' and len(parts) == 2 and method in ('GET', 'DELETE'):
                return self._send(200, run(lambda: s.component_job_status(parts[1], method == 'DELETE')))
            if head == 'motion':
                return self._send(200, run(lambda: s.motion_request(method, parts, body)))
            if head == 'views':
                return self._send(201 if method == 'POST' and len(parts) == 1 else 200,
                                  run(lambda: s.saved_view_request(method, parts, body)))
            if head == "render":
                return png(s.render_request(q))
            if head == "screenshot":
                return png(run(s.screenshot))
            if head == "capture" and method == "POST":
                return png(run(lambda: s.capture(body)))
            if parts == ["save", "thumbnail"] and method == "POST":
                return self._send(200, run(lambda: s.save_with_thumbnail(body.get("path"))))
            if head == "new" and len(parts) == 1 and method == "POST":
                # Writes a new file only; the open document is not touched.
                return self._send(201, s.new_file(body.get("path")))
            if parts == ["import", "units"] and method == "GET":
                # Reads the mesh file only: off the GUI thread and the document lock.
                return self._send(200, s.mesh_units(q.get("path")))
            if head == "save":
                return self._send(200, run(lambda: s.save(body.get("path"))))
            if head == "open":
                return self._send(200, run(lambda: s.open(body["path"])))
            if head == 'loads' and len(parts) == 2 and method in ('GET', 'DELETE'):
                return self._send(200, run(lambda: s.load_status(parts[1], method == 'DELETE')))
            if head == "export":
                return self._send(200, run(lambda: s.export(body["format"], body["path"], body.get("settings"), body.get("ids"))))
            if head == "import":
                return self._send(200, run(lambda: s.import_file(body["path"], body.get("unit", "mm"))))
            if head == "robot":
                return self._send(200, run(s.ops.robot))
            if head == "motors":
                return self._send(200, run(s.ops.motor_library))
            if head == "performance":
                def performance():
                    v=s.app.viewport if s.app is not None else None
                    return {"revision":s.doc.revision,"nodes":len(s.doc.nodes),
                            "last_frame_ms":v.frame_ms if v is not None else None,
                            "display_triangles":sum(it.indices.size//3 for it in v.items.values()) if v is not None else None}
                return self._send(200,run(performance))
            if head == "physical":
                flex = q.get("flex", "1") not in ("0", "false")
                planar = q.get("planar", "0") not in ("0", "false")
                if s.app is None:
                    if planar:
                        from .physical import export_physical_model
                        return self._send(200, run(lambda: export_physical_model(s.doc, q.get("path"), planar=Plane.xz(), flex=flex)))
                    return self._send(200, run(lambda: s.ops.physical(q.get("path"), flex=flex)))
                # Desktop: snapshot on the GUI thread, derive in a child process
                # from this request thread, so the editor stays responsive.
                from .snapshots import capture
                from .export_worker import export_snapshot
                snapshot, source = run(lambda: (capture(s.doc), s.doc.path))
                return self._send(200, export_snapshot(snapshot, q.get("path"), flex=flex, planar=planar, source_file=source))
            if head == "actuator-profiles":
                if method == "GET":
                    return self._send(200, run(lambda: s.doc.robot_settings.get("actuator_profiles")))
                if method == "POST":
                    return self._send(200, run(lambda: s.ops.set_actuator_profiles(body["profiles"])))
            if head == "results":
                if len(parts) > 1 and parts[1] == "load":
                    return self._send(200, run(lambda: s.ops.load_results(body["path"])))
                if len(parts) > 1 and parts[1] == "nodes":
                    if method != "GET":
                        raise ApiError(405, f"{method} /results/nodes: read-only (GET)")
                    return self._send(200, run(s.results_nodes))
                return self._send(200, run(lambda: s.doc.results or {}))
            if head == "identification":
                return self._send(200, run(lambda: s.ops.apply_identification(body["path"])))
            if head == "sensors":
                if method == "POST":
                    return self._send(201, run(lambda: node_detail(s.doc, s.node(s.ops.add_sensor(body["kind"], body["body"], tuple(body["point"]), body.get("axes"), body.get("name"), body.get("joint"), **{k: v for k, v in body.items() if k in ("rate_hz", "noise", "bias", "bias_walk", "quantization", "range")})))))
                return self._send(200, run(lambda: [node_detail(s.doc, n) for n in s.doc.walk() if n.kind == "sensor"]))
            if head == "cables":
                if method == "POST":
                    return self._send(201, run(lambda: node_detail(s.doc, s.node(s.ops.add_cable(body["from_body"], tuple(body["from_point"]), body["to_body"], tuple(body["to_point"]), body.get("length"), body.get("mass"), body.get("stiffness"), body.get("name"), body.get("damping"), int(body.get("segments", 4)))))))
                return self._send(200, run(lambda: [node_detail(s.doc, n) for n in s.doc.walk() if n.kind == "cable"]))
            if head == "battery":
                if method == "PUT":
                    return self._send(200, run(lambda: s.ops.set_battery(**body)))
                return self._send(200, run(lambda: s.doc.robot_settings.get("battery")))
            if head == "control":
                if method == "PUT":
                    return self._send(200, run(lambda: s.ops.set_control(**body)))
                return self._send(200, run(lambda: s.doc.robot_settings.get("control")))
            if head == "uncertainty":
                if method == "PUT":
                    return self._send(200, run(lambda: s.ops.set_uncertainty(**body)))
                return self._send(200, run(lambda: s.doc.robot_settings.get("uncertainty")))
            if head == "materials":
                if method == "POST":
                    return self._send(201, run(lambda: s.add_material(body)))
                return self._send(200, run(s.materials))
            if head == "commands":
                if len(parts) > 1:
                    return self._send(200, run(lambda: s.run_command(parts[1])))
                return self._send(200, run(s.commands))
            raise ApiError(404, f"no route {method} {url.path}")

        def _handle(self, method):
            try:
                self._route(method)
            except ApiError as e:
                self._send(e.status, {"error": str(e)})
            except Exception as e:
                self._send(500, {"error": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()})

        def do_GET(self):
            self._handle("GET")

        def do_POST(self):
            self._handle("POST")

        def do_PUT(self):
            self._handle("PUT")

        def do_PATCH(self):
            self._handle("PATCH")

        def do_DELETE(self):
            self._handle("DELETE")

        def do_OPTIONS(self):
            self.send_response(204)
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS")
            self.send_header("Access-Control-Allow-Headers", "Content-Type")
            self.end_headers()

    return Handler


class ApiServer:
    """Serve on a thread. With `app`, every request runs on the Qt thread
    through a queue the app drains from a timer, so the GUI and the API
    never touch the document at the same time."""

    def __init__(self, doc: Document, ops: Optional[Ops] = None, app=None, host: str = "127.0.0.1", port: int = DEFAULT_PORT):
        self.host, self.port = host, port
        self._queue: "queue.Queue[tuple[Callable, dict]]" = queue.Queue()
        self._state_lock = threading.Lock()
        self.service = Service(doc, ops, app, self._run_on_main if app is not None else None)
        self._server: Optional[ThreadingHTTPServer] = None
        self._thread: Optional[threading.Thread] = None
        self._timer = None
        if app is not None:
            from PySide6.QtCore import QTimer

            self._timer = QTimer(app)
            self._timer.timeout.connect(self._drain)
            self._timer.start(15)

    def _run_on_main(self, fn: Callable):
        done = threading.Event()
        slot: dict = {}
        ctx = {"done": done, "slot": slot, "state": "pending"}
        self._queue.put((fn, ctx))
        if not done.wait(120):
            with self._state_lock:
                started = ctx["state"] == "running"
                ctx["state"] = "cancelled" if not started else ctx["state"]
            if not started:
                # Never let a request the client was told failed edit the document later.
                raise ApiError(504, "the GUI did not answer in time; the request was cancelled")
            done.wait()
        if "error" in slot:
            raise slot["error"]
        return slot.get("result")

    def _drain(self):
        # Yield between requests so an API client cannot starve mouse/paint events.
        deadline = time.perf_counter() + 0.008
        while time.perf_counter() < deadline:
            try:
                fn, ctx = self._queue.get_nowait()
            except queue.Empty:
                return
            with self._state_lock:
                if ctx["state"] == "cancelled":
                    continue
                ctx["state"] = "running"
            try:
                ctx["slot"]["result"] = fn()
            except BaseException as e:  # hand the exception back to the request thread
                ctx["slot"]["error"] = e
            finally:
                ctx["done"].set()

    def start(self) -> "ApiServer":
        self._server = ThreadingHTTPServer((self.host, self.port), make_handler(self.service))
        self.port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return self

    @property
    def url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def stop(self):
        if self._server:
            self._server.shutdown()
            self._server = None
        if self._timer:
            self._timer.stop()
        if self.service.app is None and hasattr(self.service, '_experiments'):
            self.service._experiments.close()


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    import argparse

    ap = argparse.ArgumentParser(description="robocad headless REST API")
    ap.add_argument("path", nargs="?", help=".rcad to open")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    ap.add_argument("--host", default="127.0.0.1")
    a = ap.parse_args(argv)
    doc = Document.load(a.path) if a.path else Document()
    server = ApiServer(doc, host=a.host, port=a.port).start()
    print(f"robocad API on {server.url} ({a.path or 'new document'})", flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        server.stop()


if __name__ == "__main__":
    main()
