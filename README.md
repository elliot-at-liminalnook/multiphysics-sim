# Multiphysics Sim

**Design a robot in CAD, simulate every physical effect that matters, and run
the same controller on real hardware.**

![The CAD-derived quadruped walking in the browser under keyboard control, on the same Rust physics as the desktop tools](docs/images/web-wasd.png)

A multiphysics simulator in Rust, built around one loop:
**CAD → physics → controller → measured result → back into CAD.** Mechanics,
motors, circuits, heat, magnetics, fluids, acoustics, sensors and controllers
connect through typed ports and are solved together. A quadruped robot proves
the loop end to end. It is designed in CAD, simulated with motors measured on
the bench, searched for gaits on cloud machines, walked in the browser and
driven on a physical leg through an FPGA.

## What's inside

- **One solver for many domains.** 17 domain libraries of reusable equation
  elements, one compiler and one implicit integrator. There is no special-case
  robot runtime.
- **CAD that owns the physics.** RoboCAD (Python, OCCT, Qt) holds geometry,
  materials, joints, transmissions, actuators, sensors and limits, each with
  units and provenance.
- **Linked viewers and a system builder.** A schematic view and a 3D physical
  view show one running system; select in one, see it in the other. Build
  hierarchical systems from a parts library, with component icons, responsive
  grid dragging and overlap checks. Placement is display-only; CAD owns physics.
- **Discussions on the model.** Click annotation pins that follow parts and
  groups, read Markdown replies, and ask Codex with project files and model
  context. Watch its activity or opt into automatic replies to new comments.
- **The same physics in the browser.** The Rust runtime compiles to
  WebAssembly. Drive the quadruped with WASD; commands go through its
  controller, never straight to the robot.
- **Gait search at scale.** Bayesian and CMA-ES optimizers explore thousands
  of gaits an hour on a qualified low-fidelity model. They scale to 96-core
  cloud machines. Gaits are also readable YAML files, so an LLM can write,
  score and refine them.
- **Real hardware in the loop.** HX-30HM servos behind an FPGA safety bridge,
  a calibration panel (native in the viewer's Robot mode, and in the
  browser), and a registry of measured motor models. What the bench measures
  is what the simulator uses.
- **Printable parts.** Split CAD parts to fit the printer and join them with
  pins, heat-set inserts or dovetails; check strength across the print layers
  under loads read from a running simulation; choose print direction and
  settings; lay out 3MF plates; write assembly steps; and break test coupons
  whose results replace the estimates in the print registry.
- **Controllers in any language.** Rust and Rhai natively; Python, C or any
  C-ABI library through a deterministic lockstep seam; a Gym-style interface
  for learning.

| | |
|---|---|
| ![RoboCAD editing the parametric quadruped](docs/images/cad-editor.png) | ![Physical view running the motor-driver board live, parts tinted by temperature](docs/images/physical-viewer-live.png) |
| **RoboCAD:** parametric legs, one edit updates all four | **Physical view:** a motor-driver board running live, tinted by temperature |
| ![System builder with a parts library and an inspector](docs/images/system-builder.png) | ![Schematic of the full quadruped grouped into subsystems](docs/images/schematic-full-robot.png) |
| **System builder:** place parts, wire typed ports, group, drill in | **Schematic:** the whole quadruped, 195 components in subsystems |

## Quick start

```sh
cargo run --release -p sim-spatial -- --phenomena --exhibit quadruped   # desktop exhibit (phenomena mode)
cargo run --release -p sim-phenomena --bin sim-phenomena -- list   # physics phenomena gallery
examples/systems-viewer/run-live.sh                           # linked schematic + physical views
cargo run -p sim-spatial -- examples/systems-builder/motor-driver-board/board.system.json   # native viewer; switch modes in the window
cad/run.sh examples/components/quadruped-parametric/model/robot.rcad   # CAD editor (RoboCAD, the reference)
cargo run -p sim-spatial -- examples/components/quadruped-parametric/model/robot.rcad   # the same document in the native viewer's CAD mode
```

Any window can open every mode's document from inside the window: pick a
mode in the switcher strip along the bottom, and if that mode has nothing
open, a document picker lists its robot presets, recent documents (kept in
your config directory, never in the repository), the repository's example
files and lesson or place folders, plus an "Open file…" path field.
`cargo run -p sim-spatial` with no arguments is enough to reach every mode.

