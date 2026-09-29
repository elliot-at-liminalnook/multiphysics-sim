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

## Audit fixes (2026-09-23)

- Grid snapping is now `SessionConfig.grid_snapping` (set by
  `system_builder::default_config`), so the schematic live panel's worker,
  the physical viewer and headless `simulate` all run system documents with
  the same clock. Older launch files omit it (false). Rebuild
  `sim-system-worker` together with `sim-viewer`.
- Schematic build mode compiles off the UI thread (one job at a time, stale
  revisions discarded); both viewers' `system_state` report `compiling`.
- Physical-viewer REST `system_undo`/`system_redo` return the shared-history
  result or its refusal instead of always succeeding.
- Build-mode annotations use `<system file>.annotations.json` in both viewers.

## Learning library, snapping, worm gear, graphs (2026-09-25)

Asked for: a component library to learn from (names, dynamics, text and
annotations), trade-offs between motors, gears and linear actuators, parts
that snap together when their ports fit, selecting a part shows what else
connects, a physically accurate motor driving a worm gear first, then graphs
in the physical builder.

- [x] `sim_core::ComponentNotes` on registry descriptors (`with_notes`):
  summary, explanation, equations, trade-offs, limits, parameter help,
  `pairs_with`, `typical` starter values, `derived` (geometry→physics
  functions). Exposed through `library::ElementEntry.notes` (REST, both
  viewers). Written for the rotational set, worm gear, lead screw, brushed
  motor, switched supply, electrical ground/source/resistor, translational
  mass/ground/force source. Other elements show ports/parameters only.
- [x] `rotational.worm_gear` and `bridge.lead_screw` on one sliding helical
  contact model (`sim_domain_rotational::helical`): lead angle, ratio,
  directional efficiency and self-locking follow from Shigley's force
  analysis. New helpers: `rotational.load_torque`,
  `electrical.switched_voltage_source` (scheduled on/off edges).
- [x] Proving case `examples/systems-builder/worm-drive/winch.system.json`
  plus library gearboxes `worm_gearbox_30` / `spur_gearbox_30` (same
  interface, swappable). `sim-runtime/tests/worm_drive.rs` (4 pass): motor
  speed and current match closed form to 1e-5, efficiency from simulated
  powers = textbook η, worm holds after power-off, spur swap back-drives at
  the analytic brake speed (0.02 %), step halving < 0.5 %.
- [x] `sim_system::snap`: per-port typed suggestions (curated first),
  structural conflicts (a second lane provider on one node → use a
  coupling), `snap` = import + add (coaxial placement next to the source) +
  connect as one undoable edit; `starter` applies typical values recorded
  as *estimated* provenance. `sim-system/tests/snap.rs` (3 pass) builds a
  running winch only by snapping.
- [x] Physical viewer: library click opens a card (notes, equations,
  ports, parameters with help, derived values, pairs-with chips, place or
  attach to the selected part); inspector About (derived values at current
  parameters) and Snap on sections; Plot chips; graph dock (toolbar
  Graphs) with up to four live charts from the background run's history.
  REST: `system_suggest`, `system_snap`, `system_component`, `system_plot`.
- [x] CAD display models: worm, worm wheel + drum, worm-gear frame, lead
  screw + nut (`cad/scripts/component_models.py`).
- [x] Animation: only single-shaft parts spin (housings with a shaft and a
  case, or two gear ports, stay still).

Remaining from this request: schematic (egui) build panel does not show
notes, snap or graphs yet; notes cover the drivetrain set only; worm μ is
constant (no sliding-speed dependence); lead screw has no example scene;
brushless motors, belts, rack-and-pinion, harmonic/planetary with losses
are not in the library yet; display models are fixed-size (not driven by
parameters).

## Builder roadmap M1–M7 (2026-09-25)

All of `builder-roadmap.md` is implemented; its Evidence table lists the
test that proves each milestone. New pieces, by layer:

- sim-core: `ComponentNotes` (+ `active`, `realtime`, `typical`,
  `derived_with`), `BehaviorRegistry::replace`.
- sim-parts (new): `.part` equation parts: units, parser with file:line
  errors, dual-number Jacobians, 128 factory slots, `PartLibrary` hot reload.
- sim-system: `Study`/`SetStudy`, `snap`, `profile::realtime`,
  `RealtimeProfile`/`SetRealtime`, `UpdateDefinitions`, `ExposeParameter`,
  definition `version`/`realtime`, library `publish`/`stale`/`sync`/
  `where_used`/`cad_physics_commands`, parameter `uncertainty`.
- sim-runtime: `system_study`, `bench` (datasheets), `run_history`,
  `realtime_fidelity`, `part_fit`, `SystemSession::hot_swap`,
  `system_launch` (so `system_builder` compiles for wasm), `system_registry()`.
  CLI: `study`, `datasheet`, `parts`, `realtime`, `fit`, `cad-params`.
- sim-web: `SystemRun`; `web/system-builder/` runner and
  `web/system-realtime-check.mjs`.
- sim-spatial: Studies tab (studies, runs, results), sweep and compare
  buttons, datasheet on cards, publish/where-used/updates/expose, live hot
  swap, Detailed/Realtime toggle, frame-time measurement, REST for all.
