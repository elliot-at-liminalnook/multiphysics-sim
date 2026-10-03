# Native CAD archive and physical inspection

Scope: `cad-rust-physical-derivations`, CD1–CD4. Verification is by source
review only, unexecuted. No builds, tests, GUI launches, servers, screenshots or
hardware operations establish this change. Historical CAD receipts remain
historical and do not validate this implementation.

## Ownership and boundaries

The initial checkout contained user edits in AGENTS.md, Cargo.toml,
cad/files/mod.rs, native-viewer.md and tools/claude-pair/**. Those edits are
preserved and excluded from implementation staging. The files module contained
two user-owned format-string brace repairs. Workspace membership and architecture
additions are staged as owned hunks. Legacy Python/browser references and existing
CAD archives remain unchanged.

The archive/kernel implementer owns `crates/sim-cad` except mass derivation;
the mass implementer owns its mass module; the native implementer owns viewer
integration. The lead owns documentation, workspace dependency changes,
integration repairs, reviews and commits.

The Rust archive is durable source data. The kernel produces immutable numeric
snapshots on a jobs worker; native handles never enter Bevy resources. Exact
OCCT volume integration supplies physical properties; display triangles never
substitute for those properties. CAD bodies already contain world placement.
Instance placement is applied once during resolution.

## Archive observations

Read-only inspection of existing archives on 2026-10-03 found:

| Archive | Format version | Nodes | B-reps | Component definitions |
|---|---:|---:|---:|---:|
| examples/camera-turntable/cad/turntable.rcad | 1 | 37 | 21 | 0 |
| examples/components/parametric-quadruped/assembly.rcad | 1 | 24 | 2 | 3 |
| examples/components/quadruped-parametric/model/robot.rcad | 1 | 262 | 114 | 23 |

Each inspected B-rep begins with `DBRep_DrawableShape` and `CASCADE Topology
V3`. Format version 1 is the archive envelope version, not the OCCT topology
version. Component occurrences can omit B-reps: their definitions and features
must resolve before geometry is available. Retaining their metadata alone does
not prove that they display correctly.

## Unexecuted acceptance cases

Compare exact B-rep reference and replacement volume, centroid and each tensor
element before comparing tessellation. Start with an analytic rectangular box,
then a rotated anisotropic box, translated bodies, and a compound with different
material densities. Use relative tolerance 1e-8 for analytic volume/mass/inertia
and absolute centroid tolerance 1e-9 m as proposed acceptance limits; establish
corpus tolerances against the same OCCT release before interpreting differences.
These are proposed checks, not measured agreement.

Declared measurements must preserve source, zero mass with included_in,
finite COM, full symmetric physical inertia and the reference validation
tolerances. Invalid declarations must fail by document/node; they must not
silently derive a replacement. Include stale solid-material indices, missing
materials, mixed origins, hidden/disabled ancestors, linked occurrence placement,
unknown metadata and opaque ZIP entries. Cancel before reading, between kernel
calls and during replacement; deliver an old result after a newer request or
mode exit. Verify that failed/cancelled replacement preserves the current
document and selection. No native call may construct a CAD HTTP client or launch
RoboCAD, including ancillary inspectors and mode teardown.

Full modelling, sketch editing, booleans, fillets, print/flex derivations and
physical simrobot export remain outside this batch. Their legacy implementations
are retained as references. Native controls must name migration status rather
than start or contact them.

## Selected workflow inventory

| Workflow / entry point | Reusable Rust layer / source owner | Remaining native gap / dependencies | Reading evidence (unexecuted) |
|---|---|---|---|
| Existing archive: positional `.rcad`, CAD path picker, REST `cad_open` | `sim-cad::ArchiveDocument`; original bytes, manifest and opaque ZIP entries own source definition | OCCT development/runtime libraries; archive schema 1; unsupported content must fail contextually | `sim-spatial/src/main.rs` dispatch and `cad/sync/mod.rs::enter` produce CadOpen; `cad/files/mod.rs::open` uses that handler; `cad/actions.rs::open` requests the same local job |
| Body display, tree and viewport body selection | `sim-cad::geometry`; numeric snapshot; native CadMeshes; DocumentRegistry and shared Selection | Imported NPZ/reference-image display and other unmigrated content are not authoritative B-rep replacements | `cad/sync/mod.rs::request_load` loads geometry; `cad/mesh.rs::sync` builds local meshes; `cad/pick.rs` maps hits to shared selection; `cad/selection/mod.rs::publish` has no remote push |
| Body mass, centroid and full tensor | `sim-cad::mass`; effective document metadata and exact OCCT B-reps | Declared measurements take precedence; material derivations remain provisional unless calibrated | `mass.rs::derive_document_with`; `cad/inspector/node.rs::inspector` reads local per-body results |
| Visible assembly and physical source assembly | Same shared mass implementation, separate inclusion sets | Visible assembly includes instances; physical reference uses body/sheet inventory including hidden/disabled bodies; both labels are explicit | `mass.rs` full parallel-axis aggregation and inclusion; `cad/inspector/sections.rs` shows both results and identities |
| Replacement failure/cancel/reload | CadDocument owns current and pending snapshots; jobs owns expensive work | Cooperative cancellation cannot forcibly interrupt a single OCCT operation | `cad/actions.rs::open`; `cad/sync/mod.rs::{request_load,cancel_load,receive}` captured source generation/revision and request ownership; mode leave drops pending job |

Paths above are under `crates/` unless prefixed with `cad/` below. Display layout,
camera, highlighting and selection never edit the archived physical definition.
The archive is currently read-only in native CAD mode; saving/editing await their
own Rust migration. Unknown archive content is preserved rather than projected
back through UI DTOs.

## Reference/replacement ledger

| Rule | Reference source | Rust replacement source |
|---|---|---|
| Archive manifest, IDs, metadata, snapshots and opaque payloads | `cad/robocad/document.py:499` manifest, `:526` archive snapshot, `:556` load; `components.py:177` definition entries | `crates/sim-cad/src/archive.rs:12` owned raw/effective data, `:25` open_with; `sim-cad/src/component.rs:762` embedded occurrence reconstruction |
| B-rep compatibility and exact geometry | `kernel/occt.py:1272` serialize, `:1285` deserialize, `:989` inertial properties | `sim-cad/native/bridge.cpp:59` stream read, exact BRepGProp properties and tessellation; `src/geometry.rs:378` jobs-callable numeric query |
| Declared measurements, source, zero/included_in, symmetry/PSD/principal bounds | `physical.py:116` metadata validation and `:134` body precedence; `assembly.py:29` shared validation consumer | `sim-cad/src/mass.rs:99` inertia validator; `:256` derivation with contextual validation and measured override |
| Density/defaults, unit factors and per-solid regions | `document.py:127` defaults, `:493` density; `physical.py:108` factors, `:145` regional properties; `kernel/occt.py:864` unjoin | `sim-cad/src/mass.rs:166` density restoration, strict regions, 1e-6 mass/1e-3 length/1e-12 inertia factors; bridge reference region enumeration |
| Bodies already world placed; instance transform/mirror once | `document.py:454` resolved_body; `components.py:108` physical_transform, `:433` restore_occurrences | `sim-cad/src/geometry.rs:341` source resolution and matrices; `sim-cad/src/component.rs:334` component physics/placement remap; mass accepts world numeric properties |
| Assembly centroid/full inertia and physical inclusion | `document.py:430` bodies, `:433` visibility; `physical.py:714` assembly, `:751` centroid/tensor sum | `sim-cad/src/mass.rs:127` weighted full-tensor parallel-axis aggregation, separate display/physical inclusion and reference denominator floors |
| Model and production derivation identity | Actual archive bytes; reference snapshot reuse `document.py:532` and assembly geometry dependencies `physical.py:722` | `sim-cad/src/archive.rs:32` SHA-256 original archive; `sim-cad/src/lib.rs:9` production_source_identity; `sim-cad/src/mass.rs:510` derivation_identity includes arithmetic, schema and loaded OCCT build identity; saved/declaration attribution retained alongside derived timestamp |
| Native positional, picker and REST opening | Historical service launch/poll has been superseded | `sim-spatial/src/main.rs:246`, `sim-spatial/src/app/picker/discover.rs`, `sim-spatial/src/cad/files/mod.rs:282`, `sim-spatial/src/cad/actions.rs:643`, `sim-spatial/src/cad/sync/mod.rs:123`; one typed CadOpen path |
| Local results, staleness and cancellation | Native jobs/generation contract architecture §4 | `sim-spatial/src/cad/sync/mod.rs:133` sole snapshot landing point; `sim-spatial/src/cad/document/state.rs:150` blockers; `sim-spatial/src/app/switch/leave.rs:94` pending cancellation; local selected paths never construct CadClient |

The native retains some `cad_client` value DTOs for display compatibility and
inactive legacy feature implementations. This is not an active network adapter:
local documents never construct the client, launch the service or poll it.
Controls for unmigrated operations must refuse before their legacy handler can
send anything. The closure audit distinguishes these retained types and references
from actual calls; searching for an import alone cannot establish either parity
or a live network dependency.

## Decisions and review repairs

Use a narrow OCCT C ABI bridge for the required stream/property/display subset;
retain source ownership in Rust and confine native handles to a serialized job
call. General wrappers and a pure Rust kernel were rejected because inspected
coverage did not establish this archive and exact-tensor contract. Revisit the
binding after an inspected wrapper supplies those operations with equivalent
ownership and deployment evidence. OCCT 7.7/7.8 native linking remains a host
prerequisite, not a Python/service dependency.

Interpret only the embedded component catalogue needed to reconstruct existing
occurrences (box/cylinder generation, rigid placement, nested definitions,
families, bindings and typed arithmetic). This is opening behaviour; it does
not add a modelling UI. Unknown schema/features/expressions fail contextually
and preserve the original source. Revisit the supported subset when new archive
content requires additional reference behaviour.

Show visible and physical source assemblies separately: the display inclusion
rule and reference physical body inventory differ. Neither result silently
replaces the other. Keep density-based results provisional and declarations
labelled `declared` unless the declaration explicitly supplies its origin.
Inspection of the quadruped archive found source text combining a specified
52 g mass with estimated COM/inertia; a generic declaration must not acquire a
`measured` label merely because it overrides geometry. Its original source and
all declaration metadata remain preserved.

Source reviews found and repaired sheet midpoint centroids and regional
enumeration, nested component placement composition, external port-bound
included_in remapping, primitive regeneration's archived input handling,
Python unary/power precedence, component-stage cancellation, refresh blocker
checks, acceptance-time dirty checks, and display coordinate overflow. Current
source evidence is not execution evidence. Matching deployed native dependencies,
complete archive corpus comparisons and rendered selection/GUI parity remain
unexecuted. Old server-backed receipts and compatibility fixtures remain
historical; they do not certify this implementation.

## Batch checklist (source-reviewed, unexecuted)

| Required ID | Source evidence / scope |
|---|---|
| cad-rust-physical-derivations:task-CD1 | `sim-cad/src/archive.rs:25` bytes/manifest/opaque entries; component reconstruction; `geometry.rs::load_geometry` directly calls `native/bridge.cpp` OCCT query; native dependency and compatibility evidence in crate README |
| cad-rust-physical-derivations:task-CD2 | `sim-cad/src/mass.rs:256` declared/zero/included_in and density/regions; `:127` full tensor aggregation; archive identity and fingerprinted production/kernel implementation |
| cad-rust-physical-derivations:task-CD3 | `sim-spatial/src/cad/actions.rs:643` local open; `cad/sync/mod.rs:75` jobs and `:133` sole landing point; local meshes, shared selection, inspector and refusal gates |
| cad-rust-physical-derivations:task-CD4 | This inventory/reference ledger, architecture §9, CAD checklist/parity notices and README launch instructions; independent reading reviews and owned-file commit |
| cad-rust-physical-derivations:outcome-1 | Picker `cad/files/mod.rs:282`, positional `main.rs:246` → `cad/sync/mod.rs:123`, REST → `cad/actions.rs:643` → same local loading job |
| cad-rust-physical-derivations:outcome-2 | `cad/mesh.rs:408` consumes accepted local triangles; `cad/pick.rs` raycasts those entities and typed body selections reach the shared Selection; `cad/selection/mod.rs:69` never pushes to a server |
| cad-rust-physical-derivations:outcome-3 | Reference/replacement ledger above; SI conversions, declared origin/source and complete tensors retained; visible and physical inclusion sets explicitly separate; numerical agreement remains unexecuted |
| cad-rust-physical-derivations:outcome-4 | `cad/actions.rs:482` supported-action gate; file/tree/display gates and permanently absent local CAD client prevent legacy adapters from running; service launch and remote selection/poll implementations removed; legacy Python/browser sources preserved |

The checklist records implementation reading evidence, not Director acceptance,
compilation, executed numeric agreement or full CAD parity. Header version is
labelled separately from loaded native-library identity. The latter fingerprints
loaded Mach-O UUID / ELF GNU build-ID, instruction bytes and complete distribution files so replacing a kernel
implementation cannot retain an identity merely by sharing a version string.
Unsupported OS/static-library provenance or unreadable native images refuse
derivation explicitly. Linking, loader packaging and the archive corpus still
need an authorized execution pass.
