"""Print studies from CAD: export parts as STL with the regions where they
are held and loaded (``sim.print-study/1``), run the Rust ``sim-print`` tool
off the UI thread (progress, cancel), and read its results back, including
the per-voxel failure field used to colour a preview.

Regions are in the part's CAD frame (mm). They are best built from geometry:
faces of a body, faces touching another body (the load path through a
contact), or plain spheres/boxes/cylinders.
"""

from __future__ import annotations

import json
import os
import struct
import subprocess
import threading
from dataclasses import dataclass, field
from typing import Callable, Optional, Sequence

import numpy as np

from .kernel.base import Mesh, v_cross, v_norm, v_sub
from .print_registry import REPO

Vec3 = tuple[float, float, float]


def binary() -> str:
    path = os.environ.get('SIM_PRINT_BIN') or os.path.join(REPO, 'target', 'release', 'sim-print')
    if not os.path.isfile(path):
        raise FileNotFoundError(f'{path} is missing: build it with `cargo build --release -p sim-runtime --bin sim-print`')
    return path


# ------------------------------------------------------------------ meshes

def body_mesh(doc, node_id: str, tolerance: float = 0.1) -> Mesh:
    from .printing import weld
    node = doc.nodes[node_id]
    return weld(doc.kernel.tessellate(node.body, tolerance))


def body_mesh_of(kernel, body, tolerance: float = 0.1) -> Mesh:
    """A welded mesh of a body that is not (yet) a document node (a split piece)."""
    from .printing import weld
    return weld(kernel.tessellate(body, tolerance))


def write_stl(mesh: Mesh, path: str) -> None:
    """Binary STL in millimetres."""
    with open(path, 'wb') as f:
        f.write(b'robocad print study'.ljust(80, b' '))
        f.write(struct.pack('<I', len(mesh.triangles)))
        for a, b, c in mesh.triangles:
            pa, pb, pc = mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]
            n = v_cross(v_sub(pb, pa), v_sub(pc, pa))
            ln = v_norm(n) or 1.0
            f.write(struct.pack('<12fH', n[0] / ln, n[1] / ln, n[2] / ln, *pa, *pb, *pc, 0))


# ----------------------------------------------------------------- regions

def sphere(center: Vec3, radius: float) -> dict:
    return {'sphere': {'center': list(center), 'radius': radius}}


def box(lo: Vec3, hi: Vec3) -> dict:
    return {'box': {'min': list(lo), 'max': list(hi)}}


def cylinder(base: Vec3, axis: Vec3, radius: float, length: float) -> dict:
    return {'cylinder': {'base': list(base), 'axis': list(axis), 'radius': radius, 'length': length}}


def below(axis: Vec3, height: float) -> dict:
    return {'below': {'axis': list(axis), 'height': height}}


def _sample_points(mesh: Mesh, faces: Optional[set] = None, spacing: float = 1.0) -> list[Vec3]:
    """Triangle centroids plus vertices of the selected faces, thinned to about `spacing` mm."""
    pts = []
    for ti, (a, b, c) in enumerate(mesh.triangles):
        if faces is not None and mesh.triangle_face[ti] not in faces:
            continue
        pa, pb, pc = mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]
        pts.extend([pa, pb, pc, tuple((pa[k] + pb[k] + pc[k]) / 3 for k in range(3))])
    if not pts:
        return []
    arr = np.asarray(pts)
    key = np.floor(arr / spacing).astype(np.int64)
    _, keep = np.unique(key, axis=0, return_index=True)
    return [tuple(map(float, arr[i])) for i in sorted(keep)]


def faces_region(doc, node_id: str, face_indices: Sequence[int], radius: float = 1.0) -> dict:
    """The surface of these faces of a body (indices as in ``kernel.faces``)."""
    node = doc.nodes[node_id]
    mesh = doc.kernel.tessellate(node.body, 0.1)
    pts = _sample_points(mesh, set(face_indices), spacing=max(radius * 0.8, 0.3))
    if not pts:
        raise ValueError(f'{node.name}: faces {list(face_indices)} have no tessellation')
    return {'points': {'points': [list(p) for p in pts], 'radius': radius}}


def contact_region(doc, node_id: str, other_id: str, gap: float = 0.3, radius: float = 1.0) -> dict:
    """The surface of one body that touches (within `gap` mm) another: where a
    load passes between them, e.g. a servo case bearing on its cradle."""
    k = doc.kernel
    node, other = doc.nodes[node_id], doc.nodes[other_id]
    mesh = k.tessellate(node.body, 0.1)
    lo, hi = doc.mesh_of(other_id).bounds()
    pts = []
    for p in _sample_points(mesh, None, spacing=max(radius * 0.8, 0.3)):
        if all(lo[i] - gap <= p[i] <= hi[i] + gap for i in range(3)) and k.contains(other.body, p, gap):
            pts.append(list(p))
    if not pts:
        raise ValueError(f'{node.name} does not touch {other.name} within {gap} mm')
    return {'points': {'points': pts, 'radius': radius}}


