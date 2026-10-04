// Modelling operations of the in-process CAD editor, ported from RoboCAD's
// kernel/occt.py (OcctKernel). One entry point, `sim_cad_op`: an operation
// code, its input B-reps (BRepTools text), numeric arguments and integer
// arguments (face/edge indices in TopExp::MapShapes order, RoboCAD's
// `occ_faces`/`occ_edges`); every result shape is handed back with its kind
// (0 solid, 1 sheet, 2 wire). `sim_cad_measure` answers numeric queries.
// Every native handle lives and dies inside one call on the caller's thread.
#include <Standard_Version.hxx>
#include <BRepTools.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <BRepGProp.hxx>
#include <GProp_GProps.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRepLProp_CLProps.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_NurbsConvert.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BRepAlgoAPI_Defeaturing.hxx>
#include <BRepAlgo_NormalProjection.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepOffsetAPI_MakeFilling.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BRepIntCurveSurface_Inter.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepTools_ReShape.hxx>
#include <BRepLib.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <ShapeFix_Solid.hxx>
#include <ShapeFix_Shape.hxx>
#include <HLRBRep_Algo.hxx>
#include <HLRBRep_HLRToShape.hxx>
#include <HLRAlgo_Projector.hxx>
#include <GeomAPI_Interpolate.hxx>
#include <GeomAPI_PointsToBSplineSurface.hxx>
#include <GeomConvert.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_Surface.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <Law_Linear.hxx>
#include <TColgp_HArray1OfPnt.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <TColgp_Array2OfPnt.hxx>
#include <TColStd_Array1OfReal.hxx>
#include <TColStd_Array1OfInteger.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <Standard_Failure.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>
#include <gp_Lin.hxx>
#include <gp_Pln.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <cmath>
#include <cstring>
#include <cstdint>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <algorithm>

