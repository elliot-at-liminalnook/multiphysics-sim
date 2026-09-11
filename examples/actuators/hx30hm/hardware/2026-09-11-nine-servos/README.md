# Nine HX-30HM servos: first hardware characterization

**Latest results:** [full-rotation speed, concurrent ramps, braking/coasting, and smoothness comparison](rotation-report.md). All nine reached full PWM together using ramps; individual sustained speeds, power-distribution evidence and remaining measurement gaps are recorded there.

Measured on 2026-09-11 through the Sipeed Tang Primer 25K FPGA release bridge, USB UART channel B /dev/cu.usbserial-20250303171. Physical chain reported by user as 12→4; responding addresses verified as 4–12. Firmware registers report 3.15 for all nine.

Subsequent authorized motion tests are documented separately in [motion-report.md](motion-report.md), with per-run supply conditions, target trajectories, raw transactions, and final configuration readbacks. The stationary results below retain their original scope.

The user then selected PWM control. Initial bounded pulses are in [pwm-report.md](pwm-report.md). Later commissioning verified the FPGA command, telemetry, and host-traffic watchdogs in [safety-watchdog-reply-window/run.json](safety-watchdog-reply-window/run.json); the deployed source/image is in [safety-commissioned-deployment](safety-commissioned-deployment). All nine completed the individual short-pulse full-duty sweep. See [current-bench-state.json](current-bench-state.json) for the last recorded state and its verification provenance; the earlier position-mode state below is historical.

At the user-confirmed 12.6 V / 3 A bench setting, abrupt all-nine pulses completed through 70% PWM but lost a reply at 80%. [The review](bench-3a-review.json) records servo-reported voltage down to 9.4 V. The user's [bench-display observation](bench-3a-user-display-observation.json) was CV, 12.6 V and up to 0.9 A, so bench current limiting is not established. Connector voltage and transient current remain unmeasured. Longer unrestricted-shaft measurements use [the rotation runtime](rotation-runtime/manifest.json) and retain raw and unwrapped encoder readings.

[Subsequent direction tests](pwm-direction-report.md) verified that zero-based bit 10 reverses PWM on all nine units. Bits 11 and 15 were rejected on ID 12. The [full characterization plan](characterization-plan.md) tracks the remaining measurements; commissioning is not completion of the sim-to-real characterization goal.

## Stationary measurements

4922 valid 15-byte telemetry replies in 30.003 s, 0 failed transactions. Each unit received about 18.2 polls/s, sequentially over the shared bus. All report position mode, zero status flags and zero speed. Reported encoder counts stayed constant on every unit during this window. Reported initial target tracking error spans −3 to +4 counts (−0.264° to +0.352°); this is not a calibrated accuracy measurement.

| ID | FW | Voltage V | Temp °C | Current register median | Position − target ° | Reads |
|---|---|---|---|---|---|---|
| 4 | 3.15 | 10.4–10.4 | 49–49 | 0 | 0.088 | 547 |
| 5 | 3.15 | 10.5–10.5 | 47–47 | 0 | 0.176 | 547 |
| 6 | 3.15 | 10.3–10.3 | 47–48 | 14 | 0.352 | 547 |
| 7 | 3.15 | 10.4–10.5 | 47–47 | 0 | -0.088 | 547 |
| 8 | 3.15 | 10.5–10.5 | 47–47 | 40 | -0.264 | 547 |
| 9 | 3.15 | 10.5–10.5 | 47–48 | 33 | 0.000 | 547 |
| 10 | 3.15 | 10.4–10.4 | 48–48 | 0 | 0.176 | 547 |
| 11 | 3.15 | 10.6–10.6 | 48–48 | 38 | 0.352 | 547 |
| 12 | 3.15 | 10.4–10.5 | 46–46 | 0 | 0.176 | 546 |

Current register counts use a provisional 1 mA/count conversion in the CSV. These are internal readings, not total supply currents. Temperatures are reported sensor values after earlier activity, not known ambient conditions. Voltage differences may include each servo's ADC calibration error; they do not isolate harness voltage drop.

## Voltage correction

The earlier code read two bytes at 0x3E and called the result millivolts. Byte 0x3F is actually temperature. For example [104,49] means 10.4 V and 49°C, not 12.648 V. Hiwonder's [own decoding example](https://wiki.hiwonder.com/projects/NexArm/en/esp32-version/docs/2_ESP32_Development_Basics.html) explicitly scales register 62 by 0.1 and reads register 63 as temperature. The Rust capture decoder now handles both historical two-byte voltage reads and correct one-byte reads. Existing raw logs are unchanged. Initial-registers.json retains its original misleading voltage_mV key solely as raw acquisition provenance; use the corrected summary.

## Acquisition and limitations

During the initial stationary phase, the FPGA release bridge was retained. Later safety deployment and hardware commissioning are linked above. Prior bench notes identify TX 220 ohms / RX 1 kohm and RELEASE_TX=1. A saved build hash alone identifies a candidate on disk, not independent proof of the running SRAM image.

A separate 15-second compatibility-driver run yielded 2358 clean replies (262 per unit). The first Rust attempt returned no bytes because configuring the macOS FTDI port before opening the persistent descriptor allowed its settings to reset. The recorder was corrected to configure the port while open; the subsequent 30-second run above succeeded. The failed attempt is retained under stationary-30s and excluded from servo reliability statistics; its original completed flag means the process finished, not successful acquisition. Current code distinguishes duration completion from a failure-threshold exit.

Host monotonic request/reply windows include USB buffering and FPGA forwarding. The currently loaded bridge provides no device-clock timestamps, so sub-millisecond actuator latency and internal sensor update rates are not identified. Contiguous register reads are not proof of simultaneous internal sampling.

- Host timestamps include USB and FPGA store-and-forward delay; no FPGA timestamps in the currently loaded bridge.
- Unchanged reported position is not proof of zero mechanical motion or absolute encoder accuracy.
- Current 1 mA/count interpretation is uncalibrated; raw current is retained. Zero internal current does not imply zero servo supply consumption.
- Targets are initial register readbacks, not independently measured angle references.
- This stationary acquisition did not identify speed, acceleration, torque, backlash, inertia, thermal resistance or dynamic controller gains. See the separate motion report for subsequent speed and step-response measurements.

## Next measurements

The user subsequently confirmed secured housings and clearance and authorized progressive individual and all-nine concurrent motion. Bounded bidirectional position steps are recorded in the motion report. Torque/friction and thermal identification additionally need a known external load/lever arm or torque measurement. No servo configuration was written or motion commanded during the stationary acquisition itself.

## Reproduce

Run the shared Rust example from the physics-simulator repository with an explicit port and new output directory:

```sh
cargo run --locked --release -p sim-runtime --example characterize_hx_bridge -- /dev/cu.usbserial-20250303171 30 4,5,6,7,8,9,10,11,12 NEW_OUTPUT
node examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/summarize.mjs
```
