"""Split a CAD body into printable pieces and join them again.

``plan_split`` finds cut planes so every piece fits the printer (from the
print registry), preferring cuts through a single, solid cross-section away
from holes, and optionally away from the load. ``build_split`` cuts the body
and adds joints at each seam, built with booleans on the pieces:

* dowel pins: steel pins across the seam; a press fit in the minus piece, a
  slip fit in the plus piece (clearances from the registry);
* heat-set inserts and screws: an insert pocket in the minus piece, a
  clearance hole with a counterbore from the plus piece's far side;
* a dovetail rail: a tail on the minus piece that slides into a groove in
  the plus piece (angle and clearance from the registry).

Joint capacities are not guessed here: each seam is written into a print
study (``seams``) and checked by ``sim-print analyze`` with the same
formulas as the library parts, against the loads the part really carries.

Nothing here edits the document: ``apply_split`` adds the pieces (and the
hardware as reference bodies) as one undoable step and leaves the source.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional, Sequence

import numpy as np

from . import print_registry as reg
from .kernel.base import Body, BooleanOp, KernelError, Plane, v_add, v_cross, v_dot, v_norm, v_scale, v_sub, v_unit
from .kernel.sketch import Sketch

Vec3 = tuple[float, float, float]
AXES: tuple[Vec3, Vec3, Vec3] = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


@dataclass
class SplitOptions:
    printer: str = 'bambu-h2c'
    # 'auto' (pins + screws where a screw can reach, else a dovetail), 'pins+screws', 'dovetail', 'pins'
    joint: str = 'auto'
    screw: str = 'M3'
    pin_diameter: float = 4.0
    # Material left around a joint feature (mm).
    wall: float = 2.0
    # Area of seam per screw (mm²): one screw per 40 × 40 mm by default.
    area_per_screw: float = 1600.0
    max_screws: int = 6
    max_pieces: int = 24
    # Candidate cut positions tried per cut.
    samples: int = 13
    # Extra cuts: planes the caller wants (e.g. to print a piece in its strong orientation).
    extra_planes: list = field(default_factory=list)
    # Pieces keep the part's up direction (turning only about z) when fitting the printer.
    keep_up: bool = True


# ------------------------------------------------------------------ fitting

def fits(size: Sequence[float], usable: Sequence[float], keep_up: bool = False) -> bool:
    """Whether a box fits the build volume: with its z kept up (turning only
    about z), or in any axis-aligned orientation."""
    if keep_up:
        (x, y, z), (a, b, c) = size, usable
        return z <= c + 1e-6 and ((x <= a + 1e-6 and y <= b + 1e-6) or (x <= b + 1e-6 and y <= a + 1e-6))
    return all(a <= b + 1e-6 for a, b in zip(sorted(size), sorted(usable)))


def limits(size: Sequence[float], usable: Sequence[float], keep_up: bool) -> list[float]:
    """The build limit each axis of a piece must meet (longest side to the longest room)."""
    if keep_up:
        big, small = max(usable[0], usable[1]), min(usable[0], usable[1])
        return [big, small, usable[2]] if size[0] >= size[1] else [small, big, usable[2]]
    order = np.argsort(size)
    out = [0.0, 0.0, 0.0]
    for axis, room in zip(order, sorted(usable)):
        out[int(axis)] = room
    return out


def body_size(k, body: Body) -> tuple[Vec3, Vec3]:
    lo, hi = k.bounding_box(body)
    return tuple(lo), tuple(hi)


# ----------------------------------------------------------------- sections

def plane_basis(normal: Vec3) -> tuple[Vec3, Vec3]:
    n = v_unit(normal)
    x = v_unit(v_cross(n, (0.0, 0.0, 1.0))) if abs(n[2]) < 0.9 else v_unit(v_cross(n, (1.0, 0.0, 0.0)))
    # Keep x in the plane and y = n × x.
    return x, v_unit(v_cross(n, x))


@dataclass
class Section:
    plane: Plane
    loops: list          # 2D loops (u, v) in plane coordinates
    area: float
    islands: int         # outer loops (separate regions)
    holes: int

    def inside(self, pts: np.ndarray) -> np.ndarray:
        """Even-odd point-in-polygon over all loops."""
        inside = np.zeros(len(pts), bool)
        for loop in self.loops:
            a = np.asarray(loop)
            b = np.roll(a, -1, axis=0)
            for (x0, y0), (x1, y1) in zip(a, b):
                cond = (y0 > pts[:, 1]) != (y1 > pts[:, 1])
                with np.errstate(divide='ignore', invalid='ignore'):
                    xc = x0 + (pts[:, 1] - y0) * (x1 - x0) / (y1 - y0)
                inside ^= cond & (pts[:, 0] < xc)
        return inside

    def clearance(self, pts: np.ndarray) -> np.ndarray:
        """Distance from each point to the nearest section edge."""
        segs = []
        for loop in self.loops:
            a = np.asarray(loop)
            segs.append(np.stack([a, np.roll(a, -1, axis=0)], 1))
        s = np.concatenate(segs)
        a, b = s[:, 0], s[:, 1]
        ab = b - a
        L2 = np.maximum((ab ** 2).sum(1), 1e-12)
        out = np.full(len(pts), np.inf)
        for i in range(0, len(pts), 4096):
            p = pts[i:i + 4096][:, None, :]
            t = np.clip(((p - a) * ab).sum(2) / L2, 0, 1)
            d = np.linalg.norm(p - (a + t[..., None] * ab), axis=2)
            out[i:i + 4096] = d.min(1)
        return out


def _chain(edges: Sequence[Sequence[Vec3]], tol: float = 1e-3) -> list[list[Vec3]]:
    """Join section edges (polylines, any order and direction) into closed loops."""
    todo = [list(e) for e in edges if len(e) >= 2]
    loops = []
    close = lambda a, b: abs(a[0] - b[0]) < tol and abs(a[1] - b[1]) < tol and abs(a[2] - b[2]) < tol
    while todo:
        loop = todo.pop()
        grew = True
        while grew and not close(loop[0], loop[-1]):
            grew = False
            for i, e in enumerate(todo):
                if close(loop[-1], e[0]):
                    loop.extend(e[1:])
                elif close(loop[-1], e[-1]):
                    loop.extend(list(reversed(e))[1:])
                elif close(loop[0], e[-1]):
                    loop[:0] = e[:-1]
                elif close(loop[0], e[0]):
                    loop[:0] = list(reversed(e))[:-1]
                else:
                    continue
                todo.pop(i)
                grew = True
                break
        if close(loop[0], loop[-1]) and len(loop) > 3:
            loops.append(loop[:-1])
    return loops


def section(k, body: Body, point: Vec3, normal: Vec3) -> Optional[Section]:
    x, _ = plane_basis(normal)
    plane = Plane(tuple(point), v_unit(normal), x)
    raw = _chain(k.section(body, plane))
    loops = []
    for loop in raw:
        uv = [plane.to_local(p)[:2] for p in loop]
        if len(uv) >= 3:
            loops.append(uv)
    if not loops:
        return None
    signed = [0.5 * sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(l, l[1:] + l[:1])) for l in loops]
    # A loop inside another loop is a hole (even nesting depth: outer; odd: hole).
    hole = []
    for i, l in enumerate(loops):
        depth = sum(1 for j, m in enumerate(loops) if j != i and Section(plane, [m], 0, 0, 0).inside(np.asarray([l[0]]))[0])
        hole.append(depth % 2 == 1)
    area = sum(-abs(a) if h else abs(a) for a, h in zip(signed, hole))
    outer = hole.count(False)
    return Section(plane, loops, area, outer, len(loops) - outer)


# -------------------------------------------------------------------- cuts

@dataclass
class Cut:
    point: Vec3
    normal: Vec3
    score: float
    area: float
    islands: int
    holes: int
    why: str


def choose_cut(k, body: Body, axis: int, lo: float, hi: float, window: tuple[float, float], samples: int,
               load_penalty=None, nominal: Optional[float] = None) -> Cut:
    """Best plane normal to `axis` between window[0] and window[1]."""
    n = AXES[axis]
    blo, bhi = body_size(k, body)
    centre = [(blo[i] + bhi[i]) / 2 for i in range(3)]
    best: Optional[Cut] = None
    a, b = max(window[0], lo + 1.0), min(window[1], hi - 1.0)
    if b < a:
        a = b = (window[0] + window[1]) / 2
    for s in np.linspace(a, b, max(samples, 1)):
        p = list(centre)
        p[axis] = float(s)
        sec = section(k, body, tuple(p), n)
        if sec is None or sec.area <= 0:
            continue
        # Prefer one solid region with no holes and a generous area to join across;
        # every extra island or hole is a seam that is harder to align and weaker.
        score = math.log(sec.area) - 1.5 * (sec.islands - 1) - 0.75 * sec.holes
        if nominal is not None:
            # Between equal sections, pieces of equal size.
            score -= 0.3 * abs(s - nominal) / max(hi - lo, 1e-9)
        why = f'{sec.area:.0f} mm², {sec.islands} region(s), {sec.holes} hole(s)'
        if load_penalty is not None:
            pen, note = load_penalty(tuple(p), n)
            score -= pen
            why += f', {note}'
        if best is None or score > best.score:
            best = Cut(tuple(p), n, score, sec.area, sec.islands, sec.holes, why)
    if best is None:
        raise ValueError(f'no plane normal to {"xyz"[axis]} between {a:.1f} and {b:.1f} mm cuts the body')
    return best


def plan_cuts(k, body: Body, usable: Sequence[float], options: SplitOptions, load_penalty=None) -> list[Cut]:
    """Cut planes (axis-aligned, part frame) that make every piece fit."""
    cuts: list[Cut] = []
    pieces = [body]
    size_of = lambda p: np.subtract(*body_size(k, p)[::-1])
    for _ in range(12):
        todo = [p for p in pieces if not fits(size_of(p), usable, options.keep_up)]
        if not todo:
            return cuts
        piece = max(todo, key=lambda p: max(size_of(p)))
        lo, hi = body_size(k, piece)
        size = np.subtract(hi, lo)
        lim = limits(size, usable, options.keep_up)
        # Cut across the axis that overflows its limit the most.
        axis = int(np.argmax(size / np.maximum(lim, 1e-9)))
        target = lim[axis]
        count = max(int(math.ceil(size[axis] / target)), 2)
        step = size[axis] / count
        new_cuts = []
        for i in range(1, count):
            nominal = lo[axis] + i * step
            # Each piece may be up to `target` long: keep the window where both sides still fit.
            slack = max(0.0, (target - step) * 0.9)
            c = choose_cut(k, piece, axis, lo[axis], hi[axis], (nominal - slack, nominal + slack), options.samples, load_penalty, nominal)
            new_cuts.append(c)
        cuts.extend(new_cuts)
        pieces = cut_pieces(k, body, cuts)
        if len(pieces) > options.max_pieces:
            raise ValueError(f'more than {options.max_pieces} pieces needed: use a larger printer')
    raise ValueError('could not find cuts that make every piece fit')


def cut_pieces(k, body: Body, cuts: Sequence[Cut]) -> list[Body]:
    pieces = [body]
    for c in cuts:
        x, _ = plane_basis(c.normal)
        plane = Plane(c.point, c.normal, x)
        nxt = []
        for p in pieces:
            lo, hi = body_size(k, p)
            d = [v_dot(v_sub(corner, c.point), c.normal) for corner in _corners(lo, hi)]
            if min(d) < -1e-6 and max(d) > 1e-6:
                nxt.extend(b for b in k.cut_with_plane(p, plane) if k.mass_properties(b).volume > 1e-3)
            else:
                nxt.append(p)
        pieces = nxt
    return pieces


def _corners(lo, hi):
    return [(x, y, z) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]


# ------------------------------------------------------------------- joints

@dataclass
class JointPlan:
    kind: str                 # dowel | insert_screw | dovetail
    at: Vec3                  # on the seam plane
    spec: dict                # what sim-print checks (Joint in sim_print::joints)
    hardware: list            # [{'item', 'size', 'count'}]
    notes: list = field(default_factory=list)
    geometry: dict = field(default_factory=dict)   # how CAD cuts it (screw access, …)


@dataclass
class Seam:
    name: str
    point: Vec3
    normal: Vec3              # from the minus piece into the plus piece
    minus: int                # piece indices
    plus: int
    area: float
    joints: list = field(default_factory=list)
    notes: list = field(default_factory=list)

    def study(self) -> dict:
        return {'name': self.name, 'point': list(self.point), 'normal': list(self.normal), 'joints': [j.spec for j in self.joints]}


def _thickness(k, body: Body, point: Vec3, direction: Vec3) -> float:
    """Material along `direction` from a point on the body's face (mm)."""
    start = v_add(point, v_scale(direction, 0.02))
    hits = sorted(h[0] for h in k.ray_hits(body, start, direction))
    return hits[0] + 0.02 if hits else 0.0


