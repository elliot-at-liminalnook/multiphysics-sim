#if defined(__linux__) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif
#include <Standard_Version.hxx>
#if OCC_VERSION_MAJOR != 7 || (OCC_VERSION_MINOR != 7 && OCC_VERSION_MINOR != 8)
#error "sim-cad source-reviewed bridge requires OCCT 7.7.x/7.8.x; requalify APIs/archive compatibility before changing this pin"
#endif
#include <BRepTools.hxx>
#include <BRep_Builder.hxx>
#include <BRepGProp.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <GProp_GProps.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepLProp_SLProps.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Shape.hxx>
#include <TopLoc_Location.hxx>
#include <Poly_Triangulation.hxx>
#include <Standard_Failure.hxx>
#include <gp_Trsf.hxx>
#include <gp_Ax2.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <TopExp.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <gp_Pln.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <sstream>
#include <vector>
#include <string>
#include <cstring>
#include <exception>
#include <stdexcept>
#include <utility>
#include <cstdint>

extern "C" {
const char* sim_cad_kernel_version() noexcept { return OCC_VERSION_COMPLETE; }
typedef void (*PropertyCallback)(void*, int, const double*);
typedef void (*VertexCallback)(void*, const double*, const double*);
typedef void (*TriangleCallback)(void*, uint32_t,uint32_t,uint32_t,uint32_t);
typedef bool (*CancelCallback)(void*);
}
static void properties(const TopoDS_Shape& shape, int solid, void* context, PropertyCallback callback, bool volume) {
  GProp_GProps p;
  bool has_volume=volume && TopExp_Explorer(shape,TopAbs_SOLID).More();
  if (has_volume) BRepGProp::VolumeProperties(shape,p,true,false,false);
  double out[13] = {}; out[0]=p.Mass();
  gp_Pnt c;
  if(has_volume) c=p.CentreOfMass();
  else {Bnd_Box bounds;BRepBndLib::Add(shape,bounds,true);if(!bounds.IsVoid()){double x0,y0,z0,x1,y1,z1;bounds.Get(x0,y0,z0,x1,y1,z1);c=gp_Pnt((x0+x1)*0.5,(y0+y1)*0.5,(z0+z1)*0.5);}}
  out[1]=c.X();out[2]=c.Y();out[3]=c.Z();
  gp_Mat m=p.MatrixOfInertia();for(int i=1;i<=3;i++)for(int j=1;j<=3;j++)out[4+(i-1)*3+j-1]=m.Value(i,j);
  callback(context,solid,out);
}
extern "C" int sim_cad_query(const unsigned char* bytes,size_t size,int primitive,const double* args,
 const double* matrices,size_t matrix_count,bool volume,double tolerance,void* context,
 PropertyCallback property,VertexCallback vertex,TriangleCallback triangle,CancelCallback cancel,
 char* error,size_t error_size) noexcept {
 try {
  TopoDS_Shape shape;
  if(size>0 || primitive==0) { std::string payload(reinterpret_cast<const char*>(bytes),size);std::istringstream stream(payload); BRep_Builder builder;BRepTools::Read(shape,stream,builder);if(shape.IsNull()||stream.bad())throw std::runtime_error("BRepTools::Read failed"); }
  if (primitive==1) shape=BRepPrimAPI_MakeBox(gp_Pnt(args[0],args[1],args[2]),args[3],args[4],args[5]).Shape();
  else if (primitive==2) shape=BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(args[0],args[1],args[2]),gp_Dir(args[3],args[4],args[5])),args[6],args[7]).Shape();

  for(size_t i=0;i<matrix_count;i++) {
   if(cancel(context))throw std::runtime_error("cancelled between native transforms");
   const double* m=matrices+12*i;gp_Trsf tr;tr.SetValues(m[0],m[1],m[2],m[3],m[4],m[5],m[6],m[7],m[8],m[9],m[10],m[11]);
   BRepBuilderAPI_Transform op(shape,tr,true);if(!op.IsDone())throw std::runtime_error("OCCT transform failed");shape=op.Shape();
  }
  if(cancel(context))throw std::runtime_error("cancelled before exact properties");
  properties(shape,-1,context,property,volume);
  int solid_count=0;for(TopExp_Explorer it(shape,TopAbs_SOLID);it.More();it.Next())++solid_count;
  int si=0;TopAbs_ShapeEnum regions=(!volume && solid_count<=1)?TopAbs_FACE:TopAbs_SOLID;
  if(solid_count<=1 && volume){properties(shape,si++,context,property,volume);}
  else for(TopExp_Explorer it(shape,regions);it.More();it.Next(),si++) {
   if(cancel(context))throw std::runtime_error("cancelled between solids");properties(it.Current(),si,context,property,volume);
  }
  if(cancel(context))throw std::runtime_error("cancelled before tessellation");
  BRepMesh_IncrementalMesh mesher(shape,tolerance,false,0.3490658503988659,false);
  if(!mesher.IsDone())throw std::runtime_error("OCCT tessellation failed");
  uint32_t base=0,fi=0;
  for(TopExp_Explorer it(shape,TopAbs_FACE);it.More();it.Next(),fi++) {
   if(cancel(context))throw std::runtime_error("cancelled between faces");
   TopoDS_Face face=TopoDS::Face(it.Current());TopLoc_Location loc;Handle(Poly_Triangulation) mesh=BRep_Tool::Triangulation(face,loc);
   if(mesh.IsNull())continue;
   bool reversed=face.Orientation()==TopAbs_REVERSED; BRepAdaptor_Surface surface(face);
   if(uint64_t(base)+uint64_t(mesh->NbNodes())>UINT32_MAX)throw std::runtime_error("mesh exceeds 32-bit index capacity");
   for(int i=1;i<=mesh->NbNodes();i++) {
    gp_Pnt p=mesh->Node(i).Transformed(loc.Transformation());double pos[3]={p.X(),p.Y(),p.Z()},normal[3]={0,0,1};
    if(mesh->HasUVNodes()) {gp_Pnt2d uv=mesh->UVNode(i);BRepLProp_SLProps props(surface,uv.X(),uv.Y(),1,1e-6);if(props.IsNormalDefined()){gp_Dir n=props.Normal();double sign=reversed?-1:1;normal[0]=sign*n.X();normal[1]=sign*n.Y();normal[2]=sign*n.Z();}}
    vertex(context,pos,normal);
   }
   for(int i=1;i<=mesh->NbTriangles();i++){int a,b,c;mesh->Triangle(i).Get(a,b,c);if(reversed)std::swap(b,c);triangle(context,base+a-1,base+b-1,base+c-1,fi);}
   base+=mesh->NbNodes();
  }
  return 0;
 } catch(const Standard_Failure& e) { if(error_size){std::strncpy(error,e.GetMessageString()?e.GetMessageString():"OCCT Standard_Failure",error_size-1);error[error_size-1]=0;} }
 catch(const std::exception& e){if(error_size){std::strncpy(error,e.what(),error_size-1);error[error_size-1]=0;}}
 catch(...){if(error_size){std::strncpy(error,"unknown native OCCT exception",error_size-1);error[error_size-1]=0;}}
 return 1;
}

