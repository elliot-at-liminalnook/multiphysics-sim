# Autonomous FPGA scheduler progress

The [sequencer core and integration contract](experiment-scheduler.md) are frozen
here with their [test and synthesis evidence](manifest.json). Fifteen cycle-level
test groups passed, and the core maps to Gowin logic. This is not an integrated
bitstream and does not establish physical controller speed or accuracy.

The sequencer schedules fresh telemetry, the existing compiled controller,
and torque/PWM audits on fixed device-clock frames. Missing deadlines, lost
supervision, evidence backpressure and audit failures stop the run. Completed
transactions, in-flight interruptions and stop transmission have distinct timestamps.

Remaining work is whole-packet host/autonomous arbitration, verified trajectory
storage, a bounded timestamp/data stream and Rust capture adapter, integrated
UART/watchdog tests and place-and-route, then physical commissioning and fresh
motor/model comparisons. The current motor recordings and accuracy failures are
unchanged. No hardware was accessed in this increment.

Reproduce component checks in the hardware repository with
`make sim-experiment-scheduler` and `make check-experiment-scheduler`.
The frozen RTL/testbench can also be compiled directly with Icarus Verilog;
the Makefile snapshot includes other hardware targets whose sources remain in
that repository.
