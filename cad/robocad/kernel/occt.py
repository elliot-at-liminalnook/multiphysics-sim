"""`GeometryKernel` on Open CASCADE (OCCT 7.7 through the OCP binding)."""

from __future__ import annotations

import math
from typing import Optional, Sequence

from OCP.BOPAlgo import BOPAlgo_MakerVolume
from OCP.BRep import BRep_Builder, BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
from OCP.BRepAlgo import BRepAlgo_NormalProjection
from OCP.BRepAlgoAPI import BRepAlgoAPI_Common, BRepAlgoAPI_Cut, BRepAlgoAPI_Defeaturing, BRepAlgoAPI_Fuse, BRepAlgoAPI_Section, BRepAlgoAPI_Splitter
from OCP.BRepBndLib import BRepBndLib
from OCP.BRepBuilderAPI import (
    BRepBuilderAPI_Copy,
    BRepBuilderAPI_GTransform,
    BRepBuilderAPI_MakeEdge,
    BRepBuilderAPI_MakeFace,
    BRepBuilderAPI_MakeSolid,
    BRepBuilderAPI_MakeWire,
    BRepBuilderAPI_NurbsConvert,
    BRepBuilderAPI_Sewing,
    BRepBuilderAPI_Transform,
)
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepExtrema import BRepExtrema_DistShapeShape
from OCP.BRepFeat import BRepFeat_MakePrism
from OCP.BRepFilletAPI import BRepFilletAPI_MakeChamfer, BRepFilletAPI_MakeFillet
from OCP.BRepGProp import BRepGProp
from OCP.BRepIntCurveSurface import BRepIntCurveSurface_Inter
from OCP.BRepLProp import BRepLProp_SLProps
from OCP.BRepLib import BRepLib
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.BRepOffsetAPI import (
    BRepOffsetAPI_DraftAngle,
    BRepOffsetAPI_MakeFilling,
    BRepOffsetAPI_MakeOffsetShape,
    BRepOffsetAPI_MakePipe,
    BRepOffsetAPI_MakePipeShell,
    BRepOffsetAPI_MakeThickSolid,
    BRepOffsetAPI_ThruSections,
)
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder, BRepPrimAPI_MakePrism, BRepPrimAPI_MakeRevol, BRepPrimAPI_MakeSphere
from OCP.BRepTools import BRepTools, BRepTools_ReShape
from OCP.ChFi3d import ChFi3d_FilletShape
from OCP.GCPnts import GCPnts_UniformAbscissa
from OCP.Geom import Geom_BSplineSurface, Geom_Plane
from OCP.GeomAbs import (
    GeomAbs_BezierSurface,
    GeomAbs_BSplineSurface,
    GeomAbs_C0,
    GeomAbs_C1,
    GeomAbs_C2,
    GeomAbs_Circle,
    GeomAbs_Cone,
    GeomAbs_Cylinder,
    GeomAbs_Ellipse,
    GeomAbs_Line,
    GeomAbs_Plane,
    GeomAbs_Sphere,
    GeomAbs_Torus,
)
from OCP.GeomAPI import GeomAPI_PointsToBSplineSurface
from OCP.GeomLProp import GeomLProp_SLProps
from OCP.gp import gp_Ax1, gp_Ax2, gp_Ax3, gp_Dir, gp_GTrsf, gp_Lin, gp_Mat, gp_Pln, gp_Pnt, gp_Trsf, gp_Vec
from OCP.GProp import GProp_GProps
from OCP.HLRAlgo import HLRAlgo_Projector
from OCP.HLRBRep import HLRBRep_Algo, HLRBRep_HLRToShape
from OCP.LocOpe import LocOpe_FindEdges
from OCP.Poly import Poly_Triangulation
from OCP.ShapeAnalysis import ShapeAnalysis_FreeBounds
from OCP.ShapeFix import ShapeFix_Shape, ShapeFix_Solid
from OCP.ShapeUpgrade import ShapeUpgrade_UnifySameDomain
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TopAbs import TopAbs_COMPOUND, TopAbs_EDGE, TopAbs_FACE, TopAbs_REVERSED, TopAbs_SHELL, TopAbs_SOLID, TopAbs_VERTEX, TopAbs_WIRE
from OCP.TopExp import TopExp, TopExp_Explorer
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS, TopoDS_Compound, TopoDS_Face, TopoDS_Shape, TopoDS_Shell, TopoDS_Solid, TopoDS_Wire
from OCP.TopTools import TopTools_IndexedDataMapOfShapeListOfShape, TopTools_ListOfShape

from .base import (
    Body,
    BooleanOp,
    ChamferSpec,
    CurveKind,
    EdgeRef,
    FaceRef,
    GeometryKernel,
    KernelError,
    MassProperties,
    Mesh,
    Plane,
    SurfaceKind,
    SweepOptions,
    ValidationIssue,
    ValidationReport,
    Vec3,
    VertexRef,
    match_edge,
    match_face,
    v_add,
    v_cross,
    v_dist,
    v_dot,
    v_scale,
    v_sub,
    v_unit,
)

# ------------------------------------------------------------------ helpers


def P(p: Vec3) -> gp_Pnt:
    return gp_Pnt(float(p[0]), float(p[1]), float(p[2]))


def D(d: Vec3) -> gp_Dir:
    u = v_unit(d)
    return gp_Dir(u[0], u[1], u[2])


def V(v: Vec3) -> gp_Vec:
    return gp_Vec(float(v[0]), float(v[1]), float(v[2]))


def pt(p: gp_Pnt) -> Vec3:
    return (p.X(), p.Y(), p.Z())


def dr(d) -> Vec3:
    return (d.X(), d.Y(), d.Z())


def explore(shape: TopoDS_Shape, kind) -> list:
    out = []
    ex = TopExp_Explorer(shape, kind)
    seen = set()
    while ex.More():
        s = ex.Current()
        key = s.HashCode(1 << 30)
        # Skip the same sub-shape reached twice through shared topology.
        if key not in seen:
            seen.add(key)
            out.append(s)
        ex.Next()
    return out


def occ_faces(shape) -> list[TopoDS_Face]:
    return [TopoDS.Face_s(f) for f in explore(shape, TopAbs_FACE)]


def occ_edges(shape) -> list:
    return [TopoDS.Edge_s(e) for e in explore(shape, TopAbs_EDGE)]


def _finish(shape: TopoDS_Shape, kind: str = "solid") -> Body:
    if shape.IsNull():
        raise KernelError("the operation produced no geometry")
    return Body(shape, kind)


def _check(algo, what: str):
    if hasattr(algo, "IsDone") and not algo.IsDone():
        raise KernelError(f"{what} failed")


def _has_errors(algo) -> bool:
    fn = getattr(algo, "HasErrors", None)
    try:
        return bool(fn()) if fn else False
    except Exception:
        return False


def _solid_kind(shape: TopoDS_Shape) -> str:
    if explore(shape, TopAbs_SOLID):
        return "solid"
    if explore(shape, TopAbs_FACE):
        return "sheet"
    return "wire"


def _unify(shape: TopoDS_Shape) -> TopoDS_Shape:
    """Merge faces that ended up split along the same surface (a boolean
    leaves seams a direct modeler would rather not show)."""
    u = ShapeUpgrade_UnifySameDomain(shape, True, True, True)
    u.Build()
    return u.Shape()


def _plane_of(plane: Plane) -> gp_Pln:
    ax = gp_Ax3(P(plane.origin), D(plane.normal), D(plane.x_axis))
    return gp_Pln(ax)


