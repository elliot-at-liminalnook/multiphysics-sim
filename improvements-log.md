# Improvements log — 2026-09-03 evening sprint (21:16–22:55 EST)

Running log of changes made in the time box, newest last. Each entry: what,
where, how it was checked (quick checks only).

**Where the evening started from** (all done earlier the same day, before
the sprint): the control seam (`control.external`, `sim-couple`, Python/C
clients, sensing domain, chain element, plates 27–32), then the solver
work — modified Newton with Jacobian reuse, element-wise Jacobian assembly
with analytic partials, sparse LU (`faer`), and compile-time elimination of
signal and rate-lane unknowns.

**Validation policy during the sprint:** per change, one build plus the
narrowest test or plate that exercises it; the full 34-plate suite and the
workspace tests run once at the end (launched 21:47, results in the
closing section).

- **21:16** Plate 33 `scaling-ladder` landed: cost per step ∝ unknowns^0.98
  (25→800 rungs, 104→3204 stored unknowns, 0.10→2.8 ms/step); the solver
  carries 75 % of the unknowns after elimination. Threshold on the
  elimination check set to "at least a fifth" (it was a guess of a third).
- **21:20** Scratch-buffer reuse: the island's reduced residual and the
  implicit step no longer allocate per evaluation (`Island.scratch`,
  `RefCell` stage buffers). Gain is small (sample-rate 19.7→19.1 s) — the
  allocator was not the bottleneck; kept because it is free.
- **21:22** Analytic Jacobians for `rotational.ideal_gear`, the sensing
  chain (`Chain::jacobian`: raw mirror, lags, sample-and-hold rows and the
  published value), the encoder, tachometer, voltmeter, ammeter and load
  cell. Assembled Jacobian still matches whole-system differences to 1e-8
  on the quadruped (`tests/jacobian.rs`); only the chain, contacts and IMU
  are differenced now.
- **21:23** `sim_couple::python(root, script, args)` and `sim_couple::c(root,
  source, binary, args)` — the library now spawns Python controllers (with
  `PYTHONPATH`) and compiles-and-spawns C ones; the phenomena crate's private
  helpers moved onto them. `sim-app --scene phenomena --exhibit quadruped`
  replaces the environment variable.
- **21:23** `sim_couple::DynamicCoupler`: a controller as a shared library
  (`simloop_open/sample/close` C ABI) loaded with `libloading` and called
  in-process — no pipe, no JSON, the cost of a Rust closure. `compile()`
  builds a C source to a dylib when stale. Test compiles a P law and checks it.
- **21:25** The quadruped trot as a shared library
  (`clients/c/examples/quadruped_gait_dl.c`, `Lang::Dylib`): the viewer now
  prefers the in-process controller, then the C process, then Python.
  Quick test `tests/dylib_gait.rs`: 0.6 s of quadruped in 3.4 s wall.
- **21:26** faer's symbolic LU analysis memoised by sparsity pattern
  (`sim_solve::SYMBOLIC`), so a rebuild only refactorises numerically.
  No change on small plates; matters at thousands of unknowns.
- **21:27** One ordered residual pass: producers before consumers, signal
  values filled in as they are produced (`Island::residual_ordered`,
  `eval_order`), instead of an expansion pass plus a residual pass.
  `SIM_NO_REDUCE=1` disables elimination for comparisons.
- **21:29** Open question left deliberately: `sample-rate-instability` runs
  in ~17 s now against ~11 s measured once on the sparse-only build;
  disabling elimination does not change it and removing allocations from
  guards/energy did not either, so it is not the reduction. Not chased.
- **21:31** Step-size control: `Simulation::run_adaptive(duration, h0, tol,
  h_min, h_max)` and `Runtime::advance_adaptive`. Error estimate = distance
  of the implicit step from the explicit prediction on differential
  unknowns; reject and shorten above one, grow (×≤2) below a quarter; a
  step with an event keeps its size. Test: a stiff decay to 2 s in under
  400 steps where a 1 ms grid takes 2000, end value within 1e-3.
- **21:31** `Runtime::attach_python(behavior, clients_root, script, args)`.
- **21:31** Plate 33 gets an exhibit (`LadderExhibit`: rung count knob,
  live ms-per-step readout); README counts 33.
