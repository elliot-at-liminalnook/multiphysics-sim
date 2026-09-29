# Leg characterization campaign — plan and status

Goal: an automated, staged campaign that measures the leg's motors, joints and
whole leg on the real device, safely, so that measured values replace the
simulation's estimates and feed the gait search. This file is the plan and the
implementation tracker. Everything is implemented and exercised in simulation
first (low fidelity), then run on hardware.

## Why

- In the actuator profile the gait search uses (`hx30hm-provisional`), every
  value except the controller period and encoder quantum is *estimated*:
  resistance, torque/back-EMF constants, gear ratio (200:1 assumed), friction,
  backlash (0), gear stiffness (50) and damping, inertia, latency and the servo's
  internal gains.
- Earlier campaigns measured unloaded motors 10–12 only (≤55% drive, ~123°/s
  peaks, 12.6% sag). The leg's gravity, friction and linkage are uncharacterized.
- The gait search's speed screen uses the published 5.5 rad/s no-load speed and
  the searched reference-governor range is an assumption.

## Safety layers

1. **Rehearsal.** Every test runs first on the simulated bench with the current
   model, producing a predicted trace.
2. **Divergence abort.** During the real test, measured position/speed/drive/
   voltage are compared with the prediction; leaving a band stops the test,
   logs why and blocks escalation.
3. **Gated escalation.** Each drive/speed/range step runs only if the previous
   passed: voltage sag < 10%, temperature < 50 °C (cool-down before the next),
   no position drift after returning to start, verified stop.
4. **Travel limits.** Saved poses with an inset; full speed only in the middle;
   near the ends speed is capped by the braking measured in stage C. The FPGA
   window still blocks outward drive.
5. **Joint combinations.** Multi-joint motions are checked for self-collision
   with CAD (kinematic mirror + sampled inter-link penetration) before running.
6. **Unchanged:** FPGA watchdogs, temperature/voltage trips, Z and S2.

## Test stages

| Stage | Test | Measures | Feeds |
|---|---|---|---|
| A | Slow constant-speed sweeps each way, 3 speeds | friction vs position/direction, gravity along range | gear friction, gravity model, stuck threshold |
| B | Holds at 8–10 positions | hold duty vs position, worm self-locking | load model, hold settings |
| C | Braking from increasing speeds, each way | stopping distance, deceleration | safety envelope, automatic-motion limits |
| D | Constant-drive steps, several levels/positions | speed per duty, lag, saturation, back-EMF, sag | motor gain, winding constants, time constant |
| E | Small-signal sine sweeps 0.2–15 Hz at several poses | frequency response, resonance, delay | gear stiffness/damping, compliance, latency |
| F | Servo position-mode steps and ramps | servo PID and planner | firmware gains in the model |
| G | Effort ladder 20→100%, middle first then wider (C-limited) | top speed, acceleration, torque per direction | gait-search speed screen and governor ranges |
| H | Multi-joint: move one, hold others; all together | interaction, combined supply sag | leg dynamics check, power model |
| I | Replay gait-search candidates in the air | real vs simulated tracking of candidate gaits | direct sim-to-real error |
| J | Duty cycles to thermal plateau | thermal capacity/resistance, derating | thermal model, continuous limits |
| K | Repeat A, D, G at the end | drift, wear, warm vs cold | uncertainty on every fitted value |

Fast stages (E, high-effort G) should use the FPGA's sealed-trajectory or
buffered capture on hardware; the host serial loop is 30–60 ms.

## Automation

- Declarative plan: per test the motion, limits, gates, prediction, repeats,
  cool-down.
- Campaign runner shared by simulation and hardware (same safety code as the
  panel), resumable, immutable receipts, live progress and Stop.
- Fitting into an actuator profile with measured values, uncertainty and
  provenance; replay recorded commands through the model and score the error;
  promote to CAD only after that; re-derive gait-search screens and governor
  ranges from measured envelopes.
- Test selection: rank parameters by uncertainty × gait sensitivity and spend
  test time on the top ones.