def bottom_region(doc, node_id: str, up: Vec3 = (0, 0, 1), depth: float = 0.3) -> dict:
    """Everything within `depth` mm of the body's lowest point along `up` (it sits there)."""
    mesh = doc.mesh_of(node_id)
    u = np.asarray(up, float) / np.linalg.norm(up)
    h = float(np.min(np.asarray(mesh.vertices) @ u))
    return below(tuple(u), h + depth)


# ------------------------------------------------------------------- study

@dataclass
class PartSpec:
    node_id: str
    name: Optional[str] = None
    build_direction: Vec3 = (0.0, 0.0, 1.0)
    settings: dict = field(default_factory=lambda: {'walls': 3, 'infill': 0.15, 'pattern': 'gyroid', 'layer_height': 0.2, 'top_bottom_layers': 5})
    fixtures: list = field(default_factory=list)   # [{'name', 'region'}]
    loads: list = field(default_factory=list)      # [{'name', 'region', 'direction', 'magnitude'}]
    acceleration: Optional[Vec3] = (0.0, 0.0, -9.81)
    sections: list = field(default_factory=list)
    seams: list = field(default_factory=list)
    directions: Optional[list] = None
    mesh: Optional[Mesh] = None                    # instead of the body's own (e.g. a piece)


def slug(name: str) -> str:
    s = ''.join(c.lower() if c.isalnum() else '-' for c in name)
    return '-'.join(x for x in s.split('-') if x)


def write_study(doc, parts: Sequence[PartSpec], out_dir: str, printer: str = 'bambu-h2c', material: str = 'pla-basic',
                simulation: Optional[dict] = None, safety_target: float = 2.0, voxels: int = 40000,
                voxel_mm: Optional[float] = None, plan: Optional[dict] = None, provenance: Optional[dict] = None) -> str:
    """Write STLs and ``study.json`` into `out_dir`; paths in it are relative to it."""
    os.makedirs(out_dir, exist_ok=True)
    entries = []
    for p in parts:
        name = p.name or doc.nodes[p.node_id].name
        stl = f'{slug(name)}.stl'
        write_stl(p.mesh or body_mesh(doc, p.node_id), os.path.join(out_dir, stl))
        e = {'name': name, 'mesh': stl, 'build_direction': list(p.build_direction), 'settings': p.settings,
             'fixtures': p.fixtures, 'loads': p.loads, 'sections': p.sections, 'seams': p.seams}
        if p.acceleration is not None:
            e['acceleration'] = list(p.acceleration)
        if p.directions:
            e['directions'] = [list(d) for d in p.directions]
        entries.append(e)
    study = {'schema': 'sim.print-study/1', 'printer': printer, 'material': material, 'voxels': voxels,
             'safety_target': safety_target, 'parts': entries, 'provenance': provenance or {}}
    if voxel_mm:
        study['voxel_mm'] = voxel_mm
    if simulation:
        sim = dict(simulation)
        sim['system'] = os.path.relpath(os.path.abspath(sim['system']), out_dir)
        study['simulation'] = sim
    if plan:
        study['plan'] = plan
    path = os.path.join(out_dir, 'study.json')
    with open(path, 'w') as f:
        json.dump(study, f, indent=1)
        f.write('\n')
    return path


# ------------------------------------------------------------------- runs