namespace {
typedef void (*ShapeOut)(void*, int32_t, const unsigned char*, size_t);
typedef void (*NumbersOut)(void*, const double*, size_t);

struct Fail : std::runtime_error { using std::runtime_error::runtime_error; };
void need(bool ok, const char* why) { if (!ok) throw Fail(why); }

TopoDS_Shape read(const unsigned char* bytes, size_t size) {
  std::string payload(reinterpret_cast<const char*>(bytes), size);
  std::istringstream stream(payload);
  TopoDS_Shape shape; BRep_Builder builder; BRepTools::Read(shape, stream, builder);
  need(!shape.IsNull() && !stream.bad(), "could not read the B-rep data");
  return shape;
}
bool has(const TopoDS_Shape& s, TopAbs_ShapeEnum k) { return TopExp_Explorer(s, k).More(); }
int32_t kind_of(const TopoDS_Shape& s) { return has(s, TopAbs_SOLID) ? 0 : has(s, TopAbs_FACE) ? 1 : 2; }

// The numeric argument reader: args are consumed in order.
struct Args {
  const double* a; size_t n; size_t at = 0;
  double num() { need(at < n, "missing numeric argument"); return a[at++]; }
  gp_Pnt pnt() { double x = num(), y = num(), z = num(); return gp_Pnt(x, y, z); }
  gp_Vec vec() { double x = num(), y = num(), z = num(); return gp_Vec(x, y, z); }
  gp_Dir dir() { gp_Vec v = vec(); need(v.Magnitude() > 1e-12, "a direction must be nonzero"); return gp_Dir(v); }
  bool more() const { return at < n; }
};
struct Ints {
  const int32_t* a; size_t n; size_t at = 0;
  int32_t next() { need(at < n, "missing index argument"); return a[at++]; }
  bool more() const { return at < n; }
};

std::vector<TopoDS_Shape> faces(const TopoDS_Shape& s) { TopTools_IndexedMapOfShape m; TopExp::MapShapes(s, TopAbs_FACE, m); std::vector<TopoDS_Shape> v; for (int i = 1; i <= m.Extent(); i++) v.push_back(m(i)); return v; }
std::vector<TopoDS_Shape> edges(const TopoDS_Shape& s) { TopTools_IndexedMapOfShape m; TopExp::MapShapes(s, TopAbs_EDGE, m); std::vector<TopoDS_Shape> v; for (int i = 1; i <= m.Extent(); i++) v.push_back(m(i)); return v; }
std::vector<TopoDS_Shape> solids(const TopoDS_Shape& s) { TopTools_IndexedMapOfShape m; TopExp::MapShapes(s, TopAbs_SOLID, m); std::vector<TopoDS_Shape> v; for (int i = 1; i <= m.Extent(); i++) v.push_back(m(i)); return v; }
TopoDS_Face face_at(const TopoDS_Shape& s, int32_t i) { auto f = faces(s); need(i >= 0 && i < (int32_t)f.size(), "face does not exist"); return TopoDS::Face(f[i]); }
TopoDS_Edge edge_at(const TopoDS_Shape& s, int32_t i) { auto e = edges(s); need(i >= 0 && i < (int32_t)e.size(), "edge does not exist"); return TopoDS::Edge(e[i]); }

TopoDS_Compound compound(const std::vector<TopoDS_Shape>& shapes) { TopoDS_Compound c; BRep_Builder b; b.MakeCompound(c); for (auto& s : shapes) b.Add(c, s); return c; }
TopTools_ListOfShape list(const std::vector<TopoDS_Shape>& shapes) { TopTools_ListOfShape l; for (auto& s : shapes) l.Append(s); return l; }

TopoDS_Shape unify(const TopoDS_Shape& s) { ShapeUpgrade_UnifySameDomain u(s, true, true, true); u.Build(); return u.Shape(); }

double volume(const TopoDS_Shape& s) { GProp_GProps p; BRepGProp::VolumeProperties(s, p); return p.Mass(); }
gp_Pnt centroid(const TopoDS_Shape& s) {
  GProp_GProps p;
  if (has(s, TopAbs_SOLID)) BRepGProp::VolumeProperties(s, p); else if (has(s, TopAbs_FACE)) BRepGProp::SurfaceProperties(s, p); else BRepGProp::LinearProperties(s, p);
  return p.CentreOfMass();
}

// A face as RoboCAD's `_face_ref` describes it.
struct FaceInfo { GeomAbs_SurfaceType type; gp_Pnt centroid, point; gp_Vec normal; double area = 0, radius = 0; gp_Pnt axis_point; gp_Dir axis_dir; bool axis = false; };
FaceInfo info(const TopoDS_Face& f) {
  FaceInfo r; GProp_GProps p; BRepGProp::SurfaceProperties(f, p); r.area = p.Mass(); r.centroid = p.CentreOfMass();
  BRepAdaptor_Surface ad(f); r.type = ad.GetType();
  if (r.type == GeomAbs_Cylinder) { gp_Cylinder c = ad.Cylinder(); r.axis_point = c.Location(); r.axis_dir = c.Axis().Direction(); r.radius = c.Radius(); r.axis = true; }
  else if (r.type == GeomAbs_Cone) { gp_Cone c = ad.Cone(); r.axis_point = c.Location(); r.axis_dir = c.Axis().Direction(); r.radius = c.RefRadius(); r.axis = true; }
  double u0, u1, v0, v1; BRepTools::UVBounds(f, u0, u1, v0, v1);
  BRepLProp_SLProps props(ad, 0.5 * (u0 + u1), 0.5 * (v0 + v1), 1, 1e-6); r.point = props.Value();
  if (props.IsNormalDefined()) { gp_Dir n = props.Normal(); r.normal = gp_Vec(n); if (f.Orientation() == TopAbs_REVERSED) r.normal.Reverse(); } else r.normal = gp_Vec(0, 0, 1);
  return r;
}

TopoDS_Shape boolean(int op, const TopoDS_Shape& a, const TopoDS_Shape& b) {
  BRepAlgoAPI_Fuse fuse; BRepAlgoAPI_Cut cut; BRepAlgoAPI_Common common;
  BRepAlgoAPI_BooleanOperation* algo = op == 0 ? static_cast<BRepAlgoAPI_BooleanOperation*>(&fuse) : op == 1 ? static_cast<BRepAlgoAPI_BooleanOperation*>(&cut) : static_cast<BRepAlgoAPI_BooleanOperation*>(&common);
  algo->SetArguments(list({a})); algo->SetTools(list({b})); algo->SetFuzzyValue(1e-5); algo->Build();
  need(algo->IsDone() && !algo->HasErrors(), "the boolean failed: the bodies may share a coincident face; nudge one by a hair or overlap them");
  TopoDS_Shape out = unify(algo->Shape());
  if (op == 2) need(has(out, TopAbs_SOLID), "the bodies do not overlap: intersection is empty");
  return out;
}

TopoDS_Wire wire_of(const TopoDS_Shape& s) {
  if (s.ShapeType() == TopAbs_WIRE) return TopoDS::Wire(s);
  TopExp_Explorer w(s, TopAbs_WIRE); if (w.More()) return TopoDS::Wire(w.Current());
  BRepBuilderAPI_MakeWire mk; bool any = false;
  for (auto& e : edges(s)) { mk.Add(TopoDS::Edge(e)); any = true; }
  need(any && mk.IsDone(), "expected a curve");
  return mk.Wire();
}
TopoDS_Shape profile_face(const TopoDS_Shape& s) {
  if (has(s, TopAbs_FACE)) return s;
  BRepBuilderAPI_MakeFace mk(wire_of(s), true);
  need(mk.IsDone(), "the profile is not a closed planar curve");
  return mk.Face();
}
TopoDS_Shape translate(const TopoDS_Shape& s, const gp_Vec& v) { gp_Trsf t; t.SetTranslation(v); return BRepBuilderAPI_Transform(s, t, true).Shape(); }

// RoboCAD's `_cylinder_is_hole`: the face normal points at its axis.
bool is_hole(const FaceInfo& f) {
  if (!f.axis) return false;
  gp_Vec d(f.axis_point, f.point); gp_Vec ax(f.axis_dir); d -= ax * d.Dot(ax);
  return f.normal.Dot(d) < 0;
}
// RoboCAD's `_cylinder_span`: base and height along the axis, overshooting the ends.
std::pair<gp_Pnt, double> span(const TopoDS_Face& face, const FaceInfo& f, double over) {
  std::vector<gp_Pnt> pts;
  for (TopExp_Explorer v(face, TopAbs_VERTEX); v.More(); v.Next()) pts.push_back(BRep_Tool::Pnt(TopoDS::Vertex(v.Current())));
  BRepAdaptor_Surface ad(face); double u0, u1, v0, v1; BRepTools::UVBounds(face, u0, u1, v0, v1);
  for (double u : {u0, 0.5 * (u0 + u1), u1}) for (double v : {v0, v1}) pts.push_back(ad.Value(u, v));
  gp_Vec ax(f.axis_dir); double lo = 1e300, hi = -1e300;
  for (auto& p : pts) { double t = gp_Vec(f.axis_point, p).Dot(ax); lo = std::min(lo, t); hi = std::max(hi, t); }
  lo -= over; hi += over;
  return {f.axis_point.Translated(ax * lo), hi - lo};
}
TopoDS_Shape cylinder(const gp_Pnt& base, const gp_Dir& axis, double r, double h) { need(r > 0 && h > 0, "a cylinder needs a positive radius and height"); return BRepPrimAPI_MakeCylinder(gp_Ax2(base, axis), r, h).Shape(); }

TopoDS_Shape set_radius(const TopoDS_Shape& body, int32_t index, double radius) {
  need(radius > 0, "radius must be positive");
  TopoDS_Face face = face_at(body, index); FaceInfo f = info(face);
  need(f.type == GeomAbs_Cylinder, "that face is not a cylinder");
  bool hole = is_hole(f);
  auto s = span(face, f, 0.01); auto e = span(face, f, 0.0);
  TopoDS_Shape old = cylinder(e.first, f.axis_dir, f.radius, e.second);
  TopoDS_Shape fresh = cylinder(s.first, f.axis_dir, radius, s.second);
  TopoDS_Shape exact = cylinder(e.first, f.axis_dir, radius, e.second);
  if (hole) {
    if (radius > f.radius) return boolean(1, body, fresh);
    return boolean(1, boolean(0, body, old), fresh);
  }
  if (radius < f.radius) return boolean(1, body, boolean(1, cylinder(s.first, f.axis_dir, f.radius, s.second), fresh));
  return boolean(0, body, exact);
}

TopoDS_Shape push_pull(const TopoDS_Shape& body, int32_t index, double distance);
TopoDS_Shape offset_faces(const TopoDS_Shape& body, const std::vector<int32_t>& idx, double distance) {
  need(!idx.empty(), "select at least one face");
  std::vector<FaceInfo> infos; for (auto i : idx) infos.push_back(info(face_at(body, i)));
  bool planar = std::all_of(infos.begin(), infos.end(), [](const FaceInfo& f) { return f.type == GeomAbs_Plane; });
  TopoDS_Shape out = body;
  if (planar) {
    // Indices shift after each edit: find each face again by its centroid and normal.
    for (auto& want : infos) {
      auto fs = faces(out); int best = -1; double bd = 1e300;
      for (size_t k = 0; k < fs.size(); k++) { FaceInfo g = info(TopoDS::Face(fs[k])); if (g.type != GeomAbs_Plane || g.normal.Dot(want.normal) < 0.999) continue; double d = g.centroid.Distance(want.centroid); if (d < bd) { bd = d; best = (int)k; } }
      need(best >= 0, "a face to offset could not be found again after the previous one moved");
      out = push_pull(out, best, distance);
    }
    return out;
  }
  for (size_t k = 0; k < idx.size(); k++) {
    const FaceInfo& f = infos[k];
    need(f.type == GeomAbs_Cylinder, "offset of this face type is not supported; push/pull planar faces or edit cylinder radii");
    // Find it again (indices shift), then grow a boss or shrink a hole.
    auto fs = faces(out); int best = -1; double bd = 1e300;
    for (size_t j = 0; j < fs.size(); j++) { FaceInfo g = info(TopoDS::Face(fs[j])); if (g.type != GeomAbs_Cylinder || std::fabs(g.radius - f.radius) > 1e-6) continue; double d = g.axis_point.Distance(f.axis_point) + gp_Vec(g.axis_dir).Crossed(gp_Vec(f.axis_dir)).Magnitude(); if (d < bd) { bd = d; best = (int)j; } }
    need(best >= 0, "a cylinder to offset could not be found again");
    out = set_radius(out, best, f.radius + (is_hole(f) ? -distance : distance));
  }
  return out;
}
TopoDS_Shape push_pull(const TopoDS_Shape& body, int32_t index, double distance) {
  need(std::fabs(distance) > 1e-12, "push/pull distance is zero");
  TopoDS_Face face = face_at(body, index); FaceInfo f = info(face);
  if (f.type != GeomAbs_Plane) return offset_faces(body, {index}, distance);
  gp_Vec n = f.normal.Normalized();
  TopoDS_Shape prism = BRepPrimAPI_MakePrism(face, n * distance).Shape();
  return boolean(distance > 0 ? 0 : 1, body, prism);
}

TopoDS_Shape move_cylinder(const TopoDS_Shape& body, int32_t index, const gp_Vec& t) {
  TopoDS_Face face = face_at(body, index); FaceInfo f = info(face); bool hole = is_hole(f);
  auto s = span(face, f, 0.01); auto e = span(face, f, 0.0);
  TopoDS_Shape old = cylinder(hole ? e.first : s.first, f.axis_dir, f.radius, hole ? e.second : s.second);
  TopoDS_Shape fresh = cylinder((hole ? s.first : e.first).Translated(t), f.axis_dir, f.radius, hole ? s.second : e.second);
  if (hole) return boolean(1, boolean(0, body, old), fresh);
  return boolean(0, boolean(1, body, old), fresh);
}

TopoDS_Shape fillet_finish(BRepFilletAPI_MakeFillet& mk, double radius) {
  try { mk.Build(); } catch (const Standard_Failure&) {}
  need(mk.IsDone(), "the fillet is too large for that edge: it would consume a neighbouring face. Try a smaller radius, or fillet the neighbours first.");
  TopoDS_Shape s = mk.Shape(); (void)radius;
  need(BRepCheck_Analyzer(s).IsValid(), "the fillet produced an invalid solid; try a smaller radius");
  return s;
}

// Faces sharing an edge (their indices).
std::vector<int> faces_of_edge(const TopoDS_Shape& body, const TopoDS_Edge& e) {
  TopTools_IndexedDataMapOfShapeListOfShape m; TopExp::MapShapesAndAncestors(body, TopAbs_EDGE, TopAbs_FACE, m);
  TopTools_IndexedMapOfShape fm; TopExp::MapShapes(body, TopAbs_FACE, fm);
  std::vector<int> out;
  if (m.Contains(e)) for (TopTools_ListIteratorOfListOfShape it(m.FindFromKey(e)); it.More(); it.Next()) { int k = fm.FindIndex(it.Value()) - 1; if (k >= 0 && std::find(out.begin(), out.end(), k) == out.end()) out.push_back(k); }
  return out;
}

// Sketch curves: kinds and their numbers, see kernel.rs `Curve`.
TopoDS_Wire curve_wire(Args& a, Ints& n) {
  BRepBuilderAPI_MakeWire mk;
  int32_t segments = n.next();
  for (int32_t s = 0; s < segments; s++) {
    int32_t kind = n.next();
    switch (kind) {
    case 1: { gp_Pnt p = a.pnt(), q = a.pnt(); if (p.Distance(q) > 1e-9) mk.Add(BRepBuilderAPI_MakeEdge(p, q).Edge()); break; }
    case 2: { gp_Pnt c = a.pnt(); gp_Dir nn = a.dir(); gp_Dir x = a.dir(); double r = a.num(); need(r > 0, "a circle needs a positive radius"); mk.Add(BRepBuilderAPI_MakeEdge(gp_Circ(gp_Ax2(c, nn, x), r)).Edge()); break; }
    case 3: { gp_Pnt c = a.pnt(); gp_Dir nn = a.dir(); gp_Dir x = a.dir(); double r = a.num(), a0 = a.num(), a1 = a.num(); need(r > 0, "an arc needs a positive radius");
      if (a1 < a0) { nn.Reverse(); a0 = -a0; a1 = -a1; }
      mk.Add(BRepBuilderAPI_MakeEdge(gp_Circ(gp_Ax2(c, nn, x), r), a0, a1).Edge()); break; }
    case 4: { gp_Pnt c = a.pnt(); gp_Dir nn = a.dir(); gp_Dir x = a.dir(); double big = a.num(), small = a.num(); need(small > 0 && big >= small, "an ellipse needs positive radii"); mk.Add(BRepBuilderAPI_MakeEdge(gp_Elips(gp_Ax2(c, nn, x), big, small)).Edge()); break; }
    case 5: { int32_t count = n.next(); bool closed = n.next() != 0; need(count >= 2, "a spline needs at least two points");
      Handle(TColgp_HArray1OfPnt) pts = new TColgp_HArray1OfPnt(1, count); for (int32_t i = 1; i <= count; i++) pts->SetValue(i, a.pnt());
      GeomAPI_Interpolate in(pts, closed, 1e-6); in.Perform(); need(in.IsDone(), "the spline could not be interpolated (repeated points?)");
      mk.Add(BRepBuilderAPI_MakeEdge(in.Curve()).Edge()); break; }
    case 6: { int32_t count = n.next(); int32_t degree = n.next(); need(count >= 2, "a control curve needs at least two points");
      int32_t k = std::min(degree, count - 1); TColgp_Array1OfPnt poles(1, count); for (int32_t i = 1; i <= count; i++) poles.SetValue(i, a.pnt());
      int32_t nk = count - k + 1; TColStd_Array1OfReal knots(1, nk); TColStd_Array1OfInteger mults(1, nk);
      for (int32_t i = 1; i <= nk; i++) { knots.SetValue(i, double(i - 1) / (nk - 1)); mults.SetValue(i, (i == 1 || i == nk) ? k + 1 : 1); }
      mk.Add(BRepBuilderAPI_MakeEdge(new Geom_BSplineCurve(poles, knots, mults, k)).Edge()); break; }
    case 7: { gp_Pnt p = a.pnt(), m = a.pnt(), q = a.pnt(); GC_MakeArcOfCircle arc(p, m, q); need(arc.IsDone(), "the arc's three points are collinear"); mk.Add(BRepBuilderAPI_MakeEdge(arc.Value()).Edge()); break; }
    default: throw Fail("unknown sketch curve kind");
    }
  }
  need(mk.IsDone(), "the curve's pieces do not join end to end");
  return mk.Wire();
}

// RoboCAD's `Sketch.to_face`: the outer curve, holes cut out of it.
TopoDS_Shape face_of_curves(Args& a, Ints& n) {
  int32_t curves = n.next(); need(curves >= 1, "a profile needs a curve");
  BRepBuilderAPI_MakeFace mk(curve_wire(a, n), true); need(mk.IsDone(), "the outer curve is not closed");
  TopoDS_Shape face = mk.Face();
  for (int32_t c = 1; c < curves; c++) {
    BRepBuilderAPI_MakeFace hk(curve_wire(a, n), true); need(hk.IsDone(), "a hole curve is not closed");
    BRepAlgoAPI_Cut cut(face, hk.Face()); cut.Build(); need(cut.IsDone(), "could not cut the hole from the profile");
    TopExp_Explorer f(cut.Shape(), TopAbs_FACE); need(f.More(), "the hole removed the whole profile"); face = f.Current();
  }
  return face;
}

TopoDS_Shape replace_face(const TopoDS_Shape& body, const TopoDS_Face& old, const Handle(Geom_Surface)& surface) {
  TopoDS_Face fresh = BRepBuilderAPI_MakeFace(surface, 1e-6).Face();
  Handle(BRepTools_ReShape) rs = new BRepTools_ReShape(); rs->Replace(old, fresh);
  ShapeFix_Shape fix(rs->Apply(body)); fix.Perform(); return fix.Shape();
}
Handle(Geom_BSplineSurface) bspline_of(const TopoDS_Face& face) {
  Handle(Geom_Surface) s = BRep_Tool::Surface(face);
  Handle(Geom_BSplineSurface) bs = Handle(Geom_BSplineSurface)::DownCast(s);
  if (!bs.IsNull()) return Handle(Geom_BSplineSurface)::DownCast(bs->Copy());
  BRepBuilderAPI_NurbsConvert conv(face, true);
  return GeomConvert::SurfaceToBSplineSurface(BRep_Tool::Surface(TopoDS::Face(conv.Shape())));
}

void write(void* ctx, ShapeOut out, const TopoDS_Shape& shape) {
  need(!shape.IsNull(), "the operation produced no geometry");
  std::ostringstream s; BRepTools::Write(shape, s); std::string t = s.str();
  out(ctx, kind_of(shape), reinterpret_cast<const unsigned char*>(t.data()), t.size());
}
void fail(char* error, size_t size, const char* what) { if (size) { std::strncpy(error, what, size - 1); error[size - 1] = 0; } }
}  // namespace

