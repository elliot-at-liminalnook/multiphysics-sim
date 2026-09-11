# HX-30HM characterization — provisional simulation

The HX-30HM is a magnetic-encoder serial-bus servo. This study runs 54 configured experiments through the shared Rust compiled multiphysics runtime. It characterizes an explicit family of hypotheses about the actuator; no hardware was measured. Open [the interactive plots](report.html) to inspect individual traces.

## Published inputs and evidence

Hiwonder lists 11.1 V nominal (9–12.6 V operating), 30 kgf·cm stall torque, 0.19 s/60° speed, 3 A stall current, 100 mA no-load current, 52 g mass, a 12-bit encoder and 0.3° servo accuracy. It describes PID and acceleration/deceleration control. The manufacturer table has misplaced headings; torque is corroborated by the product title. [Product page](https://www.hiwonder.com/products/hx-30hm), accessed 2026-09-11.

Hiwonder's [NexArm development documentation](https://wiki.hiwonder.com/projects/NexArm/en/esp32-version/docs/2_ESP32_Development_Basics.html) identifies the integrated HX-30HM and a half-duplex UART interface up to 1 Mbaud. Its command example sends acceleration, position and speed. Those register values do not establish physical acceleration limits or firmware loop timing.

## What was simulated

The bench connects registered motor, gearbox, winding electrical storage, rotational load inertia/torque, sampled servo firmware, averaged H-bridge, encoder, and two thermal capacities. Heat conducts winding → case → ambient and mounting fixture. The fixture holds mount temperature at ambient; this is an explicit idealization. Voltage curves apply ideal motor terminal voltage; position tests use the bridge. Cold runs clamp temperature at 25°C. All parameters and overrides are in [plan.json](plan.json), with exact CAD motor source in [cad-motor.json](cad-motor.json).

The original output-equivalent CAD model has Kt ≈ 0.981 N·m/A and Ke ≈ 2.014 V·s/rad. Those unequal SI constants do not describe a reciprocal electromagnetic converter. For the detailed energy study, assume R=V/Istall=3.7 Ω, hypothetical internal ratio N=200, Kt=Ke=(V−R I₀)/(N ω₀)=0.00973408, and efficiency τstall/(N Kt Istall)=0.5037. This fits the endpoint ratings; it does **not identify** the true ratio, motor constants or efficiency. The reported no-load current is provisionally treated as motor loss current; actual electronics consumption could be part of it.

Internal rotor inertia uses the CAD estimate; gearbox output inertia is explicitly 2×10⁻⁵ kg·m². Temperature changes R using the CAD coefficient; Kt/Ke are held constant to preserve reciprocity. Thermal capacities, resistances, gear stiffness, damping and servo gains are unmeasured. Extra Coulomb friction is zero in the endpoint model to avoid counting fitted no-load losses twice.

## Torque, speed and power

| Load (N·m) | Speed (rad/s) | Speed (rpm) | Current (A) | Output (W) |
| --- | --- | --- | --- | --- |
| 0.000 | 5.512 | 52.6 | 0.100 | 0.000 |
| 0.735 | 4.086 | 39.0 | 0.850 | 3.005 |
| 1.471 | 2.661 | 25.4 | 1.600 | 3.914 |
| 2.206 | 1.235 | 11.8 | 2.350 | 2.726 |
| 2.648 | 0.380 | 3.6 | 2.800 | 1.006 |

At rated voltage the motor produces zero output mechanical power at stall even while consuming about 33.3 W electrically. The sampled moving curve peaks near half stall load. Stall torque is not continuous torque. The 12.6 V ideal-source stall case exceeds 3 A; it is an extrapolation without current protection, not a usable operating rating.

The original detailed CAD preset gives 5.095 rad/s and 0.227 A at no load, versus 5.512 rad/s and 0.100 A for the endpoint fit. This discrepancy is directly relevant when substituting the detailed actuator for the gait's effective servo. It does not retroactively change any recorded gait result.

## Positioning and motion

A 60° target step starts from zero with no proprietary acceleration planner. Angles are relative to a fixture reference (which can be assigned to the hardware mid-position), not raw absolute register commands. Inertia is attached at the output shaft. “Settled” means every remaining 1 ms sample lies within 0.3° of target through the one-second run. It is not proof of indefinite stability.

| Load inertia (kg·m²) | First 90% (s) | Settled (s) | Overshoot (°) | Peak sampled current (A) |
| --- | --- | --- | --- | --- |
| 0.0001 | 0.182 | 0.316 | -0.013 | 2.014 |
| 0.001 | 0.184 | 0.319 | -0.010 | 2.023 |
| 0.01 | 0.197 | 0.390 | 0.131 | 2.024 |

| Holding load (N·m) | Final error (°) | Tail RMS error (°) |
| --- | --- | --- |
| 0.5 | 0.890 | 0.892 |
| 1 | 1.717 | 1.727 |
| 2 | 3.442 | 3.442 |

The inherited controller is PD with zero integral gain. Its load-dependent position error is a model outcome, not evidence that the real product has the same holding error. The real PID and trapezoidal planner need identification.

Oscillation uses ±0.25 rad (±14.3°), starting at zero. Gain and phase are the fundamental Fourier response over the final half of each run. Saturation makes these large-amplitude results distinct from small-signal bandwidth.

| Frequency (Hz) | Amplitude gain | Phase (°) | Tail RMS tracking error (°) |
| --- | --- | --- | --- |
| 0.5 | 0.992 | -7.2 | 1.275 |
| 1 | 0.974 | -13.4 | 2.345 |
| 2 | 0.912 | -24.9 | 4.264 |
| 4 | 0.749 | -43.0 | 6.907 |
| 8 | 0.483 | -64.1 | 9.120 |

Latency (1, 5, 20 ms), stiffness (20, 50, 200 N·m/rad) and reversal gap (0, 0.2, 1°) sweeps are preserved in the full case table. These are hypotheses, not statistical confidence intervals. The 20 ms delayed case can expose instability in these assumed gains. H-bridge current limiting is a soft foldback, so a 3 A parameter does not guarantee a hard instantaneous clamp.

## Heating

| Scenario | First ≥110°C winding (s) | Winding at 60 s (°C) | Case at 60 s (°C) |
| --- | --- | --- | --- |
| thermal_stall_r0.5 | 10.35 | 168.9 | 45.2 |
| thermal_stall_r1 | 8.40 | 239.8 | 41.3 |
| thermal_stall_r2 | 7.70 | 316.9 | 36.0 |
| thermal_free | — | 37.1 | 25.8 |
| thermal_halfload | 18.80 | 200.8 | 36.5 |
| thermal_stall_fine | 8.40 | 239.8 | 41.3 |

**These are unprotected mathematical extrapolations.** 110°C is a provisional CAD marker, not a known HX-30HM shutdown threshold. Runs continue to 60 s without modeling insulation failure, electronics shutdown, magnet changes or structural damage; values above the marker show the model's heating tendency, not survivable hardware temperatures. Case telemetry can lag winding temperature substantially. There is insufficient evidence to claim a continuous torque or safe duty cycle. Thermal resistance multipliers 0.5–2 illustrate mounting/cooling sensitivity.

## Encoder and predicted dynamics

Uniform 12-bit quantization gives 0.087890625° per count and ±0.0439453125° rounding error at a sample instant, smaller than the manufacturer's separate 0.3° accuracy figure. Accuracy includes effects this ideal quantizer cannot discover. A 0.1 m lever turns one count into approximately 0.153 mm tangential displacement.

| Read interval | One-count velocity step (rad/s) | One-count acceleration step (rad/s²) |
| --- | --- | --- |
| 1 ms | 1.534 | 1534.0 |
| 5 ms | 0.307 | 61.4 |
| 20 ms | 0.077 | 3.8 |

The shared sensor.encoder samples the simulated shaft at 1, 5 and 20 ms. [analysis.json](analysis.json) compares angle and derivative errors against the same physical angle stencils. For 5 and 20 ms sensors, the analysis reads one recorded millisecond after each nominal sensor tick and associates it with the physical angle at that tick. Captures exactly on a floating-point event boundary can still contain the previous held reading; boundary-read errors are retained separately. This timing artifact is not physical encoder noise. Differentiating encoder counts twice greatly amplifies quantization; future-dynamics training should distinguish latent physical velocity/acceleration from measured/estimated versions and include the observation timing/filter in its model. Encoder radians are unwrapped; actual bus register rollover and magnetic field imperfections are not modeled.

## Verification and scope

All cases completed: **true**. Endpoint relative speed error 7.68e-14; stall torque error 9.43e-5. These test parameter construction, not independent hardware validation.

Step and 4 Hz runs compare 0.5, 0.25 and 0.125 ms integration; heated stall compares 5 and 2.5 ms. The 0.25→0.125 ms position check changes settling time by 1 ms and final angle by about 0.0002°. The coarse clock underestimates startup current: extra 10 and 5 µs integration runs, sampled every 10 µs, predict 2.245 and 2.250 A peaks, differing by 0.26%. Thermal tests disable sampled encoder events so sensor ticks do not silently shorten the compared steps. Numerical comparisons are in analysis.json. [validation.json](validation.json) records 11 passing simulation checks with explicit thresholds. Fourteen focused motor, analytic-Jacobian, reduction and energy tests passed. The new energy test covers both motoring and backdrive, including winding storage, reflected inertia, gear elastic storage, no-load loss and damping heat.

The library correction now returns no-load friction and coupling damping to heat, uses directional gearbox loss, and reports gear inertia/elastic storage in its energy inventory. All mechanical heat is lumped into the winding port; the actual partition to housing is unknown. Existing unequal Kt/Ke presets and temperature-dependent Kt alone can still violate reciprocity. No automatic CAD parameter promotion was made.

No magnetic field FEM is claimed: magnet geometry/material, air gap, sensor IC, eccentricity and calibration data are unavailable. No real control bandwidth, backlash, torque-speed curve, protection threshold or thermal rating was measured.

## Reproduce and calibrate

From the repository root:

```sh
node examples/actuators/hx30hm/prepare.mjs
cargo run --locked --release -p sim-runtime --example characterize_actuator -- examples/actuators/hx30hm/plan.json /path/to/new-output-directory
node examples/actuators/hx30hm/report.mjs /path/to/new-output-directory/
```

The result stores library and experiment source identities, the full parameter plan and per-case CSV traces. [manifest.json](manifest.json) hashes the raw results. Existing robot CAD and browser gait were preserved.

To constrain this model, collect timestamped target and measured shaft angle under known inertia and load, terminal voltage/current, cold no-load and brief stall endpoints, reversal curves, and case/winding thermal observations where available. Identify physical command acceleration and latency using step/ramp commands; measure current with an external instrument if the servo does not expose it. Accepted measured values belong in CAD with provenance and uncertainty.
