# Builder roadmap: plug-and-play multiphysics

**Goal:** invent a physical part, prove it behaves physically, package it,
snap it into bigger machines and compare it against alternatives in
minutes, with no recompile. Every value is labelled measured, estimated or
derived.

**Final test:** write a new motor variant (for example coreless, with
temperature-dependent R and magnet), get its datasheet automatically, turn
it into a gearmotor, snap it into the winch, compare it against the brushed
motor, sweep magnet strength and save the chosen design to the library in
under 30 minutes, with no Rust recompile.

## Targets

- [x] Idea to first result: < 2 min
- [x] Edit to rerun: < 2 s (≤ 50 parts)
- [x] New part to library: < 10 min, no recompile
- [x] Every library part passes its automatic checks

## Milestones

### M1 · Compare and sweep
- [x] Run one scenario across 2–4 alternatives, with overlaid traces and a trade-off table
- [x] Sweep one parameter and plot any metric against it
- **Done:** worm vs spur vs planetary in one click; the starts sweep shows efficiency rising and self-locking lost; reproducible from the file

### M2 · Parts defined by equations
- [x] Text part format: ports, parameters with units, states, equations, notes
- [x] Compiled by the Rust library: unit checks, automatic Jacobian, hot reload
- **Done:** the coreless motor in about 50 lines matches a Rust reference to 1e-9; unit and port errors point to the offending line

### M3 · Automatic bench and datasheet
- [x] Benches for each part kind (motor: stall, no-load, speed-torque, efficiency map; gear: efficiency both ways, backdrive)
- [x] Energy balance and step-size checks for every part
- **Done:** every noted part has a datasheet; a part that creates energy is caught; CI reruns the benches

### M4 · Parts from parts
- [x] Choose exposed ports and parameters, then promote to a versioned, hashed library part with an interface
- [x] Swap anywhere with the same interface; show where a part is used
- **Done:** motor → gearmotor → actuator, used in the winch and a leg; one motor edit updates both

### M5 · Live iteration
- [x] Edits apply while running (parameters at once, structure after a fast recompile)
- [x] Every run kept with seed, settings and results; runs can be compared
- **Done:** the edit-to-rerun target is met on the winch and the board; no UI-thread stalls (measured)

### M6 · Fidelity levels and realtime
- [x] Each part offers a detailed model and a realtime model, with the error between them measured
- **Done:** a system built from new parts runs in realtime in the browser, with a published error bound

### M7 · CAD and hardware
- [x] Parameters derived from CAD geometry
- [x] Fit a part to bench data, promote it from estimated to measured, record uncertainty
- **Done:** a CAD edit updates the datasheet; one real motor is fitted to measured data

**Order:** M1–M3 first, then M4–M5, then M6–M7.

## Evidence (2026-09-25)

| Milestone | Proof |
|---|---|
| M1 | `sim-runtime --test worm_drive` (saved studies: worm/spur/planetary; worm-starts sweep 45→76 % efficiency, locking only at 1 start) |
| M2 | `--test authored_parts`: `.part` motor = Rust motor (difference 0); coreless energy balance 0.3 %; errors name file:line; hot reload |
| M3 | `--test datasheets`: 26 noted parts, all checks pass, datasheets reproduce; energy-creating part caught |
| M4 | `--test parts_from_parts`: motor → gearmotor → actuator in winch and leg; one motor edit reaches both (26.7 → 22.4 rad/s) |
| M5 | `--test live_iteration`: live parameter edit keeps state; edit → new frame 8 ms (winch), 16 ms (29-part board); runs replay exactly |
| M6 | `--test realtime_profiles` + `node web/system-realtime-check.mjs`: realtime profiles 68–109× realtime in Chrome, within published bounds |
| M7 | `cad/tests/test_gear_derivation.py` (CAD edit → datasheet); `--test part_fit`: knee servo k = 0.012132 ± 0.000386 V·s/rad (campaign 0.012132 ± 0.001011), published as measured |
| Targets | `--test final_scenario`: new motor → datasheet → gearmotor → winch → compare → sweep → library in 3 s of tool time |

Open: desktop build-mode UI not yet inspected on screen; schematic window lacks the new panels.

