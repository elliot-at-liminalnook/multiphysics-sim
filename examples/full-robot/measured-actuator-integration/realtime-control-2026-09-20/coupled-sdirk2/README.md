# Coupled SDIRK2 investigation

The shared motor-event adapter now has an opt-in two-stage SDIRK2 candidate using
`ImplicitStepConfig.sdirk2`. Both stages solve the registered motor, gearbox,
mechanical and contact-history equations. The existing hybrid scheduler still
owns sampled controls, deadlines, mode jumps and rejected-step subdivision.

The second-stage anchor applies RK weights only to differential motor states.
Differential/algebraic classification comes from the registered component's rate
reads, as in the shared compiler, including quasistatic winding/rotor parameters.
The generic motor component's classification is parameter-dependent. No motor
force law is copied into the adapter. Mechanical coordinates use the existing
closed chart and world-frame quaternion updates. Each stage has a fresh numerical
workspace; accepted-state history is not extrapolated through affine anchors.

This remains an experimental candidate, not a selected browser profile. Continuous
power/thermal controls need their explicit differential/algebraic layout before
this adapter can advance them with SDIRK2; unsupported combinations reject rather
than silently dropping states. The existing backward-Euler contact impulse trace
is also rejected because its endpoint-force quadrature would be incorrect for
SDIRK2. Ordinary endpoint contact observations remain available.

Tests cover an independently assembled coupled circuit/gearbox/load system,
algebraic winding current, exact RL transient convergence order, and deterministic
FPGA controller deadlines/replay. Whole-robot performance and matched-error gates
are separate experiments under `step-1`, `step-4`, and `step-8`; until those pass,
the original 6400 Hz backward-Euler profile remains selected.

## Current verification

All 63 focused robot tests pass, including the new coupled SDIRK2 tests and the
existing BE, contact, constraint and motor-event regressions. WASM compilation
passes. The shared solver body reproduces every physical BE frame field exactly.

The 1.25 ms SDIRK candidate completes three seconds in 17.983 s (1.70x faster than
the paired detailed baseline) but is rejected: maximum errors are 0.447 degrees,
4.437 mm, 0.181 A and 27.915 N, with differing contact identities. The 0.625 ms
candidate also fails the unchanged physical gates. Both preserve controller
cadence and finish without a detected fall. Profiled/unprofiled physical fields
match exactly. Neither is a realtime or browser qualification. Finer SDIRK and
backward-Euler references are the next comparison; see the parent `STATUS.md`.

The fine references have now completed. `../refinement-comparison.json` shows
that the original 6400 Hz BE trajectory itself fails the existing limits against
12800 Hz BE. Fine SDIRK2 and refined BE also differ beyond those limits. This
exposes unresolved full-robot timestep/controller/contact sensitivity; neither
reference is assumed exact. No larger-step recipe is selected. See `../STATUS.md`
for the completed-run checkpoint and next numerical/performance investigation.