MAX_TUNNEL = 30.0   # mm: deeper than this a hex key does not reach comfortably
MAX_POCKET_REACH = 30.0


def _side_access(k, body: Body, p: Vec3, n: Vec3, clamp: float, radius: float) -> Optional[tuple[Vec3, float]]:
    """The shortest way out sideways (in the seam plane's directions) from the
    screw head's seat: (direction, distance to the outside)."""
    seat = v_add(p, v_scale(n, clamp + 1.5))
    x, y = plane_basis(n)
    best = None
    for a in range(16):
        t = 2 * math.pi * a / 16
        d = v_unit(v_add(v_scale(x, math.cos(t)), v_scale(y, math.sin(t))))
        hits = sorted(h[0] for h in k.ray_hits(body, seat, d))
        if hits and hits[0] <= MAX_POCKET_REACH and (best is None or hits[0] < best[1]):
            best = (d, hits[0])
    return best


def _pick_spread(candidates: np.ndarray, count: int, avoid: Sequence[np.ndarray] = (), spacing: float = 0.0) -> list[int]:
    """Farthest-point picks: spread out, away from `avoid`."""
    if len(candidates) == 0:
        return []
    chosen: list[int] = []
    d = np.full(len(candidates), np.inf)
    for a in avoid:
        d = np.minimum(d, np.linalg.norm(candidates - a, axis=1))
    if not avoid:
        # Start at the point farthest from the centroid.
        c = candidates.mean(0)
        first = int(np.argmax(np.linalg.norm(candidates - c, axis=1)))
        chosen.append(first)
        d = np.minimum(d, np.linalg.norm(candidates - candidates[first], axis=1))
    while len(chosen) < count:
        i = int(np.argmax(d))
        if not np.isfinite(d[i]) or d[i] < spacing:
            break
        chosen.append(i)
        d = np.minimum(d, np.linalg.norm(candidates - candidates[i], axis=1))
    return chosen


