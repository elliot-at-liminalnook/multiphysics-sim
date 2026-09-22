# Browser gait control with simultaneous hardware comparison

The first finite browser-to-hardware slice is now implemented and tested on the
three connected bench motors. See [working bench UI and verification](browser-hardware/README.md).
The UI maps a selected CAD leg's hip, worm and foot to editable motor IDs and
polarities, previews and plays bounded recorded gait targets, and displays actual
encoder/voltage/temperature traces. Two complete 100 Hz clips and separate Stop
button and controlling-tab disconnect checks passed with physical stop verification.
The broader live-simulation and continuous-steering design below remains pending.

The browser should send motion intent through the shared Rust controller. A local
Rust hardware bridge should distribute a timestamped joint-reference stream to
the simulation and FPGA. Keep the FPGA's low-level control, clocked execution,
watchdogs and stop handling independent of rendering. Log actual device execution
and telemetry times, sequence numbers, model/controller identities and commands.

Use the same references and controller law with each side's own feedback. Identical
targets do not imply identical PWM or motion. Display requested, simulated and
measured joint angles together, with timing skew and stale/missing samples visible.
Do not claim exact simultaneity across USB, a serial bus and browser presentation;
define and measure the scheduling tolerance.

Existing surfaces include the Rust/WASM `EnvironmentSimulation.step` action
contract and FPGA finite trajectory scheduling with device timestamps. Continuous
interactive streaming and browser-to-hardware session coordination still need
implementation. The detailed quadruped model is slower than realtime; use a
validated faster shared Rust profile for live comparison, or explicitly show a
delayed detailed replay. Hardware timing must never depend on simulation catching up.

The quadruped has twelve simulated actuators. Hardware participation must use an
explicit subset mapping to discovered IDs, measured encoder origins/polarities and
allowed travel. A bench motor is not a measured loaded leg. A deliberate hardware
arm action and existing stop/watchdog behavior belong in the UI contract.