- **21:33** `Runtime::advance` steps independent islands on parallel
  threads (`std::thread::scope`), and `Runtime::set_island_step(behavior,
  Some(h))` gives one island its own step: a first multirate facility for
  models whose electronics and mechanics are separate islands.
- **21:33** `Trace::write_csv(path, names)`.
- **21:35** Element-wise Jacobian assembly runs in parallel with rayon
  (each slot returns its triplets; analytic or differenced). Quadruped
  0.6 s run: 3.43 → 2.29 s wall.
- **21:36** Residual evaluation in parallel across elements on islands with
  64+ non-producer elements (producers first, in order); no gain on the
  quadruped's 60, so gated to large islands. `FrameCoupler::accept(addr)`:
  the simulator can listen and let a controller dial in.
- **21:38** Tried and reverted: a secant step before the event bisection
  (one solve for time-linear guards). It made `sample-rate-instability`
  slower (17 → 31 s) for a reason not found in the time box; the bisection
  stays. Worth a proper look later: the per-event cost is the re-solves.
- **21:40** `Instance::port` names the ports it does have when one is
  misspelled; `Instance::try_port` for non-panicking lookups.
- **21:40** `contact.wheel`: a wheel on a planar frame with its own spin,
  an `axle` rotational port for a motor, compliant normal contact at the
  rim and regularised traction on the slip `v_hub + ω·r` — the part a car
  is four of. Test: a driven unicycle body accelerates at τ/(r(m + I/r²))
  within 5 % and rolls without slip.
- **21:43** `planar.rigid_body` takes a `slope` (a tilted world = a hill).
  Plate 34 `cruise-control`: two-wheel car on the seam holds 10 m/s on
  the flat and on a 6 % grade; the torque the integrator finds on the
  grade is m·g·sin θ·r within 4 %; open loop the hill takes a quarter of
  the speed. Forward is −x in this convention (noted in the plate).
- **21:43** Scaling ladder with parallel assembly: see the run below.
scaling-ladder  (126.03s)
       800 rungs: ms per step                   = 13.697798
       cost exponent: seconds per step ∝ (unknowns)^p = 0.991148
Wed Sep  2 21:40:34 EDT 2026
[exited with code 0]
- **21:46** Exhibit for plate 34 (`CruiseExhibit`: grade knob, tilted road,
  speed / torque / grade-torque readouts). Viewer now has 34 exhibits.
- **21:46** Client READMEs: the shared-library route (C) and the
  `sim_couple::python` / `Runtime::attach_python` route.
- **21:48** `planar.drag` (quadratic drag on a frame, analytic Jacobian);
  test: a coasting body slows as v₀/(1 + c·v₀·t/m) within 1 %.
- **21:48** Parallel residual made opt-in (`SIM_PARALLEL_RESIDUAL=1`): on
  the 800-rung ladder it measured 5.7 ms/step against 2.8 serial (the
  per-evaluation reduction outweighs the element work at this size).
  Parallel *Jacobian* assembly stays on — that one pays.
- **21:49** `sim-phenomena -- list`; README "Run" lines for `list`, one
  plate, the gallery, and `--exhibit`.
- **21:50** Analytic Jacobian for `contact.point_plane_compliant` (normal
  and regularised-friction partials, lever-arm term). The quadruped's
  assembled Jacobian now differs from forward differences by 9e-4 at the
  friction kink — the differences' error, not the analytic one (the test
  tolerance loosened from 1e-4 to 2e-3 with a note).
- **21:56** Regression found by the smoke test and chased: the geyser
  stopped converging. Bisected to the single-pass ordered residual (not
  the sparse LU, not the elimination, not the parallel assembly — each
  tried in turn); the two-pass residual is the default again and the
  ordered pass is opt-in (`SIM_ORDERED_RESIDUAL=1`) until the bug is
  found. Also added on the way: dense LU below 256 unknowns
  (`SIM_SPARSE_FROM` overrides), column equilibration for the sparse
  factorisation. Suite restarted on this build at 21:57.