class Run:
    """``sim-print COMMAND STUDY`` in a subprocess: progress callbacks,
    cancellation, and the parsed result when it finishes."""

    def __init__(self, command: str, study: str, out: Optional[str] = None, extra: Sequence[str] = (),
                 on_progress: Optional[Callable[[float, str], None]] = None):
        self.out = out or os.path.join(os.path.dirname(study), 'print-results' if command == 'analyze' else 'print-plan')
        self.args = [binary(), command, study, '--out', self.out, *extra]
        self.on_progress = on_progress
        self.fraction, self.message = 0.0, 'starting'
        self.stdout, self.errors = '', []
        self.process: Optional[subprocess.Popen] = None
        self.cancelled = False
        self.done = threading.Event()
        self.returncode: Optional[int] = None

    def start(self) -> 'Run':
        self.process = subprocess.Popen(self.args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=REPO)
        threading.Thread(target=self._read_err, daemon=True).start()
        threading.Thread(target=self._wait, daemon=True).start()
        return self

    def _read_err(self):
        for line in self.process.stderr:
            line = line.rstrip('\n')
            if line.startswith('progress '):
                try:
                    _, frac, msg = line.split(' ', 2)
                    self.fraction, self.message = float(frac), msg
                except ValueError:
                    continue
                if self.on_progress:
                    self.on_progress(self.fraction, self.message)
            elif line:
                self.errors.append(line)

    def _wait(self):
        self.stdout = self.process.stdout.read()
        self.returncode = self.process.wait()
        self.done.set()

    def cancel(self):
        self.cancelled = True
        if self.process and self.process.poll() is None:
            self.process.kill()

    def wait(self, timeout: Optional[float] = None) -> dict:
        self.done.wait(timeout)
        if not self.done.is_set():
            raise TimeoutError('sim-print is still running')
        if self.cancelled:
            raise RuntimeError('cancelled')
        if self.returncode:
            raise RuntimeError('\n'.join(e for e in self.errors if not e.startswith('progress')) or f'sim-print exited with {self.returncode}')
        name = 'result.json' if self.args[1] == 'analyze' else 'plan.json'
        with open(os.path.join(self.out, name)) as f:
            return json.load(f)


def analyze(study: str, out: Optional[str] = None, on_progress=None, timeout: Optional[float] = None) -> dict:
    return Run('analyze', study, out, on_progress=on_progress).start().wait(timeout)


# ------------------------------------------------------------------ fields

@dataclass
class Field:
    origin: np.ndarray
    h: float
    n: tuple[int, int, int]
    to_part: np.ndarray            # print frame → part frame (3×3)
    failure_index: np.ndarray      # (nz, ny, nx), NaN outside
    mode: np.ndarray

    def sample(self, points_part_mm: np.ndarray) -> np.ndarray:
        """Failure index at part-frame points (nearest solid voxel within one voxel; NaN beyond)."""
        p = np.asarray(points_part_mm, float) @ self.to_part   # part → print: Rᵀᵀ = R, i.e. p·to_part
        idx = np.floor((p - self.origin) / self.h).astype(int)
        nx, ny, nz = self.n
        out = np.full(len(p), np.nan)
        best = np.full(len(p), np.inf)
        for dk in (-1, 0, 1):
            for dj in (-1, 0, 1):
                for di in (-1, 0, 1):
                    i, j, k = idx[:, 0] + di, idx[:, 1] + dj, idx[:, 2] + dk
                    ok = (i >= 0) & (j >= 0) & (k >= 0) & (i < nx) & (j < ny) & (k < nz)
                    v = np.full(len(p), np.nan)
                    v[ok] = self.failure_index[k[ok], j[ok], i[ok]]
                    centre = self.origin + (np.stack([i, j, k], 1) + 0.5) * self.h
                    d = np.linalg.norm(centre - p, axis=1)
                    take = ok & ~np.isnan(v) & (d < best)
                    out[take], best[take] = v[take], d[take]
        return out


def load_field(result_dir: str, part: dict) -> Field:
    with open(os.path.join(result_dir, part['field']['header'])) as f:
        head = json.load(f)
    nx, ny, nz = head['n']
    raw = open(os.path.join(result_dir, part['field']['data']), 'rb').read()
    count = nx * ny * nz
    fi = np.frombuffer(raw[:4 * count], dtype='<f4').reshape(nz, ny, nx)
    mode = np.frombuffer(raw[4 * count:4 * count + count], dtype=np.uint8).reshape(nz, ny, nx)
    return Field(np.asarray(head['origin_mm']), float(head['voxel_mm']), (nx, ny, nz), np.asarray(head['to_part']), fi, mode)


_fields: dict = {}


def cached_field(result_dir: str, part: dict) -> Field:
    key = (result_dir, part['field']['data'])
    if key not in _fields:
        _fields[key] = load_field(result_dir, part)
    return _fields[key]


def failure_colors(values: np.ndarray) -> np.ndarray:
    """Colour per failure index: green (≤0.25) → yellow (0.5) → red (≥1); grey where unknown."""
    v = np.nan_to_num(np.asarray(values, float), nan=-1.0)
    t = np.clip((v - 0.25) / 0.75, 0, 1)
    rgb = np.stack([np.clip(2 * t, 0, 1), np.clip(2 - 2 * t, 0, 1) * 0.8 + 0.1, np.full_like(t, 0.15)], 1)
    rgb[v < 0] = (0.55, 0.55, 0.58)
    return rgb


def seams_from_split(result) -> list[dict]:
    """A split's seams and joints in the study's form, to check on the unsplit part."""
    return [s.study() for s in result.seams]
