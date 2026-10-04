# In-process CAD archive and exact properties

Source reviewed, unexecuted. This crate has not been built, tested or opened in a
viewer in this batch. It owns the complete original `.rcad` ZIP bytes, every
entry, original manifest JSON (including unknown fields), and a separate resolved
node projection. Opening never rewrites an archive or updates its saved timestamp.
The model identity is SHA-256 of the actual input ZIP bytes, not its pathname or
claimed document revision. Source fingerprinting includes the Rust archive,
component interpreter, geometry boundary, native bridge, and build metadata;
the mass layer additionally fingerprints its implementation and the loaded OCCT\nimplementation identity.

## Binding decision and deployment

Use a narrow C ABI bridge compiled with `cc`, directly linked to OCCT 7.7.x or
7.8.x. The Python reference pins `cadquery-ocp==7.7.2`; authoritative signatures
were read in [OCCT 7.7.2 source](https://github.com/Open-Cascade-SAS/OCCT/tree/V7_7_2/src)
and the corresponding 7.8.1 headers. This deliberately exposes the exact subset
needed rather than adopting a general wrapper whose coverage or handle-thread
safety has not been established. There is no process, server, HTTP or Python
bridge and no unavailable-kernel runtime stub.

The host must provide matching OCCT development headers and native libraries.
`OCCT_INCLUDE_DIR` points to the directory containing `Standard_Version.hxx`;
`OCCT_LIB_DIR` points to its libraries. Common Homebrew/system include locations
are discovered without invoking an external discovery process. Link dependencies
are `TKMesh`, `TKPrim`, `TKTopAlgo`, `TKBRep`, `TKGeomBase`, `TKG3d`, `TKG2d`,
`TKMath`, and `TKernel`, plus their distributor's transitive dependencies and the
platform C++ runtime. Packaging must carry the corresponding shared libraries
and loader paths, and comply with OCCT's LGPL 2.1 plus exception or commercial
license.

### Installing OCCT (no package manager needed)

`build.rs` looks in `~/.local/occt-7.7.2` first. To build it there (about
30 minutes; only the modelling modules, no graphics):

```sh
B=~/.local/occt-build; mkdir -p $B && cd $B
python3 -m venv tools && tools/bin/pip install cmake ninja
curl -sSLO https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V7_7_2.tar.gz && tar xzf V7_7_2.tar.gz
mkdir build && cd build
PATH="$B/tools/bin:$PATH" cmake ../OCCT-7_7_2 -G Ninja -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DCMAKE_BUILD_TYPE=Release -DINSTALL_DIR=$HOME/.local/occt-7.7.2 \
  -DCMAKE_INSTALL_NAME_DIR=$HOME/.local/occt-7.7.2/lib -DCMAKE_MACOSX_RPATH=OFF \
  -DBUILD_MODULE_Visualization=OFF -DBUILD_MODULE_ApplicationFramework=OFF \
  -DBUILD_MODULE_DataExchange=OFF -DBUILD_MODULE_Draw=OFF -DBUILD_MODULE_DETools=OFF \
  -DUSE_FREETYPE=OFF -DUSE_TBB=OFF -DUSE_FREEIMAGE=OFF -DUSE_RAPIDJSON=OFF -DUSE_OPENGL=OFF -DUSE_TK=OFF
PATH="$B/tools/bin:$PATH" ninja && PATH="$B/tools/bin:$PATH" ninja install
```

The absolute install name means binaries find the libraries without an rpath.
STEP and other exchange formats need `-DBUILD_MODULE_DataExchange=ON` (and its
dependencies) when the CAD export work reaches them.
OCCT 8.x is refused at compile time until its APIs and archives are requalified.
`OCC_VERSION_COMPLETE` is labelled as the compilation header version, never the
runtime library version. `kernel_identity_with` independently fingerprints the
actual loaded OCCT implementation: loader-reported exact paths, loaded Mach-O
`LC_UUID` or ELF `NT_GNU_BUILD_ID`, synchronous executable image bytes, and full
distribution-library SHA-256 hashes. The loaded UUID/build ID distinguishes the
original native build if a library file is replaced after process startup,
including builds with different constants but identical instruction bytes.
Native distributions must retain a nonzero, unique-per-build linker UUID/build ID;
missing identifiers refuse derivation rather than provide ambiguous attribution. Paths are
grouped and hashed in deterministic byte order; no address/ASLR value enters the
identity. All nine linked kernel images must be identifiable. `dladdr` also
confirms that the actually called BRepTools/BRepGProp symbols reside in OCCT
shared images. Static or unidentified native implementations refuse derivation
instead of falling back to a header-only identity.

macOS add/remove-image callbacks use the documented loader lifetime boundary
(`mach-o/dyld.h`) and protected descriptors; removal cannot unmap a captured
image until its synchronous hashing finishes. Linux `dl_iterate_phdr` supplies
loaded executable segments ([glibc loader source](https://github.com/bminor/glibc/blob/release/2.39/master/elf/link.h)).
No native pointers survive Rust callbacks. Complete library files are hashed
separately, with changes during reading refused. Cancellation is checked for each
native section/chunk, each library, and every 64 KiB file read. Platform loader
binding that changes instruction bytes intentionally changes the identity;
mutable kernel runtime state is not treated as implementation source. Compilation
and archive compatibility across these deployment versions remain unexecuted.

Coverage evidence:

- [BRepTools.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepTools/BRepTools.hxx)
  provides stream `Read`; it consumes the same `BRepTools.Write` archive payload
  the reference persists. Existing turntable bytes inspected by `unzip -p` start
  `DBRep_DrawableShape` / `CASCADE Topology V3`; despite the reference docstring's
  word “binary”, these are OCCT's serialized ASCII B-reps. Future unsupported
  headers produce a path/node/content error; no format rewrite is attempted.
- [BRepGProp.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepGProp/BRepGProp.hxx)
  supplies closed-shell `VolumeProperties` with `UseTriangulation=false`.
  [GProp_GProps.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/GProp/GProp_GProps.hxx)
  supplies volume, centroid and the full centroidal tensor. Triangles are display
  only. Non-volume inputs retain zero mass/inertia and the reference bounds midpoint.
- [BRepMesh_IncrementalMesh.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepMesh/BRepMesh_IncrementalMesh.hxx),
  [Poly_Triangulation.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/Poly/Poly_Triangulation.hxx),
  [BRepLProp_SLProps.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepLProp/BRepLProp_SLProps.hxx)
  and face orientation/location supply display vertices, surface normals,
  triangle winding and face indices. Meshing is explicitly nonparallel.
- [TopExp_Explorer.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/TopExp/TopExp_Explorer.hxx)
  supplies ordered regions: multiple solids, sheet faces, otherwise the original
  body, matching reference `unjoin`. These indices carry material regions.
- [gp_Trsf.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/gp/gp_Trsf.hxx)
  and [BRepBuilderAPI_Transform.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepBuilderAPI/BRepBuilderAPI_Transform.hxx)
  implement placed/mirrored instances and rigid component placements. Bodies
  are already world baked; their metadata transform is never applied again.
- [BRepPrimAPI_MakeBox.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepPrimAPI/BRepPrimAPI_MakeBox.hxx)
  and [BRepPrimAPI_MakeCylinder.hxx](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_7_2/src/BRepPrimAPI/BRepPrimAPI_MakeCylinder.hxx)
  provide the archived component definition's bounded primitive regeneration.
  This is required because linked occurrence B-reps are deliberately omitted
  from archives. Existing parametric-quadruped archive inspection found only two
  embedded definition B-reps for its nested occurrences.

Every native handle is automatic RAII storage inside one synchronous bridge call
on the loading job's thread. Only owned numeric buffers return to Rust. No native
handle implements `Send`/`Sync`. A cancellable global mutex conservatively
serializes kernel work, including its internally mutable caches. C++ exceptions
are caught before the ABI return and contextualized by Rust. The Rust cancellation
callback catches unwinds; callers must supply nonpanicking progress callbacks.
Cancellation is checked between archive entries, component definitions/features/
members, bodies, transforms, solids, faces, and after the native call. A single
`Read`, exact integration, transform or meshing call cannot currently be
interrupted internally. Late/cancelled results remain the viewer job owner's
responsibility; no native handle or source mutation outlives cancellation.

## Reference ledger and limits

Archive: `cad/robocad/document.py:499–636`, `components.py:172–216`;
replacement: `src/archive.rs` owns raw bytes/metadata and `src/component.rs`
resolves pinned embedded definitions, parameter expressions, family variants,
nested overrides, port remapping and rigid placements. Unknown archive content
remains owned, including NPZ/image/thumbnail bytes. Rendering/editing reference
meshes, images, sketches and the full modelling catalogue remain later migrations.
The bounded component expression interpreter uses typed units and arithmetic,
not executable scripts. Unrecognized feature/schema content is refused by name.
Joint feature arguments are validated but joint export/editing is outside this
batch. The original archive remains available unchanged for later migrations.

Geometry: `cad/robocad/document.py:454–493`, `kernel/occt.py:814–838`,
`864–872`, `961–1052`, `1272–1300`; replacement: `src/geometry.rs` resolves
body/instance inputs and `native/bridge.cpp` reads, places, enumerates, integrates
and tessellates. `io/snapshot.py:27–45` establishes visible display consumers;
visibility is retained in the document projection and interpreted by the viewer.

Unexecuted acceptance cases: existing ordinary/compound/component `.rcad`
archives; unknown metadata and opaque-entry byte retention; malformed BRep
path/node errors; cyclic/missing instances and unsupported schema refusal;
parameterized box/cylinder with nested placement/port bindings; translated,
rotated, mirrored and scaled instances; zero-volume off-origin sheets; ordered
compound material regions; cancellation during ownership wait, archive/component
stages and after each kernel call. For exact analytic boxes/cylinders expect
relative volume/inertia error <=1e-8 and centroid error <=1e-6 mm; reference corpus
comparison tolerances must be recorded with deployed kernel versions before
claiming parity. Tessellation is not bit-identical and must be compared by geometry
and the declared tolerance. These expectations are proposed checks, not results.
