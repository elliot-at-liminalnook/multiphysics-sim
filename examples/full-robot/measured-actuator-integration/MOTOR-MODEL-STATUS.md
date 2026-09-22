# What the current motor model represents

The gait experiments use a provisional physics model, not a calibrated digital
twin. The user retired the historical 0.264° criterion as a prerequisite for
gait generation; measured prediction errors remain visible.

The [offline controller tracking campaign](controller-tracking-simulation/README.md)
now compares gain settings, drive ceilings, delay/load/voltage scenarios, and shared
three/nine-motor supply behavior. It improves simulated small-motion tracking but
finds the retained full gait commands infeasible for these models. Its reference
governor is available in the shared registry; neither it nor the selected gains
have been promoted into the running quadruped. No new physical model calibration
or hardware validation is claimed.

The separate [2026-09-19 actuator-bounded gait campaign](gait-generation/actuator-bounded-2026-09-19/README.md)
now runs the governor inside every whole-robot search trial. Its CAD scenario uses
4096/4096/4096 Q8 gains and an estimated 2 ms delay. This is an offline candidate
recipe; the existing browser/hardware recipe has not been replaced. A slow
full-stride candidate lifts all four feet and travels 0.327 m in ten seconds,
but its 2.42-degree worst-joint RMS error fails the campaign's 2-degree screening
gate. That new simulation gate is not the retired physical 0.264-degree criterion.

| Aspect | Running in the current quadruped | Confidence / limitation |
| --- | --- | --- |
| Motor electrical dynamics | Resistance, inductance/current buildup, back-EMF and torque from current | Parameter estimates; current has not been calibrated against a current sensor |
| Motion response | Rotor and gearbox inertia, friction, gearing, efficiency and elastic/damped transmission | Acceleration, braking and reversal emerge from these equations; their timing is not separately qualified across loads |
| Low-level controller | Shared FPGA integer control law, 100 Hz feedback, encoder quantization, held PWM and saturation | Same control law; not a full simulation of FPGA communication or unknown internal sensor age |
| PWM driver | Averaged bridge with resistance and current limiting | Does not resolve individual PWM switching edges |
| Joint loading | Motor torque coupled to CAD-defined mechanics, gravity, contacts and transmissions | No physical leg measurements yet |
| Gear play and delay | Model supports backlash and latency | Both are zero in this provisional gait recipe; that is an assumption, not a measurement |
| Supply and heating | Voltage-dependent motor response and current/power/loss outputs | Current gait runs impose 11.1 V and fixed temperature; no active battery sag/depletion or temperature rise |
| Differences between motors | CAD supports per-unit profiles | All twelve joints currently use the same provisional family; physical motor assignments remain unknown |

The shared runtime also implements CAD battery/branch coupling and terminal-energy
integration. That capability is tested separately, but the quadruped has no authored,
measured battery/wiring profile and does not use it in these gait runs. Replaying
measured voltage during a bench comparison is not predicting the supply sag.

## Measured accuracy

The provisional family's unloaded own-feedback validation comparison, conditioned
on each real motor's recorded terminal voltage, gives:

| Physical motor | RMS angle prediction error | Peak sampled angle prediction error |
| --- | --- | --- |
| 10 | 1.773° | 3.604° |
| 11 | 1.709° | 3.955° |
| 12 | 1.654° | 3.955° |

Source: [baseline validation](voltage-conditioning/validation-baseline-closed-loop.json).
Peak values convert the recorded encoder-count error using 360/4096 degrees/count.
These compare simulated and measured angles; they are not real-controller target
tracking errors and are not universal error bounds for arbitrary motions.

Per-motor refits gave 1.916°, 1.005° and 1.058° RMS on that validation pattern:
two improved and one regressed. The refits remain unpromoted. Faster low-drive and
stress comparisons also have nonzero errors; the timing captures are diagnostic,
not newly designated clean validation data. See [fit results](voltage-conditioning/fit-summary.json).

Loaded torque, reversal timing under leg load, actual battery behavior, thermal
drift and calibrated current/power accuracy are unverified. Numerical convergence
and exact replay establish simulation consistency, not agreement with hardware.
The model can support provisional gait exploration; simulated walking speed is
not yet a reliable prediction of physical robot speed.
