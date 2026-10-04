# Project rules

Build a fast, reproducible CAD → physics → controller → measured-result loop for
robot design and eventual sim-to-real learning.

- **CAD owns the robot's physical definition.** Store materials, mass/inertia,
  joints, transmissions, friction/compliance, actuators, sensors, and limits in
  the CAD model. Declare units, coordinate frames, provenance, and uncertainty;
  distinguish measured, derived, and estimated values. Geometry-to-physics
  derivations must be explicit.
- **Everything runs in Rust, in the viewer's own process.** Rust owns
  simulation, environments, the user interface, CAD and hardware driving. Use
  Rust controllers or Rhai scripts backed by Rust library components. Nothing
  the viewer does may rely on a separate server:
  - **CAD** runs in Rust inside the native viewer: the document model and
    history, modelling operations, derivations, export and annotations, on a
    geometry kernel called from Rust (chosen on evidence and recorded). Keep
    `.rcad` files compatible. RoboCAD's Python source is the reference for
    behaviour only; the viewer never starts, calls or waits for it, and each
    path through its REST client is deleted once Rust serves every caller.
  - **Hardware** calibration and driving run in the viewer's process through
    the shared Rust hardware layer, not over HTTP to a calibration or
    motor-bench server. Server logic moves into library modules; example
    binaries are thin wrappers over them or are deleted.

  Other external-language integrations (the controller seam's clients, the
  browser) are compatibility surfaces, not the default architecture for new
  work.
- **Contribute to the shared library first.** Reuse an existing component, extend
  it, or add a reusable component with a focused example. Keep robot-specific
  topology, parameters, and policies in configuration/examples. Do not introduce
  dedicated leg/robot runtimes or duplicate physics in viewers and scripts.
- **Keep component descriptions shared.** Expose parameters, typed ports, units,
  and validation through the registry so CAD inspectors, exports, and Rhai use
  the same definitions.
- **Separate robot, world, and policy.** Environments own terrain and task
  conditions; controllers consume observations and command actuators. Never
  silently fill in missing robot properties in either. Record experimental
  overrides and provide a path to promote accepted values back into CAD.
- **One execution path.** Interactive viewing, headless experiments, and learning
  should share the Rust runtime and observation/action contract. Advance physics
  on simulation time independently of rendering; record seeds and inputs for
  replay. Teleoperation requests motion through the controller.
- **Realtime browser walking is required.** Explicit browser fidelity profiles
  may substantially simplify physics through shared Rust components. Retain the
  detailed validation model; measure realtime performance and approximation error.
- **Protect responsiveness and user work.** Run expensive geometry, export, and
  simulation work off the UI thread with progress and cancellation. Preserve
  unsaved CAD edits before reload/restart. Previews must not mutate source geometry.
- **Measured values have one source.** Accepted actuator models live in the
  actuator registry and flow to CAD, simulation, screens and hardware limits from
  there. Promote new measurements into it; never hand-copy numbers into configs.
- **Hardware safety is independent and never bypassed.** The FPGA supervisor,
  taught travel windows, watchdogs and STOP must hold even if host code is wrong.
  Moving driver logic into the viewer's process carries every host-side rule
  across unchanged: one front end owns a motion session, STOP works from every
  section, hold-to-move releases on focus loss and panel close.
  Load FPGA images only with motor power off and record a deployment receipt.
  Drive motors only with the operator present and the fixture supported. Record
  every raised limit, with the reason and the previous value.
- **Simulate the control loop the hardware runs.** Loop period, latency and gains
  shape which motions are feasible. When the real loop differs from the
  simulated one, use a screen for it and label results with the assumption.
- **Qualify every shortcut.** Reduced-fidelity models must be qualified against
  the detailed model and re-qualified after library changes. Label results with
  their fidelity and confirm finalists on the detailed model before hardware.
- **Make workflows automatable.** UI and REST operations should share validation,
  commands, and undo semantics. Prefer reusable inspection and batch APIs over
  one-off automation shortcuts. Human- and LLM-editable files (gaits, poses) must
  convert exactly to shared library types and fail with errors that name the path.
- **Every screen is usable by an AI.** New functionality in any mode ships with
  its REST commands in the same change: typed actions shared with the UI (same
  validation, undo and refusals), described in `GET /v1/capabilities` with a
  working example, and covered by that mode's guide command (like `cad_guide`,
  also `GET /v1/<mode>_guide`): concepts, workflows in order, every command, the
  rules, so an agent starting cold begins at the right point. Expose what an AI
  needs to see as well as to change (state, topology, renders, feeds of
  comments), give long waits a non-blocking path (published resources and
  `/v1/events/…`), and add AI-facing features (an in-window assistant, agent
  comments, saved views it can make for the person) wherever a screen would
  benefit.
- **Prove behavior, not just animation.** Check analytic cases, conservation where
  applicable, constraint closure, contact, controller behavior, and timestep
  sensitivity. Set explicit accuracy and performance expectations and enforce
  representative acceptance cases in CI. Label uncalibrated physics and
  provisional limits honestly.
- **Preserve reproducibility.** Version or durably reference CAD artifacts as well
  as code; ignored `runs/` is not a baseline. Record model/schema versions,
  controller configuration, and results. Keep bulky outputs such as recordings and
  captures out of git; keep what reproduces them. Run checks relevant to each
  change and report remaining limitations without claiming unverified sim-to-real
  accuracy.
- **Paid compute needs a ceiling.** Cloud runs need the user's approval, a stated
  cost, an automatic shutdown, periodic result sync, and termination when done.