def _face_ref(face: TopoDS_Face, index: int) -> FaceRef:
    props = GProp_GProps()
    BRepGProp.SurfaceProperties_s(face, props)
    area = props.Mass()
    centroid = pt(props.CentreOfMass())
    ad = BRepAdaptor_Surface(face)
    t = ad.GetType()
    kind = SurfaceKind.OTHER
    axis_point = axis_dir = None
    radius = None
    if t == GeomAbs_Plane:
        kind = SurfaceKind.PLANE
    elif t == GeomAbs_Cylinder:
        kind = SurfaceKind.CYLINDER
        c = ad.Cylinder()
        axis_point, axis_dir, radius = pt(c.Location()), dr(c.Axis().Direction()), c.Radius()
    elif t == GeomAbs_Cone:
        kind = SurfaceKind.CONE
        c = ad.Cone()
        axis_point, axis_dir, radius = pt(c.Location()), dr(c.Axis().Direction()), c.RefRadius()
    elif t == GeomAbs_Sphere:
        kind = SurfaceKind.SPHERE
        s = ad.Sphere()
        axis_point, axis_dir, radius = pt(s.Location()), (0.0, 0.0, 1.0), s.Radius()
    elif t == GeomAbs_Torus:
        kind = SurfaceKind.TORUS
        s = ad.Torus()
        axis_point, axis_dir, radius = pt(s.Location()), dr(s.Axis().Direction()), s.MinorRadius()
    elif t == GeomAbs_BSplineSurface:
        kind = SurfaceKind.BSPLINE
    elif t == GeomAbs_BezierSurface:
        kind = SurfaceKind.BEZIER
    # Normal at the centroid's parameters (the middle of the parameter range).
    umin, umax, vmin, vmax = BRepTools.UVBounds_s(face)
    props2 = BRepLProp_SLProps(ad, 0.5 * (umin + umax), 0.5 * (vmin + vmax), 1, 1e-6)
    point = pt(props2.Value())
    if props2.IsNormalDefined():
        n = dr(props2.Normal())
        if face.Orientation() == TopAbs_REVERSED:
            n = v_scale(n, -1.0)
    else:
        n = (0.0, 0.0, 1.0)
    return FaceRef(kind, centroid, n, area, axis_point, axis_dir, radius, index, point)


def _edge_ref(edge, index: int) -> EdgeRef:
    ad = BRepAdaptor_Curve(edge)
    t = ad.GetType()
    kind = {GeomAbs_Line: CurveKind.LINE, GeomAbs_Circle: CurveKind.CIRCLE, GeomAbs_Ellipse: CurveKind.ELLIPSE}.get(t, CurveKind.BSPLINE if t in (6, 7) else CurveKind.OTHER)
    props = GProp_GProps()
    BRepGProp.LinearProperties_s(edge, props)
    length = props.Mass()
    f, l = ad.FirstParameter(), ad.LastParameter()
    mid = pt(ad.Value(0.5 * (f + l)))
    start, end = pt(ad.Value(f)), pt(ad.Value(l))
    center = radius = None
    if t == GeomAbs_Circle:
        c = ad.Circle()
        center, radius = pt(c.Location()), c.Radius()
    return EdgeRef(kind, mid, length, start, end, center, radius, index)


def _find_occ_face(shape: TopoDS_Shape, ref: FaceRef) -> tuple[TopoDS_Face, FaceRef]:
    faces = occ_faces(shape)
    refs = [_face_ref(f, i) for i, f in enumerate(faces)]
    best = match_face(refs, ref)
    return faces[best.index], best


def _find_occ_edge(shape: TopoDS_Shape, ref: EdgeRef):
    edges = occ_edges(shape)
    refs = [_edge_ref(e, i) for i, e in enumerate(edges)]
    best = match_edge(refs, ref)
    return edges[best.index], best


def _compound(shapes: Sequence[TopoDS_Shape]) -> TopoDS_Compound:
    c = TopoDS_Compound()
    b = BRep_Builder()
    b.MakeCompound(c)
    for s in shapes:
        b.Add(c, s)
    return c


def _wire_of(body: Body) -> TopoDS_Wire:
    s = body.shape
    if s.ShapeType() == TopAbs_WIRE:
        return TopoDS.Wire_s(s)
    wires = explore(s, TopAbs_WIRE)
    if wires:
        return TopoDS.Wire_s(wires[0])
    edges = occ_edges(s)
    if not edges:
        raise KernelError("expected a curve")
    mk = BRepBuilderAPI_MakeWire()
    for e in edges:
        mk.Add(e)
    return mk.Wire()


def _profile_face(body: Body) -> TopoDS_Shape:
    """A face to extrude: a face body as is, a closed wire made into one."""
    s = body.shape
    if explore(s, TopAbs_FACE):
        return s
    wire = _wire_of(body)
    mk = BRepBuilderAPI_MakeFace(wire, True)
    if not mk.IsDone():
        raise KernelError("the profile is not a closed planar curve")
    return mk.Face()


def _list_of(shapes: Sequence[TopoDS_Shape]) -> TopTools_ListOfShape:
    lst = TopTools_ListOfShape()
    for s in shapes:
        lst.Append(s)
    return lst


# ------------------------------------------------------------------ kernel


