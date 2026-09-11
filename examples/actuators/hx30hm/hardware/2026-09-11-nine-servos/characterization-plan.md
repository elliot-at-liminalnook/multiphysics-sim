# Full characterization target and evidence gaps

Objective: identify the capabilities and limits of all nine HX-30HM servos, including extreme concurrent conditions, sufficiently to fit and validate robot simulations against hardware. Passing the initial movement tests does not complete this objective.

| Required capability | Current evidence | Remaining measurement |
|---|---|---|
| Identity, firmware, register interpretation | All nine IDs 4–12, firmware 3.15, verified voltage/temperature byte separation | Calibrate onboard voltage/current/temperature against external instruments |
| Position motion in both directions | Completed individual amplitude/speed ladders at two reported supply conditions | Repeated trajectories, acceleration limits, near-target behavior, load dependence |
| PWM control | All nine passed positive 2.5–20% requested drive pulses; verified zero-drive stopping | Reverse bit 10 verified on all nine; full-duty speed and repeatability remain |
| Velocity control | Documented; historical non-versioned bench notes | Repeat validated signed velocity trials on all nine with retained raw logs |
| Concurrent speed and transients | All nine passed small position stages, then rail sag stopped escalation | Adequate current headroom, power-distribution measurements, repeated concurrent PWM/velocity tests |
| Deadband, asymmetry, friction | Low-drive response differs between servos; ID 8 did not move at 25 or 50/1000 | Dense bidirectional drive sweep with repetitions and known load; separate stiction from controller thresholds |
| Torque-speed envelope and saturation | Manufacturer ratings only | Known lever geometry and calibrated external force/weight; controlled load increments on each servo |
| Acceleration, inertia, compliance, backlash | Host-window step records; mechanisms not separately identified | Known attached inertia and reversible load, independently measured output angle/force |
| Thermal behavior and protection | Reported warm-bench temperatures and test stop gates | Ambient measurement, current calibration, heating/cooling curves under known loads, repeatability and controlled protection approach |
| Timing, sensor freshness, command-loss behavior | Host timestamp windows and verified checksums | FPGA timestamps, sensor update-rate measurement, hardware verification of the software-tested FPGA timeout, bus-load sensitivity |
| Simulation agreement | Earlier multiphysics model contains explicit estimates | Fit identifiable parameters in shared Rust components; evaluate held-out measured trajectories and confidence intervals; promote accepted physical values into CAD with provenance |

## Experimental order

1. Establish bidirectional PWM with small pulses and verified zero drive.
2. Add and verify an FPGA-side drive timeout before prolonged or high-bandwidth PWM operation. Keep host STOP and independent torque-disable cleanup.
3. Measure single-servo PWM/velocity envelopes and repeatability, increasing drive and duration only after the preceding stage is measured. Preserve voltage and temperature conditions per run.
4. Repeat across all nine concurrently with adequate supply current headroom; isolate supply/harness limits from servo limits.
5. Add known external loads and inertia, then measure torque, dynamics, backlash/compliance, and thermal behavior. Unloaded motion cannot identify these uniquely.
6. Fit and validate the shared simulation against held-out trajectories. Report identified combinations separately from individual parameters that remain ambiguous.

## Physical inputs currently requested

- Confirmation of a 2 A current limit at the user's selected 12.6 V before retrying demanding all-nine motion. Last confirmed limit is 1 A; a measured rail collapse is retained.
- Available horn/lever, known weights, or force scale/load cell for torque and loaded thermal tests. No load has been invented or assumed from internal current readings.

Never infer an absolute mechanical speed limit from a command plateau alone, nor calibrated torque from open-loop PWM or an uncalibrated current register. Each measured limit belongs to its stated supply, load, temperature, mode, and timing conditions.

## Offline preparation after power removal

See [software validation](../../software-validation/README.md): FPGA supervisor built and simulated, Rust full-duty individual/concurrent PWM schedule and serial rehearsal, shared-physics direction/timestep checks. No new hardware measurements were taken. The safety image is not yet flashed; hardware commissioning precedes the full sweep.

## Initial identification from existing PWM records

[Shared Rust pulse-response fitting](../../pwm-identification/README.md) used 27 training pulses and 36 held-out pulses. 33/36 meet the declared encoder-error tolerances; ID 8 low-drive behavior and ID 9 reverse response need richer experiments. All nine numerical fits reached stationarity, but ID 10 remains at a zero-delay bound. These are empirical low-drive response fits, not separate physical motor parameters; nothing was promoted into CAD.