def plan_joints(k, seam: Seam, sec: Section, minus: Body, plus: Body, options: SplitOptions) -> None:
    """Choose joint positions on a seam (fills `seam.joints`)."""
    n = seam.normal
    plane = sec.plane
    # Candidate points on a grid over the section, with their clearance to the edge.
    us = np.concatenate([np.asarray(l)[:, 0] for l in sec.loops])
    vs = np.concatenate([np.asarray(l)[:, 1] for l in sec.loops])
    step = max(0.75, min(np.ptp(us), np.ptp(vs)) / 60)
    gu, gv = np.meshgrid(np.arange(us.min(), us.max(), step), np.arange(vs.min(), vs.max(), step))
    pts = np.stack([gu.ravel(), gv.ravel()], 1)
    pts = pts[sec.inside(pts)]
    if len(pts) == 0:
        seam.notes.append('the section is too small for any joint')
        return
    clear = sec.clearance(pts)
    world = lambda uv: plane.to_world(float(uv[0]), float(uv[1]))
    d_pin = options.pin_diameter
    pin = reg.joint('dowel_pin')
    ins = reg.insert(options.screw)
    scr = reg.screw(options.screw)
    per_d = reg.value(pin['engagement_per_diameter'])
    want = options.joint

    def dowel_at(uv) -> Optional[JointPlan]:
        p = world(uv)
        depth = d_pin * per_d
        tm, tp = _thickness(k, minus, p, v_scale(n, -1)), _thickness(k, plus, p, n)
        dm, dp = min(depth, tm - 1.2), min(depth, tp - 1.2)
        if min(dm, dp) < d_pin:
            return None
        lengths = [L for L in pin['lengths_mm'] if L <= dm + dp - 0.5]
        if not lengths:
            return None
        L = max(lengths)
        return JointPlan('dowel', p, {'kind': 'dowel', 'at': list(p), 'diameter_mm': d_pin, 'depth_minus_mm': round(dm, 2), 'depth_plus_mm': round(dp, 2)},
                         [{'item': 'steel dowel pin', 'size': f'Ø{d_pin:g} × {L:g} mm', 'count': 1}],
                         [f'holes {dm:.1f} mm into the minus piece (press fit) and {dp:.1f} mm into the plus piece (slip fit); pin {L:g} mm'])

    def screw_at(uv) -> Optional[JointPlan]:
        p = world(uv)
        tm, tp = _thickness(k, minus, p, v_scale(n, -1)), _thickness(k, plus, p, n)
        if tm < ins['depth_mm'] + 1.5:
            return None
        cb_d, cb_depth = scr['counterbore_mm']
        # Clamp at most 8 mm of the plus piece. Deeper: a counterbore tunnel from the
        # far face (up to MAX_TUNNEL), else a pocket from the nearest side face.
        clamp = min(tp - 0.5, 8.0)
        if clamp < 2.5:
            return None
        grip = ins['length_mm'] * 0.9
        lengths = [L for L in scr['lengths_mm'] if clamp + 1.0 <= L <= clamp + grip]
        if not lengths:
            return None
        L = max(lengths)
        tunnel = tp - clamp
        geometry = {'access': 'tunnel', 'tunnel_mm': round(max(tunnel, 0.0), 2)}
        how = f' (counterbore tunnel {tunnel:.1f} mm deep, Ø{cb_d})' if tunnel > 0.5 else ''
        if tunnel > MAX_TUNNEL:
            side = _side_access(k, plus, p, n, clamp, cb_d / 2 + 0.5)
            if side is None:
                return None
            direction, reach = side
            height = L + scr['head_mm'] * 0.6 + 1.5
            if clamp + height > tp - options.wall:
                return None
            geometry = {'access': 'pocket', 'direction': list(direction), 'reach_mm': round(reach, 2), 'height_mm': round(height, 2), 'width_mm': round(cb_d + 1.0, 2)}
            how = f' (screw pocket {cb_d + 1:.1f} mm wide, {height:.1f} mm tall, opening {reach:.1f} mm away through the side)'
        return JointPlan('insert_screw', p, {'kind': 'insert_screw', 'at': list(p), 'size': options.screw, 'screw_length_mm': L, 'clamp_mm': round(clamp, 2)},
                         [{'item': 'heat-set insert', 'size': options.screw, 'count': 1}, {'item': 'socket head screw', 'size': f'{options.screw} × {L:g} mm', 'count': 1}],
                         [f'insert {ins["hole_mm"]} × {ins["depth_mm"]} mm pocket in the minus piece; {options.screw} × {L:g} screw through {clamp:.1f} mm of the plus piece' + how],
                         geometry)

    placed: list[JointPlan] = []
    # Two pins, far apart, where there is room for a pin and its wall; smaller pins if none fit.
    for d_try in sorted((d for d in pin['diameters_mm'] if d <= d_pin), reverse=True):
        d_pin = d_try
        need = d_pin / 2 + options.wall
        ok = pts[clear >= need]
        for i in _pick_spread(ok, 8):
            if len(placed) == 2:
                break
            j = dowel_at(ok[i])
            if j and all(v_norm(v_sub(j.at, q.at)) > 3 * d_pin for q in placed):
                placed.append(j)
        if placed:
            break
    if want in ('auto', 'pins+screws'):
        count = int(np.clip(round(sec.area / options.area_per_screw), 2, options.max_screws))
        # A small section takes smaller screws: try the chosen size, then the next ones down.
        sizes = [options.screw] + [z for z in ('M2.5', 'M2') if z != options.screw and float(z[1:]) < float(options.screw[1:])]
        for size in sizes:
            ins, scr = reg.insert(size), reg.screw(size)
            need = max(ins['knurl_mm'] / 2, scr['counterbore_mm'][0] / 2) + options.wall
            if int((clear >= need).sum()) >= 2 and np.sort(clear)[-1] >= need:
                # Room for two screws a spacing apart?
                ok_pts = pts[clear >= need]
                if len(ok_pts) and np.ptp(ok_pts, axis=0).max() >= 2.2 * need:
                    break
        options = SplitOptions(**{**options.__dict__, 'screw': size})
        need = max(ins['knurl_mm'] / 2, scr['counterbore_mm'][0] / 2) + options.wall
        ok = pts[clear >= need]
        avoid = [np.asarray(plane.to_local(j.at)[:2]) for j in placed]
        screws = []
        for i in _pick_spread(ok, count * 3, avoid, spacing=need * 2):
            if len(screws) == count:
                break
            j = screw_at(ok[i])
            if j and all(v_norm(v_sub(j.at, q.at)) > 2.2 * need for q in placed + screws):
                screws.append(j)
        placed.extend(screws)
        if not screws:
            seam.notes.append('no screw fits here (too thin for an insert or its clamp)')
            if want == 'auto':
                want = 'dovetail'
    if want == 'dovetail':
        dv = dovetail_joints(k, seam, sec, minus, plus, options, pts, [j.at for j in placed], step)
        if dv:
            placed.extend(dv)
        else:
            seam.notes.append('no room for a dovetail either')
    if not any(j.kind in ('insert_screw', 'dovetail') for j in placed):
        seam.notes.append('nothing holds this seam closed: glue it, or add a clamp')
    seam.joints = placed