// ---- Building: each call reads its input B-reps, makes one shape and hands
// it back as BRepTools text (RoboCAD's `serialize` format). Face and edge
// indices are TopExp::MapShapes order: RoboCAD's `occ_faces`/`occ_edges`
// (explorer order, shared sub-shapes once).
extern "C" {
typedef void (*BytesCallback)(void*, const unsigned char*, size_t);
typedef void (*PolylineCallback)(void*, const double*, size_t);
typedef void (*FaceCallback)(void*, uint32_t, int32_t, const double*);
typedef void (*EdgeCallback)(void*, uint32_t, int32_t, const double*);
}
static TopoDS_Shape read_shape(const unsigned char* bytes,size_t size) {
  std::string payload(reinterpret_cast<const char*>(bytes),size);std::istringstream stream(payload);
  TopoDS_Shape shape;BRep_Builder builder;BRepTools::Read(shape,stream,builder);
  if(shape.IsNull()||stream.bad())throw std::runtime_error("could not read the B-rep data");
  return shape;
}
static void need(bool ok,const char* why){ if(!ok) throw std::runtime_error(why); }
static void valid(const TopoDS_Shape& shape,const char* why){ need(!shape.IsNull(),"the operation produced no geometry"); need(BRepCheck_Analyzer(shape).IsValid(),why); }
static TopoDS_Shape boolean(int op,const std::vector<TopoDS_Shape>& in) {
  need(in.size()>=2,"a boolean needs at least two bodies");
  TopTools_ListOfShape args,tools; args.Append(in[0]); for(size_t i=1;i<in.size();i++) tools.Append(in[i]);
  BRepAlgoAPI_BooleanOperation* algo; BRepAlgoAPI_Fuse fuse; BRepAlgoAPI_Cut cut; BRepAlgoAPI_Common common;
  algo = op==6 ? static_cast<BRepAlgoAPI_BooleanOperation*>(&fuse) : op==7 ? static_cast<BRepAlgoAPI_BooleanOperation*>(&cut) : static_cast<BRepAlgoAPI_BooleanOperation*>(&common);
  algo->SetArguments(args); algo->SetTools(tools); algo->Build();
  need(algo->IsDone() && !algo->HasErrors(),"the boolean failed: the bodies may only touch, or one is not a closed solid");
  if(op==6) algo->SimplifyResult();
  TopoDS_Shape out=algo->Shape();
  need(!(op==8 && TopExp_Explorer(out,TopAbs_SOLID).More()==false),"the bodies do not overlap: their common part is empty");
  valid(out,"the boolean produced an invalid solid");
  return out;
}
extern "C" int sim_cad_build(int op,const unsigned char* const* inputs,const size_t* sizes,size_t input_count,
 const double* args,size_t arg_count,const int32_t* ints,size_t int_count,void* context,BytesCallback out,
 char* error,size_t error_size) noexcept {
 try {
  std::vector<TopoDS_Shape> in; for(size_t i=0;i<input_count;i++) in.push_back(read_shape(inputs[i],sizes[i]));
  auto arg=[&](size_t i){ need(i<arg_count,"missing numeric argument"); return args[i]; };
  TopoDS_Shape shape;
  switch(op) {
  case 1: need(arg(3)>0&&arg(4)>0&&arg(5)>0,"box sizes must be positive");
   shape=BRepPrimAPI_MakeBox(gp_Pnt(arg(0),arg(1),arg(2)),arg(3),arg(4),arg(5)).Shape(); break;
  case 2: need(arg(6)>0&&arg(7)>0,"cylinder radius and height must be positive");
   shape=BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(arg(0),arg(1),arg(2)),gp_Dir(arg(3),arg(4),arg(5))),arg(6),arg(7)).Shape(); break;
  case 3: need(arg(3)>0,"sphere radius must be positive");
   shape=BRepPrimAPI_MakeSphere(gp_Pnt(arg(0),arg(1),arg(2)),arg(3)).Shape(); break;
  case 4: need(arg(6)>=0&&arg(7)>=0&&(arg(6)>0||arg(7)>0)&&arg(8)>0,"cone radii must be nonnegative (one positive) and its height positive");
   shape=BRepPrimAPI_MakeCone(gp_Ax2(gp_Pnt(arg(0),arg(1),arg(2)),gp_Dir(arg(3),arg(4),arg(5))),arg(6),arg(7),arg(8)).Shape(); break;
  case 5: { need(in.size()==1,"a transform takes one body"); gp_Trsf tr; tr.SetValues(arg(0),arg(1),arg(2),arg(3),arg(4),arg(5),arg(6),arg(7),arg(8),arg(9),arg(10),arg(11));
   BRepBuilderAPI_Transform t(in[0],tr,true); need(t.IsDone(),"OCCT transform failed"); shape=t.Shape(); break; }
  case 6: case 7: case 8: shape=boolean(op,in); break;
  case 9: { need(in.size()==1,"a fillet takes one body"); double r=arg(0); need(r>0,"fillet radius must be positive");
   BRepFilletAPI_MakeFillet mk(in[0]); TopTools_IndexedMapOfShape edges; TopExp::MapShapes(in[0],TopAbs_EDGE,edges);
   if(int_count==0) { for(int i=1;i<=edges.Extent();i++){ BRepAdaptor_Curve c(TopoDS::Edge(edges(i))); if(c.GetType()==GeomAbs_Line||c.GetType()==GeomAbs_Circle) mk.Add(r,TopoDS::Edge(edges(i))); } }
   else for(size_t i=0;i<int_count;i++){ need(ints[i]>=0&&ints[i]<edges.Extent(),"fillet edge does not exist"); mk.Add(r,TopoDS::Edge(edges(ints[i]+1))); }
   try { mk.Build(); } catch(const Standard_Failure&) {}
   need(mk.IsDone(),"the fillet is too large for that edge: it would consume a neighbouring face. Try a smaller radius, or fillet the neighbours first.");
   shape=mk.Shape(); valid(shape,"the fillet produced an invalid solid; try a smaller radius"); break; }
  case 10: { need(in.size()==1,"a chamfer takes one body"); double d=arg(0); need(d>0,"chamfer distance must be positive");
   BRepFilletAPI_MakeChamfer mk(in[0]); TopTools_IndexedMapOfShape edges; TopExp::MapShapes(in[0],TopAbs_EDGE,edges);
   if(int_count==0) for(int i=1;i<=edges.Extent();i++) mk.Add(d,TopoDS::Edge(edges(i)));
   else for(size_t i=0;i<int_count;i++){ need(ints[i]>=0&&ints[i]<edges.Extent(),"chamfer edge does not exist"); mk.Add(d,TopoDS::Edge(edges(ints[i]+1))); }
   try { mk.Build(); } catch(const Standard_Failure&) {}
   need(mk.IsDone(),"the chamfer is too large for that edge"); shape=mk.Shape(); valid(shape,"the chamfer produced an invalid solid"); break; }
  case 11: { // Extrude closed polygon loops (outer first, then holes) along a vector.
   gp_Vec dir(arg(0),arg(1),arg(2)); need(dir.Magnitude()>1e-9,"extrusion distance must be nonzero");
   size_t points=(arg_count-3)/3; need(arg_count>=12&&(arg_count-3)%3==0,"a profile needs at least three points");
   std::vector<size_t> loops; if(int_count==0) loops.push_back(points); else for(size_t i=0;i<int_count;i++){ need(ints[i]>=3,"each loop needs at least three points"); loops.push_back(size_t(ints[i])); }
   size_t at=0; TopoDS_Face face;
   for(size_t l=0;l<loops.size();l++) {
    need(at+loops[l]<=points,"loop sizes exceed the profile's points");
    BRepBuilderAPI_MakePolygon poly; for(size_t i=0;i<loops[l];i++){ const double* p=args+3+3*(at+i); poly.Add(gp_Pnt(p[0],p[1],p[2])); } poly.Close();
    need(poly.IsDone(),"the profile loop is degenerate (repeated or collinear points)"); at+=loops[l];
    if(l==0){ BRepBuilderAPI_MakeFace mf(poly.Wire(),true); need(mf.IsDone(),"the profile is not planar"); face=mf.Face(); }
    else { TopoDS_Wire hole=poly.Wire(); BRepBuilderAPI_MakeFace mf(face); mf.Add(TopoDS::Wire(hole.Reversed())); need(mf.IsDone(),"a hole loop could not be added to the profile"); face=mf.Face(); }
   }
   BRepPrimAPI_MakePrism prism(face,dir); need(prism.IsDone(),"the extrusion failed"); shape=prism.Shape(); valid(shape,"the extrusion produced an invalid solid (a self-intersecting profile?)"); break; }
  default: throw std::runtime_error("unknown build operation");
  }
  std::ostringstream stream; BRepTools::Write(shape,stream); std::string text=stream.str();
  need(!text.empty(),"OCCT wrote no B-rep data");
  out(context,reinterpret_cast<const unsigned char*>(text.data()),text.size());
  return 0;
 } catch(const Standard_Failure& e) { if(error_size){std::strncpy(error,e.GetMessageString()?e.GetMessageString():"OCCT Standard_Failure",error_size-1);error[error_size-1]=0;} }
 catch(const std::exception& e){if(error_size){std::strncpy(error,e.what(),error_size-1);error[error_size-1]=0;}}
 catch(...){if(error_size){std::strncpy(error,"unknown native OCCT exception",error_size-1);error[error_size-1]=0;}}
 return 1;
}
static void sample(const TopoDS_Edge& e,std::vector<double>& pts) {
  BRepAdaptor_Curve c(e); double f=c.FirstParameter(),l=c.LastParameter(); int n=c.GetType()==GeomAbs_Line?2:24;
  for(int i=0;i<n;i++){ gp_Pnt p=c.Value(f+(l-f)*i/(n-1)); pts.push_back(p.X());pts.push_back(p.Y());pts.push_back(p.Z()); }
}
// The exact plane section as polylines (RoboCAD's `section`: a line edge's
// two ends, any other edge 24 samples).
extern "C" int sim_cad_section(const unsigned char* bytes,size_t size,const double* plane,void* context,PolylineCallback line,
 char* error,size_t error_size) noexcept {
 try {
  TopoDS_Shape shape=read_shape(bytes,size);
  BRepAlgoAPI_Section sec(shape,gp_Pln(gp_Pnt(plane[0],plane[1],plane[2]),gp_Dir(plane[3],plane[4],plane[5])),false);
  sec.ComputePCurveOn1(true); sec.Approximation(true); sec.Build();
  if(!sec.IsDone()) return 0;
  TopTools_IndexedMapOfShape edges; TopExp::MapShapes(sec.Shape(),TopAbs_EDGE,edges);
  for(int i=1;i<=edges.Extent();i++){ std::vector<double> pts; sample(TopoDS::Edge(edges(i)),pts); line(context,pts.data(),pts.size()/3); }
  return 0;
 } catch(const Standard_Failure& e) { if(error_size){std::strncpy(error,e.GetMessageString()?e.GetMessageString():"OCCT Standard_Failure",error_size-1);error[error_size-1]=0;} }
 catch(const std::exception& e){if(error_size){std::strncpy(error,e.what(),error_size-1);error[error_size-1]=0;}}
 catch(...){if(error_size){std::strncpy(error,"unknown native OCCT exception",error_size-1);error[error_size-1]=0;}}
 return 1;
}
// Faces (type, centre, normal at the centre's parameters, area) and edges
// (type, midpoint, length, ends) by index, so a REST caller can name the
// face or edge an operation takes.
extern "C" int sim_cad_topology(const unsigned char* bytes,size_t size,void* context,FaceCallback face,EdgeCallback edge,
 char* error,size_t error_size) noexcept {
 try {
  TopoDS_Shape shape=read_shape(bytes,size);
  TopTools_IndexedMapOfShape faces; TopExp::MapShapes(shape,TopAbs_FACE,faces);
  for(int i=1;i<=faces.Extent();i++){
   TopoDS_Face f=TopoDS::Face(faces(i)); GProp_GProps p; BRepGProp::SurfaceProperties(f,p); gp_Pnt c=p.CentreOfMass();
   BRepAdaptor_Surface s(f); double u=(s.FirstUParameter()+s.LastUParameter())*0.5,v=(s.FirstVParameter()+s.LastVParameter())*0.5;
   double out[7]={c.X(),c.Y(),c.Z(),0,0,0,p.Mass()};
   BRepLProp_SLProps props(s,u,v,1,1e-6); if(props.IsNormalDefined()){ gp_Dir n=props.Normal(); double k=f.Orientation()==TopAbs_REVERSED?-1:1; out[3]=k*n.X();out[4]=k*n.Y();out[5]=k*n.Z(); }
   face(context,uint32_t(i-1),int32_t(s.GetType()),out);
  }
  TopTools_IndexedMapOfShape edges; TopExp::MapShapes(shape,TopAbs_EDGE,edges);
  for(int i=1;i<=edges.Extent();i++){
   TopoDS_Edge e=TopoDS::Edge(edges(i)); BRepAdaptor_Curve c(e); GProp_GProps p; BRepGProp::LinearProperties(e,p);
   double f0=c.FirstParameter(),l0=c.LastParameter(); gp_Pnt m=c.Value((f0+l0)*0.5),a=c.Value(f0),b=c.Value(l0);
   double out[10]={m.X(),m.Y(),m.Z(),p.Mass(),a.X(),a.Y(),a.Z(),b.X(),b.Y(),b.Z()};
   edge(context,uint32_t(i-1),int32_t(c.GetType()),out);
  }
  return 0;
 } catch(const Standard_Failure& e) { if(error_size){std::strncpy(error,e.GetMessageString()?e.GetMessageString():"OCCT Standard_Failure",error_size-1);error[error_size-1]=0;} }
 catch(const std::exception& e){if(error_size){std::strncpy(error,e.what(),error_size-1);error[error_size-1]=0;}}
 catch(...){if(error_size){std::strncpy(error,"unknown native OCCT exception",error_size-1);error[error_size-1]=0;}}
 return 1;
}

