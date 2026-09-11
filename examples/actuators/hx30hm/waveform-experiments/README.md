# HX-30HM waveform experiments

These are acquisition plans for the shared Rust sweep scheduler and the
FPGA-supervised `characterize_hx_bridge` example. A generated schedule is a
software preview, **not evidence that its trials ran on hardware**.

The plans target gaps in the initial 2.5–20% duty pulse fits:

* `individual.json`: onset/deadband, ascending versus descending drive,
  pulse duration, direct reversal, and zero-PWM versus torque-off release.
* `concurrent.json`: the same selected comparisons with all nine servos,
  retaining individual trials as anchors for supply-coupling comparisons.
* `dynamic.json`: individual PRBS and chirp inputs for checking whether the
  fitted response predicts trajectories outside simple step experiments.

Drive units are signed thousandths of full PWM. Firmware 3.15 on this bench
uses direction bit 10, verified from encoder displacement. The waveform
executor records actual host command request/bridge-receipt windows and
telemetry request/completion windows; nominal segment duration is not a
device timestamp or a guaranteed update rate. Each segment must observe every
participating servo, otherwise the run aborts. The 20 ms individual segments
therefore require hardware acquisition-rate validation before interpreting
their frequency response. Concurrent plans use longer segments.

Zero PWM keeps torque enabled; an explicit torque-off segment verifies the
torque register and is terminal until the next stationary trial preflight.
No loaded torque, calibrated current, or identified electrical constants are
implied by these plans. Supply settings, load, fixture and ambient changes
must be recorded as experimental conditions.

Run a software-only schedule preview:

```sh
cargo run --release -p sim-runtime --example characterize_hx_bridge -- \
  --validate-sweep examples/actuators/hx30hm/waveform-experiments/individual.json \
  4,5,6,7,8,9,10,11,12
```

Hardware execution requires a new output directory and the commissioned
safety FPGA profile. Checkpoints bind the exact plan, condition description,
and runner source; changed experiments start a new run.
