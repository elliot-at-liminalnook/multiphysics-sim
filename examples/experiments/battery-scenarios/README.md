# Electrical and battery validation

These are **simulation scenarios, not battery measurements or an accepted motor
calibration**. The compact input review preserves the original recordings, their
roles and fixture context. Its notes identify the full, unchanged source review.
The motor parameters remain the endpoint-derived baseline. Illustrative battery,
sensor, electronics-load and protection values are explicit in each request.

The Rust viewer's **Controller design & accuracy → Electrical & battery** panel
edits the source and controller sensors, runs the shared circuit, displays V/A/W,
energy and state of charge, and compares electrical measurement channels. The
result selector retains earlier scenarios. Tracking and electrical acceptance are
separate. Missing thresholds are unscored.

- `charged-request.json`: example charged battery and a 0.25 A auxiliary load.
- `low-charge-request.json`: lower charge, larger source resistance and a 0.75 A
  auxiliary load, to exercise sampled controller protection and failed limits.
- `regulated-request.json`: constant source at the original recording's initial
  voltage, for comparison with captured servo-voltage variation. No amps or watts
  are inferred from the existing current register.

Reproduce from the repository root, choosing unused output filenames:

```sh
cargo run --locked -p sim-runtime --example review_controller -- \
  simulate-controller examples/experiments/battery-scenarios/input-review.json \
  examples/experiments/battery-scenarios/charged-request.json /tmp/charged-review.json
cargo run --locked -p sim-runtime --example review_controller -- \
  simulate-controller /tmp/charged-review.json \
  examples/experiments/battery-scenarios/low-charge-request.json /tmp/battery-review.json
cargo run --locked -p sim-viewer -- --experiments /tmp/battery-review.json
```

To compare recorded voltage:

```sh
cargo run --locked -p sim-runtime --example review_controller -- \
  simulate-controller examples/experiments/battery-scenarios/input-review.json \
  examples/experiments/battery-scenarios/regulated-request.json /tmp/regulated-review.json
cargo run --locked -p sim-runtime --example review_controller -- \
  predict-recording /tmp/regulated-review.json 0 replay /tmp/electrical-prediction.json
cargo run --locked -p sim-runtime --example review_controller -- \
  compare-electrical /tmp/electrical-prediction.json 0 servo-voltage /tmp/voltage-review.json
```

`compare-electrical` also accepts an electrical-measurement JSON file instead of
`servo-voltage`. Use the format below. The UI imports that same format and uses
the same comparison function. Run work happens off the UI thread with cancellation.

## Electrical measurement format

The sidecar refers to the immutable recording's BLAKE3 fingerprint; it does not
rewrite the original controller observations. `source_hashes` contains the
BLAKE3 hashes of the original sensor files and calibration evidence. Every channel
retains raw values, sensor identity, circuit location, signed conversion, units,
calibration evidence, uncertainty, and measurement windows in the recording clock.

```json
{
  "version": 1,
  "recording_hash": "REPLACE_WITH_64_HEX_RECORDING_BLAKE3",
  "source_hashes": {"sensor-log.csv": "REPLACE_WITH_64_HEX_FILE_BLAKE3"},
  "timing_evidence": "Describe clock synchronization, offset derivation and timing uncertainty",
  "channels": [
    {
      "name": "supply_current",
      "calibration": {
        "sensor": "Identify the actual sensor and revision",
        "circuit_location": "servo supply node",
        "raw_unit": "ADC count",
        "gain": 0.001,
        "offset": 0.0,
        "evidence": "Replace this illustrative conversion with calibration evidence",
        "uncertainty": "Declare measured error and bandwidth limits"
      },
      "raw_samples": [
        {"time_s": 0.01, "request_s": 0.009, "completion_s": 0.011, "value": 250.0},
        {"time_s": 0.02, "request_s": 0.019, "completion_s": 0.021, "value": 300.0}
      ]
    }
  ],
  "limits": {}
}
```

The placeholders above intentionally do not validate. Canonical value is
`raw * gain + offset`. Supported inputs are `supply_voltage`, `supply_current`,
`winding_voltage` and `winding_current`, converted to V or A. Positive supply
current means discharge; negative means return to the source. Winding polarity
must match the model's motor terminals. A current-only recording can compare amps
but cannot produce watts or joules.

Add voltage and current with identical sample times, windows and explicit circuit
location to derive `supply_power` or `winding_power`. Measured channels are never
silently interpolated to manufacture synchronization. Prediction channels are
explicitly linearly sampled from the retained fine-step simulation; the report
states its maximum interval and rejects extrapolation. Energy uses the same
measurement interval and separates consumption from return.

