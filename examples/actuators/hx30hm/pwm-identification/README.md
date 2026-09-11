# Measured PWM response: preliminary identification

Offline analysis of 63 recorded pulses from all nine servos. No hardware was accessed. Training uses each servo’s positive 25, 100 and 200/1000 pilot pulses; the 50 and 150/1000 pulses and independent short ±100/1000 pulses are held out. Fits use encoder displacement and the logged command/read windows, not the coarse internal speed register.

The shared Rust fitter estimates an empirical input/output response: deadband, gain, drive/release lags, and combined delay. It does not identify motor resistance, torque constant, inertia, friction, or sensor latency separately. Values below are provisional and valid only over the observed conditions and drive range.

| ID | Fitted deadband % | Drive / release lag ms | Combined delay ms | Worst held-out RMSE, counts | Held-out passes | Solver status |
|---|---:|---:|---:|---:|---:|---|
| 4 | 2.41 | 25.6 / 13.6 | 3.9 | 1.29 | 4/4 | stationary |
| 5 | 1.67 | 20.2 / 10.3 | 6.6 | 1.60 | 4/4 | stationary |
| 6 | 3.20 | 17.5 / 8.7 | 8.4 | 2.96 | 4/4 | stationary |
| 7 | 2.23 | 14.2 / 9.8 | 7.9 | 2.92 | 4/4 | stationary |
| 8 | 3.65 | 19.3 / 10.4 | 8.8 | 4.98 | 2/4 | stationary |
| 9 | 2.47 | 22.3 / 13.7 | 5.4 | 3.80 | 3/4 | stationary |
| 10 | 2.43 | 33.2 / 14.7 | 0.0 | 2.37 | 4/4 | stationary (bound active) |
| 11 | 2.47 | 20.6 / 13.1 | 5.2 | 1.35 | 4/4 | stationary |
| 12 | 2.48 | 17.3 / 13.2 | 5.7 | 2.32 | 4/4 | stationary |

**33/36 held-out pulses pass** the predeclared limits: encoder RMSE ≤3 counts (0.264°) and final displacement error ≤5 counts (0.439°). This is a predictive acceptance test, not a confidence interval or physical calibration certificate.

## What the failures tell us

- **ID 8:** the simple model predicts motion at 50/1000 where this particular test recorded none. Its held-out 150/1000 response also misses tolerance. One fixed deadband and gain do not explain all its low-drive behavior. Breakaway friction, control behavior, starting angle, and experimental variation remain competing explanations.
- **ID 9:** the short reverse pulse travels less than the symmetric model predicts. A single reverse trial is insufficient to attribute this uniquely to directional friction or motor asymmetry; repeat it at matched angle, voltage, temperature and load.
- **ID 10:** the initial fit hit the zero-delay bound and the iteration budget. An active-bound refinement followed by reopening all original bounds reached stationarity, with essentially unchanged predictions. The zero-delay boundary still prevents interpreting this as measured physical latency.

## Acquisition and interpretation limits

All nonzero pilot speed-register readings are multiples of 50 counts/s (about 4.4°/s), while position resolution is one count (0.088°). Some host read windows span 49–59 ms. Residual scales include one encoder count plus observed speed × half that window; this is a descriptive weighting, not a calibrated stochastic noise model. Request/complete timestamp sensitivity fits are saved in results.json. Unknown internal sample age is outside those timing brackets.
On-drive empirical lags are roughly 14–33 ms and release lags roughly 9–15 ms in these unloaded records. Their unequal values support testing zero-drive braking/coasting explicitly. They must not be copied into CAD as physical inertia or electrical constants. Voltage and temperature differ between runs; fitted servo differences are not isolated manufacturing tolerances.
No full-duty speed prediction, torque estimate, confidence interval, thermal fit, or loaded dynamics claim follows from these 2.5–20% drive pulses. No fitted values have been promoted into CAD or default simulation parameters.

## Next measurements selected by these results

1. Repeated low-drive onset tests, especially ID 8: 25–100/1000 in fine increments, both directions, multiple starting angles. Record no-motion outcomes rather than forcing a linear curve through them.
2. Continuous ascending/descending drive segments to distinguish breakaway from running friction. The current stationary-start pulse sweep alone cannot measure that hysteresis.
3. Matched repeated +100/−100 short pulses for ID 9 and the others; vary duration at fixed drive to separate delay from rise/release dynamics.
4. Zero-drive versus torque-off versus controlled reversal, initially at small drive, to identify braking/coast behavior before higher-speed reversals.
5. Supply/temperature-conditioned full-drive runs, known loads, and shared-supply concurrent runs. Reserve whole later runs for validation, not just random adjacent samples.

## Reproduce

```sh
cargo test --release -p sim-solve pulse_response
cargo run --release -p sim-runtime --example identify_hx_pwm -- \
  examples/actuators/hx30hm/hardware/2026-09-11-nine-servos NEW_OUTPUT
```

Inputs are hash-referenced in results.json; observation extraction is independently checked against the measured final-position records by this report generator. The original fit source and two analytic/recovery test results are retained alongside the report.