## Hardware prerequisites

- Supply current sensing (servo current register reads 0).
- Joint-side angle sensing (IMU per segment or markers) for backlash/compliance.
- Known loads (optional) for absolute torque.
- Fix or keep disabled the sticky knee before stage A.

## Implementation status (simulation)

Legend: [x] implemented and tested in simulation · [~] partial · [ ] not yet.
Updated as work lands; see the log at the end.

- [x] Rig interface (simulated bench in simulated time; hardware bus in real
      time: `BusRig` arms each axis's window, keeps watchdogs, Stop cancels)
- [x] Simulated leg bench: shared supply with sag, supply current sensor,
      thermal model, transmission backlash/compliance with joint-side angle,
      position-dependent gravity, known-load attachment
- [x] Safety: rehearsal prediction, divergence abort, gates (sag, temperature,
      drift, preflight cool-down), travel inset, braking-limited speed near
      ends, collision check for joint combinations
- [x] Stages A–K (E and J omitted from the first hardware plan; see README)
- [x] Fitting with uncertainty → actuator profile with provenance
- [x] Replay through model and residual scoring (before/after fit)
- [x] Promotion outputs: profile diff vs CAD estimates, gait-search screen and
      governor range patch
- [x] Test selection by uncertainty × sensitivity
- [x] Campaign runner in the calibration server with progress, Stop, receipts
      and Resume; panel UI
- [x] End-to-end simulated campaign and report (README.md, results-sim/)
- [x] On hardware: first pass with `plan.hardware.json`, knee and belt/hip
      (worm skipped: poses from an older multi-turn session), 2026-09-23
- [x] Belt protection: per-role `limits` (duty ramp, max duty, acceleration
      cap with spike abort, extra inset, skipped stages); tested in simulation,
      not yet run on hardware
- [ ] Hardware prerequisites above (supply current, joint-side angle, FPGA
      capture for E and fast G)

## Log

- 2026-09-23: Plan written. Library `acquisition::characterization` (Rig,
  SimRig, Session guard, stages A–K, fitting, replay, promotion, ranking,
  resumable `run_with` with receipts); bench extended (supply, thermal,
  transmission, gravity, loads); `run_characterization` example with CAD
  collision vetting and stage I from the qualified gait baseline.
  Fixed during validation:
  - stage E phase sign;
  - thermal fit (first order);
  - short-step speed fit (first order with dead time; too-short steps
    rejected);
  - gait patch uses full-drive speed rather than travel-limited speed;
  - H/I results no longer block axis 1;
  - sag at full gait speed (stage I time scale);
  - preflight cool-down.

  Tests: `tests/characterization.rs` (8, including recovery of the hidden truth
  and each gate), `campaign_runs_on_the_bus_rig_in_real_time`, and the browser
  test `web/tests/calibration-bench-e2e.mjs`, which runs the campaign, stops
  it, resumes from receipts and finishes. Simulation only; nothing was run on
  the leg.
- 2026-09-23 (hardware, first pass): `measurements/campaigns/campaign-1790182543587`,
  19 stages, none stopped by a gate, 13.5 min, knee and belt/hip.
  - Speed gain: knee 3508 and belt 3059 counts/s per duty (tuned values were
    4199 and 4410).
  - Braking: 833–1667 counts/s².
  - Top speed reached: 1400–1900 counts/s.
  - Replay error: the fitted model beats the tuned model (27 vs 35 counts knee,
    44 vs 76 belt).

  This run had **no belt limits**. Stage G drove the belt at 13–18k counts/s²
  (transients to about 40k), the kind of motion the operator reports makes the
  belt skip. The load profiles before and after cannot confirm or rule out a
  skip (they cover too little of the range), so a physical alignment check is
  needed. Belt limits were added right after:
  - 0.25 duty/s ramp and 0.5 maximum duty;
  - 1500 counts/s² acceleration abort;
  - 100-count extra inset;
  - no stage F.

  In simulation the belt's peak is 899 counts/s² with the limits on.
