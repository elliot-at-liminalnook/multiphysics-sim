# Supervised HX-30HM bench bridge

Software-validated profile for Tang Primer 25K, 50 MHz, USB UART 115200,
servo bus 1 Mbaud, IDs 4–12. The default debug bridge remains a separate,
unsupervised profile. `make bridge-safety` builds; `make sim-safety` tests.
Neither command programs the board. This profile has **not been flashed or
verified on connected servos**; the user unplugged them for offline validation.

The safety profile starts latched, releases TX when listening, and broadcasts
PWM zero followed by torque disable. It repeats that pair every 50 ms while
latched. Dock S2/key2 is a synchronized, debounced (1 ms) active-high stop.
LED DONE stays on while latched.

For each explicitly armed servo the FPGA checks:

| Condition | Trip policy |
|---|---|
| Temperature | >=60 °C |
| Voltage | <9.0 V or >12.6 V |
| Internal current register | >=2000 raw counts; scale uncalibrated |
| Reply/device status | nonzero |
| Valid requested telemetry missing | 200 ms |
| Explicit command heartbeat missing | 300 ms per ID |
| S2, host STOP, FIFO overflow, host packet collision | latch immediately after detection |

Telemetry is the checksum-valid 15-byte read at address 0x38. Configuration
reads and acknowledgements do not refresh telemetry. Polls and actuator writes
do not renew command leases. Only ARM/HEARTBEAT does. A fault clears every arm
bit; healthy feedback alone never rearms. S2 held down prevents rearming.

A trip allows an already-transmitting packet to finish, then sends the stop
pair once the wire is quiet for 20 bit times. With a functioning, otherwise idle
bus the pair takes about 0.2 ms; a maximum-sized outgoing packet adds <0.65 ms.
These are transport bounds, **not measured mechanical stopping times**.
A stuck-low/disconnected bus can prevent delivery. This design cannot cut supply
power, cannot detect external supply current, and cannot protect electronics
from an excessive rail voltage merely by disabling torque. Hardware power
isolation and external sensing require additional wiring/components.

## Host protocol, version 1

Normal FF FF packets, ID FE, instruction A0, parameters:

- `[0]`: global STOP.
- `[1, id]`: ARM, requires fresh healthy telemetry for that ID.
- `[2]`: STATUS.
- `[3, id]`: HEARTBEAT for an already armed ID.
- `[4, id]`: DISARM and global STOP.

These packets never reach a servo. Replies use FF FF FE 0F 00,
13 status bytes, then the normal inverted-sum checksum:

`version, latched, reason, fault_id, armed_mask_le16, fresh_mask_le16,
temperature_max, voltage_min_raw, voltage_max_raw, current_max_raw_le16`.

Mask bit zero is ID 4. Reasons: 0 healthy, 1 boot, 2 heat, 3 undervoltage,
4 overvoltage, 5 current, 6 device error, 7 telemetry timeout, 8 command timeout,
9 S2, 10 host stop/disarm, 11 bridge fault. Thresholds describe this bench
profile, not calibrated device ratings.

PING/READ, zero PWM, torque-off, and NVS relock are allowed while unarmed.
Other addressed writes require that ID armed and the global latch clear.
SYNC WRITE supports two-byte position/PWM tuples and one-byte torque tuples;
every nonzero tuple must target an armed servo. Other instruction types,
malformed lengths/checksums, and unsafe broadcast writes are blocked.
Addressed torque-off disarms its ID. Broadcast torque-off disarms all IDs.

A forwarded SYNC WRITE returns the same local status packet **after** its final
byte leaves the wire. This receipt proves bridge transmission, not execution
or acknowledgement by a servo. The Rust sweep waits for it before continuing.
Send one host transaction at a time; do not pipeline packets.

## Validation and reconnection

`hx_safety_tb.v` injects sensor faults, checksum errors, absent/unsolicited
telemetry, independent lease expiry, S2, bridge/host stops, boundary values,
rearming and mixed-ID command gating. `bridge_safety_tb.v` exercises real UART
bits, tri-state ownership, local replies, forwarded telemetry, SYNC receipts,
actual stop broadcasts and autonomous stale-feedback stopping. The original
release-bridge test is retained as a regression.

Before full-drive hardware operation, load this SRAM image with servo power
off, query its profile, then verify a small-drive S2 stop, host-disconnect stop,
telemetry-loss stop, and explicit rearm on hardware. Observe encoder motion
until stationary; an ACK alone is insufficient. Do not test real overheating
or overvoltage to validate a software fault path.

The shared Rust acquisition protocol and sweep policy are in physics-simulator;
FPGA RTL remains in this hardware repository. A frozen source/build snapshot
and offline validation report live under
`examples/actuators/hx30hm/software-validation/` in physics-simulator.