- **21:58** Workspace tests on the final build: everything passes; the
  exhibits smoke test's only complaint is two exhibits (Levitron, leg)
  at 1.15 s per 2 s of real time, measured while the 34-plate suite was
  hogging the machine (it passed at 7.5 s total on a quiet one earlier).
- **22:16** Second regression from the suite: with elimination on, plate
  18 (motor hogging) linearises to a different eigenvalue (−0.1, the
  thermal mode, instead of the ±0.01 hogging mode); off, it passes.
  Elimination is therefore opt-in for now (`sim_compile::set_elimination`,
  `SIM_REDUCE=1`); the scaling ladder enables it for itself. Suite
  rerun at 22:16.

## Closing (22:37)

- Full 34-plate suite on the final build: **34 passed, 0 failed**, 1227 s
  of plate time (the quadruped's 422 s is three Python/C-driven runs plus
  the in-process one). Gallery regenerated and republished at
  https://claude.ai/code/artifact/0cba195c-d2bb-48b2-9ae7-0faa8b1e96ac.
- Workspace tests: all green (the exhibits real-time check flagged two
  exhibits at 1.15 s per 2 s while the suite shared the machine).
- Landed tonight: parallel Jacobian assembly; sparse/dense factorisation
  by size with column equilibration; symbolic-LU memo; analytic Jacobians
  for the ideal gear, the sensor chains and the compliant contact;
  step-size control (`run_adaptive`, `advance_adaptive`,
  `advance_recording_adaptive`); parallel islands with per-island steps;
  the in-process shared-library coupler (`DynamicCoupler`), the C trot as a
  dylib the viewer prefers, `sim_couple::{python, c}`,
  `Runtime::attach_python`, `FrameCoupler::accept`; `contact.wheel`,
  `planar.drag`, the rigid body's `slope`; plates 33 (scaling ladder,
  exponent 0.98) and 34 (cruise control on a hill) with exhibits;
  `Trace::write_csv`, `sim-phenomena -- list`, `sim-app --exhibit`,
  friendlier port errors; docs and memory.
- Left open, deliberately: the single-pass ordered residual (opt-in, bug
  breaks the geyser); compile-time elimination (opt-in, changes plate 18's
  linearised mode); the secant event location (reverted, slower);
  `sample-rate-instability` at 17 s vs an 11 s measurement once seen.

## The quadruped's lag (3 September, morning)

The trot in the viewer was far slower than the physics warranted. A
`sample` of the live process plus a new opt-in profiler
(`sim_solve::profile`, printed by
`cargo run --release -p sim-phenomena --example quadruped_profile`) put
the plate at 34.6 ms per 1 ms step, 123 unknowns. Five causes, all fixed:

1. **Dense factorisation's singularity check** rebuilt the n×n upper
   triangle once per diagonal element (`lu.u()` inside the loop): 3.7 ms
   per factorisation, 91% of the step. Now reads the packed factor's
   diagonal. Every island under the sparse threshold felt this.
2. **Event location by bisection** to 1e-6 of the step cost about 20
   trial implicit solves per controller sample, each with its own fresh
   Jacobian; 65% of the step. The locator is now regula falsi (Illinois)
   with a midpoint fallback, accepts a crossing in value as well as in
   time, hugs the bracket ends by a tolerance so a crossing at a step
   boundary settles in one trial, and the step's factorisation is kept
   aside during the search rather than evicted by it. A controller sample
   now costs about 0.5 ms instead of 90.
3. **The viewer's step pattern**: one step per frame of frame × time
   scale, jittering with the frame clock, so the cache keyed on the exact
   step never matched. Exhibits can declare a `grid`; the viewer
   accumulates frame time and advances gridded exhibits in whole steps.
   The cache key tolerates a step within a part in ten thousand (the
   remainder after an event step, the rounding on a run's last step). The
   factorisation now survives a jump too (Newton rebuilds a stale one the
   moment it stops contracting).
4. **The Levitron's Floquet sweep** (88 growth-rate evaluations, ~100 s,
   finite-difference Jacobians flooding the rayon pool) started at
   launch whatever exhibit was shown; it now starts the first time the
   Levitron exhibit is advanced.
