# Project rules

Build a fast, reproducible CAD → physics → controller → measured-result loop for
robot design and eventual sim-to-real learning.

- **CAD owns the robot's physical definition.** Store materials, mass/inertia,
  joints, transmissions, friction/compliance, actuators, sensors, and limits in
  the CAD model. Declare units, coordinate frames, provenance, and uncertainty;
  distinguish measured, derived, and estimated values. Geometry-to-physics
  derivations must be explicit.
- **Rust owns simulation and environments; Python stays on the CAD side.** Use
  Rust controllers or Rhai scripts backed by Rust library components. Existing
  external-language integrations are compatibility surfaces, not the default
  architecture for new work.
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
- **Make workflows automatable.** UI and REST operations should share validation,
  commands, and undo semantics. Prefer reusable inspection and batch APIs over
  one-off automation shortcuts.
- **Prove behavior, not just animation.** Check analytic cases, conservation where
  applicable, constraint closure, contact, controller behavior, and timestep
  sensitivity. Set explicit accuracy and performance expectations and enforce
  representative acceptance cases in CI. Label uncalibrated physics and
  provisional limits honestly.
- **Preserve reproducibility.** Version or durably reference CAD artifacts as well
  as code; ignored `runs/` is not a baseline. Record model/schema versions,
  controller configuration, and results. Run checks relevant to each change and
  report remaining limitations without claiming unverified sim-to-real accuracy.
