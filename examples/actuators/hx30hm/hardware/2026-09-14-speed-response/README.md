# Actual FPGA speed-ramp and reversal experiments

September 14, 2026. These are **physical motor measurements**, followed by a
separate Rust simulation of the same integer controller. The motors remain
unloaded; accurate motor-model acceptance is still unfinished.

[Measured plots](measured-response.png) · [Native viewer study](study-predicted.json) ·
[Exported study](study-predicted.html) · [Measurement summary](measured-summary.json) ·
[Speed-response comparison](nine-repeat-comparison.json)

## What ran

The initial two supervisor requests received no bytes. Their failed inspections
are retained. The previous commissioned, bounded-drive SRAM image was restored
after checking all 12 retained source/build SHA-256 entries. Loading succeeded;
the unfinished autonomous bridge was not built or programmed.

All nine IDs 4–12 then responded in PWM mode, stationary and torque disabled,
at 12.1–12.4 V and 40–42 °C. Physical command-loss, telemetry-loss and absent-host
watchdog probes passed, including autonomous zero/off and stationary-tail checks.
A 14-frame all-nine zero-command trial then passed.

Three finite motion trials followed: ID4 alone, all nine together, and a separately
reserved all-nine repeat. Each requested +3.515625°/s, then −3.515625°/s, then
+3.515625°/s, inside ±60 encoder counts (±5.2734375°) of its starting position.
Targets, gains and analysis requests were saved before each motion trial. The
FPGA computed every PWM command; the host scheduled targets and observations.
Kp=3, Kd=0, velocity feedforward=4, drive ceiling=75/1000 (7.5%). Single-motor
cadence was 100 ms; all-nine cadence was 150 ms. These are low-speed bounded
response tests, not maximum-drive or endurance tests.

All three completed: **1,052 physical motor updates**, with every recorded PWM
matching the shared Rust controller arithmetic and per-frame torque enable
confirmed. Every trial verified all-nine PWM zero, torque off and stationary
feedback afterward. A separate final inspection confirmed those states again.
Final temperatures were 41–44 °C. Servo voltage reached 11.7 V during group motion;
raw current remains uncalibrated, so amps, watts and battery behavior are unproven.

## What the measurements say

| Physical trial | Tracking RMS | Worst tracking peak |
| --- | --- | --- |
| ID4 alone | 0.292° | 0.703° |
| All nine | 0.315–0.639° | 1.230° |
| All-nine repeat | 0.382–0.577° | 1.318° |

The unchanged limits are 0.2637° RMS and 0.8789° peak. All motion trials fail the
RMS gate. These patterns differ from yesterday's mixed-frequency patterns, so
the smaller numbers do not establish a general controller improvement.

The position plots resemble ramps, but estimated speed varies substantially
inside them. The existing simulation is smoother: own-feedback prediction of the
reserved repeat has **0.487–0.705° RMS position error**, failing the original
three-count prediction gate for every motor. No model, gain revision or CAD
parameter was promoted. This reserved repeat is now inspected evidence; future
fitted candidates need new confirmation data.

## Speed-up and reversal timing

The shared Rust analyzer derives speed from the same quantized position samples
and timing windows in measurement and simulation. It also reports separated
secant acceleration, departure from the initial speed band, 10–90% rise, settling,
zero crossing, opposite-speed settling and sampled forward excursion. This is
observable sampled behavior, not perfect simulated velocity versus noisy hardware.

The requests used a two-sample lag (nominal 0.20/0.30 s support), 0.02 rad/s
tolerance and 0.25 s dwell, declared before motion. This tolerance is for response
timing, not a replacement for the existing tracking/model accuracy gates. Sensor
sample age remains unknown. Windows include USB/FPGA transport and cannot support
millisecond-precise mechanical claims.

All **57 declared speed transitions** lacked a resolved final steady-speed tail.
The initial steady speeds needed for complete reversal timing were also unresolved.
The analysis preserves those missing results; it does not turn the first sign
change or a position ramp into a claimed complete reversal time. These captures
demonstrate actual bidirectional movement, but do **not** establish how quickly
the motor can reach its physical maximum speed or fully reverse at that speed.

Next, distinguish drive/low-speed motion variation from sampling effects, improve
the sampling cadence, and collect dedicated holds with sufficient travel and dwell.
Keep both directions, motor identity, voltage, temperature and simultaneous-load
conditions. Refine inertia, losses and drive dynamics in the shared model against
held-out response evidence; do not add a universal hardcoded reversal delay.

## Software and evidence verification

- Nine motor-response tests pass: analytic acceleration, speed-up and both
  reversal directions, incomplete/quantized data, timing uncertainty, real
  nine-motor evidence, and shared MotorUnit inertia/timestep behavior.
- Seven FPGA-controller, six device-event and seven upload tests still pass.
- The native viewer compiles with a background speed-response analyzer, per-motor
  measured/predicted speed and acceleration plots, explicit unresolved reasons,
  condition metadata and new-file JSON export. Native interaction with the new
  section has not been manually exercised in this increment.
- The real recordings, conditions, plans, source hashes, raw transactions, initial
  failed inspections, restored-image load receipt and watchdog evidence are retained.
  Early software fixture build/runtime errors and subsequent passing logs are
  retained; they did not affect the acquisition controller or loaded FPGA image.

Review without touching hardware, from the repository root:

```sh
cargo run -p sim-runtime --example review_controller -- review-motor-response \
  examples/actuators/hx30hm/hardware/2026-09-14-speed-response/ramp-nine-repeat/fpga-recording.json \
  examples/actuators/hx30hm/hardware/2026-09-14-speed-response/ramp-nine-repeat-request.json \
  /tmp/new-motor-response.json \
  examples/actuators/hx30hm/hardware/2026-09-14-speed-response/nine-repeat-predictions.json
```

Open `study-predicted.json` using `cargo run -p sim-viewer -- --experiments PATH`.
It retains the earlier 15 recordings and these three new ones, plus the unchanged
model's fresh own-feedback prediction. The separate response JSON preserves the
exact estimator/request and source hashes. `render_response.py` plots saved Rust
results only (`uv run --with matplotlib python PATH/render_response.py`).

The live Sipeed tree still contains an **incomplete, untested** autonomous-bridge
integration (`experiment_session.v` and partial `bridge_control.v` wiring).
Complete arbitration, START handling, output serialization and full UART tests
before using that source for a new hardware image. Today's loaded image is the
older frozen `2026-09-13-controller-refinement/fpga-control/firmware-bounded-drive/impl/bridge_control.fs`.