5. **The exhibit ran at 3% of real time.** Now 25% on a 1 ms grid (half
   speed fills 0.95 s of every wall second with physics and starves the
   frame loop; the Up arrow doubles the speed when choppy is acceptable);
   the leg exhibit runs at 50%. `SIM_VIEWER_STATS=1` prints the viewer's
   simulated-to-wall ratio once a second.

Validation: the 34-plate suite passes on the final build in 487 s (638 s
this morning, 1227 s of plate time last night); core crate tests green.

A lesson the suite taught on the way: a committed rate lane holds the
step's *average* rate (the midpoint rule's rate is the mid-step value),
so a jump that samples a rate — the sample-rate plate's tachometer, the
quantisation hunt's encoder — must not land on a whole-step boundary.
The old bisection always ended on a step of h/2²⁰ before the jump,
which hid this; the new locator keeps that property deliberately (a
guard already on its crossing gets exactly that step — the quantisation
hunt's limit cycle flips between two sampled modes if it is 1e-6·h
instead, a sensitivity worth knowing about), and a first attempt at
snapping the run loop's last step to `h` was dropped because it pushed
crossings inside whole steps. When that short event step will not
converge (the rigid-contact leg at a sample instant; the old code got
through on a factorisation left behind by the search) the jump fires at
the committed state instead of failing the run. All three plates read
what they read last night.

Measured on the plate pattern: 34.6 → 1.9 ms per step (fresh Jacobians
per step 8.6 → 3.3; event location 27 s → 0.16 s per 1.2 s of trot). The
remaining cost is genuine Newton work on the contacts: assembly (the four
chain legs are finite-differenced) and factorisation at about 0.24 ms
each, three to four times a step. `SIM_SPARSE_FROM=64` takes another 20%
off the factorisation here and is left as a knob.

## Walk the plank: a training environment (3 September, afternoon)

The seam turned around for a learner, and the first task on it, after
Dai et al.'s "Walk the PLANC" (physics-guided RL on stepping stones).

- **Snapshot and restore**: `Simulation::snapshot`/`restore` (state,
  clock, predictor; the factorisation is dropped) and
  `Runtime::snapshot`/`restore` (every island, then a commit).
- **Environment mode of the seam**: `sim_couple::Environment` (`spaces`,
  `reset(seed, level)`, `step(action)`, `snapshot`, `restore`) and
  `sim_couple::serve`, newline-delimited JSON over stdio for a batch of
  environments stepped on parallel threads; `simloop.Gym` in Python with
  `(envs, …)` arrays, partial resets, snapshot round trips. A protocol
  round-trip test in `sim-couple`.
- **Terrain**: `contact.point_terrain_compliant` — the penalty contact on
  horizontal patches `patch<i>.{x0,x1,y}` with a fade at the edges; the
  `Terrain` generator (flat stones, varying heights, stairs up and down)
  from a seed and the curriculum level with the paper's ranges.
- **The biped and its planner** (`walk_the_plank.rs`): torso on two chain
  legs, servos and sensors behind the seam with a 1 kHz PD loop; the LIP
  planner (footholds from the terrain, step timing from the capture point
  and re-timed every policy period, swing arc, stance height, pitch
  through the stance hip). Tuning that mattered: contacts ten times a
  paw's stiffness (two point feet a hand apart must out-stiffen gravity's
  tipping moment), joint PD of 1200 N·m/rad, a timing margin of 0.10 m
  and a placement lead of 30 ms for the PD's lag on a target moving
  backwards through the body frame. Level 0: 8/8 courses; 0.3: 7/8;
  0.6: 3/8 — the curriculum shape the paper relies on.
- **Plate 35** (66 s, 12 checks) and the `sim-gym` binary.
- **Training**: `clients/python/examples/planc` — CLF reward on the
  privileged LIP channels, a numpy PPO (no framework), residual policy on
  the planner's references, success curriculum, `baseline.py` for the
  planner alone. A 40-iteration smoke run learns nothing decisive in four
  minutes but runs: about 50 ms per batched step of eight environments.
