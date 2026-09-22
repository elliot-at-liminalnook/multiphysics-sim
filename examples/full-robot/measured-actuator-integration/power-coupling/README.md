# Shared motor power coupling

This adds a shared-library foundation for milestone 4. The full quadruped session
still needs CAD power authoring and runtime selection; its existing 50 ms result
continues to use imposed supply voltages.

## Implemented

- `EmbeddedPowerBank` connects the registered battery, H-bridges and resistors.
  Configuration describes a radial tree: a shared trunk, separate feeds, and
  successive daisy-chain segments can all be represented explicitly.
- Motor winding currents, mechanical motion, battery state of charge, terminal
  voltage, branch voltages and signed terminal energy share the implicit solve.
  Every trial uses current trial loads, including during event location.
- The existing sampled-control adapter can carry continuous power states. It
  preserves the same registered controller, PWM hold and exact sample deadlines.
  Supply coupling disables independent-motor Jacobian coloring.
- Startup and PWM changes reconcile algebraic voltages without changing charge
  or accumulated energy. Checkpoints contain all physical and controller history;
  failed intervals do not commit partial results.
- Readings expose pack volts, discharge amps, watts, SOC and energy, plus branch
  voltage/current/wiring losses and existing per-driver readings.

## Verification

Run from the repository root:

```sh
cargo test --locked -p sim-domain-robot --test embedded_power --test embedded_motor
```

The power tests use an explicitly synthetic two-motor circuit. They check KCL,
pack and branch voltage drops, charge balance, terminal energy and wiring power
through a direction reversal; the shared FPGA integer controller with exact
subperiod PWM delivery; simultaneous-load effects and an ideal shared-wire
comparison; exact controller sample endpoints; checkpoint replay; failed-interval
rollback; and invalid topology or missing battery parameters. Existing motor
tests retain their independent circuit/mechanics and sampled-controller checks.

**Verified:** all 5 power/controller checks and all 16 existing motor checks pass;
see [power/controller test log](power-controller-tests.log) and
[motor regression log](motor-power-tests.log). The workspace CI test command
includes both targets; remote CI has not been run for this work. Existing CAD
profile, session, environment and battery-accounting suites also pass (32 tests;
[runtime regression log](runtime-tests.log)). These retain their original scope:
they do not demonstrate power-tree selection in the full quadruped session. The
WebAssembly library check also passes ([log](wasm-check.log)); browser execution
and realtime performance have not been qualified.

These are implementation checks, not new physical measurements or calibration.
The test battery capacity and wiring resistances are analytic fixtures, not
quadruped parameters. Source snapshots and hashes are retained in `source/` and
`source-identity.json`.

## Remaining integration and limits

1. Author a versioned power distribution in CAD with shared registry validation,
   units, provenance, uncertainty and evidence. Bind branches to stable CAD motor
   IDs and preserve save/reload/export/undo semantics. Reuse the existing
   actuator-profile authoring command and native validator; do not create a
   separate viewer-only source of physical parameters. Reject conflicting legacy
   battery and profile declarations rather than silently choosing one.
2. Resolve that declaration into this adapter in `EmbeddedSession`, carry its
   state through replay/environment observations, and expose electrical readings
   in the viewer. Reject declared power that a chosen execution mode would ignore.
3. Add an explicitly provisional quadruped power revision and run matched gait,
   timestep, terminal-accounting and browser performance comparisons.

The registered battery uses an uncalibrated discharge curve. Battery cutoff/BMS,
SOC operating bounds, bench-supply CV/CC behavior, switching losses, wiring
inductance, thermal evolution and loaded-leg calibration are not supplied by this
adapter. H-bridge foldback and regeneration retain the existing component's
limitations. Calibrated supply current, battery and wiring measurements remain
unavailable; no electrical hardware accuracy is claimed.