The browser workspace is built and served as described in
[web/README.md](web/README.md).

### CAD mode in the native viewer

`cargo run -p sim-spatial -- path/to/model.rcad` opens CAD mode: the viewer
starts RoboCAD's headless REST service on the file (`cad/.venv/bin/python -m
robocad.api`, from `cad/`; run `cad/run.sh` once to create the venv) and
shows its model tree, tessellated bodies and inspector. To work beside
RoboCAD's own window, attach to it instead:
`cargo run -p sim-spatial -- --cad-url http://127.0.0.1:8420` (selection is
shared). Every edit (visibility, lock, material, name, delete, undo, redo,
save, registry commands) goes through RoboCAD's command layer, so its undo
and `.rcad` file stay its own; the viewer never saves for you: it refuses to
leave CAD mode while a document it started has unsaved edits, and keeps that
service running (logging its URL) if the window closes. Sketching, direct-edit
tools, printing, captured experiment/candidate review and kinematic pose/program
controls now have native source paths. Experiments use the headless reference
service and shared Rust runner; native video export requires local ffmpeg.
These implementations are reviewed by reading, with execution/parity still
unverified
([parity ledger](docs/cad-parity.md)); compare the two step by step with
[docs/cad-checklist.md](docs/cad-checklist.md). Not compiled or run yet.

### Leg calibration in the native viewer

Offline measured-PWM studies use **Build → Actuators → Measured evidence** in
this same window. Open an identification archive or saved review, edit exploratory
candidate parameters and conditions, evaluate captured trial sets, inspect comparison
traces, record decisions/notes, then save or export to a new filename. The
[source workflow guide](docs/native-identification-authoring.md) records the native
controls and REST path. T46 is implemented for reading-only review; compilation,
fixture execution and parity remain unverified. Controller/FPGA/power refinement
and accepted-model promotion still require their existing external workflows.

Start the calibration server as today
([fixture README](examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/README.md)),
then open Robot mode connected to it:

```sh
cargo run -p sim-spatial -- --robot-preset robot-measured-400hz --hardware http://127.0.0.1:4194
```

Or press **Leg calibration** in the robot header and press **Connect**. Add
`--motor-bench http://127.0.0.1:4180` for live motor sync with
`serve_motor_bench`. REST and `system_ui` may read status, list gaits,
export, connect, change the mirror's display and STOP; anything that starts,
changes or arms motion (selecting or enabling a motor, jogging, speeds,
sweeps, tune, campaign, gait choice and play, drive settings, the safety
confirmations, raw step, live sync's mapping and start) is refused by name
and needs the operator at the window. Losing window focus, closing the panel
or the window, and leaving Robot mode stop any drive. The panel is not
compiled or run yet; see the [parity ledger](docs/hardware-parity.md). The
browser page at the server's URL stays available until the
[hardware checklist](docs/hardware-checklist.md) is signed off.

## Principles

- **One execution path.** Desktop, browser, headless experiments and learning
  share one runtime and one observation/action contract.
- **Automatable editing.** Builder UI and REST controls share validation,
  commands and undo, including placement, discussions and agent controls.
- **Measured over assumed.** Every value states whether it was measured,
  derived or estimated. Uncalibrated physics is labelled as such.
- **Prove behavior, not animation.** Analytic cases, timestep sensitivity and
  real-time budgets are checked in CI.

## Learn more

| | |
|---|---|
| [CAD guide](cad/README.md) | Modelling, the CAD → sim loop, REST API |
| [Printable parts](cad/PRINTING.md) | Splitting, joints, strength from simulation, print settings, assembly, coupons |
| [System builder](examples/systems-builder/README.md) | Hierarchical systems and the motor-driver example |
| [Builder editing](examples/systems-builder/DISPLAY-EDITING.md) | Grid placement, discussions, Codex and REST control |
| [Browser workspace](web/README.md) | Building and serving the WebAssembly viewer |
| [Gait lab](examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/README.md) | YAML gaits, poses and LLM-driven search |
| [Motorized pendulum](examples/motorized-pendulum/README.md) | A small end-to-end CAD → controller → measurement example |
| [Control roadmap](control-roadmap.md) · [Domain roadmap](domain-roadmap.md) | Controller seam, sensing, domains and status |
| [Phenomena tests](surprise-tests.md) | The physics acceptance suite |
| [Project rules](AGENTS.md) | How the codebase is meant to grow |
