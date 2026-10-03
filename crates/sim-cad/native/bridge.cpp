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
#include <sstream>
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