extern "C" int sim_cad_op(int op, const unsigned char* const* inputs, const size_t* sizes, size_t input_count,
  const double* args, size_t arg_count, const int32_t* ints, size_t int_count, void* ctx, ShapeOut out,
  char* error, size_t error_size) noexcept {
 try {
  std::vector<TopoDS_Shape> in; for (size_t i = 0; i < input_count; i++) in.push_back(read(inputs[i], sizes[i]));
  Args a{args, arg_count}; Ints n{ints, int_count};
  auto one = [&](size_t i) -> const TopoDS_Shape& { need(i < in.size(), "missing input body"); return in[i]; };
  auto rest = [&]() { std::vector<int32_t> v; while (n.more()) v.push_back(n.next()); return v; };
  switch (op) {
  case 1: { // extrude(profile, direction, distance, taper_deg, symmetric)
    TopoDS_Shape face = profile_face(one(0)); gp_Dir d = a.dir(); double dist = a.num(), taper = a.num(); bool sym = a.num() != 0;
    need(std::fabs(dist) > 1e-12, "extrude distance is zero");
    if (sym) face = translate(face, gp_Vec(d) * (-0.5 * dist));
    TopoDS_Shape shape;
    if (std::fabs(taper) < 1e-9) shape = BRepPrimAPI_MakePrism(face, gp_Vec(d) * dist).Shape();
    else {
      TopoDS_Wire w = wire_of(face); BRepOffsetAPI_MakeOffset off; off.Init(TopoDS::Face(faces(face)[0])); off.Perform(-dist * std::tan(taper * M_PI / 180.0));
      need(off.IsDone(), "the tapered top profile could not be made (taper too steep?)");
      TopoDS_Shape top = translate(off.Shape(), gp_Vec(d) * dist);
      BRepOffsetAPI_ThruSections loft(true, true); loft.AddWire(w); loft.AddWire(wire_of(top)); loft.Build(); need(loft.IsDone(), "the tapered extrusion failed");
      shape = loft.Shape();
    }
    write(ctx, out, shape); break; }
  case 2: { // revolve(profile, axis point, axis dir, angle_deg)
    TopoDS_Shape face = profile_face(one(0)); gp_Pnt p = a.pnt(); gp_Dir d = a.dir(); double ang = a.num();
    BRepPrimAPI_MakeRevol mk(face, gp_Ax1(p, d), ang * M_PI / 180.0); need(mk.IsDone(), "the revolve failed (does the axis cross the profile?)");
    write(ctx, out, mk.Shape()); break; }
  case 3: { // sweep(profile, path, scale_end, frenet, round)
    TopoDS_Wire spine = wire_of(one(1)); TopoDS_Wire prof = wire_of(one(0)); double scale_end = a.num(); bool frenet = a.num() != 0, round = a.num() != 0;
    BRepOffsetAPI_MakePipeShell mk(spine); mk.SetTransitionMode(round ? BRepBuilderAPI_RoundCorner : BRepBuilderAPI_RightCorner);
    if (frenet) mk.SetMode(true);
    if (std::fabs(scale_end - 1.0) > 1e-9) { Handle(Law_Linear) law = new Law_Linear(); law->Set(0.0, 1.0, 1.0, scale_end); mk.SetLaw(prof, law, false, false); }
    else mk.Add(prof, false, false);
    mk.Build(); need(mk.IsDone(), "sweep failed: the profile may self-intersect along the path (try a smaller profile or a smoother path)");
    if (!mk.MakeSolid()) { write(ctx, out, mk.Shape()); break; }
    TopoDS_Shape s = mk.Shape();
    need(BRepCheck_Analyzer(s).IsValid(), "sweep produced a self-intersecting solid: the path bends tighter than the profile is wide");
    need(std::fabs(volume(s)) > 1e-9, "sweep produced nothing: place the profile at the start of the path, across it");
    write(ctx, out, s); break; }
  case 4: { // pipe(path, diameter)
    TopoDS_Wire spine = wire_of(one(0)); double dia = a.num(); need(dia > 0, "pipe diameter must be positive");
    auto es = edges(spine); BRepAdaptor_Curve ad(TopoDS::Edge(es[0])); gp_Pnt p0; gp_Vec t; ad.D1(ad.FirstParameter(), p0, t);
    TopoDS_Wire circle = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(gp_Circ(gp_Ax2(p0, gp_Dir(t)), dia / 2)).Edge()).Wire();
    BRepOffsetAPI_MakePipeShell mk(spine); mk.Add(circle, false, false); mk.Build(); need(mk.IsDone(), "pipe failed"); mk.MakeSolid();
    write(ctx, out, mk.Shape()); break; }
  case 5: { // loft(profiles..., solid, ruled)
    bool solid = a.num() != 0, ruled = a.num() != 0; need(in.size() >= 2, "a loft needs two or more profiles");
    BRepOffsetAPI_ThruSections mk(solid, ruled, 1e-4);
    for (auto& p : in) { if (p.ShapeType() == TopAbs_VERTEX) mk.AddVertex(TopoDS::Vertex(p)); else mk.AddWire(wire_of(p)); }
    mk.CheckCompatibility(true); try { mk.Build(); } catch (const Standard_Failure&) {}
    need(mk.IsDone(), "loft failed: the sections could not be matched (check their orientation and vertex counts)");
    write(ctx, out, mk.Shape()); break; }
  case 6: { // fill(edges)
    BRepOffsetAPI_MakeFilling mk; for (auto& e : edges(one(0))) mk.Add(TopoDS::Edge(e), GeomAbs_C0, true);
    mk.Build(); need(mk.IsDone(), "could not fill the hole: the boundary is not closed or is too twisted"); write(ctx, out, mk.Shape()); break; }
  case 7: { // bridge(a, b)
    BRepOffsetAPI_ThruSections mk(false, true); mk.AddWire(wire_of(one(0))); mk.AddWire(wire_of(one(1))); mk.Build(); need(mk.IsDone(), "bridge failed"); write(ctx, out, mk.Shape()); break; }
  case 8: { // join(bodies): sheets sewn (closed into a solid when they close), solids fused, else a compound
    need(!in.empty(), "nothing to join");
    if (in.size() == 1) { write(ctx, out, in[0]); break; }
    bool sheets = std::all_of(in.begin(), in.end(), [](const TopoDS_Shape& s) { return !has(s, TopAbs_SOLID) && has(s, TopAbs_FACE); });
    if (sheets) {
      BRepBuilderAPI_Sewing sew(1e-4); for (auto& s : in) sew.Add(s); sew.Perform(); TopoDS_Shape s = sew.SewedShape();
      if (s.ShapeType() == TopAbs_SHELL) { BRepBuilderAPI_MakeSolid ms(TopoDS::Shell(s)); if (ms.IsDone() && BRepCheck_Analyzer(ms.Solid()).IsValid()) { ShapeFix_Solid fx(ms.Solid()); fx.Perform(); write(ctx, out, fx.Solid()); break; } }
      write(ctx, out, s); break;
    }
    TopoDS_Shape acc = in[0];
    for (size_t i = 1; i < in.size(); i++) { try { acc = boolean(0, acc, in[i]); } catch (const Fail&) { acc = compound({acc, in[i]}); } }
    write(ctx, out, acc); break; }
  case 9: { // split by plane(body, origin, normal, keep: 0 both, 1 positive, 2 negative) -> parts
    gp_Pnt o = a.pnt(); gp_Dir nn = a.dir(); int keep = (int)a.num();
    TopoDS_Face half = BRepBuilderAPI_MakeFace(gp_Pln(o, nn), -1e4, 1e4, -1e4, 1e4).Face();
    BRepAlgoAPI_Splitter sp; sp.SetArguments(list({one(0)})); sp.SetTools(list({half})); sp.SetFuzzyValue(1e-5); sp.Build(); need(sp.IsDone(), "split failed");
    auto parts = solids(sp.Shape()); if (parts.empty()) parts.push_back(sp.Shape()); int written = 0;
    for (auto& p : parts) { double side = gp_Vec(o, centroid(p)).Dot(gp_Vec(nn)); if (keep == 0 || (keep == 1 && side > 0) || (keep == 2 && side < 0)) { write(ctx, out, p); written++; } }
    need(written > 0, "the plane does not cut that body"); break; }
  case 10: { // split by tool(body, tool) -> parts
    BRepAlgoAPI_Splitter sp; sp.SetArguments(list({one(0)})); sp.SetTools(list({one(1)})); sp.SetFuzzyValue(1e-5); sp.Build(); need(sp.IsDone(), "split failed");
    auto parts = solids(sp.Shape()); if (parts.empty()) parts.push_back(sp.Shape()); for (auto& p : parts) write(ctx, out, p); break; }
  case 11: { TopoDS_Shape b = one(0); int32_t f = n.next(); write(ctx, out, push_pull(b, f, a.num())); break; }
  case 12: { TopoDS_Shape b = one(0); double d = a.num(); write(ctx, out, offset_faces(b, rest(), d)); break; }
  case 13: { // move_faces(body, faces, translation)
    gp_Vec t = a.vec(); TopoDS_Shape acc = one(0);
    std::vector<FaceInfo> want; for (auto i : rest()) want.push_back(info(face_at(acc, i)));
    for (auto& w : want) {
      auto fs = faces(acc); int best = -1; double bd = 1e300;
      for (size_t k = 0; k < fs.size(); k++) { FaceInfo g = info(TopoDS::Face(fs[k])); if (g.type != w.type) continue; double d = g.centroid.Distance(w.centroid) + (g.normal - w.normal).Magnitude(); if (d < bd) { bd = d; best = (int)k; } }
      need(best >= 0, "a face to move could not be found again");
      if (w.type == GeomAbs_Plane) { double d = t.Dot(w.normal.Normalized()); if (std::fabs(d) > 1e-9) acc = push_pull(acc, best, d); }
      else if (w.type == GeomAbs_Cylinder) acc = move_cylinder(acc, best, t);
      else throw Fail("only planar and cylindrical faces can be moved directly");
    }
    write(ctx, out, acc); break; }
  case 14: { // rotate_faces(body, faces, axis point, axis dir, angle)
    gp_Pnt p = a.pnt(); gp_Dir d = a.dir(); double ang = a.num() * M_PI / 180.0; TopoDS_Shape acc = one(0);
    for (auto i : rest()) {
      TopoDS_Face face = face_at(acc, i); FaceInfo f = info(face); need(f.type == GeomAbs_Plane, "only planar faces can be rotated directly");
      gp_Trsf tr; tr.SetRotation(gp_Ax1(p, d), ang); TopoDS_Shape moved = BRepBuilderAPI_Transform(face, tr, true).Shape();
      BRepOffsetAPI_ThruSections wedge(true, true); wedge.AddWire(wire_of(face)); wedge.AddWire(wire_of(moved)); wedge.Build(); need(wedge.IsDone(), "the rotated face could not be bridged");
      bool adding = gp_Vec(f.centroid, centroid(wedge.Shape())).Dot(f.normal) > 0;
      acc = boolean(adding ? 0 : 1, acc, wedge.Shape());
    }
    write(ctx, out, acc); break; }
  case 15: { TopoDS_Shape b = one(0); int32_t f = n.next(); write(ctx, out, set_radius(b, f, a.num())); break; }
  case 16: { // draft(body, faces, pull dir, angle, neutral origin, normal)
    gp_Dir pull = a.dir(); double ang = a.num() * M_PI / 180.0; gp_Pnt o = a.pnt(); gp_Dir nn = a.dir();
    BRepOffsetAPI_DraftAngle mk(one(0));
    for (auto i : rest()) { mk.Add(face_at(one(0), i), pull, ang, gp_Pln(o, nn)); need(mk.AddDone(), "draft failed on a face: choose a neutral plane that crosses it"); }
    mk.Build(); need(mk.IsDone(), "draft failed"); write(ctx, out, mk.Shape()); break; }
  case 17: { // delete_faces(body, faces)
    BRepAlgoAPI_Defeaturing df; df.SetShape(one(0)); for (auto i : rest()) df.AddFaceToRemove(face_at(one(0), i));
    df.Build(); need(df.IsDone() && !df.HasErrors(), "could not delete those faces: the neighbours cannot be extended to close the gap");
    write(ctx, out, unify(df.Shape())); break; }
  case 18: { // imprint(body, tool)
    BRepAlgoAPI_Splitter sp; sp.SetArguments(list({one(0)})); sp.SetTools(list({one(1)})); sp.Build(); need(sp.IsDone(), "imprint failed"); write(ctx, out, sp.Shape()); break; }
  case 19: { // shell(body, thickness, open faces)
    double t = a.num(); need(t > 0, "wall thickness must be positive"); std::vector<TopoDS_Shape> open; for (auto i : rest()) open.push_back(face_at(one(0), i));
    BRepOffsetAPI_MakeThickSolid mk; mk.MakeThickSolidByJoin(one(0), list(open), -t, 1e-4, BRepOffset_Skin, false, false, GeomAbs_Arc);
    need(mk.IsDone(), "hollowing failed: walls would meet; try a thinner wall or remove fillets first"); write(ctx, out, unify(mk.Shape())); break; }
  case 20: { // thicken(sheet, thickness)
    BRepOffsetAPI_MakeThickSolid mk; mk.MakeThickSolidBySimple(one(0), a.num()); need(mk.IsDone(), "thicken failed");
    TopoDS_Shape s = mk.Shape(); if (volume(s) < 0) s.Reverse(); write(ctx, out, s); break; }
  case 21: { // fillet(body, radius, radius_end (<=0: constant), edges)
    double r = a.num(), r_end = a.num(); need(r > 0, "fillet radius must be positive");
    BRepFilletAPI_MakeFillet mk(one(0), ChFi3d_Rational);
    for (auto i : rest()) { TopoDS_Edge e = edge_at(one(0), i); if (r_end > 0) mk.Add(r, r_end, e); else mk.Add(r, e); }
    write(ctx, out, fillet_finish(mk, r)); break; }
  case 22: { // fillet_chordal(body, chord, edges): per edge, radius = chord / (2 sin((pi - angle) / 2))
    double chord = a.num(); need(chord > 0, "chord must be positive"); TopoDS_Shape acc = one(0);
    std::vector<std::pair<gp_Pnt, double>> want; for (auto i : rest()) { TopoDS_Edge e = edge_at(acc, i); BRepAdaptor_Curve c(e); want.push_back({c.Value(0.5 * (c.FirstParameter() + c.LastParameter())), 0}); }
    for (auto& w : want) {
      auto es = edges(acc); int best = -1; double bd = 1e300;
      for (size_t k = 0; k < es.size(); k++) { BRepAdaptor_Curve c(TopoDS::Edge(es[k])); double d = c.Value(0.5 * (c.FirstParameter() + c.LastParameter())).Distance(w.first); if (d < bd) { bd = d; best = (int)k; } }
      TopoDS_Edge e = TopoDS::Edge(es[best]); auto fs = faces_of_edge(acc, e); double angle = M_PI / 2;
      if (fs.size() == 2) { double c = std::fabs(info(face_at(acc, fs[0])).normal.Normalized().Dot(info(face_at(acc, fs[1])).normal.Normalized())); angle = std::acos(std::max(-1.0, std::min(1.0, c))); }
      double radius = angle > 1e-6 ? chord / (2.0 * std::sin(0.5 * (M_PI - angle))) : chord;
      BRepFilletAPI_MakeFillet mk(acc, ChFi3d_Rational); mk.Add(radius, e); acc = fillet_finish(mk, radius);
    }
    write(ctx, out, acc); break; }
  case 23: { // full_round(body, edge a, edge b): radius half their midpoint distance
    TopoDS_Edge ea = edge_at(one(0), n.next()), eb = edge_at(one(0), n.next());
    BRepAdaptor_Curve ca(ea), cb(eb); double r = 0.5 * ca.Value(0.5 * (ca.FirstParameter() + ca.LastParameter())).Distance(cb.Value(0.5 * (cb.FirstParameter() + cb.LastParameter()))) * 0.999;
    BRepFilletAPI_MakeFillet mk(one(0), ChFi3d_Rational); mk.Add(r, ea); mk.Add(r, eb); write(ctx, out, fillet_finish(mk, r)); break; }
  case 24: { // mirror(body, origin, normal)
    gp_Pnt o = a.pnt(); gp_Dir nn = a.dir(); gp_Trsf tr; tr.SetMirror(gp_Ax2(o, nn)); write(ctx, out, BRepBuilderAPI_Transform(one(0), tr, true).Shape()); break; }
  case 25: { // unjoin(body) -> solids, or the faces of a sheet
    auto ss = solids(one(0));
    if (ss.size() > 1) { for (auto& s : ss) write(ctx, out, s); }
    else if (!has(one(0), TopAbs_SOLID) && has(one(0), TopAbs_FACE)) { for (auto& f : faces(one(0))) write(ctx, out, f); }
    else write(ctx, out, one(0));
    break; }
  case 26: write(ctx, out, unify(one(0))); break; // dissolve
  case 27: { // extract_components(body, groups: [count, indices...]...) -> remainder, then each component
    auto ss = solids(one(0)); std::vector<std::vector<int32_t>> groups; std::vector<int32_t> used;
    while (n.more()) { int32_t c = n.next(); need(c > 0, "Each component must contain at least one solid"); std::vector<int32_t> g; for (int32_t k = 0; k < c; k++) { int32_t i = n.next(); need(i >= 0 && i < (int32_t)ss.size(), "Solid index is out of range"); need(std::find(used.begin(), used.end(), i) == used.end(), "A solid cannot belong to more than one component"); used.push_back(i); g.push_back(i); } groups.push_back(g); }
    need(!groups.empty(), "Each component must contain at least one solid");
    need(used.size() < ss.size(), "Leave at least one solid in the source; use Unjoin to separate everything");
    Handle(BRepTools_ReShape) rs = new BRepTools_ReShape(); for (auto i : used) rs->Remove(ss[i]);
    write(ctx, out, rs->Apply(one(0)));
    for (auto& g : groups) { std::vector<TopoDS_Shape> parts; for (auto i : g) parts.push_back(ss[i]); write(ctx, out, parts.size() == 1 ? parts[0] : compound(parts)); }
    break; }
  case 28: { // offset_face_to(body, target, face, clearance)
    int32_t fi = n.next(); TopoDS_Face face = face_at(one(0), fi); FaceInfo f = info(face); double clearance = a.num();
    BRepIntCurveSurface_Inter hit; hit.Init(one(1), gp_Lin(f.centroid, gp_Dir(f.normal)), 1e-6); double best = 1e300;
    for (; hit.More(); hit.Next()) if (hit.W() > 1e-9) best = std::min(best, hit.W());
    need(best < 1e299, "the target body is not in front of that face");
    write(ctx, out, push_pull(one(0), fi, best - clearance)); break; }
  case 29: { Args& aa = a; Ints& nn = n; std::vector<TopoDS_Shape> wires; while (nn.more()) wires.push_back(curve_wire(aa, nn)); write(ctx, out, wires.size() == 1 ? wires[0] : compound(wires)); break; } // wires of curves
  case 30: write(ctx, out, face_of_curves(a, n)); break; // profile face of curves
  case 31: { // silhouette(body, plane origin, normal, x axis)
    gp_Pnt o = a.pnt(); gp_Dir nn = a.dir(); gp_Dir x = a.dir();
    Handle(HLRBRep_Algo) algo = new HLRBRep_Algo(); algo->Add(one(0)); algo->Projector(HLRAlgo_Projector(gp_Ax2(o, nn, x))); algo->Update(); algo->Hide();
    HLRBRep_HLRToShape to(algo); std::vector<TopoDS_Shape> parts;
    for (TopoDS_Shape s : {to.OutLineVCompound(), to.VCompound()}) if (!s.IsNull()) parts.push_back(s);
    need(!parts.empty(), "the body has no silhouette from that plane");
    TopoDS_Compound sc = compound(parts); BRepLib::BuildCurves3d(sc); write(ctx, out, sc); break; }
  case 32: { // project_curve(wire, body, direction): normal projection, as RoboCAD
    BRepAlgo_NormalProjection proj(one(1)); proj.Add(wire_of(one(0))); proj.Build(); need(proj.IsDone(), "projection failed"); write(ctx, out, proj.Projection()); break; }
  case 33: { // set_control_points(body, face, nu, nv, points)
    TopoDS_Face face = face_at(one(0), n.next()); int32_t nu = n.next(), nv = n.next(); Handle(Geom_BSplineSurface) bs = bspline_of(face);
    if (bs->NbUPoles() != nu || bs->NbVPoles() != nv) { std::string m = "control points must be a " + std::to_string(bs->NbUPoles()) + " x " + std::to_string(bs->NbVPoles()) + " grid for this face"; throw Fail(m.c_str()); }
    for (int32_t i = 1; i <= nu; i++) for (int32_t j = 1; j <= nv; j++) bs->SetPole(i, j, a.pnt());
    write(ctx, out, replace_face(one(0), face, bs)); break; }
  case 34: { // raise_degree(body, face, du, dv)
    TopoDS_Face face = face_at(one(0), n.next()); int32_t du = n.next(), dv = n.next(); Handle(Geom_BSplineSurface) bs = bspline_of(face);
    bs->IncreaseDegree(std::max(bs->UDegree(), (int)du), std::max(bs->VDegree(), (int)dv)); write(ctx, out, replace_face(one(0), face, bs)); break; }
  case 35: { // rebuild_face(body, face, spans u, spans v, degree)
    TopoDS_Face face = face_at(one(0), n.next()); int32_t su = n.next(), sv = n.next(), deg = n.next(); need(su > 0 && sv > 0 && deg >= 1, "spans and degree must be positive");
    Handle(Geom_Surface) s = BRep_Tool::Surface(face); double u0, u1, v0, v1; BRepTools::UVBounds(face, u0, u1, v0, v1);
    int nu = su + deg, nv = sv + deg; TColgp_Array2OfPnt arr(1, nu, 1, nv);
    for (int i = 0; i < nu; i++) for (int j = 0; j < nv; j++) arr.SetValue(i + 1, j + 1, s->Value(u0 + (u1 - u0) * i / (nu - 1), v0 + (v1 - v0) * j / (nv - 1)));
    GeomAPI_PointsToBSplineSurface fit(arr, deg, deg, GeomAbs_C2, 1e-3); write(ctx, out, replace_face(one(0), face, fit.Surface())); break; }
  case 36: { // extrude_up_to(profile, direction, target)
    TopoDS_Shape face = profile_face(one(0)); gp_Dir d = a.dir(); gp_Pnt c = centroid(face);
    BRepIntCurveSurface_Inter hit; hit.Init(one(1), gp_Lin(c, d), 1e-6); double best = 1e300; for (; hit.More(); hit.Next()) if (hit.W() > 1e-9) best = std::min(best, hit.W());
    need(best < 1e299, "nothing in that direction to extrude up to"); write(ctx, out, BRepPrimAPI_MakePrism(face, gp_Vec(d) * best).Shape()); break; }
  case 37: { // boolean(target, tools..., op 0 union 1 subtract 2 intersect)
    int bop = (int)a.num(); need(in.size() >= 2, "a boolean needs a target and tools"); TopoDS_Shape acc = in[0];
    for (size_t i = 1; i < in.size(); i++) acc = boolean(bop, acc, in[i]);
    write(ctx, out, acc); break; }
  case 38: { // hlr(bodies, eye direction d, x axis): visible, then hidden edges in the view's 2D frame
    gp_Dir d = a.dir(); gp_Dir x = a.dir();
    Handle(HLRBRep_Algo) algo = new HLRBRep_Algo(); for (auto& b : in) algo->Add(b);
    algo->Projector(HLRAlgo_Projector(gp_Ax2(gp_Pnt(0, 0, 0), d.Reversed(), x))); algo->Update(); algo->Hide();
    HLRBRep_HLRToShape to(algo);
    std::vector<TopoDS_Shape> vis, hid;
    for (TopoDS_Shape s : {to.VCompound(), to.OutLineVCompound(), to.Rg1LineVCompound()}) if (!s.IsNull()) vis.push_back(s);
    for (TopoDS_Shape s : {to.HCompound(), to.OutLineHCompound()}) if (!s.IsNull()) hid.push_back(s);
    // HLR edges carry 2D curves in the projection plane only: give them 3D curves (z = 0).
    TopoDS_Compound cv = compound(vis), ch = compound(hid); BRepLib::BuildCurves3d(cv); BRepLib::BuildCurves3d(ch);
    write(ctx, out, cv); write(ctx, out, ch); break; }
  default: throw Fail("unknown modelling operation");
  }
  return 0;
 } catch (const Standard_Failure& e) { fail(error, error_size, e.GetMessageString() ? e.GetMessageString() : "OCCT Standard_Failure"); }
 catch (const std::exception& e) { fail(error, error_size, e.what()); }
 catch (...) { fail(error, error_size, "unknown native OCCT exception"); }
 return 1;
}