def dovetail_joints(k, seam: Seam, sec: Section, minus: Body, plus: Body, options: SplitOptions,
                    grid: np.ndarray, avoid: Sequence[Vec3] = (), step: float = 1.0) -> list[JointPlan]:
    """Dovetails across a seam. A thin section (a plate) gets jigsaw tabs through
    its thickness, spaced along the seam; a thick one gets one sliding rail
    along its long direction. `grid` is interior sample points (u, v)."""
    if len(grid) < 3:
        return []
    c = grid.mean(0)
    w, v = np.linalg.eigh(np.cov((grid - c).T))
    t2, s2 = v[:, 1], v[:, 0]
    to3 = lambda d2: v_unit(v_sub(sec.plane.to_world(float(d2[0]), float(d2[1])), sec.plane.origin))
    # CAD parts are mostly square to their axes: snap the seam's directions to a
    # part axis in the seam plane when within 15° (a hub or rim skews the fit).
    t3 = to3(t2)
    for ax in AXES + tuple(v_scale(a, -1.0) for a in AXES):
        if abs(v_dot(ax, seam.normal)) < 1e-6 and v_dot(ax, t3) > math.cos(math.radians(15)):
            t3 = ax
    s3 = v_unit(v_cross(seam.normal, t3))
    to2 = lambda d3: np.asarray(sec.plane.to_local(v_add(sec.plane.origin, d3))[:2])
    t2, s2 = to2(t3), to2(s3)
    along_t, along_s = t3, s3
    ut, us = (grid - c) @ t2, (grid - c) @ s2
    thickness = float(np.ptp(us))
    angle = math.radians(reg.joint('dovetail.angle_deg'))
    plans = []
    # Stations along the seam: at each, the longest continuous run of material
    # across it. Where that run is plate-like (2.5–25 mm), a tab goes through it.
    bins = np.round(ut / max(step, 1e-6)).astype(int)
    stations = []
    for b in np.unique(bins):
        vals = np.sort(us[bins == b])
        breaks = np.where(np.diff(vals) > 1.5 * step)[0]
        runs = np.split(vals, breaks + 1)
        run = max(runs, key=lambda r: r[-1] - r[0])
        local = float(run[-1] - run[0] + step)
        if 2.5 <= local <= 25.0:
            stations.append((float(b * step), float((run[0] + run[-1]) / 2), local))
    if stations:
        span = stations[-1][0] - stations[0][0]
        count = int(np.clip(round(span / 50.0), 1, 6))
        spacing = span / count if count else span
        neck = float(np.clip(0.35 * min(spacing, 40.0), 6.0, 14.0))
        depth = 0.9 * neck
        targets = [stations[0][0] + spacing * (i + 0.5) for i in range(count)]
        for target in targets:
            u, mid, local = min(stations, key=lambda st: abs(st[0] - target))
            if any(abs(u - float((np.asarray(sec.plane.to_local(q)[:2]) - c) @ t2)) < neck for q in avoid):
                continue
            centre2 = c + t2 * u + s2 * mid
            centre = sec.plane.to_world(float(centre2[0]), float(centre2[1]))
            # The tab reaches `depth` into the plus piece; both sides need material there.
            if _thickness(k, plus, centre, seam.normal) < depth + options.wall or _thickness(k, minus, centre, v_scale(seam.normal, -1)) < options.wall:
                continue
            plans.append(JointPlan('dovetail', centre, {'kind': 'dovetail', 'at': list(centre), 'along': list(along_s), 'rail_length_mm': round(local, 2), 'neck_mm': round(neck, 2), 'depth_mm': round(depth, 2)},
                                   [], [f'jigsaw tab {neck:.1f} mm neck, {depth:.1f} mm deep, through {local:.1f} mm of plate; drops in along {tuple(round(x, 2) for x in along_s)}'],
                                   {'extrude_mm': local + 20.0}))
        return plans
    length = float(np.ptp(ut))
    neck = float(np.clip(0.3 * thickness, 5.0, 20.0))
    depth = float(np.clip(0.6 * neck, 3.0, 10.0))
    # The interior point nearest the centroid (the centroid may be in a hole).
    centre2 = grid[int(np.argmin(np.linalg.norm(grid - c, axis=1)))]
    centre = sec.plane.to_world(float(centre2[0]), float(centre2[1]))
    for f in np.linspace(-0.4, 0.4, 5):
        p = v_add(centre, v_scale(along_t, f * length))
        if _thickness(k, plus, p, seam.normal) < depth + options.wall or _thickness(k, minus, p, v_scale(seam.normal, -1)) < options.wall:
            return []
    return [JointPlan('dovetail', centre, {'kind': 'dovetail', 'at': list(centre), 'along': list(along_t), 'rail_length_mm': round(length, 2), 'neck_mm': round(neck, 2), 'depth_mm': round(depth, 2)},
                      [], [f'rail {length:.0f} mm long, neck {neck:.1f} mm, {depth:.1f} mm deep at {math.degrees(angle):.0f}°; slides in along {tuple(round(x, 2) for x in along_t)}'],
                      {'extrude_mm': length + 20.0})]