// Runtime provenance: header macros alone do not identify the loaded kernel.
// macOS loader callbacks maintain live-image descriptors under their loader
// lifetime boundary; Linux dl_iterate_phdr supplies the equivalent snapshot.
#include <dlfcn.h>
#include <map>
#include <vector>
#include <mutex>
#include <atomic>
#if defined(__APPLE__)
#include <mach-o/dyld.h>
#include <mach-o/loader.h>
#elif defined(__linux__)
#include <link.h>
#else
#error "sim-cad runtime kernel provenance supports macOS/Linux; add inspected loader identity before enabling another platform"
#endif
extern "C" {
typedef bool (*ImageCallback)(void*,const char*,const unsigned char*,size_t);
}
static bool occt_image(const char* path) noexcept {
 if(!path)return false;const char* base=std::strrchr(path,'/');base=base?base+1:path;
 return std::strncmp(base,"libTK",5)==0;
}
#if defined(__APPLE__)
struct NativeImage {std::string path;const unsigned char* build_id=nullptr;std::vector<std::pair<const unsigned char*,size_t>> instructions;};
static std::mutex image_mutex;
static std::map<const mach_header*,NativeImage> images;
static std::atomic<bool> image_collection_failed{false};
static std::once_flag image_registration;
static void image_removed(const mach_header* h,intptr_t) noexcept {
 try{std::lock_guard<std::mutex> lock(image_mutex);images.erase(h);}catch(...){image_collection_failed=true;}
}
static void image_added(const mach_header* h,intptr_t slide) noexcept {
 try{
  Dl_info info{};if(!dladdr(h,&info)||!occt_image(info.dli_fname))return;
  if(h->magic!=MH_MAGIC_64)throw std::runtime_error("OCCT provenance requires native 64-bit Mach-O");
  const mach_header_64* header=reinterpret_cast<const mach_header_64*>(h);
  NativeImage image;image.path=info.dli_fname;
  const unsigned char* command=reinterpret_cast<const unsigned char*>(header+1);
  const unsigned char* end=command+header->sizeofcmds;
  for(uint32_t i=0;i<header->ncmds;i++){
   if(size_t(end-command)<sizeof(load_command))throw std::runtime_error("invalid native Mach-O commands");
   const load_command* lc=reinterpret_cast<const load_command*>(command);
   if(lc->cmdsize<sizeof(load_command)||lc->cmdsize>size_t(end-command))throw std::runtime_error("invalid native Mach-O command size");
   if(lc->cmd==LC_UUID){
    if(lc->cmdsize!=sizeof(uuid_command)||image.build_id)throw std::runtime_error("missing/ambiguous native Mach-O UUID identity");
    const uuid_command* uuid=reinterpret_cast<const uuid_command*>(command);
    bool nonzero=false;for(unsigned char byte:uuid->uuid)nonzero|=byte!=0;
    if(!nonzero)throw std::runtime_error("native Mach-O UUID is zero");
    image.build_id=uuid->uuid;
   }
   if(lc->cmd==LC_SEGMENT_64){

    if(lc->cmdsize<sizeof(segment_command_64))throw std::runtime_error("invalid native Mach-O segment");
    const segment_command_64* segment=reinterpret_cast<const segment_command_64*>(command);
    if(segment->nsects>(lc->cmdsize-sizeof(segment_command_64))/sizeof(section_64))throw std::runtime_error("invalid native Mach-O sections");
    const section_64* section=reinterpret_cast<const section_64*>(segment+1);
    for(uint32_t j=0;j<segment->nsects;j++)if(section[j].size&&(section[j].flags&(S_ATTR_PURE_INSTRUCTIONS|S_ATTR_SOME_INSTRUCTIONS))){
     if(section[j].addr<segment->vmaddr||section[j].size>segment->vmsize||section[j].addr-segment->vmaddr>segment->vmsize-section[j].size)throw std::runtime_error("native instruction section outside segment");
     image.instructions.emplace_back(reinterpret_cast<const unsigned char*>(uintptr_t(section[j].addr)+slide),size_t(section[j].size));
    }
   }
   command+=lc->cmdsize;
  }
  if(!image.build_id)throw std::runtime_error("OCCT shared image requires loaded LC_UUID for runtime build identity");
  if(image.instructions.empty())throw std::runtime_error("OCCT image has no executable sections");
  std::lock_guard<std::mutex> lock(image_mutex);images[h]=std::move(image);
 }catch(...){image_collection_failed=true;}
}
#else
struct ImageVisit {void* context;ImageCallback callback;bool failed;};
static int visit_image(dl_phdr_info* info,size_t,void* opaque) noexcept {
 ImageVisit* visit=static_cast<ImageVisit*>(opaque);if(!occt_image(info->dlpi_name))return 0;
 const unsigned char* build_id=nullptr;size_t build_id_size=0;
 for(ElfW(Half) i=0;i<info->dlpi_phnum;i++){
  const ElfW(Phdr)& p=info->dlpi_phdr[i];if(p.p_type!=PT_NOTE||p.p_memsz<sizeof(ElfW(Nhdr)))continue;
  // A PT_NOTE need not itself be mapped; only read notes fully contained in a
  // readable PT_LOAD, while the loader snapshot holds the image alive.
  bool mapped=false;for(ElfW(Half) j=0;j<info->dlpi_phnum;j++){
   const ElfW(Phdr)& load=info->dlpi_phdr[j];
   if(load.p_type==PT_LOAD&&(load.p_flags&PF_R)&&p.p_vaddr>=load.p_vaddr&&p.p_memsz<=load.p_memsz&&p.p_vaddr-load.p_vaddr<=load.p_memsz-p.p_memsz){mapped=true;break;}
  }
  if(!mapped)continue;
  const unsigned char* note=reinterpret_cast<const unsigned char*>(info->dlpi_addr+p.p_vaddr);size_t remaining=size_t(p.p_memsz);
  while(remaining>=sizeof(ElfW(Nhdr))){
   ElfW(Nhdr) header;std::memcpy(&header,note,sizeof(header));
   const uint64_t name_size=(uint64_t(header.n_namesz)+3u)&~uint64_t(3u);
   const uint64_t desc_size=(uint64_t(header.n_descsz)+3u)&~uint64_t(3u);
   const uint64_t total=sizeof(header)+name_size+desc_size;
   if(total>remaining){visit->failed=true;return 1;}
   if(header.n_type==NT_GNU_BUILD_ID&&header.n_namesz==4&&std::memcmp(note+sizeof(header),"GNU\0",4)==0){
    if(build_id||header.n_descsz==0){visit->failed=true;return 1;}
    build_id=note+sizeof(header)+name_size;build_id_size=header.n_descsz;
   }
   note+=size_t(total);remaining-=size_t(total);
  }
 }
 if(!build_id){visit->failed=true;return 1;}
 bool nonzero=false;for(size_t i=0;i<build_id_size;i++)nonzero|=build_id[i]!=0;if(!nonzero){visit->failed=true;return 1;}
 static const unsigned char build_schema[]="ELF GNU build-id";
 if(!visit->callback(visit->context,info->dlpi_name,build_schema,sizeof(build_schema))||!visit->callback(visit->context,info->dlpi_name,build_id,build_id_size)){visit->failed=true;return 1;}
 bool any=false;
 for(ElfW(Half) i=0;i<info->dlpi_phnum;i++){
  const ElfW(Phdr)& p=info->dlpi_phdr[i];
  if(p.p_type==PT_LOAD&&(p.p_flags&PF_X)&&p.p_memsz){any=true;if(!visit->callback(visit->context,info->dlpi_name,reinterpret_cast<const unsigned char*>(info->dlpi_addr+p.p_vaddr),size_t(p.p_memsz))){visit->failed=true;return 1;}}
 }
 if(!any){visit->failed=true;return 1;}return 0;
}
#endif
extern "C" int sim_cad_kernel_images(void* context,ImageCallback callback,char* error,size_t error_size) noexcept {
 try{
  // These resolved function addresses verify the kernel actually called by the
  // bridge is in an OCCT shared image, rather than an executable/static fallback.
  using ReadFn=void(*)(TopoDS_Shape&,Standard_IStream&,const BRep_Builder&,const Message_ProgressRange&);
  using VolumeFn=void(*)(const TopoDS_Shape&,GProp_GProps&,Standard_Boolean,Standard_Boolean,Standard_Boolean);
  const void* symbols[]={reinterpret_cast<const void*>(static_cast<ReadFn>(&BRepTools::Read)),reinterpret_cast<const void*>(static_cast<VolumeFn>(&BRepGProp::VolumeProperties))};
  for(const void* symbol:symbols){Dl_info info{};if(!dladdr(symbol,&info)||!occt_image(info.dli_fname))throw std::runtime_error("cannot establish shared OCCT identity for called BRepTools/BRepGProp symbol");}
#if defined(__APPLE__)
  std::call_once(image_registration,[]{_dyld_register_func_for_remove_image(image_removed);_dyld_register_func_for_add_image(image_added);});
  if(image_collection_failed)throw std::runtime_error("OCCT loaded-image collection failed");
  // Removal callbacks hold this same mutex before unmapping, so every pointer
  // stays live for the synchronous callback. Rust retains hashes, never pointers.
  std::lock_guard<std::mutex> lock(image_mutex);
  static const unsigned char uuid_schema[]="Mach-O LC_UUID";
  for(const auto& item:images){
   if(!callback(context,item.second.path.c_str(),uuid_schema,sizeof(uuid_schema))||!callback(context,item.second.path.c_str(),item.second.build_id,16))throw std::runtime_error("OCCT loaded UUID hashing callback refused");
   for(const auto& bytes:item.second.instructions)if(!callback(context,item.second.path.c_str(),bytes.first,bytes.second))throw std::runtime_error("OCCT loaded image identity requires a valid native build ID and successful hashing callback");
  }

#else
  ImageVisit visit{context,callback,false};dl_iterate_phdr(visit_image,&visit);
  if(visit.failed)throw std::runtime_error("OCCT loaded image identity requires a valid native build ID and successful hashing callback");
#endif
  return 0;
 }catch(const std::exception& e){if(error_size){std::strncpy(error,e.what(),error_size-1);error[error_size-1]=0;}}
 catch(...){if(error_size){std::strncpy(error,"unknown OCCT image provenance failure",error_size-1);error[error_size-1]=0;}}
 return 1;
}