Optional `limits` entries use channel names and `{ "rmse": ..., "final_abs_error": ... }`
in canonical V, A or W. They are stored with each immutable comparison. No defaults
claim electrical acceptance; the servo-voltage convenience comparison is unscored.
Electrical measurement uncertainty is displayed as evidence, not converted into
an unsupported statistical confidence interval.

## What this does and does not establish

- Supply current and winding current come from distinct points in the shared
  averaged H-bridge circuit. Supply watts include the explicit auxiliary load.
  Sampled peaks are not switching-current peaks.
- The battery uses the shared generic open-circuit voltage curve, internal
  resistance and coulomb counting. Capacity, resistance, initial charge and
  auxiliary load are hypotheses until measured. Runs stop when state of charge
  leaves [0,1]; extrapolated depletion/overcharge is not a valid prediction.
- Controller-visible channels are sampled/quantized and use the declared
  observation and command delays. Optional voltage compensation rescales PWM
  within its duty bound. Sampled protection requests zero PWM; it does **not**
  disconnect a battery or remove an auxiliary load. Independent FPGA protection
  remains separate.
- Existing bench voltage has 0.1 V register resolution and unknown absolute error
  and sample age. Existing current counts have neither an established amps
  calibration nor an established circuit location. Current/power/energy accuracy
  on the actual bench therefore remains unvalidated. The live bridge rejects
  controller plans requesting calibrated current channels it cannot provide.
- This is a single-actuator fixture. A constant auxiliary current can stress the
  shared source, but does not reproduce coordinated loads from nine real motors.
  Whole-robot battery distribution, wiring drops, pack/BMS limits, thermal effects,
  measured discharge/relaxation, automated fitting of electrical traces, and an
  external live current-sensor adapter still need implementation/validation.
- Supply scenario changes are retained as experiment hypotheses. CAD proposals
  explicitly flag them as unmapped; they cannot silently become motor properties.


## Retained results

See the [native panel preview](viewer-battery-review.png). Open [battery-review.json](battery-review.json) in the viewer, or inspect the
[standalone battery report](battery-review.html). Both scenarios and their original
thresholds are retained. [voltage-review.json](voltage-review.json) and its
[report](voltage-review.html) compare the original 60 ID 4 supply-voltage samples;
all report 12.1 V, so the constant-source voltage RMS is zero at that coarse
resolution. This comparison is unscored and does not validate current, power or
battery dynamics. [results-summary.json](results-summary.json) contains the compact
numerical outcomes. The charged case passes the illustrative electrical thresholds
but fails tracking. The low-charge case fails voltage/current limits and activates
controller protection on every tick; auxiliary load continues consuming energy.


## Shared CAD robot accounting

The CAD robot runtime now records the actual shared battery terminals with
`accounting_version: 2`: minimum and final voltage, signed current/power, drawn
and returned energy/charge, source samples and their maximum interval. Net
`energy_j` means drawn minus returned source energy. Earlier battery reports used
final voltage as the minimum and summed absolute winding energy; those older
metrics must not be treated as battery-terminal measurements. The source
integration function is shared with the single-motor experiment and measurement
comparisons. CAD cutoff crossings are reported; no BMS behavior is invented.

The retained [two-motor report](two-motor-source-report.json),
[PNG plot](two-motor-source.png) and [SVG plot](two-motor-source.svg) exercise two
oppositely driven motor/bridge branches on one source and shaft. This synthetic
fixture removes no-load loss/backlash and gravity explicitly to check circuit
accounting. It is not a measurement of the Hiwonder motors or a battery. The
report retains source/fixture hashes, runtime identity and experimental overrides.
Generate a new report with:

```sh
SIM_ROBOT_POWER_TEST_REPORT=/tmp/two-motor-source-report.json   cargo test --locked -p sim-runtime --test robot_power
```

The tests check branch-current balance, load-dependent sag, charge consumption,
voltage recovery, distinct source/winding energy, signed draw/return accounting,
step-size convergence and invariance to host-call chunking. Depletion and runtime
rewinds cannot produce apparently valid battery histories.

The current quadruped capture has 12 motors and no declared battery. Its topology
is separate from the nine motors on the bench. Whole-robot battery experiments
still require an explicit pack/wiring definition and physical validation. The
optimized embedded motor path continues to label its imposed supply boundaries;
these changes do not introduce battery dynamics into that approximation.

The corrected report captures the controller's zero-PWM event at 10 ms and brackets
source-current cessation with the next 0.1 ms sample. Its net battery energy is
6.9408 mJ. The [initial timing error](two-motor-source-timing-rejection.md), source
and plots remain retained separately; they are not the accepted demonstration.