# --------------------------------------------------------------- geometry

def _trapezoid_prism(k, centre: Vec3, along: Vec3, normal: Vec3, neck: float, depth: float, angle: float, length: float, grow: float = 0.0) -> Body:
    """The dovetail tail: neck on the seam plane, widening into the plus side."""
    across = v_unit(v_cross(normal, along))
    half0 = neck / 2 + grow
    half1 = neck / 2 + depth * math.tan(angle) + grow
    start = v_add(centre, v_scale(along, -length / 2))
    plane = Plane(start, along, across)   # u = across, v = normal × ... (y = along × across = ±normal)
    y = plane.y_axis
    sign = 1.0 if v_dot(y, normal) > 0 else -1.0
    lo = -grow  # a little into the minus side so the groove's floor clears
    pts = [(-half0, sign * lo), (half0, sign * lo), (half1, sign * (depth + grow)), (-half1, sign * (depth + grow))]
    sk = Sketch(plane)
    sk.polyline(pts, closed=True)
    return k.extrude(sk.to_body(), along, length)


def build_split(k, body: Body, cuts: Sequence[Cut], options: SplitOptions) -> tuple[list[Body], list[Seam]]:
    """Cut the body and add every seam's joints to its pieces."""
    pieces = cut_pieces(k, body, cuts)
    centroids = [k.mass_properties(p).centroid for p in pieces]
    seams: list[Seam] = []
    for ci, c in enumerate(cuts):
        # Pairs of pieces that meet on this plane.
        sides = [v_dot(v_sub(cp, c.point), c.normal) for cp in centroids]
        for i, a in enumerate(pieces):
            if sides[i] >= 0:
                continue
            for j, b in enumerate(pieces):
                if sides[j] <= 0:
                    continue
                shared = _shared_section(k, a, b, c)
                if shared is None:
                    continue
                seam = Seam(f'seam {len(seams) + 1}', c.point, c.normal, i, j, shared.area)
                plan_joints(k, seam, shared, a, b, options)
                seams.append(seam)
    for s in seams:
        pieces[s.minus], pieces[s.plus] = cut_joints(k, pieces[s.minus], pieces[s.plus], s, options)
    return pieces, seams