// Numeric queries: 1 control points (body, face) -> nu, nv, poles; 2 continuity
// (body, edge) -> code (0 boundary, 1 G0, 2 G1, 3 G2); 3 ray hits (body,
// origin, dir) -> (w, x, y, z, face)*; 4 distance (a, b) -> d, p, q;
// 5 contains (body, point, tol) -> 0/1; 6 validate (body) -> valid;
// 7 bounding box (body) -> min, max; 8 face normal at point (body, face, point) -> n.
extern "C" int sim_cad_measure(int op, const unsigned char* const* inputs, const size_t* sizes, size_t input_count,
  const double* args, size_t arg_count, const int32_t* ints, size_t int_count, void* ctx, NumbersOut out,
  char* error, size_t error_size) noexcept {
 try {
  std::vector<TopoDS_Shape> in; for (size_t i = 0; i < input_count; i++) in.push_back(read(inputs[i], sizes[i]));
  Args a{args, arg_count}; Ints n{ints, int_count};
  auto one = [&](size_t i) -> const TopoDS_Shape& { need(i < in.size(), "missing input body"); return in[i]; };
  std::vector<double> r;
  switch (op) {
  case 1: { TopoDS_Face face = face_at(one(0), n.next()); Handle(Geom_BSplineSurface) bs = bspline_of(face); r.push_back(bs->NbUPoles()); r.push_back(bs->NbVPoles());
    for (int i = 1; i <= bs->NbUPoles(); i++) for (int j = 1; j <= bs->NbVPoles(); j++) { gp_Pnt p = bs->Pole(i, j); r.push_back(p.X()); r.push_back(p.Y()); r.push_back(p.Z()); } break; }
  case 2: { TopoDS_Edge e = edge_at(one(0), n.next()); auto fs = faces_of_edge(one(0), e); if (fs.size() < 2) { r.push_back(0); break; }
    TopoDS_Face f0 = face_at(one(0), fs[0]), f1 = face_at(one(0), fs[1]); BRepAdaptor_Curve c(e); int code = 3;
    for (int k = 0; k < 7 && code > 1; k++) {
      gp_Pnt p = c.Value(c.FirstParameter() + (c.LastParameter() - c.FirstParameter()) * k / 6.0);
      double nk[2][3]; int fi = 0;
      for (const TopoDS_Face& f : {f0, f1}) {
        BRepAdaptor_Surface s(f); double u0, u1, v0, v1; BRepTools::UVBounds(f, u0, u1, v0, v1);
        double bu = u0, bv = v0, bd = 1e300; for (int i = 0; i <= 12; i++) for (int j = 0; j <= 12; j++) { double u = u0 + (u1 - u0) * i / 12, v = v0 + (v1 - v0) * j / 12; double d = s.Value(u, v).Distance(p); if (d < bd) { bd = d; bu = u; bv = v; } }
        BRepLProp_SLProps pr(s, bu, bv, 2, 1e-6); gp_Dir nd = pr.IsNormalDefined() ? pr.Normal() : gp_Dir(0, 0, 1);
        nk[fi][0] = nd.X(); nk[fi][1] = nd.Y(); nk[fi][2] = nd.Z(); fi++;
        if (fi == 2) {
          double dot = std::fabs(nk[0][0] * nk[1][0] + nk[0][1] * nk[1][1] + nk[0][2] * nk[1][2]);
          if (std::fabs(dot - 1.0) > 1e-3) code = 1;
        }
      }
    }
    r.push_back(code); break; }
  case 3: { gp_Pnt o = a.pnt(); gp_Dir d = a.dir(); TopTools_IndexedMapOfShape fm; TopExp::MapShapes(one(0), TopAbs_FACE, fm);
    BRepIntCurveSurface_Inter hit; hit.Init(one(0), gp_Lin(o, d), 1e-6);
    for (; hit.More(); hit.Next()) if (hit.W() > 1e-9) { gp_Pnt p = hit.Pnt(); r.insert(r.end(), {hit.W(), p.X(), p.Y(), p.Z(), double(fm.FindIndex(hit.Face()) - 1)}); } break; }
  case 4: { BRepExtrema_DistShapeShape d(one(0), one(1)); d.Perform(); need(d.IsDone() && d.NbSolution() > 0, "distance failed"); gp_Pnt p = d.PointOnShape1(1), q = d.PointOnShape2(1);
    r.insert(r.end(), {d.Value(), p.X(), p.Y(), p.Z(), q.X(), q.Y(), q.Z()}); break; }
  case 5: { gp_Pnt p = a.pnt(); double tol = a.num(); BRepClass3d_SolidClassifier c(one(0), p, tol); r.push_back(c.State() == TopAbs_IN || c.State() == TopAbs_ON ? 1 : 0); break; }
  case 6: r.push_back(BRepCheck_Analyzer(one(0)).IsValid() ? 1 : 0); break;
  case 7: { Bnd_Box b; BRepBndLib::Add(one(0), b, true); need(!b.IsVoid(), "the body has no extent"); double x0, y0, z0, x1, y1, z1; b.Get(x0, y0, z0, x1, y1, z1); r.insert(r.end(), {x0, y0, z0, x1, y1, z1}); break; }
  case 8: { TopoDS_Face f = face_at(one(0), n.next()); FaceInfo fi = info(f); gp_Vec nn = fi.normal.Normalized(); r.insert(r.end(), {nn.X(), nn.Y(), nn.Z(), fi.centroid.X(), fi.centroid.Y(), fi.centroid.Z(), fi.radius, double(fi.type), fi.axis ? 1.0 : 0.0, fi.axis_point.X(), fi.axis_point.Y(), fi.axis_point.Z(), fi.axis_dir.X(), fi.axis_dir.Y(), fi.axis_dir.Z(), is_hole(fi) ? 1.0 : 0.0}); break; }
  case 9: { // full topology(body, samples): faces, edges with sampled polylines, vertices
    int samples = n.more() ? n.next() : 24;
    auto fs = faces(one(0)); r.push_back(double(fs.size()));
    for (auto& f : fs) {
      FaceInfo fi = info(TopoDS::Face(f)); gp_Vec nn = fi.normal.Normalized();
      double rad = 0; bool axis = fi.axis; gp_Pnt ap = fi.axis_point; gp_Dir ad = fi.axis_dir;
      BRepAdaptor_Surface s(TopoDS::Face(f));
      if (fi.type == GeomAbs_Sphere) { gp_Sphere sp = s.Sphere(); ap = sp.Location(); ad = gp_Dir(0, 0, 1); rad = sp.Radius(); axis = true; }
      else if (fi.type == GeomAbs_Torus) { gp_Torus t = s.Torus(); ap = t.Location(); ad = t.Axis().Direction(); rad = t.MinorRadius(); axis = true; }
      else rad = fi.radius;
      r.insert(r.end(), {double(fi.type), fi.centroid.X(), fi.centroid.Y(), fi.centroid.Z(), nn.X(), nn.Y(), nn.Z(), fi.area, axis ? 1.0 : 0.0, ap.X(), ap.Y(), ap.Z(), ad.X(), ad.Y(), ad.Z(), rad, fi.point.X(), fi.point.Y(), fi.point.Z()});
    }
    auto all = edges(one(0)); std::vector<std::pair<size_t, TopoDS_Shape>> es;
    // An edge without a 3D curve (a degenerate seam, some hidden-line output)
    // has no polyline; the others keep their index (the one fillets take).
    for (size_t k = 0; k < all.size(); k++) { double f, l; if (!BRep_Tool::Degenerated(TopoDS::Edge(all[k])) && !BRep_Tool::Curve(TopoDS::Edge(all[k]), f, l).IsNull()) es.push_back({k, all[k]}); }
    r.push_back(double(es.size()));
    for (auto& [index, e] : es) {
      BRepAdaptor_Curve c(TopoDS::Edge(e)); GProp_GProps p; BRepGProp::LinearProperties(e, p);
      double f0 = c.FirstParameter(), l0 = c.LastParameter(); gp_Pnt m = c.Value(0.5 * (f0 + l0)), a0 = c.Value(f0), b0 = c.Value(l0);
      bool circle = c.GetType() == GeomAbs_Circle; gp_Circ ci = circle ? c.Circle() : gp_Circ();
      r.insert(r.end(), {double(index), double(c.GetType()), m.X(), m.Y(), m.Z(), p.Mass(), a0.X(), a0.Y(), a0.Z(), b0.X(), b0.Y(), b0.Z(), circle ? 1.0 : 0.0, ci.Location().X(), ci.Location().Y(), ci.Location().Z(), circle ? ci.Radius() : 0.0});
      int k = c.GetType() == GeomAbs_Line ? 2 : std::max(2, samples); r.push_back(k);
      for (int i = 0; i < k; i++) { gp_Pnt q = c.Value(f0 + (l0 - f0) * i / (k - 1)); r.insert(r.end(), {q.X(), q.Y(), q.Z()}); }
    }
    TopTools_IndexedMapOfShape vs; TopExp::MapShapes(one(0), TopAbs_VERTEX, vs); r.push_back(vs.Extent());
    for (int i = 1; i <= vs.Extent(); i++) { gp_Pnt q = BRep_Tool::Pnt(TopoDS::Vertex(vs(i))); r.insert(r.end(), {q.X(), q.Y(), q.Z()}); }
    break; }
  case 10: { // curvature comb(wire, samples, scale): (point, point + normal·curvature·scale) per sample
    int samples = n.next(); double scale = a.num();
    for (auto& e : edges(one(0))) {
      double f, l; if (BRep_Tool::Curve(TopoDS::Edge(e), f, l).IsNull()) continue;
      BRepAdaptor_Curve c(TopoDS::Edge(e)); double f0 = c.FirstParameter(), l0 = c.LastParameter();
      for (int i = 0; i < samples; i++) {
        double u = f0 + (l0 - f0) * i / (samples - 1); BRepLProp_CLProps pr(c, u, 2, 1e-6);
        gp_Pnt p = pr.Value(); double k = pr.Curvature(); gp_Dir nn(0, 0, 1);
        if (k > 1e-12 && pr.IsTangentDefined()) { try { pr.Normal(nn); } catch (const Standard_Failure&) { k = 0; } }
        gp_Pnt q = p.Translated(gp_Vec(nn) * (k * scale));
        r.insert(r.end(), {p.X(), p.Y(), p.Z(), q.X(), q.Y(), q.Z()});
      }
    }
    break; }
  case 11: { // RoboCAD's annotation stamp inputs (annotations.py `stamp`): volume, area,
    // centroid, bbox (after meshing at `tolerance` when > 0, as RoboCAD's display
    // does: its box reads the triangulation), then each face's `_face_ref`.
    double tol = a.num(); bool solid = n.next() != 0; TopoDS_Shape s = one(0);
    if (tol > 0) { BRepMesh_IncrementalMesh mesh(s, tol, false, 20.0 * M_PI / 180.0, true); }
    GProp_GProps vol, area;
    bool has_solid = solid && has(s, TopAbs_SOLID);
    if (has_solid) BRepGProp::VolumeProperties(s, vol, true); else BRepGProp::SurfaceProperties(s, vol);
    BRepGProp::SurfaceProperties(s, area);
    Bnd_Box bb; BRepBndLib::Add(s, bb, true); double x0 = 0, y0 = 0, z0 = 0, x1 = 0, y1 = 0, z1 = 0; if (!bb.IsVoid()) bb.Get(x0, y0, z0, x1, y1, z1);
    gp_Pnt c = vol.CentreOfMass();
    r.insert(r.end(), {has_solid ? vol.Mass() : 0.0, area.Mass(), c.X(), c.Y(), c.Z(), x0, y0, z0, x1, y1, z1});
    std::vector<TopoDS_Shape> fs; { TopTools_IndexedMapOfShape m; TopExp::MapShapes(s, TopAbs_FACE, m); for (int i = 1; i <= m.Extent(); i++) fs.push_back(m(i)); }
    r.push_back(double(fs.size()));
    for (auto& f0 : fs) {
      TopoDS_Face f = TopoDS::Face(f0); GProp_GProps p; BRepGProp::SurfaceProperties(f, p); gp_Pnt fc = p.CentreOfMass();
      BRepAdaptor_Surface ad(f); GeomAbs_SurfaceType t = ad.GetType();
      int kind = 7; bool axis = false; gp_Pnt ap; gp_Dir adir(0, 0, 1); double rad = 0;
      if (t == GeomAbs_Plane) kind = 0;
      else if (t == GeomAbs_Cylinder) { kind = 1; gp_Cylinder cy = ad.Cylinder(); ap = cy.Location(); adir = cy.Axis().Direction(); rad = cy.Radius(); axis = true; }
      else if (t == GeomAbs_Cone) { kind = 2; gp_Cone co = ad.Cone(); ap = co.Location(); adir = co.Axis().Direction(); rad = co.RefRadius(); axis = true; }
      else if (t == GeomAbs_Sphere) { kind = 3; gp_Sphere sp = ad.Sphere(); ap = sp.Location(); adir = gp_Dir(0, 0, 1); rad = sp.Radius(); axis = true; }
      else if (t == GeomAbs_Torus) { kind = 4; gp_Torus to = ad.Torus(); ap = to.Location(); adir = to.Axis().Direction(); rad = to.MinorRadius(); axis = true; }
      else if (t == GeomAbs_BSplineSurface) kind = 5;
      else if (t == GeomAbs_BezierSurface) kind = 6;
      double u0, u1, v0, v1; BRepTools::UVBounds(f, u0, u1, v0, v1);
      BRepLProp_SLProps pr(ad, 0.5 * (u0 + u1), 0.5 * (v0 + v1), 1, 1e-6); gp_Pnt pt = pr.Value(); double nx = 0, ny = 0, nz = 1;
      if (pr.IsNormalDefined()) { gp_Dir nd = pr.Normal(); double k = f.Orientation() == TopAbs_REVERSED ? -1 : 1; nx = nd.X() * k; ny = nd.Y() * k; nz = nd.Z() * k; }
      r.insert(r.end(), {double(kind), fc.X(), fc.Y(), fc.Z(), nx, ny, nz, p.Mass(), axis ? 1.0 : 0.0, ap.X(), ap.Y(), ap.Z(), adir.X(), adir.Y(), adir.Z(), rad, pt.X(), pt.Y(), pt.Z()});
    }
    break; }
  default: throw Fail("unknown measurement");
  }
  out(ctx, r.data(), r.size());
  return 0;
 } catch (const Standard_Failure& e) { fail(error, error_size, e.GetMessageString() ? e.GetMessageString() : "OCCT Standard_Failure"); }
 catch (const std::exception& e) { fail(error, error_size, e.what()); }
 catch (...) { fail(error, error_size, "unknown native OCCT exception"); }
 return 1;
}

