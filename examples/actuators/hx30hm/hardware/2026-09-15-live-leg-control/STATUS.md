# Live leg control

Goal: FPGA-owned 100 Hz live feedback, tuned physical tracking, then calibrated loaded-leg control.

## Current status

- Latest user update: the hardware is disconnected. Work has pivoted to offline controller tuning and robustness tests. No hardware commands or flashing are part of this phase.
- The simulation campaign is in `examples/full-robot/measured-actuator-integration/controller-tracking-simulation/`. It uses shared Rust motor physics and the FPGA integer law, preserves the provisional plant parameters, and separates training patterns from hold/reversal/chirp/gait validation. Simulated results do not commission firmware or establish loaded-leg accuracy.
- Offline phase complete: 162 gain configurations, 240 validation cases, 36 passing timestep comparisons, and 12 three/nine-motor shared-supply scenarios. Full-drive small-motion tracking improved, but the original full gait still has 9–19° RMS errors and requests up to ~3,000°/s. A reusable registered reference governor and a separately scored speed/acceleration envelope are implemented. See the simulation campaign README for plots, retained regressions, and the required gait-generation follow-up. No candidate was promoted to the live viewer or CAD.

- Existing browser WASD prototype still runs at 10 Hz. Its last 12-second physical run completed and stopped, but hip RMS error was 0.855 degrees with sustained oscillation. Do not treat that as accepted tracking.
- Implemented a finite 16-row streamed-reference queue, 1,200-frame execution, tunable power-of-two gains, shared Rust capture auditing, and browser/server integration source.
- 4,096 Rust/RTL controller vectors pass. Complete UART tests pass for 280 frames, explicit STOP, and missing refill. Shared capture mutation/audit tests pass. All 15 acquisition tests pass after correcting the synthetic fixture's reply arbitration; no actual watchdog deadline was relaxed.
- First compact build synthesized but failed routing with conflicting GND/VCC arcs at X89Y28/LSR0. Its pre-route estimate was 49.07 MHz, below the required 50 MHz; it is rejected and was never flashed.
- The block-memory variant also failed routing. Reserving two ordinary flip-flop locations at the GSR tile did not resolve the conflict in the tunable build. The smaller fixed-gain build also failed routing; its synthetic 280-frame UART test passed. The FPGA build issue remains unresolved. No new live-control firmware has been loaded.
- Last inspection before disconnection: IDs 10–12 responded, torque off, 11.9–12.0 V, 45–48 C. These are historical readings, not current hardware state. The motors were unloaded.

## Next

The offline search and checks are complete. Next simulation milestone is to generate gaits with actuator/reference constraints included and validate whole-robot foot placement and balance. The following hardware steps are deferred until reconnection; simulation is not a substitute for them.

1. Obtain a routed image passing 50 MHz. Preserve source, commands, build logs and hashes; flash only an accepted image.
2. Verify 12-second 100 Hz zero-drive capture, stale-input stopping, then small physical references with stop readback.
3. Compare gains using retained identical Rust-controller reference traces, then test the actual browser WASD page. Retain failures. Current tunable profile uses one gain triple for the selected group; per-motor gain storage is not implemented.
4. Activate updated viewer caption/config only after actual firmware commissioning. Existing serving bundle still correctly says 10 Hz.
5. Calibrate joint zero, polarity, transmission, travel and drive limits with the actual leg. Verify slow loaded lift/place against foot-placement tolerances. These steps require the physical leg/fixture and are not established by unloaded tests.

`verification/` holds synthetic evidence and regression logs. `firmware/` holds the source snapshot of the first compact candidate (before the later memory-mapping attribute). `before-rtl/` preserves the original hardware source. Powered reload authorization remains valid. No wiring changes.