def _interior_samples(sec: Section, count: int) -> np.ndarray:
    pts = np.concatenate([np.asarray(l) for l in sec.loops])
    lo, hi = pts.min(0), pts.max(0)
    n = int(math.sqrt(count)) + 1
    gu, gv = np.meshgrid(np.linspace(lo[0], hi[0], n), np.linspace(lo[1], hi[1], n))
    g = np.stack([gu.ravel(), gv.ravel()], 1)
    return g[sec.inside(g)]


def _shared_section(k, a: Body, b: Body, c: Cut) -> Optional[Section]:
    """Where two pieces meet on a cut plane: sections of both, intersected on a grid."""
    sa = section(k, a, v_add(c.point, v_scale(c.normal, -0.05)), c.normal)
    sb = section(k, b, v_add(c.point, v_scale(c.normal, 0.05)), c.normal)
    if not sa or not sb:
        return None
    # Do the two faces overlap? Sample points inside a's section and test them in b's.
    # A real shared face, not pieces touching along a line.
    samples = _interior_samples(sa, 900)
    if len(samples) == 0 or sb.inside(samples).sum() < max(4, 0.05 * len(samples)):
        return None
    sa.plane = Plane(c.point, c.normal, sa.plane.x_axis)
    return sa


def _box(k, corner: Vec3, axes: tuple[Vec3, Vec3, Vec3], size: tuple[float, float, float]) -> Body:
    """A box with edges along three orthonormal axes from one corner."""
    a, b, c = axes
    plane = Plane(corner, c, a)
    sign = 1.0 if v_dot(plane.y_axis, b) > 0 else -1.0
    sk = Sketch(plane)
    sk.polyline([(0.0, 0.0), (size[0], 0.0), (size[0], sign * size[1]), (0.0, sign * size[1])], closed=True)
    return k.extrude(sk.to_body(), c, size[2])