// ---- Exchange: STEP and IGES (RoboCAD's io/exporters.py `export_step`,
// `export_iges` and io/importers.py's readers). Plain geometry: the
// writers carry no names or colours (those need OCCT's XDE toolkits).
#include <STEPControl_Writer.hxx>
#include <STEPControl_Reader.hxx>
#include <IGESControl_Writer.hxx>
#include <IGESControl_Reader.hxx>
#include <Interface_Static.hxx>
#include <IFSelect_ReturnStatus.hxx>

extern "C" int sim_cad_export(int format, const unsigned char* const* inputs, const size_t* sizes, size_t input_count,
  const char* path, const char* schema, char* error, size_t error_size) noexcept {
 try {
  std::vector<TopoDS_Shape> in; for (size_t i = 0; i < input_count; i++) in.push_back(read(inputs[i], sizes[i]));
  need(!in.empty(), "nothing to export");
  if (format == 1) {
    std::string s = schema ? schema : "AP214";
    Interface_Static::SetCVal("write.step.schema", s == "AP203" ? "AP203" : s == "AP242" ? "AP242DIS" : "AP214IS");
    Interface_Static::SetCVal("write.step.unit", "MM");
    STEPControl_Writer w;
    for (auto& sh : in) need(w.Transfer(sh, STEPControl_AsIs) == IFSelect_RetDone, "a body could not be written as STEP");
    need(w.Write(path) == IFSelect_RetDone, "the STEP file could not be written");
  } else if (format == 2) {
    IGESControl_Writer w("MM", 0);
    for (auto& sh : in) need(w.AddShape(sh), "a body could not be written as IGES");
    w.ComputeModel();
    need(w.Write(path), "the IGES file could not be written");
  } else throw Fail("unknown exchange format");
  return 0;
 } catch (const Standard_Failure& e) { fail(error, error_size, e.GetMessageString() ? e.GetMessageString() : "OCCT Standard_Failure"); }
 catch (const std::exception& e) { fail(error, error_size, e.what()); }
 catch (...) { fail(error, error_size, "unknown native OCCT exception"); }
 return 1;
}