- Viewer: exhibit 35 "Walk the plank" (the planner alone, knob = the
  curriculum level, auto-restarts on the next seed, counts crossings and
  falls). The PD loop's period went from 1 ms to 2 ms (still 8/8 at level
  0; 4 ms breaks it), a third off the biped's step.
- Not done: standing still on point feet (three balance controllers in
  the time box; the plate states the physics instead), an analytic
  Jacobian for the terrain contact (finite-differenced today: the biped's
  policy step of 20 ms costs about 35 ms wall).

## robocad: the CAD tool (3 September, evening)

A direct-modeling CAD application in `cad/` (Python 3.9, OCCT 7.7 via
OCP, PySide6 + OpenGL), built to the full specification, with the
CAD ↔ simulation loop the user asked for.

- Kernel behind `GeometryKernel` (`kernel/base.py`, `kernel/occt.py`):
  primitives; extrude (taper, symmetric, up-to, inline boolean), revolve,
  sweep, pipe, loft, fill, bridge; booleans, split, plane cuts, push/pull,
  offset faces, dependent offset to a body, move/rotate faces, cylinder
  radius edits (fill-and-recut with exact spans), draft, delete face with
  healing, imprint, shell, thicken, fillets (constant, variable, chordal,
  full round, all edges), chamfers, transform, mirror, join/unjoin,
  dissolve, projection, silhouette; queries incl. tessellation with
  per-face IDs, validation, sections, ray hits, curvature, continuity,
  control points. Geometric face/edge references survive edits.
- Sketching (`kernel/sketch.py`): lines, polylines, splines, control
  curves, circles (centre/2pt/3pt/tangent), ellipses, arcs, rectangles,
  polygons (remember sides), slots, spirals, text (fontTools); trim,
  split, extend, corner fillet, offset, join/unjoin, rebuild.
- Document (`document.py`): scene graph with groups, live instances and
  mirror instances, meshes, images, planes, measurements; materials with
  density; `.rcad` zip persistence with thumbnails; autosave; clipboard
  with placement. Commands/undo (`commands.py`) with `Ops` as the
  scripting façade.
- Print helpers (`printing.py`): M2–M8 fastener library (clearance, tap,
  counterbore, countersink, heat-set insert pockets, insert bosses),
  clearance offsets, wall-thickness check, manifold validation gating every
  mesh export, overhang shading and build plate. Analysis (`analysis.py`).
- I/O: STEP (XDE names/colours/assemblies both ways), IGES, STL, 3MF
  (multi-body, colours, names), OBJ+MTL, sketch SVG, technical-drawing SVG
  (HLR visible/hidden, section hatching, multi-view sheet), mesh import via
  trimesh with a unit prompt, SVG curves, reference images with
  calibration. Headless software renderer for screenshots.
- UI: viewport with turntable/trackball, ortho snapping, focus, view cube,
  six display modes incl. matcap and render (lights + ground shadow),
  ID-buffer picking of bodies/faces/edges/vertices, snapping, section
  plane, gizmos with Tab-to-type, command palette with conflict warnings,
  JSON keymap, radial menus, outliner (DnD, search, states, isolate,
  active group), selection panel with live dimensions, materials panel
  (drag to bodies), high-contrast theme, localisation table, SpaceMouse
  hook, multiple windows.
- Bridges: websocket live link + Blender add-on (collections, sharp
  edges/seams, stable face IDs), static web viewer.
- Simulation loop: `simbridge.py` exports `*.simrobot.json` (mass, COM,
  planar inertia about the plane normal, section outlines, `joint:` planes,
  `ground`); Rust `sim_phenomena::scenarios::cad_robot` builds the planar
  multibody with PD-held servos sized from outboard inertia; `sim-cad`
  runs it headless; `sim-app --scene cad --model` draws and rebuilds on
  every save. Demo: `scripts/robot_leg_demo.py`.
- Tests: 49 (units, kernel fixtures, document/undo, export validation and
  round trips, drawing, wall check, bridge, UI smoke offscreen); the
  acceptance torso scenario runs end to end (`scripts/acceptance.py`).
- Known limits: FBX needs an external converter; loft guide curves are
  accepted but not enforced by OCCT's ThruSections; sweep with twist uses
  a linear law; the headless renderer is a painter's algorithm.