def cut_joints(k, minus: Body, plus: Body, seam: Seam, options: SplitOptions) -> tuple[Body, Body]:
    n = seam.normal
    pin = reg.joint('dowel_pin')
    slip, press = reg.value(pin['slip_clearance_mm']), reg.value(pin['press_clearance_mm'])
    kept = []
    for j in seam.joints:
        try:
            minus, plus = _cut_joint(k, minus, plus, n, j, slip, press)
            kept.append(j)
        except KernelError as e:
            seam.notes.append(f'{j.kind} at {tuple(round(x, 1) for x in j.at)} left out: {e}')
    seam.joints = kept
    return minus, plus


def _cut_joint(k, minus: Body, plus: Body, n: Vec3, j: 'JointPlan', slip: float, press: float) -> tuple[Body, Body]:
    s = j.spec
    p = j.at
    if True:
        if j.kind == 'dowel':
            d = s['diameter_mm']
            minus = k.boolean(minus, k.cylinder(v_add(p, v_scale(n, 0.1)), v_scale(n, -1), (d + press) / 2, s['depth_minus_mm'] + 0.1), BooleanOp.SUBTRACT)
            plus = k.boolean(plus, k.cylinder(v_add(p, v_scale(n, -0.1)), n, (d + slip) / 2, s['depth_plus_mm'] + 0.1), BooleanOp.SUBTRACT)
        elif j.kind == 'insert_screw':
            ins, scr = reg.insert(s['size']), reg.screw(s['size'])
            minus = k.boolean(minus, k.cylinder(v_add(p, v_scale(n, 0.1)), v_scale(n, -1), ins['hole_mm'] / 2, ins['depth_mm'] + 0.6), BooleanOp.SUBTRACT)
            # Pilot below the insert for the screw tip.
            minus = k.boolean(minus, k.cylinder(p, v_scale(n, -1), scr['clearance_mm'] / 2 - 0.3, ins['depth_mm'] + 3.0), BooleanOp.SUBTRACT)
            tp = _thickness(k, plus, p, n)
            reach = s['clamp_mm'] + 0.6 if j.geometry.get('access') == 'pocket' else tp + 0.2
            plus = k.boolean(plus, k.cylinder(v_add(p, v_scale(n, -0.1)), n, scr['clearance_mm'] / 2, reach), BooleanOp.SUBTRACT)
            cb_d, _ = scr['counterbore_mm']
            clamp = s['clamp_mm']
            g = j.geometry
            if g.get('access') == 'pocket':
                # A window from the side: head seat at `clamp`, tall enough to put the screw in.
                d = tuple(g['direction'])
                w = g['width_mm']
                side = v_unit(v_cross(n, d))
                seat = v_add(p, v_scale(n, clamp))
                reach = g['reach_mm'] + 1.0
                corner = v_add(v_add(seat, v_scale(side, -w / 2)), v_scale(d, -w / 2))
                pocket = _box(k, corner, (side, d, n), (w, reach + w / 2, g['height_mm']))
                plus = k.boolean(plus, pocket, BooleanOp.SUBTRACT)
                # The clearance hole only needs to reach the pocket.
            else:
                plus = k.boolean(plus, k.cylinder(v_add(p, v_scale(n, clamp)), n, cb_d / 2, tp - clamp + 0.2), BooleanOp.SUBTRACT)
        elif j.kind == 'dovetail':
            angle = math.radians(reg.joint('dovetail.angle_deg'))
            clearance = reg.joint('dovetail.sliding_clearance_mm')
            along = tuple(s['along'])
            length = j.geometry.get('extrude_mm', s['rail_length_mm'] + 20.0)
            tail = _trapezoid_prism(k, p, along, n, s['neck_mm'], s['depth_mm'], angle, length)
            groove = _trapezoid_prism(k, p, along, n, s['neck_mm'], s['depth_mm'], angle, length, grow=clearance)
            # The tail is the part of the prism inside the plus piece's volume; it moves to the minus piece.
            tail_in = k.boolean(tail, plus, BooleanOp.INTERSECT)
            minus = k.boolean(minus, tail_in, BooleanOp.UNION)
            plus = k.boolean(plus, groove, BooleanOp.SUBTRACT)
    return minus, plus