// Every solid (else shell, else the whole shape) of a STEP or IGES file.
extern "C" int sim_cad_import(int format, const char* path, double scale, void* ctx, ShapeOut out, char* error, size_t error_size) noexcept {
 try {
  TopoDS_Shape shape;
  if (format == 1) {
    STEPControl_Reader r; need(r.ReadFile(path) == IFSelect_RetDone, "the STEP file could not be read");
    r.TransferRoots(); shape = r.OneShape();
  } else if (format == 2) {
    IGESControl_Reader r; need(r.ReadFile(path) == IFSelect_RetDone, "the IGES file could not be read");
    r.TransferRoots(); shape = r.OneShape();
  } else throw Fail("unknown exchange format");
  need(!shape.IsNull(), "the file holds no geometry");
  if (std::fabs(scale - 1.0) > 1e-12) { gp_Trsf t; t.SetScale(gp_Pnt(0, 0, 0), scale); shape = BRepBuilderAPI_Transform(shape, t, true).Shape(); }
  auto ss = solids(shape);
  if (ss.empty()) write(ctx, out, shape); else for (auto& s : ss) write(ctx, out, s);
  return 0;
 } catch (const Standard_Failure& e) { fail(error, error_size, e.GetMessageString() ? e.GetMessageString() : "OCCT Standard_Failure"); }
 catch (const std::exception& e) { fail(error, error_size, e.what()); }
 catch (...) { fail(error, error_size, "unknown native OCCT exception"); }
 return 1;
}
