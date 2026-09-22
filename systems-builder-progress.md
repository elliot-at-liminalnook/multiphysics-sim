# Systems builder: progress tracker

Goal (agreed 2026-09-22): build hierarchical multiphysics systems (circuits,
motors, thermal paths, anything in the component library) in the Rust physical
viewer and schematic, drill into components and implement them further
("systems within systems"), group selected parts, use reference images in CAD
and both Rust viewers, and simulate the result through the one shared Rust
runtime. A separate versioned **system file** is the source of truth; the CAD
file and both viewers reference it and edit it through one command layer.

Keep this file current. Mark items `[x]` only with evidence (tests, commands,
screenshots). Record what remains honestly.

## Decisions

- System file: `*.system.json`, schema `sim.system/1`, owned by crate `sim-system`.
  CAD references it by path + content hash; it does not embed it.
- Hierarchy is flattened at compile time into the existing flat `ModelWorld`;
  the solver is unchanged. Flattened component IDs are hierarchical paths
  (`driver/bridge/q1`); subsystem instances become nested description groups.
- Editing a subsystem's contents edits its **definition** (shared by every
  instance, like CAD linked components). `make_unique` forks a definition.
- One command set (`sim_system::Command`) with shared validation and a shared
  on-disk undo journal, used by both viewers, REST and the `sim-system` CLI.
- Reference images are content-hashed asset files next to the system file
  (not embedded), so large images can stay out of Git.
- Library = registry elements (typed ports, units, defaults) + saved subsystem
  definitions in `library/systems/*.definition.json`.
- Future: importers for common circuit formats (SPICE netlists, KiCad).
  Keep the document format importer-friendly; not started.

## The seven agreed items

1. [x] Reference images in both Rust viewers. Physical: textured plane per
   level, drop a PNG/JPEG or import by path, width, opacity, lock, two-point
   calibration, remove. Schematic: backdrop behind the diagram with opacity,
   width and position. Stored as content-hashed files next to the system file.
   Evidence: `sim-system` test `library_round_trip_and_reference_images`;
   screenshot of an imported image under the board (2026-09-22).
2. [x] Hierarchical system document `sim.system/1` (`crates/sim-system`):
   definitions, instances, typed boundary ports, parameters with inheritance,
   nets, references, assets, recorded run settings.
3. [x] Flattening with path IDs (`driver/bridge/q1`), nested groups, spatial
   parts, animation bindings and live capture (`sim_runtime::system_builder`).
   Evidence: analytic RC test identical before/after group, swap, ungroup.
4. [x] Drill-in (levels in both viewers; editing a shared definition reports
   placements; make unique) and swap with port-contract checks and ranked
   alternatives (same interface first; document, library and registry).
5. [x] One command layer with shared on-disk undo/redo and stale-edit
   rejection; REST (`system`, `system_state`, `system_level`, `system_select`,
   `system_undo`, `system_redo`, `system_run`, `system_import_image`) on both
   viewers; `sim-system` CLI; grouping selected parts; library save/import.
   Evidence: `shared_store_history_undo_redo_and_stale_edits`; schematic REST
   edit reloaded live in the physical viewer.
6. [x] Library additions: `electrical.mosfet` (body diode, output capacitance,
   temperature-dependent on-resistance, heat port), `electrical.diode`,
   `electrical.voltage_sense`, `control.pwm`, `control.h_bridge_pwm`.
7. [x] Proving case: motor-driver board (`examples/systems-builder`), with
   `crates/sim-runtime/tests/system_builder_proving.rs` (3 tests pass):
   5 V rail ±1 %, MOSFET heat = I²·2R_on(T) ±5 %, averaged vs switching
   bridge mean speed ±1 %, 10 µs vs 5 µs ±0.5 %.

## Viewer work

- [x] Physical viewer build mode: toolbar (breadcrumb, select/connect,
  group/ungroup/swap, undo/redo, run), tabbed sidebar (library with search
  and category chips, outline, references), inspector (identity, parameters,
  ports, implementation, actions, live values), status bar; IBM Plex Sans.
  Compiles off the UI thread.
- [x] Realistic component models: 12 CAD-built models
  (`cad/scripts/component_models.py` → `library/models/*.obj`, catalog with
  defaults per component type, per-instance override in `appearance.model`).
- [x] Schematic build mode (egui panel): same commands, levels, palette,
  parameters, ports, swap, references behind the diagram, live panel.
- [x] CAD: link a system file (path + SHA-256), change detection, accept,
  unlink (undoable), open in builder; physical export carries the reference.
  Evidence: `cad/tests/test_system_link.py` (2 pass).

## Remaining

- Schematic build panel is functional but not yet restyled like the
  physical view.
- Selection is not linked between the two windows in build mode (edits are).
- The switching board runs far below real time (the averaged bridge is the
  real-time profile; the regulator has no averaged alternative yet).
- Off-screen REST `render` of the physical view draws bounding shapes, not
  the CAD models.
- Abstract elements (thermal nodes, sources, ground) show as small markers.
- Importers for SPICE netlists and KiCad (future, not started).
- Pre-existing, not from this work: `sim-runtime/tests/geometry_evaluation.rs`
  does not compile (`json!` macro), which stops CI before tests; the
  `levitron` phenomenon fails at the pushed commit identically.

## Findings worth keeping

- **Clock drift bug in the shared runtime (fixed 2026-09-22).** `Simulation::run`
  and `SystemSession` advanced time by repeated `t += h`. Rounding
  random-walked the clock; after ~33 ms with two PWM sources an edge missed
  a step end by more than the 64-ulp merge window and split off ~5e-16 s
  steps, which the implicit solve cannot condition (Newton failure or a
  wrong root caught by the second-law check). Steps and ticks now aim at
  the exact grid `start + k·h`. Regression suites must stay green.
- **Implicit midpoint rings on switching circuits.** It enforces algebraic
  constraints only at step midpoints, so endpoint values of algebraic
  quantities (battery terminal voltage) alternate after each switch edge.
  Switching systems record `backward_euler` in their run settings.
- **Floating switch nodes.** With every device off, a switch node is set only
  by tiny leakage; Newton's per-unknown correction test cannot converge.
  MOSFET output capacitance (500 pF) and diode junction capacitance
  (100 pF) keep it well posed; both are real device properties.
- **`robot.h_bridge` outputs float in common mode.** The averaged bridge
  defines only v_p − v_n. The library's averaged alternative wraps it with
  10 MΩ bleed resistors to `supply_n`. The element itself is unchanged
  because robot runs depend on it.

## Log

- 2026-09-22 (later): UI redesign of build mode, CAD component models,
  background compile, run settings in the file, clock-drift fix revised to
  snap roundoff (no change to step sizes), levitron failure shown to predate
  this work.
- 2026-09-22: `sim-system` crate (document, resolver, commands, store with
  shared undo journal, flatten, library, assets) with 9 tests incl. analytic
  RC through group/swap/ungroup. `sim_runtime::system_builder` (compile,
  check, bundle, headless simulate) and `sim-system` CLI. New elements:
  `electrical.mosfet`, `electrical.diode`, `electrical.voltage_sense`,
  `control.pwm`, `control.h_bridge_pwm`. Proving-case board builds and runs
  0.12 s with both integrators after the clock-drift fix.
- 2026-09-22: Surveyed existing code. CAD already has undoable reference images
  (`cad/robocad/references.py`). Rust model is flat; viewers were read-only.