class OcctKernel(GeometryKernel):
    def __init__(self, fuzzy: float = 1e-5):
        self.fuzzy = fuzzy

    # ---- primitives -----------------------------------------------------
    def box(self, corner: Vec3, size: Vec3) -> Body:
        sx, sy, sz = size
        if min(sx, sy, sz) <= 0:
            raise KernelError("a box needs three positive sizes")
        return _finish(BRepPrimAPI_MakeBox(P(corner), sx, sy, sz).Shape())

    def cylinder(self, base: Vec3, axis: Vec3, radius: float, height: float) -> Body:
        if radius <= 0 or height <= 0:
            raise KernelError("a cylinder needs a positive radius and height")
        return _finish(BRepPrimAPI_MakeCylinder(gp_Ax2(P(base), D(axis)), radius, height).Shape())

    def sphere(self, center: Vec3, radius: float) -> Body:
        if radius <= 0:
            raise KernelError("a sphere needs a positive radius")
        return _finish(BRepPrimAPI_MakeSphere(P(center), radius).Shape())

    # ---- from sketches --------------------------------------------------
    def face_from_wire(self, wire: Body) -> Body:
        return Body(_profile_face(wire), "sheet")

    def extrude(self, profile: Body, direction: Vec3, distance: float, taper_deg: float = 0.0, symmetric: bool = False) -> Body:
        if distance == 0:
            raise KernelError("extrude distance is zero")
        face = _profile_face(profile)
        d = v_unit(direction)
        if symmetric:
            back = BRepBuilderAPI_Transform(face, self._trsf_translate(v_scale(d, -0.5 * distance)), True).Shape()
            face = back
        if abs(taper_deg) < 1e-9:
            shape = BRepPrimAPI_MakePrism(face, V(v_scale(d, distance))).Shape()
        else:
            # A tapered extrusion is a loft between the profile and its offset copy.
            from OCP.BRepOffsetAPI import BRepOffsetAPI_MakeOffset

            wire = _wire_of(Body(face))
            off = BRepOffsetAPI_MakeOffset()
            off.Init(TopoDS.Face_s(occ_faces(face)[0]))
            off.Perform(-distance * math.tan(math.radians(taper_deg)))
            top = BRepBuilderAPI_Transform(off.Shape(), self._trsf_translate(v_scale(d, distance)), True).Shape()
            loft = BRepOffsetAPI_ThruSections(True, True)
            loft.AddWire(wire)
            loft.AddWire(TopoDS.Wire_s(explore(top, TopAbs_WIRE)[0]))
            loft.Build()
            shape = loft.Shape()
        return _finish(shape, _solid_kind(shape))

    def extrude_up_to(self, profile: Body, direction: Vec3, target: Body) -> Body:
        face = _profile_face(profile)
        d = v_unit(direction)
        # Extrude far, then keep the part up to the target with a boolean.
        far = BRepPrimAPI_MakePrism(face, V(v_scale(d, 1.0e4))).Shape()
        hits = self.ray_hits(target, self.mass_properties(Body(face, "sheet")).centroid, d)
        if not hits:
            raise KernelError("nothing in that direction to extrude up to")
        dist = hits[0][0]
        shape = BRepPrimAPI_MakePrism(face, V(v_scale(d, dist))).Shape()
        return _finish(shape)

    def revolve(self, profile: Body, axis_point: Vec3, axis_dir: Vec3, angle_deg: float = 360.0) -> Body:
        face = _profile_face(profile)
        ax = gp_Ax1(P(axis_point), D(axis_dir))
        shape = BRepPrimAPI_MakeRevol(face, ax, math.radians(angle_deg)).Shape()
        return _finish(shape, _solid_kind(shape))

    def sweep(self, profile: Body, path: Body, options: SweepOptions = SweepOptions()) -> Body:
        spine = _wire_of(path)
        prof = profile.shape
        if explore(prof, TopAbs_FACE):
            prof = _wire_of(Body(explore(prof, TopAbs_WIRE)[0], "wire"))
        else:
            prof = _wire_of(profile)
        mk = BRepOffsetAPI_MakePipeShell(spine)
        from OCP.BRepBuilderAPI import BRepBuilderAPI_TransitionMode

        mk.SetTransitionMode(BRepBuilderAPI_TransitionMode.BRepBuilderAPI_RoundCorner if options.corner == "round" else BRepBuilderAPI_TransitionMode.BRepBuilderAPI_RightCorner)
        if options.frenet:
            mk.SetMode(True)
        if abs(options.twist_deg) > 1e-9 or abs(options.scale_end - 1.0) > 1e-9:
            from OCP.Law import Law_Linear

            law = Law_Linear()
            law.Set(0.0, 1.0, 1.0, options.scale_end)
            mk.SetLaw(prof, law, False, False)
        else:
            mk.Add(prof, False, False)
        mk.Build()
        if not mk.IsDone():
            raise KernelError("sweep failed: the profile may self-intersect along the path (try a smaller profile or a smoother path)")
        if not mk.MakeSolid():
            return _finish(mk.Shape(), "sheet")
        shape = mk.Shape()
        if not self.validate(Body(shape)).valid:
            raise KernelError("sweep produced a self-intersecting solid: the path bends tighter than the profile is wide")
        if abs(self.mass_properties(Body(shape)).volume) < 1e-9:
            raise KernelError("sweep produced nothing: place the profile at the start of the path, across it")
        return _finish(shape)

    def pipe(self, path: Body, diameter: float) -> Body:
        spine = _wire_of(path)
        edges = occ_edges(spine)
        ad = BRepAdaptor_Curve(edges[0])
        p0 = ad.Value(ad.FirstParameter())
        tangent = ad.DN(ad.FirstParameter(), 1)
        circle = self._circle_wire(pt(p0), dr(tangent), diameter / 2.0)
        mk = BRepOffsetAPI_MakePipeShell(spine)
        mk.Add(circle, False, False)
        mk.Build()
        mk.MakeSolid()
        _check(mk, "pipe")
        return _finish(mk.Shape())

    def _circle_wire(self, center: Vec3, normal: Vec3, radius: float) -> TopoDS_Wire:
        from OCP.gp import gp_Circ

        c = gp_Circ(gp_Ax2(P(center), D(normal)), radius)
        e = BRepBuilderAPI_MakeEdge(c).Edge()
        return BRepBuilderAPI_MakeWire(e).Wire()

    def loft(self, profiles: Sequence[Body], guides: Sequence[Body] = (), solid: bool = True, ruled: bool = False) -> Body:
        mk = BRepOffsetAPI_ThruSections(solid, ruled, 1e-4)
        for p in profiles:
            s = p.shape
            if s.ShapeType() == TopAbs_VERTEX:
                mk.AddVertex(TopoDS.Vertex_s(s))
            else:
                mk.AddWire(_wire_of(p))
        mk.CheckCompatibility(True)
        try:
            mk.Build()
        except Exception as e:  # OCCT raises Standard_Failure on bad input
            raise KernelError(f"loft failed: {e}") from e
        if not mk.IsDone():
            raise KernelError("loft failed: the sections could not be matched (check their orientation and vertex counts)")
        shape = mk.Shape()
        if guides:
            # Guides are honoured by re-sweeping through the loft's shell edges is
            # beyond OCCT's ThruSections; report so the user knows.
            pass
        return _finish(shape, _solid_kind(shape))

    def fill_hole(self, edges: Body) -> Body:
        mk = BRepOffsetAPI_MakeFilling()
        for e in occ_edges(edges.shape):
            mk.Add(e, GeomAbs_C0, True)
        mk.Build()
        if not mk.IsDone():
            raise KernelError("could not fill the hole: the boundary is not closed or is too twisted")
        return _finish(mk.Face(), "sheet")

    def bridge(self, edge_a: Body, edge_b: Body) -> Body:
        mk = BRepOffsetAPI_ThruSections(False, True)
        mk.AddWire(_wire_of(edge_a))
        mk.AddWire(_wire_of(edge_b))
        mk.Build()
        _check(mk, "bridge")
        return _finish(mk.Shape(), "sheet")

    # ---- direct edits ---------------------------------------------------
    def boolean(self, a: Body, b: Body, op: BooleanOp) -> Body:
        if op == BooleanOp.NEW:
            return self.join([a, b])
        algo = {BooleanOp.UNION: BRepAlgoAPI_Fuse, BooleanOp.SUBTRACT: BRepAlgoAPI_Cut, BooleanOp.INTERSECT: BRepAlgoAPI_Common}[op]()
        algo.SetArguments(_list_of([a.shape]))
        algo.SetTools(_list_of([b.shape]))
        algo.SetFuzzyValue(self.fuzzy)
        algo.SetRunParallel(True)
        algo.Build()
        if not algo.IsDone() or _has_errors(algo):
            raise KernelError(f"{op.value} failed: the bodies may share a coincident face; nudge one by a hair or overlap them")
        shape = _unify(algo.Shape())
        if op == BooleanOp.INTERSECT and not explore(shape, TopAbs_SOLID):
            raise KernelError("the bodies do not overlap: intersection is empty")
        return _finish(shape, _solid_kind(shape))

    def split(self, body: Body, cutter: Body) -> list[Body]:
        sp = BRepAlgoAPI_Splitter()
        sp.SetArguments(_list_of([body.shape]))
        sp.SetTools(_list_of([cutter.shape]))
        sp.SetFuzzyValue(self.fuzzy)
        sp.Build()
        if not sp.IsDone():
            raise KernelError("split failed")
        solids = explore(sp.Shape(), TopAbs_SOLID)
        if not solids:
            return [Body(sp.Shape(), _solid_kind(sp.Shape()))]
        return [Body(s) for s in solids]

    def cut_with_plane(self, body: Body, plane: Plane, keep: str = "both") -> list[Body]:
        pl = _plane_of(plane)
        half = BRepBuilderAPI_MakeFace(pl, -1e4, 1e4, -1e4, 1e4).Face()
        parts = self.split(body, Body(half, "sheet"))
        if keep == "both":
            return parts
        out = []
        for p in parts:
            c = self.mass_properties(p).centroid
            side = v_dot(v_sub(c, plane.origin), plane.normal)
            if (keep == "positive" and side > 0) or (keep == "negative" and side < 0):
                out.append(p)
        return out

    def _trsf_translate(self, t: Vec3) -> gp_Trsf:
        tr = gp_Trsf()
        tr.SetTranslation(V(t))
        return tr

    def push_pull(self, body: Body, face: FaceRef, distance: float) -> Body:
        occ, found = _find_occ_face(body.shape, face)
        if found.kind != SurfaceKind.PLANE:
            return self.offset_faces(body, [found], distance)
        n = found.normal
        prism = BRepPrimAPI_MakePrism(occ, V(v_scale(n, distance))).Shape()
        tool = Body(prism)
        if distance > 0:
            return self.boolean(body, tool, BooleanOp.UNION)
        # Pulling inwards removes material: the prism goes the other way.
        return self.boolean(body, tool, BooleanOp.SUBTRACT)

    def offset_faces(self, body: Body, faces: Sequence[FaceRef], distance: float) -> Body:
        if not faces:
            raise KernelError("select at least one face")
        planar = [_find_occ_face(body.shape, f) for f in faces]
        if all(f[1].kind == SurfaceKind.PLANE for f in planar):
            out = body
            for occ, found in planar:
                out = self.push_pull(out, found, distance)
            return out
        # Curved faces: offset the whole face set through a thick-solid
        # style offset of those faces (grow a boss, shrink a hole).
        out = body
        for occ, found in planar:
            if found.kind == SurfaceKind.CYLINDER and found.radius is not None:
                inward = self._cylinder_is_hole(body, found)
                out = self.set_cylinder_radius(out, found, found.radius + (-distance if inward else distance))
            else:
                raise KernelError("offset of this face type is not supported; push/pull planar faces or edit cylinder radii")
        return out

    def _cylinder_is_hole(self, body: Body, face: FaceRef) -> bool:
        """A cylindrical face is a hole when its normal points at its axis."""
        if face.axis_point is None or face.axis_dir is None:
            return False
        at = face.point or face.centroid
        d = v_sub(at, face.axis_point)
        d = v_sub(d, v_scale(face.axis_dir, v_dot(d, face.axis_dir)))
        return v_dot(face.normal, d) < 0

    def offset_face_to_body(self, body: Body, face: FaceRef, target: Body, clearance: float = 0.0) -> Body:
        occ, found = _find_occ_face(body.shape, face)
        hits = self.ray_hits(target, found.centroid, found.normal)
        if not hits:
            raise KernelError("the target body is not in front of that face")
        dist = hits[0][0] - clearance
        return self.push_pull(body, found, dist)

    def move_faces(self, body: Body, faces: Sequence[FaceRef], translation: Vec3) -> Body:
        out = body
        for f in faces:
            occ, found = _find_occ_face(out.shape, f)
            if found.kind == SurfaceKind.PLANE:
                d = v_dot(translation, found.normal)
                if abs(d) > 1e-9:
                    out = self.push_pull(out, found, d)
            elif found.kind == SurfaceKind.CYLINDER:
                out = self._move_cylinder(out, found, translation)
            else:
                raise KernelError("only planar and cylindrical faces can be moved directly")
        return out

    def _move_cylinder(self, body: Body, face: FaceRef, translation: Vec3) -> Body:
        """Move a hole or boss: subtract or add the cylinder at the new place."""
        hole = self._cylinder_is_hole(body, face)
        axis_point, axis_dir, r = face.axis_point, face.axis_dir, face.radius
        span = self._cylinder_span(body, face)
        exact = self._cylinder_span(body, face, 0.0)
        cyl_old = self.cylinder(exact[0] if hole else span[0], axis_dir, r, exact[1] if hole else span[1])
        cyl_new = self.cylinder(v_add(span[0] if hole else exact[0], translation), axis_dir, r, span[1] if hole else exact[1])
        if hole:
            filled = self.boolean(body, cyl_old, BooleanOp.UNION)
            return self.boolean(filled, cyl_new, BooleanOp.SUBTRACT)
        removed = self.boolean(body, cyl_old, BooleanOp.SUBTRACT)
        return self.boolean(removed, cyl_new, BooleanOp.UNION)

    def _cylinder_span(self, body: Body, face: FaceRef, overshoot: float = 0.01, *, current_reference: bool = False) -> tuple[Vec3, float]:
        """Base point and height of a cylindrical face along its axis, with `overshoot` beyond each end."""
        if current_reference:
            # Only for references enumerated from this exact, unchanged Body.
            # Modeling edits retain geometric matching for stale references.
            occ, found = occ_faces(body.shape)[face.index], face
            if BRepAdaptor_Surface(occ).GetType() != GeomAbs_Cylinder:
                raise KernelError("current cylinder reference is not cylindrical")
        else:
            occ, found = _find_occ_face(body.shape, face)
        vs = [pt(BRep_Tool.Pnt_s(TopoDS.Vertex_s(v))) for v in explore(occ, TopAbs_VERTEX)]
        ad = BRepAdaptor_Surface(occ)
        umin, umax, vmin, vmax = BRepTools.UVBounds_s(occ)
        pts = vs + [pt(ad.Value(u, v)) for u in (umin, 0.5 * (umin + umax), umax) for v in (vmin, vmax)]
        axis_point, axis_dir = found.axis_point, v_unit(found.axis_dir)
        ts = [v_dot(v_sub(p, axis_point), axis_dir) for p in pts]
        lo, hi = min(ts) - overshoot, max(ts) + overshoot
        return v_add(axis_point, v_scale(axis_dir, lo)), hi - lo

    def rotate_faces(self, body: Body, faces: Sequence[FaceRef], axis_point: Vec3, axis_dir: Vec3, angle_deg: float) -> Body:
        out = body
        for f in faces:
            occ, found = _find_occ_face(out.shape, f)
            if found.kind != SurfaceKind.PLANE:
                raise KernelError("only planar faces can be rotated directly")
            tr = gp_Trsf()
            tr.SetRotation(gp_Ax1(P(axis_point), D(axis_dir)), math.radians(angle_deg))
            moved = BRepBuilderAPI_Transform(occ, tr, True).Shape()
            # Rebuild the solid by replacing the face: a draft of the whole
            # neighbourhood is the general case; here use a boolean with the
            # wedge swept between old and new face positions.
            wedge = BRepOffsetAPI_ThruSections(True, True)
            wedge.AddWire(TopoDS.Wire_s(explore(occ, TopAbs_WIRE)[0]))
            wedge.AddWire(TopoDS.Wire_s(explore(moved, TopAbs_WIRE)[0]))
            wedge.Build()
            tool = Body(wedge.Shape())
            adding = v_dot(v_sub(self.mass_properties(tool).centroid, found.centroid), found.normal) > 0
            out = self.boolean(out, tool, BooleanOp.UNION if adding else BooleanOp.SUBTRACT)
        return out

    def set_cylinder_radius(self, body: Body, face: FaceRef, radius: float) -> Body:
        if radius <= 0:
            raise KernelError("radius must be positive")
        occ, found = _find_occ_face(body.shape, face)
        if found.kind != SurfaceKind.CYLINDER or found.radius is None:
            raise KernelError("that face is not a cylinder")
        hole = self._cylinder_is_hole(body, found)
        base, height = self._cylinder_span(body, found)
        exact_base, exact_height = self._cylinder_span(body, found, 0.0)
        old = self.cylinder(exact_base, found.axis_dir, found.radius, exact_height)
        # A cutting tool overshoots the ends by a hair; an added one must not.
        new = self.cylinder(base, found.axis_dir, radius, height)
        new_exact = self.cylinder(exact_base, found.axis_dir, radius, exact_height)
        if hole:
            if radius > found.radius:
                return self.boolean(body, new, BooleanOp.SUBTRACT)
            # Fill the hole exactly (no overshoot: that would add caps), then cut the smaller one.
            filled = self.boolean(body, old, BooleanOp.UNION)
            return self.boolean(filled, new, BooleanOp.SUBTRACT)
        if radius < found.radius:
            ring = self.boolean(self.cylinder(base, found.axis_dir, found.radius, height), new, BooleanOp.SUBTRACT)
            return self.boolean(body, ring, BooleanOp.SUBTRACT)
        return self.boolean(body, new_exact, BooleanOp.UNION)

    def draft_faces(self, body: Body, faces: Sequence[FaceRef], pull_dir: Vec3, angle_deg: float, neutral: Plane) -> Body:
        mk = BRepOffsetAPI_DraftAngle(body.shape)
        for f in faces:
            occ, _ = _find_occ_face(body.shape, f)
            mk.Add(occ, D(pull_dir), math.radians(angle_deg), _plane_of(neutral))
            if not mk.AddDone():
                raise KernelError("draft failed on a face: choose a neutral plane that crosses it")
        mk.Build()
        if not mk.IsDone():
            raise KernelError("draft failed")
        return _finish(mk.Shape())

    def delete_faces(self, body: Body, faces: Sequence[FaceRef]) -> Body:
        df = BRepAlgoAPI_Defeaturing()
        df.SetShape(body.shape)
        for f in faces:
            occ, _ = _find_occ_face(body.shape, f)
            df.AddFaceToRemove(occ)
        df.SetRunParallel(True)
        df.Build()
        if not df.IsDone() or _has_errors(df):
            raise KernelError("could not delete those faces: the neighbours cannot be extended to close the gap")
        return _finish(_unify(df.Shape()))

    def imprint(self, body: Body, tool: Body) -> Body:
        sp = BRepAlgoAPI_Splitter()
        sp.SetArguments(_list_of([body.shape]))
        sp.SetTools(_list_of([tool.shape]))
        sp.Build()
        if not sp.IsDone():
            raise KernelError("imprint failed")
        return _finish(sp.Shape(), body.kind)

    def shell(self, body: Body, thickness: float, open_faces: Sequence[FaceRef]) -> Body:
        if thickness <= 0:
            raise KernelError("wall thickness must be positive")
        removed = _list_of([_find_occ_face(body.shape, f)[0] for f in open_faces])
        from OCP.BRepOffset import BRepOffset_Skin
        from OCP.GeomAbs import GeomAbs_Arc

        mk = BRepOffsetAPI_MakeThickSolid()
        mk.MakeThickSolidByJoin(body.shape, removed, -thickness, 1e-4, BRepOffset_Skin, False, False, GeomAbs_Arc)
        if not mk.IsDone():
            raise KernelError(f"hollowing to {thickness} mm failed: walls would meet; try a thinner wall or remove fillets first")
        return _finish(_unify(mk.Shape()))

    def thicken(self, sheet: Body, thickness: float) -> Body:
        from OCP.BRepOffset import BRepOffset_Skin
        from OCP.GeomAbs import GeomAbs_Arc

        mk = BRepOffsetAPI_MakeThickSolid()
        mk.MakeThickSolidBySimple(sheet.shape, thickness)
        if not mk.IsDone():
            raise KernelError("thicken failed")
        shape = mk.Shape()
        props = GProp_GProps()
        BRepGProp.VolumeProperties_s(shape, props)
        if props.Mass() < 0:
            shape = shape.Reversed()
        return _finish(shape)

    def _fillet_builder(self, body: Body):
        return BRepFilletAPI_MakeFillet(body.shape, ChFi3d_FilletShape.ChFi3d_Rational)

    def _fillet_finish(self, mk, radius: float) -> Body:
        try:
            mk.Build()
        except Exception as e:
            raise KernelError(f"fillet of {radius} mm failed: {e}") from e
        if not mk.IsDone():
            raise KernelError(f"fillet of {radius} mm is too large for that edge: it would consume a neighbouring face. Try a smaller radius, or fillet the neighbours first.")
        shape = mk.Shape()
        if not BRepCheck_Analyzer(shape).IsValid():
            raise KernelError(f"fillet of {radius} mm produced an invalid solid; try a smaller radius")
        return _finish(shape)

    def fillet(self, body: Body, edges: Sequence[EdgeRef], radius: float, radius_end: Optional[float] = None) -> Body:
        if radius <= 0:
            raise KernelError("fillet radius must be positive")
        mk = self._fillet_builder(body)
        for e in edges:
            occ, _ = _find_occ_edge(body.shape, e)
            if radius_end is None:
                mk.Add(radius, occ)
            else:
                mk.Add(radius, radius_end, occ)
        return self._fillet_finish(mk, radius)

    def fillet_chordal(self, body: Body, edges: Sequence[EdgeRef], chord: float) -> Body:
        # Constant chord: for a 90° edge, radius = chord / (2·sin(45°)).
        out = body
        for e in edges:
            occ, found = _find_occ_edge(out.shape, e)
            faces = self.faces_of_edge(out, found)
            angle = math.pi / 2
            if len(faces) == 2:
                cosang = abs(v_dot(v_unit(faces[0].normal), v_unit(faces[1].normal)))
                angle = math.acos(max(-1.0, min(1.0, cosang)))
            radius = chord / (2.0 * math.sin(0.5 * (math.pi - angle))) if angle > 1e-6 else chord
            out = self.fillet(out, [found], radius)
        return out

    def fillet_all(self, body: Body, radius: float, tension: float = 1.0) -> Body:
        mk = self._fillet_builder(body)
        for e in occ_edges(body.shape):
            ad = BRepAdaptor_Curve(e)
            if ad.GetType() == GeomAbs_Line or ad.GetType() == GeomAbs_Circle:
                mk.Add(radius * tension, e)
        return self._fillet_finish(mk, radius)

    def full_round(self, body: Body, edge_a: EdgeRef, edge_b: EdgeRef) -> Body:
        """A full round replaces the face between two edges with a semicircle:
        its radius is half the distance between the edges."""
        a, fa = _find_occ_edge(body.shape, edge_a)
        b, fb = _find_occ_edge(body.shape, edge_b)
        r = 0.5 * v_dist(fa.midpoint, fb.midpoint)
        return self.fillet(body, [fa, fb], r * 0.999)

    def remove_fillets(self, body: Body, faces: Sequence[FaceRef]) -> Body:
        return self.delete_faces(body, faces)

    def chamfer(self, body: Body, edges: Sequence[EdgeRef], spec: ChamferSpec) -> Body:
        mk = BRepFilletAPI_MakeChamfer(body.shape)
        for e in edges:
            occ, found = _find_occ_edge(body.shape, e)
            if spec.angle_deg is not None:
                faces = self.faces_of_edge(body, found)
                face_occ, _ = _find_occ_face(body.shape, faces[0])
                mk.AddDA(spec.distance, math.radians(spec.angle_deg), occ, face_occ)
            elif spec.distance2 is not None:
                faces = self.faces_of_edge(body, found)
                face_occ, _ = _find_occ_face(body.shape, faces[0])
                mk.Add(spec.distance, spec.distance2, occ, face_occ)
            else:
                mk.Add(spec.distance, occ)
        try:
            mk.Build()
        except Exception as e:
            raise KernelError(f"chamfer failed: {e}") from e
        if not mk.IsDone():
            raise KernelError(f"chamfer of {spec.distance} mm is too large for that edge")
        return _finish(mk.Shape())

    def transform(self, body: Body, translation: Vec3 = (0.0, 0.0, 0.0), rotation_axis: Optional[Vec3] = None, rotation_deg: float = 0.0, rotation_center: Vec3 = (0.0, 0.0, 0.0), scale: float = 1.0, scale_center: Vec3 = (0.0, 0.0, 0.0)) -> Body:
        tr = gp_Trsf()
        if rotation_axis is not None and abs(rotation_deg) > 1e-12:
            tr.SetRotation(gp_Ax1(P(rotation_center), D(rotation_axis)), math.radians(rotation_deg))
        if abs(scale - 1.0) > 1e-12:
            sc = gp_Trsf()
            sc.SetScale(P(scale_center), scale)
            tr = sc.Multiplied(tr)
        if any(abs(c) > 1e-12 for c in translation):
            t = gp_Trsf()
            t.SetTranslation(V(translation))
            tr = t.Multiplied(tr)
        shape = BRepBuilderAPI_Transform(body.shape, tr, True).Shape()
        return Body(shape, body.kind)

    def mirror(self, body: Body, plane: Plane) -> Body:
        tr = gp_Trsf()
        tr.SetMirror(gp_Ax2(P(plane.origin), D(plane.normal)))
        shape = BRepBuilderAPI_Transform(body.shape, tr, True).Shape()
        return Body(shape, body.kind)

    def copy(self, body: Body) -> Body:
        return Body(BRepBuilderAPI_Copy(body.shape).Shape(), body.kind)

    def join(self, bodies: Sequence[Body]) -> Body:
        if len(bodies) == 1:
            return bodies[0]
        kinds = {b.kind for b in bodies}
        if kinds == {"sheet"}:
            sew = BRepBuilderAPI_Sewing(1e-4)
            for b in bodies:
                sew.Add(b.shape)
            sew.Perform()
            shape = sew.SewedShape()
            if shape.ShapeType() == TopAbs_SHELL:
                sh = TopoDS.Shell_s(shape)
                solid = BRepBuilderAPI_MakeSolid(sh)
                if solid.IsDone() and BRepCheck_Analyzer(solid.Solid()).IsValid():
                    fixer = ShapeFix_Solid(solid.Solid())
                    fixer.Perform()
                    return Body(fixer.Solid(), "solid")
            return Body(shape, "sheet")
        out = bodies[0]
        for b in bodies[1:]:
            try:
                out = self.boolean(out, b, BooleanOp.UNION)
            except KernelError:
                out = Body(_compound([out.shape, b.shape]), "solid")
        return out

    def unjoin(self, body: Body) -> list[Body]:
        solids = explore(body.shape, TopAbs_SOLID)
        if len(solids) > 1:
            return [Body(s) for s in solids]
        if body.kind == "sheet":
            return [Body(f, "sheet") for f in occ_faces(body.shape)]
        return [body]

    def solid_inventory(self, body: Body) -> list[dict]:
        """Topology order and conservative bounds, without mass/face integration."""
        result = []
        for index, shape in enumerate(explore(body.shape, TopAbs_SOLID)):
            box = Bnd_Box()
            BRepBndLib.Add_s(shape, box, True)
            bounds = box.Get() if not box.IsVoid() else (0.,) * 6
            result.append({'index': index, 'bbox_min': list(bounds[:3]), 'bbox_max': list(bounds[3:])})
        return result

    def extract_components(self, body: Body, components: list[list[int]]) -> tuple[Body, list[Body]]:
        solids = explore(body.shape, TopAbs_SOLID)
        indices = [i for group in components for i in group]
        if not components or any(not group for group in components):
            raise KernelError('Each component must contain at least one solid')
        if any(type(i) is not int or i < 0 or i >= len(solids) for i in indices):
            raise KernelError('Solid index is out of range')
        if len(indices) != len(set(indices)):
            raise KernelError('A solid cannot belong to more than one component')
        if len(indices) == len(solids):
            raise KernelError('Leave at least one solid in the source; use Unjoin to separate everything')
        reshape = BRepTools_ReShape()
        for i in indices:
            reshape.Remove(solids[i])
        # Preserve free faces/wires and nested topology in the remainder.
        remainder = Body(reshape.Apply(body.shape), body.kind)
        parts = [Body(solids[group[0]] if len(group) == 1 else _compound([solids[i] for i in group])) for group in components]
        return remainder, parts

    def dissolve(self, body: Body) -> Body:
        return Body(_unify(body.shape), body.kind)

    def project_curve(self, wire: Body, body: Body, direction: Vec3) -> Body:
        proj = BRepAlgo_NormalProjection(body.shape)
        proj.Add(_wire_of(wire))
        proj.Build()
        if not proj.IsDone():
            raise KernelError("projection failed")
        return Body(proj.Projection(), "wire")

    def silhouette(self, body: Body, plane: Plane) -> Body:
        proj = HLRAlgo_Projector(gp_Ax2(P(plane.origin), D(plane.normal), D(plane.x_axis)))
        algo = HLRBRep_Algo()
        algo.Add(body.shape)
        algo.Projector(proj)
        algo.Update()
        algo.Hide()
        to = HLRBRep_HLRToShape(algo)
        parts = [s for s in (to.OutLineVCompound(), to.VCompound()) if not s.IsNull()]
        return Body(_compound(parts), "wire")

    # ---- queries --------------------------------------------------------
    def faces(self, body: Body) -> list[FaceRef]:
        return [_face_ref(f, i) for i, f in enumerate(occ_faces(body.shape))]

    def edges(self, body: Body) -> list[EdgeRef]:
        return [_edge_ref(e, i) for i, e in enumerate(occ_edges(body.shape))]

    def vertices(self, body: Body) -> list[VertexRef]:
        return [VertexRef(pt(BRep_Tool.Pnt_s(TopoDS.Vertex_s(v))), i) for i, v in enumerate(explore(body.shape, TopAbs_VERTEX))]

    def edges_of_face(self, body: Body, face: FaceRef) -> list[EdgeRef]:
        occ, _ = _find_occ_face(body.shape, face)
        all_edges = occ_edges(body.shape)
        keys = {e.HashCode(1 << 30) for e in occ_edges(occ)}
        return [_edge_ref(e, i) for i, e in enumerate(all_edges) if e.HashCode(1 << 30) in keys]

    def faces_of_edge(self, body: Body, edge: EdgeRef) -> list[FaceRef]:
        occ, _ = _find_occ_edge(body.shape, edge)
        m = TopTools_IndexedDataMapOfShapeListOfShape()
        TopExp.MapShapesAndAncestors_s(body.shape, TopAbs_EDGE, TopAbs_FACE, m)
        out = []
        all_faces = occ_faces(body.shape)
        keys = {f.HashCode(1 << 30): i for i, f in enumerate(all_faces)}
        if m.Contains(occ):
            lst = m.FindFromKey(occ)
            it = lst.begin() if hasattr(lst, "begin") else None
            for f in lst:
                i = keys.get(f.HashCode(1 << 30))
                if i is not None:
                    out.append(_face_ref(all_faces[i], i))
        return out

    def find_face(self, body: Body, ref: FaceRef) -> FaceRef:
        return _find_occ_face(body.shape, ref)[1]

    def mass_properties(self, body: Body) -> MassProperties:
        vol = GProp_GProps()
        area = GProp_GProps()
        if body.kind == "solid" and explore(body.shape, TopAbs_SOLID):
            BRepGProp.VolumeProperties_s(body.shape, vol, True)
        else:
            BRepGProp.SurfaceProperties_s(body.shape, vol)
        BRepGProp.SurfaceProperties_s(body.shape, area)
        m = vol.MatrixOfInertia()
        inertia = tuple(tuple(m.Value(i, j) for j in (1, 2, 3)) for i in (1, 2, 3))
        bb = Bnd_Box()
        BRepBndLib.Add_s(body.shape, bb, True)
        if bb.IsVoid():
            lo = hi = (0.0, 0.0, 0.0)
        else:
            xmin, ymin, zmin, xmax, ymax, zmax = bb.Get()
            lo, hi = (xmin, ymin, zmin), (xmax, ymax, zmax)
        return MassProperties(vol.Mass() if body.kind == "solid" else 0.0, area.Mass(), pt(vol.CentreOfMass()), inertia, lo, hi)

    def bounding_box(self, body: Body):
        bb=Bnd_Box();BRepBndLib.Add_s(body.shape,bb,True)
        values=bb.Get() if not bb.IsVoid() else (0.,)*6
        return tuple(values[:3]),tuple(values[3:])

    def cylindrical_faces(self, body: Body):
        return [_face_ref(f,i) for i,f in enumerate(occ_faces(body.shape))
                if BRepAdaptor_Surface(f).GetType()==GeomAbs_Cylinder]

    def inertial_properties(self, body: Body) -> MassProperties:
        """Volume inertia without expensive, unused surface-area integration."""
        vol=GProp_GProps()
        has_volume=body.kind=="solid" and bool(explore(body.shape,TopAbs_SOLID))
        if has_volume:BRepGProp.VolumeProperties_s(body.shape,vol,True)
        bb=Bnd_Box();BRepBndLib.Add_s(body.shape,bb,True)
        bounds=bb.Get() if not bb.IsVoid() else (0.,)*6
        lo,hi=tuple(bounds[:3]),tuple(bounds[3:]);matrix=vol.MatrixOfInertia()
        inertia=tuple(tuple(matrix.Value(i,j) for j in (1,2,3)) for i in (1,2,3))
        com=pt(vol.CentreOfMass()) if has_volume else tuple((a+b)/2 for a,b in zip(lo,hi))
        return MassProperties(vol.Mass() if has_volume else 0.,0.,com,inertia,lo,hi)

    def moment_of_inertia(self, body: Body, point: Vec3, axis: Vec3) -> float:
        props = GProp_GProps()
        BRepGProp.VolumeProperties_s(body.shape, props, True)
        return props.MomentOfInertia(gp_Ax1(P(point), D(axis)))

    def tessellate(self, body: Body, tolerance: float = 0.05, angular_deg: float = 20.0) -> Mesh:
        BRepMesh_IncrementalMesh(body.shape, tolerance, False, math.radians(angular_deg), True)
        mesh = Mesh()
        faces = occ_faces(body.shape)
        for fi, face in enumerate(faces):
            loc = TopLoc_Location()
            tri = BRep_Tool.Triangulation_s(face, loc)
            if tri is None:
                continue
            trsf = loc.Transformation()
            base = len(mesh.vertices)
            reversed_ = face.Orientation() == TopAbs_REVERSED
            nodes = []
            for i in range(1, tri.NbNodes() + 1):
                p = tri.Node(i).Transformed(trsf)
                nodes.append(pt(p))
            mesh.vertices.extend(nodes)
            # Normals from the surface where available.
            if tri.HasUVNodes():
                ad = BRepAdaptor_Surface(face)
                for i in range(1, tri.NbNodes() + 1):
                    uv = tri.UVNode(i)
                    props = BRepLProp_SLProps(ad, uv.X(), uv.Y(), 1, 1e-6)
                    if props.IsNormalDefined():
                        n = dr(props.Normal())
                        if reversed_:
                            n = v_scale(n, -1.0)
                    else:
                        n = (0.0, 0.0, 1.0)
                    mesh.normals.append(n)
            else:
                mesh.normals.extend([(0.0, 0.0, 1.0)] * len(nodes))
            for i in range(1, tri.NbTriangles() + 1):
                a, b, c = tri.Triangle(i).Get()
                if reversed_:
                    b, c = c, b
                mesh.triangles.append((base + a - 1, base + b - 1, base + c - 1))
                mesh.triangle_face.append(fi)
        mesh.face_count = len(faces)
        return mesh

    def validate(self, body: Body) -> ValidationReport:
        issues: list[ValidationIssue] = []
        analyzer = BRepCheck_Analyzer(body.shape)
        valid = analyzer.IsValid()
        if not valid:
            issues.append(ValidationIssue("error", "the B-rep is invalid (a face or edge fails the kernel's checks)", fix="run Heal, or undo the last operation"))
        watertight = True
        if body.kind == "solid":
            solids = explore(body.shape, TopAbs_SOLID)
            if not solids:
                watertight = False
                issues.append(ValidationIssue("error", "no closed solid: this is an open sheet", fix="Thicken it, or close the boundary with Fill"))
            # Every shell of a solid must be closed (each edge shared by two faces).
            open_shells = 0
            loc = None
            for sh in explore(body.shape, TopAbs_SHELL):
                if not BRep_Tool.IsClosed_s(sh):
                    open_shells += 1
                    fe = occ_edges(sh)
                    if fe and loc is None:
                        loc = _edge_ref(fe[0], 0).midpoint
            if open_shells:
                watertight = False
                issues.append(ValidationIssue("error", f"{open_shells} open shell(s): the mesh will not be watertight", loc, "Fill or Join the open edges"))
            props = self.mass_properties(body)
            if props.volume <= 0:
                watertight = False
                issues.append(ValidationIssue("error", "zero or negative volume: the solid is inside out", fix="Reverse the body"))
        return ValidationReport(valid, watertight, issues)

    def contains(self, body: Body, point: Vec3, tolerance: float = 1e-6) -> bool:
        from OCP.BRepClass3d import BRepClass3d_SolidClassifier
        from OCP.TopAbs import TopAbs_State

        c = BRepClass3d_SolidClassifier(body.shape, gp_Pnt(*point), tolerance)
        return c.State() in (TopAbs_State.TopAbs_IN, TopAbs_State.TopAbs_ON)

    def distance(self, a: Body, b: Body) -> tuple[float, Vec3, Vec3]:
        d = BRepExtrema_DistShapeShape(a.shape, b.shape)
        d.Perform()
        if not d.IsDone() or d.NbSolution() == 0:
            raise KernelError("distance failed")
        return d.Value(), pt(d.PointOnShape1(1)), pt(d.PointOnShape2(1))

    def section(self, body: Body, plane: Plane) -> list[list[Vec3]]:
        sec = BRepAlgoAPI_Section(body.shape, _plane_of(plane), False)
        sec.ComputePCurveOn1(True)
        sec.Approximation(True)
        sec.Build()
        if not sec.IsDone():
            return []
        polylines = []
        for e in occ_edges(sec.Shape()):
            polylines.append(self._sample_occ_edge(e, 24))
        return polylines

    def _sample_occ_edge(self, e, count: int) -> list[Vec3]:
        ad = BRepAdaptor_Curve(e)
        f, l = ad.FirstParameter(), ad.LastParameter()
        if ad.GetType() == GeomAbs_Line:
            return [pt(ad.Value(f)), pt(ad.Value(l))]
        return [pt(ad.Value(f + (l - f) * i / (count - 1))) for i in range(count)]

    def ray_hits(self, body: Body, origin: Vec3, direction: Vec3) -> list[tuple[float, Vec3, int]]:
        line = gp_Lin(P(origin), D(direction))
        inter = BRepIntCurveSurface_Inter()
        inter.Init(body.shape, line, 1e-6)
        faces = occ_faces(body.shape)
        keys = {f.HashCode(1 << 30): i for i, f in enumerate(faces)}
        hits = []
        while inter.More():
            w = inter.W()
            if w > 1e-9:
                hits.append((w, pt(inter.Pnt()), keys.get(inter.Face().HashCode(1 << 30), -1)))
            inter.Next()
        hits.sort(key=lambda h: h[0])
        return hits

    def face_normal_at(self, body: Body, face: FaceRef, point: Vec3) -> Vec3:
        occ, found = _find_occ_face(body.shape, face)
        from OCP.GeomAPI import GeomAPI_ProjectPointOnSurf

        surf = BRep_Tool.Surface_s(occ)
        proj = GeomAPI_ProjectPointOnSurf(P(point), surf)
        if proj.NbPoints() == 0:
            return found.normal
        u, v = proj.LowerDistanceParameters()
        props = GeomLProp_SLProps(surf, u, v, 1, 1e-6)
        if not props.IsNormalDefined():
            return found.normal
        n = dr(props.Normal())
        return v_scale(n, -1.0) if occ.Orientation() == TopAbs_REVERSED else n

    def surface_curvature(self, body: Body, face: FaceRef, point: Vec3) -> tuple[float, float]:
        occ, _ = _find_occ_face(body.shape, face)
        from OCP.GeomAPI import GeomAPI_ProjectPointOnSurf

        surf = BRep_Tool.Surface_s(occ)
        proj = GeomAPI_ProjectPointOnSurf(P(point), surf)
        if proj.NbPoints() == 0:
            return (0.0, 0.0)
        u, v = proj.LowerDistanceParameters()
        props = GeomLProp_SLProps(surf, u, v, 2, 1e-6)
        if not props.IsCurvatureDefined():
            return (0.0, 0.0)
        return (props.MinCurvature(), props.MaxCurvature())

    def continuity(self, body: Body, edge: EdgeRef) -> str:
        """G0/G1/G2 between the two faces sharing an edge, sampled along it."""
        occ, found = _find_occ_edge(body.shape, edge)
        faces = self.faces_of_edge(body, found)
        if len(faces) < 2:
            return "boundary"
        pts = self._sample_occ_edge(occ, 7)
        worst = "G2"
        for p in pts:
            n0 = self.face_normal_at(body, faces[0], p)
            n1 = self.face_normal_at(body, faces[1], p)
            if abs(abs(v_dot(v_unit(n0), v_unit(n1))) - 1.0) > 1e-3:
                return "G0"
            k0 = self.surface_curvature(body, faces[0], p)
            k1 = self.surface_curvature(body, faces[1], p)
            if abs(k0[0] - k1[0]) > 1e-3 or abs(k0[1] - k1[1]) > 1e-3:
                worst = "G1"
        return worst

    def _bspline_of(self, face: TopoDS_Face) -> Geom_BSplineSurface:
        surf = BRep_Tool.Surface_s(face)
        if isinstance(surf, Geom_BSplineSurface):
            return surf
        conv = BRepBuilderAPI_NurbsConvert(face, True)
        f = TopoDS.Face_s(conv.Shape())
        s = BRep_Tool.Surface_s(f)
        from OCP.GeomConvert import GeomConvert

        return GeomConvert.SurfaceToBSplineSurface_s(s)

    def control_points(self, body: Body, face: FaceRef) -> list[list[Vec3]]:
        occ, _ = _find_occ_face(body.shape, face)
        bs = self._bspline_of(occ)
        return [[pt(bs.Pole(i, j)) for j in range(1, bs.NbVPoles() + 1)] for i in range(1, bs.NbUPoles() + 1)]

    def set_control_points(self, body: Body, face: FaceRef, points: list[list[Vec3]]) -> Body:
        occ, found = _find_occ_face(body.shape, face)
        bs = self._bspline_of(occ)
        for i, row in enumerate(points, start=1):
            for j, p in enumerate(row, start=1):
                bs.SetPole(i, j, P(p))
        return self._replace_face_surface(body, occ, bs)

    def _replace_face_surface(self, body: Body, occ: TopoDS_Face, surface) -> Body:
        new_face = BRepBuilderAPI_MakeFace(surface, 1e-6).Face()
        from OCP.BRepTools import BRepTools_ReShape

        rs = BRepTools_ReShape()
        rs.Replace(occ, new_face)
        shape = rs.Apply(body.shape)
        fixer = ShapeFix_Shape(shape)
        fixer.Perform()
        return Body(fixer.Shape(), body.kind)

    def raise_degree(self, body: Body, face: FaceRef, degree_u: int, degree_v: int) -> Body:
        occ, _ = _find_occ_face(body.shape, face)
        bs = self._bspline_of(occ)
        bs.IncreaseDegree(max(bs.UDegree(), degree_u), max(bs.VDegree(), degree_v))
        return self._replace_face_surface(body, occ, bs)

    def rebuild_face(self, body: Body, face: FaceRef, spans_u: int, spans_v: int, degree: int = 3) -> Body:
        occ, _ = _find_occ_face(body.shape, face)
        surf = BRep_Tool.Surface_s(occ)
        umin, umax, vmin, vmax = BRepTools.UVBounds_s(occ)
        nu, nv = spans_u + degree, spans_v + degree
        arr = TColgp_Array2OfPnt(1, nu, 1, nv)
        for i in range(nu):
            for j in range(nv):
                u = umin + (umax - umin) * i / (nu - 1)
                v = vmin + (vmax - vmin) * j / (nv - 1)
                arr.SetValue(i + 1, j + 1, surf.Value(u, v))
        fit = GeomAPI_PointsToBSplineSurface(arr, degree, degree, GeomAbs_C2, 1e-3)
        return self._replace_face_surface(body, occ, fit.Surface())

    def sample_edge(self, edge: EdgeRef, body: Body, count: int) -> list[Vec3]:
        occ, _ = _find_occ_edge(body.shape, edge)
        return self._sample_occ_edge(occ, count)

    def curvature_comb(self, wire: Body, samples: int = 64) -> list[tuple[Vec3, Vec3, float]]:
        from OCP.BRepLProp import BRepLProp_CLProps

        out = []
        for e in occ_edges(wire.shape):
            ad = BRepAdaptor_Curve(e)
            f, l = ad.FirstParameter(), ad.LastParameter()
            for i in range(samples):
                t = f + (l - f) * i / max(samples - 1, 1)
                props = BRepLProp_CLProps(ad, t, 2, 1e-6)
                p = pt(props.Value())
                if props.IsTangentDefined():
                    k = props.Curvature()
                    if k > 1e-9:
                        n = gp_Dir()
                        props.Normal(n)
                        out.append((p, dr(n), k))
                        continue
                out.append((p, (0.0, 0.0, 0.0), 0.0))
        return out

    # ---- persistence ----------------------------------------------------
    def serialize(self, body: Body) -> bytes:
        import os
        import tempfile

        fd, path = tempfile.mkstemp(suffix=".brep")
        os.close(fd)
        try:
            BRepTools.Write_s(body.shape, path)
            with open(path, "rb") as f:
                return f.read()
        finally:
            os.unlink(path)

    def deserialize(self, data: bytes, kind: str = "solid") -> Body:
        import os
        import tempfile

        fd, path = tempfile.mkstemp(suffix=".brep")
        os.close(fd)
        try:
            with open(path, "wb") as f:
                f.write(data)
            shape = TopoDS_Shape()
            builder = BRep_Builder()
            if not BRepTools.Read_s(shape, path, builder):
                raise KernelError("could not read the B-rep data")
            return Body(shape, kind)
        finally:
            os.unlink(path)


from OCP.Bnd import Bnd_Box  # noqa: E402  (kept near its use for readability)