## 2026-09-03 — CAD: robot parts and the physical assembly description (simrobot v3)

- Robotics layer in `cad/robocad/robotics.py`: 16-entry motor library with
  generated housings/shafts/mount holes, joints (revolute, continuous,
  prismatic, fixed, ball, loop_revolute, loop_spherical), inference from
  coaxial pin/hole pairs, validation (tree, loops, limits, pivot distance,
  motor alignment, stall vs gravity load), Robot panel, motor/joint tools
  and dialogs, viewport glyphs, `GET /robot`, `GET /motors`.
- `cad/PHYSICAL_MODEL.md` is the Python↔Rust contract. `physical.py`
  derives it: SI links with full inertia tensors, decimated collision
  meshes, convex hulls and signed distance grids; joint physics as printed
  (pin/hole radii, contact length, clearance → backlash and wobble,
  Coulomb/Stribeck/viscous friction from the material pair under the
  outboard weight, wall compliance, bearing pressure vs allowable); bolted
  fixed joints from recorded fasteners (preload from the tightening-torque
  table, stiffness, shear capacity); motors with electrical, gearbox,
  thermal, firmware and driver blocks (`MOTOR_DATASHEETS`); sensors (IMU,
  encoder, current, force) and cables as nodes; battery, control and
  uncertainty settings; results (`*.simresult.json`) and identification
  round trips. Materials gained engineering properties (E, ν, σ_y, σ_u, Tg,
  k, cp, α, friction table, bearing pressure, print anisotropy).
- `flex.py`: voxel hex FE (orthotropic across layers) with RBE2 patches,
  clamped at the parent joint, reduced to six modes with per-mode frame
  motion, participation and centroid stress. A 100×10×10 mm PLA cantilever
  gives f1 = 266.6 Hz vs 271.4 Hz Euler–Bernoulli (−1.7 %) and the same
  gravity sag within 3 %; ~4 s per link.
- UI: stress overlay from loaded results, margins column in the Robot
  panel, joint physics editable in Properties, material property dialog,
  sensor/cable/power dialogs; API routes for physical, results,
  identification, sensors, cables, battery, control, uncertainty.
- Demos: `scripts/robot_leg_demo.py` (v3 with IMU, encoders, cable, 2S
  LiPo; ~8 s export with flex) and `scripts/gripper_demo.py` (parallel
  gripper, two four-bars + coupler, three loop closures; ~11 s). Tests: 68.

### Rust side (same day)

- New crate `sim-domain-robot`: `robot.articulated` (floating base +
  tree joints + `loop_*` constraints with Baumgarte + modal flex with
  thermal softening + vertex-vs-SDF contacts with floor stiction), and
  `robot.motor_unit` / `robot.h_bridge` / `robot.battery` /
  `robot.servo_firmware` / `robot.thermal_probe`. Ten analytic tests
  (pendulum period 0.003 %, drop rest height exact, sliding distance 2 %,
  four-bar drift 1e-9 m, cantilever sag 0.03 %, motor stall/no-load
  exact, thermal τ 63.2 %, backlash dead zone).
- `sim-phenomena::scenarios::cad_physical`: the v3 builder wiring
  battery → bridge → motor → joint, winding → case → mount → ambient
  thermal chain, encoder/tacho → firmware, seam for targets; results
  writer (`*.simresult.json`), `run_monte_carlo`, `fit` (Nelder–Mead
  identification). `sim-cad run|fit` CLI; `sim-app --scene cad` draws
  3D meshes, stress colouring (S), contacts (C).
- Lessons: firmware gains are volts per radian but the bridge takes a
  ±1 duty, so scale by the rated voltage (unscaled gains bang-bang at
  25 Hz); the servo datasheet defaults carried an extra 10 ms loop
  latency that made every servo hunt by 3–7° (now one loop period, the
  20 ms command cadence is the control seam's job); Monte Carlo COM
  shifts must move mass, not geometry; a slice that defeats Newton is
  retried from a snapshot with the step halved (up to 16×) instead of
  aborting the run. Leg: 7–10 s wall per simulated second with flex,
  3 s/s without.
