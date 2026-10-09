# Finish the CAD migration and close the composition gaps — plan (2026-10-08)

Two pieces of the Rust-only, in-process architecture are unfinished:

- **CAD.** The viewer edits `.rcad` files in process, but seven feature areas
  still refuse by name because they lived in RoboCAD's Python service, and the
  old HTTP client (`sim_runtime::cad_client`, 7,500 lines) is still compiled in.
- **Composition** (`docs/architecture/composition.md`, "Limits"): Robot mode
  runs the robot alone instead of the project's system; Build mode cannot
  teleoperate a robot; the articulated robot element takes its model through a
  process-local handle; a block merges every island it touches into one.

Decisions (2026-10-08, the user):

- The CAD parity harness (`cad_parity`) is retired with the rest of the client.
  RoboCAD's Python stays in `cad/` as the behaviour reference only.
- Model scripts become Rhai; Python model scripts are dropped.
- Teleoperation drives a system's **setpoint inputs** when controllers already
  own the joint targets (implemented now, not deferred).
- Write all the code first; build and test only after every step is written.

## Part B: composition

### B1. Blocks no longer merge islands

A block's inputs are sampled by the scheduler from committed state, and its
outputs are held constant between ticks, so neither needs the block's shadow
element to share an island with the plant. Island grouping ignores the
connections that touch a block's shadow element. Each island that reads a
block output gets its own held copy (a `block.held` element: one state per
output, zero rate); the scheduler writes every copy of an output at a tick.
Block inputs are read from wherever their plant source lives.

Test: two rooms, each with its own thermostat block, compile to two islands
(was one) and give the same temperatures as before within solver tolerance.

### B2. The robot model travels with the model world

`register_model` parks a model in a global list that only grows and hands out
an index. Replace it with content-addressed resources: `ModelWorld.resources`
maps a key to the model's JSON text; the key is the first 52 bits of the
text's BLAKE3 hash, so it is exact as the element's `f64` parameter
(`sim_core::resources`; a colliding key with other text is refused);
compiling installs the world's resources into a process cache keyed by it;
the factory reads the cache. A `ModelWorld` (and so a
flattened system) can be serialised and rebuilt in another process.

Test: assemble a robot, serialise the world, deserialise it, compile both, and
run both: identical states.

### B3. Teleoperation as a host block (`drive_input`)

A system file may hold a `drive_input` host block. Its outputs are the drive
profile's commands; Build mode's live run binds it to the window's drive input
(keys, gamepad and REST `system_drive`) through `system_blocks::bind_with`.
Headless runs refuse it by name. It may connect only to inputs that nothing
else drives.

### B4. Robot mode runs the project's system, and drives setpoints

When the open model belongs to a robot project, Robot mode flattens, compiles
and binds that project's system and measures it with `PhysicalRobot::attach`,
as the acceptance test does. Teleoperation is the B3 host block, connected to:

1. the robot's `<joint>.target` inputs that nothing drives, and
2. **setpoint inputs**: block inputs a controller declares as setpoints
   (`BlockPort.setpoint`: the joint it commands, or `*` for the joint the
   controller drives; FMU: a `causality="input"` variable whose
   `<Annotations>` hold `<Annotation type="sim.setpoint">joint</Annotation>`,
   empty meaning `*`; or the system command `set_block_setpoint` on the
   block instance) that nothing drives. An unconnected setpoint input holds
   its start value, so it is not a dangling port; the drive maps to it like
   a target.

Joints whose targets are driven by a controller with no free setpoint are
refused by name. A model without a project runs as before.

## Part A: CAD

| # | Feature | Approach |
|---|---|---|
| A1 | Mesh import | STL, OBJ and 3MF read in Rust into a mesh node, with RoboCAD's unit guess and a unit argument |
| A2 | Print checks | wall thickness (ray hits), validate for export, overhangs; on the viewer's job pool |
| A3 | Print studies and plans | study written from the archive, analysed by `sim-print`, plates packed and written as 3MF, results overlay |
| A4 | Split for printing | port `print_split.py` and `print_strength_split.py`: cut planes, dowel, insert and dovetail joints, one undo step |
| A5 | Assembly guide and coupons | exploded view, HTML guide, coupon geometry, results template |
| A6 | Experiments | in-process experiments on the export (`sim_runtime`), captured review from run records, candidates as staged edit batches, identification applied through the actuator registry |
| A7 | Model scripts | Rhai over `sim_cad::ops`, staged and published as one undo step; the two turntable scripts converted |
| A8 | Components and assemblies | `sim-cad`'s read-only component interpreter grows into an editor: library, definitions, parameters, occurrences, regeneration, assembly (composition) edits |
| A9 | Delete the old client | panel types moved into `sim-cad`/`sim-spatial`; `cad_client`, `loopback_http` (if unused), `cad_parity`, the attach field, the child-process slot, `CadDocument.client`, stale comments; CAD switch tests rewritten to local archives |

## Afterwards

Build, run the crates' tests, a REST walkthrough per feature in the live
viewer, guide commands updated (`cad_guide`, `system_guide`, `robot_guide`),
and an entry in `docs/architecture/native-viewer.md`.

## Status (2026-10-08)

Every A and B step is written; the build, tests and REST walkthroughs follow.

- **Composition (B1–B4).** Blocks no longer merge islands (a block's held
  outputs are copied into each island that reads them); the robot model is a
  content-addressed resource; `drive_input` host blocks (`sim_runtime::teleop`)
  teleoperate a Build-mode run (`system_add_drive_input`, `system_drive`);
  Robot mode runs a project's composed system (`compose_robot`) and jogs
  joints through free targets or controller setpoints, refusing the rest by
  name (`jog_refused`).
- **CAD (A1–A9).** Mesh import (STL, OBJ, PLY, 3MF) and print checks
  (`sim_cad::printing`); print studies, split, whole-or-split, plates (3MF),
  assembly guides and coupons (`sim_cad::print`, run as jobs; the stress check
  and planner are `sim_runtime::print_tools`, which the `sim-print` binary now
  wraps); experiments, candidates, captured review and identification
  (`sim_cad::experiments`, executed on viewer jobs with `sim_phenomena`'s
  runner); Rhai model scripts (`sim_cad::scripts`, `cad_script`; the turntable
  scripts are `.rhai` now, their Python deleted); components and composition
  editing (`sim_cad::component_edit`, `sim_cad::component_graph`); reference
  pose and motion patterns (`sim_cad::pose`). `sim_runtime::cad_client` and
  `cad_parity` are deleted; the panels' types are `sim-spatial`'s
  `cad::types`; the attach field, the child slot and `CadDocument.client` are
  gone; the CAD switch tests open local archives.

Limits, said where they apply:

- Flexible links are not derived in process: exports are rigid and say so; an
  experiment with `settings.flex` is refused (the validation profile needs
  `flex: false`).
- Geometry-derived components (`derivation` recipes) are validated but not
  derived inside experiments; such a run is refused by name.
- Process controllers belong to the controller seam; a captured experiment
  takes Rhai controllers only.
- Removing a composition component drops its ports from their connections;
  RoboCAD also dropped connections left with only signal receivers, which
  needs the native port schemas and is not done here.
- `loopback_http` and `hardware_client` stay: the hardware sessions still use
  them (moving hardware in process is separate work).