- CAD: `robocad/gear_derivation.py`, `scripts/worm_gear_set.py`.
- New elements: `rotational.coulomb_friction`, `rotational.lossy_gear`,
  `translational.load_force`; parts `coreless_motor`, `brushed_motor_eq`,
  `pendulum_gravity`.

Lessons: one lane provider per node (couplings between rigid bodies);
step checks must test convergence (fixed-step comparisons fail stiff
harnesses); midpoint integration gains tiny energy at nonsmooth kinks, so
energy audits look for creation that persists as the step shrinks; live
display frames can be unavailable at event instants (skip, never zero).

## Robot component library (2026-09-25)

96 annotated parts (was 26): every robot-relevant registry element carries
notes (per-crate `notes.rs`, attached by `BehaviorRegistry::annotate` in
`sim_runtime::registry()`), plus 16 new `.part` components in
`library/parts`: bldc_motor, stepper_motor, solenoid, voice_coil,
rack_pinion, timing_belt, hard_stop_rotary/linear, drive_wheel, propeller,
brake, limit_switch, angle/position setpoints, pid_angle/pid_position
(with feed-forward offset). Notes now carry a palette `category`.
Nine named presets (`build_robot_kit` example → library/systems) and five
starter robots in `examples/robot-kit` with saved studies; catalog in
`library/CATALOG.md` (`sim-system catalog`). CAD display models for the new
parts. Tests: `--test robot_kit` (3), `--test datasheets` (96 parts).

Findings: `robot.motor_unit` creates up to (1 − η)·5 mW when lightly
back-driven (its efficiency switch sits at −5 mW, not 0); left unchanged
(calibrated runs depend on it), listed as a known issue in the datasheet
test. Circuits need an electrical ground (a floating battery bus is
singular). A PID derivative gain kd/filter ≈ 50 /m on the drone made Newton
hop between output clamps at t = 0; a 20 ms filter fixes it.

## Discussions, Codex answers, drag placement (2026-09-26)

Built with another agent, reviewed and fixed afterwards.

- [x] Discussions: CAD-style threads stored in the system document
  (`sim_system::display` threads, comments, part/group links bound by
  lineage so renames follow), surface pins in the part frame, saved camera
  views. Physical viewer Discussions tab (`builder/discussion.rs`,
  `builder/markers.rs`); REST `system_discussions`. Drafts are never
  replaced by REST; editing a comment that changed elsewhere keeps the draft.
- [x] Codex answers: `sim-agent` (durable serial queue, lock, restart
  recovery, `codex app-server` protocol, read-only sandbox) and
  `sim-model-context` (read-only engineering context: authored bindings,
  provenance, nets, neighbors, registry notes). Replies are posted by the
  viewer as undoable comments with validated links (`builder/agent.rs`).
  REST `system_agent`, `system_context`. Model and effort come from
  `SIM_CODEX_MODEL` / `SIM_CODEX_EFFORT` (default gpt-6-astra/high); an
  unavailable model fails the run with an error that names it.
- [x] Display placement: drag parts or palette items on the definition's
  grid (snap, Alt bypass, X/Y/Z axis handles). Overlap checks run on a
  latest-position worker; the drop is checked again and saved off the UI
  thread (`builder/placement.rs`, `placement_worker.rs`). REST
  `system_move`, `system_grid`. Display only; physics is unchanged.
- [x] `system_ui`: discover and activate live controls through the same
  handlers as clicks (`builder/ui_api.rs`). Markdown notes and read-only
  source previews confined to the repository (`sim-markdown`).
- Review fixes: a drop that snaps back to its start saves nothing (it used to
  save a no-op revision and then wait forever for a scene that never came);
  a saved drop stops waiting when no rebuild is pending (including a failed
  compile); comments and Codex replies no longer cancel a drag or make its
  drop stale (the drag compares `display::scene_hash`, and the commit
  retries if only discussions changed). Tests in `placement.rs`.

## Remaining

- Codex "read-only" is enforced for files only. Its sandbox has network
  access and the prompt points it at the viewer REST port, which also
  accepts mutation commands. A read-only endpoint or token is still needed.
- Auto-answer has no spending ceiling (up to 32 queued turns of up to
  30 min each).
- The Discussions and drag UI were tested but not yet inspected on screen.
- Solver issue (found 2026-09-25, pre-existing): a low-resistance motor with
  its heat port wired straight to `thermal.ambient`, shaft and case both
  grounded, fails Newton at t = 0 (trace: the initial state already holds
  inconsistent multiplier values; steps crawl under line search). Through a
  thermal mass it runs. Benches use a 10 kJ/K mass; examples wire windings
  through capacitance + conductance.
- The new desktop panels (studies, snap, datasheets, realtime toggle) were
  built and exercised through REST but not yet inspected on screen.
- Schematic build panel is functional but not yet restyled like the
  physical view.
- Selection is not linked between the two windows in build mode (edits are).
- The switching board runs far below real time (the averaged bridge is the
  real-time profile; the regulator has no averaged alternative yet).
- Off-screen REST `render` of the physical view draws bounding shapes, not
  the CAD models.
- Abstract elements (thermal nodes, sources, ground) show as small markers.
- Importers for SPICE netlists and KiCad (future, not started).
- Pre-existing, not from this work: the `levitron` phenomenon fails at the
  pushed commit identically. (`sim-runtime/tests/geometry_evaluation.rs`
  compiles again in the working tree as of 2026-09-23.)

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