# --------------------------------------------------------------- top level

@dataclass
class SplitResult:
    source_id: str
    printer: str
    cuts: list
    pieces: list              # Body
    seams: list               # Seam
    usable_mm: tuple
    registry_sha256: str

    def hardware(self) -> list[dict]:
        total: dict[tuple[str, str], int] = {}
        for s in self.seams:
            for j in s.joints:
                for h in j.hardware:
                    key = (h['item'], h['size'])
                    total[key] = total.get(key, 0) + h['count']
        return [{'item': k[0], 'size': k[1], 'count': v} for k, v in sorted(total.items())]

    def summary(self, k) -> dict:
        return {
            'source': self.source_id, 'printer': self.printer, 'usable_mm': list(self.usable_mm), 'registry_sha256': self.registry_sha256,
            'cuts': [{'point': list(c.point), 'normal': list(c.normal), 'why': c.why} for c in self.cuts],
            'pieces': [{'index': i, 'size_mm': [round(float(x), 2) for x in np.subtract(*body_size(k, p)[::-1])], 'fits': bool(fits(np.subtract(*body_size(k, p)[::-1]), self.usable_mm, True) or fits(np.subtract(*body_size(k, p)[::-1]), self.usable_mm)), 'fits_upright': bool(fits(np.subtract(*body_size(k, p)[::-1]), self.usable_mm, True)),
                        'volume_mm3': round(float(k.mass_properties(p).volume), 1)} for i, p in enumerate(self.pieces)],
            'seams': [{'name': s.name, 'minus': s.minus, 'plus': s.plus, 'point': [round(float(x), 3) for x in s.point], 'normal': [float(x) for x in s.normal],
                       'area_mm2': round(s.area, 1), 'notes': s.notes,
                       'joints': [{'kind': j.kind, 'at': [round(x, 2) for x in j.at], 'spec': j.spec, 'hardware': j.hardware, 'notes': j.notes} for j in s.joints]} for s in self.seams],
            'hardware': self.hardware(),
        }


def split_for_printing(doc, node_id: str, options: Optional[SplitOptions] = None, load_penalty=None) -> SplitResult:
    """Plan and build a split without touching the document."""
    options = options or SplitOptions()
    k = doc.kernel
    body = doc.resolved_body(node_id)
    usable = reg.usable_mm(options.printer)
    # Tabs and rails stick out past a cut into the next piece: plan against a box
    # smaller by that allowance, check the built pieces, and allow more if needed.
    allowance = 16.0
    for attempt in range(4):
        box = tuple(u - allowance for u in usable[:2]) + (usable[2],)
        cuts = plan_cuts(k, body, box, options, load_penalty) if not fits(np.subtract(*body_size(k, body)[::-1]), usable, options.keep_up) else []
        for extra in options.extra_planes:
            cuts.append(Cut(tuple(extra['point']), v_unit(tuple(extra['normal'])), 0.0, 0.0, 0, 0, extra.get('why', 'requested')))
        pieces, seams = build_split(k, body, cuts, options) if cuts else ([body], [])
        if all(fits(np.subtract(*body_size(k, p)[::-1]), usable, options.keep_up) for p in pieces):
            break
        allowance += 12.0
    else:
        raise ValueError(f'the pieces with their joints still overflow the {options.printer} after {attempt + 1} tries')
    _, sha = reg.load()
    return SplitResult(node_id, options.printer, cuts, pieces, seams, usable, sha)


def apply_split(ops, result: SplitResult, name: Optional[str] = None) -> str:
    """Add the pieces under a new group as one undoable step; the source body is kept (hidden)."""
    from .commands import AddNodes, Composite, SetAttributes
    from .document import Node
    doc = ops.doc
    src = doc.nodes[result.source_id]
    group = Node(doc.new_id(), 'group', doc.unique_name(name or f'{src.name}: print pieces'))
    nodes = [group]
    summary = result.summary(doc.kernel)
    for i, body in enumerate(result.pieces):
        seams = [s.name for s in result.seams if i in (s.minus, s.plus)]
        node = Node(doc.new_id(), 'body', doc.unique_name(f'{src.name} · piece {i + 1}'), body=body, material=src.material, parent=group.id)
        node.robot = {'print_piece': {'source': result.source_id, 'index': i, 'seams': seams, 'printer': result.printer,
                                      'registry_sha256': result.registry_sha256, 'fits': summary['pieces'][i]['fits']}}
        nodes.append(node)
    group.robot = {'print_split': summary}
    cmds = [AddNodes('Split for printing', nodes), SetAttributes('Split for printing', {result.source_id: {'visible': False}})]
    ops.stack.push(Composite('Split for printing', cmds))
    return group.id
