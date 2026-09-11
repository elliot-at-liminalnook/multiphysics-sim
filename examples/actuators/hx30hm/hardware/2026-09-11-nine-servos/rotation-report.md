# Full-rotation PWM characterization, 2026-09-11

All nine servos completed individual full-power rotation in both directions, and all nine together reached full power with gradual PWM ramps. Abrupt all-nine starts had previously lost a reply at 80%. The ramp result establishes an operating strategy beyond that failure; it does not isolate its electrical cause.

The bench was user-confirmed at 12.6 V / 3 A, with unrestricted unloaded shafts and one daisy chain. During the earlier abrupt run the user observed CV, 12.6 V and up to 0.9 A. Connector voltage and fast supply-current transients were not independently measured. Firmware 3.15, mode 2, direction bit 10.

## Sustained individual speed

Encoder slopes in the final half-second of the 3-second full-drive holds, degrees per second (magnitudes):

| ID | Forward | Reverse |
|---|---:|---:|
| 4 | 256.5 | 262.7 |
| 5 | 275.1 | 271.1 |
| 6 | 266.3 | 263.2 |
| 7 | 246.6 | 257.0 |
| 8 | 268.7 | 274.0 |
| 9 | 260.2 | 268.6 |
| 10 | 261.5 | 262.8 |
| 11 | 272.7 | 277.9 |
| 12 | 265.3 | 270.4 |

All 18 holds changed by less than 1.4% between their last two half-second windows. Earlier 1.5-second tests also passed the 5% plateau criterion. Different starting angles, elapsed warm-up, voltage and sensor noise mean these are measurements under stated conditions, not immutable motor constants. No absolute mechanical maximum is claimed.

## Concurrent ramps and power distribution

| Simultaneously driven | Encoder speed during final full-command interval, degrees/s | Lowest sampled servo voltage |
|---|---:|---:|
| 3 | 243.1–257.9 | 11.2 V |
| 6 | 229.5–241.0 | 10.4 V |
| 9 | 217.1–240.1 | 9.8 V |

Both directions completed in each group. Nine-servo commands ramped from 25 to 900/1000 in 150 ms segments, followed by 1.5 seconds at 1000/1000. Three/six-servo ramps used 200 ms segments and 0.9 seconds at full drive. Different durations and sequential sensor polling limit direct comparisons. Nine-servo half-second windows do not always contain the five samples required by the plateau check; its null result is insufficient evidence, not a failed plateau.

Larger groups run slower and report lower voltage. The confirmed daisy chain makes shared wiring/contact resistance a plausible contributor, but telemetry calibration and brief transients remain alternatives. Repeating matched tests with measured connector voltage and shorter power paths would distinguish them.

## Braking versus torque-off

Servo 12 at full drive for 1.5 seconds:

| Stop mode | Final stationary interval begins after bridge receipt | Travel from last pre-command sample |
|---|---:|---:|
| Zero PWM, torque enabled | 0.096–0.107 s | 9.1–9.3 degrees |
| Torque disabled | 0.402–0.510 s | 52.5–65.5 degrees |

These are host-window observations, not exact command-edge stopping distances. Torque remained off by readback throughout coast observation; no PWM write was sent during that interval. The retained PWM goal was cleared only after stationarity and followed by torque-off again. The simulation must distinguish braking from coasting; the earlier zero-PWM response fit cannot stand in for both.

## Quietness and measured variation

The user identified ID12 as relatively quiet. Over the 3-second holds, its encoder-derived speed standard deviation divided by mean speed was 1.9–2.9%, versus 0.45–0.63% for ID6. This is descriptive variation after fitting local 90 ms windows, excluding startup; it includes sensor/timing effects and cannot rank audible noise. Only around two turns per direction were recorded. Angle bins are retained for follow-up; they do not diagnose a mechanical defect.

Motor torque ripple, friction and measurement resolution can all influence apparent speed smoothness ([maxon discussion](https://support.maxongroup.com/hc/en-us/articles/360016541693-Speed-measurement-and-Accuracy-of-speed-control)). No microphone, accelerometer or independent encoder was used, and no HX-specific commutation implementation is inferred.

## Reproduction and validation

- [Machine-readable review and input hashes](rotation-review.json).
- [Three-second full-drive report](rotation-full-drive-3s-report.json), [all-nine ramp report](rotation-nine-ramp-report.json), [smoothness data](rotation-smoothness-report.json).
- Raw transactions, CSV and atomic completed-trial checkpoints are in the corresponding run directories. Unexecuted prepared plans do not constitute measurements.
- [Runner source snapshot](rotation-runtime/manifest.json): shared cyclic encoder tracker, explicit unrestricted-shaft policy with provisional 8192 counts/s sampling bound, torque-off/coast correction, and bounded final stationarity observation. The speed bound rejects ambiguous wraps; it is not a CAD motor property.
- Acquisition tests: 14 passed. Runner protocol tests: 11 passed, including all-nine leases, free-rotation preflight, late settling, fault cleanup and zero-PWM versus torque-off behavior. Release build passed. The fitter adapter compiles with the unwrapped-position column; no new continuous-rotation physical fit is claimed.
- Final readbacks verify PWM zero, torque disabled and NVS locked for every ID, followed by 476 stationary samples with zero communication failures. Final temperature maximum 49 C.

No calibrated torque, current, inertia, friction, thermal model or CAD physical parameters were inferred from these unloaded tests.
