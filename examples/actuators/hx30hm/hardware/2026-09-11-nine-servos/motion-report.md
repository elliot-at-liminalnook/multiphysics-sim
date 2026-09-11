# HX-30HM measured motion through the FPGA bridge

Nine servos, IDs 4–12. Position mode, free output shafts, secured housings as reported by the user. These are warm-bench motion measurements, not calibrated torque, absolute accuracy, or loaded endurance tests.

The WANPTEK DPS3010U settings are user reports. The original approximately 0.45 A observation was consumption: the user corrected the indicator report from CC to CV. It does not establish current limiting. The interrupted 1 A run includes an unsynchronized change to 13 V and is excluded from constant-voltage comparisons. Subsequent 12.6 V runs have their own preflight and supply records.

## Run outcomes

| Run | Motion | Completed | Samples | Servo voltage range V | Maximum reported °C | Fault samples |
|---|---|---|---:|---|---:|---:|
| [progressive-individual-v1](progressive-individual-v1/run.json) | One at a time | Yes | 48368 | 9.8–10.7 | 49 | 0 |
| [progressive-individual-1a-v1](progressive-individual-1a-v1/run.json) | One at a time | No | 10342 | 10.3–12.5 | 51 | 0 |
| [progressive-individual-12p6v-v1](progressive-individual-12p6v-v1/run.json) | One at a time | Yes | 48312 | 11.2–12.2 | 54 | 0 |
| [progressive-concurrent-12p6v-v1](progressive-concurrent-12p6v-v1/run.json) | All nine together | No | 2154 | 8.9–12.2 | 55 | 0 |

## Long-travel speed by servo

Each cell is positive / negative speed in degrees per second during the ±90° stage (180° between opposite targets), using the median across eligible movements. A dash means fewer than five samples in the fitted interval. These are measured mid-travel speeds, not proof of an absolute actuator ceiling.

| ID | progressive-individual-v1 | progressive-individual-1a-v1 | progressive-individual-12p6v-v1 | progressive-concurrent-12p6v-v1 |
|---|---|---|---|---|
| 4 | 210.5 / 212.8 | — | 242.3 / 248.7 | — |
| 5 | 232.7 / 231.1 | — | 265.0 / 265.2 | — |
| 6 | 209.4 / 209.8 | — | 246.4 / 246.1 | — |
| 7 | 210.5 / 204.3 | — | 239.1 / 241.6 | — |
| 8 | 214.9 / 215.4 | — | 249.7 / 249.9 | — |
| 9 | 211.7 / 212.8 | — | 245.9 / 247.1 | — |
| 10 | 213.3 / 206.8 | — | 247.0 / 241.0 | — |
| 11 | 214.4 / 214.0 | — | 261.8 / 257.1 | — |
| 12 | 212.7 / 225.9 | — | 245.6 / 262.7 | — |

## progressive-individual-v1

Supply record: [supply-observation.json](progressive-individual-v1/supply-observation.json).

Result: all planned stages completed.

Packet audit: 49286 transactions, 0 invalid requests, 0 invalid/error replies, 0 invalid timestamp windows; 0 broadcasts correctly expect no acknowledgment.

Final readback: 9/9 stationary; 9/9 original position/time/speed RAM fields restored; 9/9 configuration blocks unchanged.

## progressive-individual-1a-v1

Supply record: [supply-observation.json](progressive-individual-1a-v1/supply-observation.json). Interrupted run with unsynchronized voltage change; do not treat as a constant-voltage 1 A comparison

Result: operator STOP requested.

Packet audit: 10652 transactions, 0 invalid requests, 0 invalid/error replies, 0 invalid timestamp windows; 0 broadcasts correctly expect no acknowledgment.

Recovery: ID 4 hold verified.

Final readback: 9/9 stationary; 8/9 original position/time/speed RAM fields restored; 9/9 configuration blocks unchanged.

## progressive-individual-12p6v-v1

Supply record: [supply-observation.json](progressive-individual-12p6v-v1/supply-observation.json).

Result: all planned stages completed.

Packet audit: 49230 transactions, 0 invalid requests, 0 invalid/error replies, 0 invalid timestamp windows; 0 broadcasts correctly expect no acknowledgment.

Final readback: 9/9 stationary; 9/9 original position/time/speed RAM fields restored; 9/9 configuration blocks unchanged.

## progressive-concurrent-12p6v-v1

Supply record: [supply-observation.json](progressive-concurrent-12p6v-v1/supply-observation.json).

Result: feedback limit: position=1672 home=1629 temperature=52 voltage=8.9 status=0.

Packet audit: 2551 transactions, 0 invalid requests, 0 invalid/error replies, 0 invalid timestamp windows; 10 broadcasts correctly expect no acknowledgment.

Recovery: ID 4 hold verified; ID 5 hold verified; ID 6 hold verified; ID 7 hold verified; ID 8 hold verified; ID 9 hold verified; ID 10 hold verified; ID 11 hold verified; ID 12 hold verified.

Final readback: 2/9 stationary; 0/9 original position/time/speed RAM fields restored; 9/9 configuration blocks unchanged.

## Measurement method and limits

- Position-to-time linear regression over 20–80% of commanded travel, using host transaction midpoints. Per-direction summaries require at least five samples. Original raw packet bytes and request/reply times are retained.
- Commands increase from ±5° at 26.4°/s to ±90° with a 527.3°/s speed command. The command value is a requested limit, not measured shaft speed. Settling gates use ±12 encoder counts (1.055°).
- Individual motion polls the active servo. Concurrent motion uses one 35-byte position SYNC WRITE for all nine, followed by sequential telemetry reads. Simultaneous command transmission does not prove simultaneous sensor sampling.
- Host timestamps include USB and FPGA buffering. Internal sensor update age is unknown. Sub-millisecond latency, peak acceleration, and peak transient supply current are not identified.
- Voltage and temperature are uncalibrated onboard readings. Current register values retain their raw units; provisional 1 mA/count values are not independent supply-current measurements.
- Temperature cutoff 60°C, reported voltage range 9–12.6 V, status faults, travel margins, and settling checks gate escalation. STOP requests hold the active servos at their latest measured positions. No nonvolatile configuration or torque-limit changes are made.
- A known load or torque instrument is still needed for torque-speed curves, friction, backlash under load, output inertia, and thermal model identification. These data support a provisional no-load response model with the voltage and temperature conditions preserved.

## Reproduce the analysis

```sh
node examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/analyze-motion.mjs progressive-individual-v1 progressive-individual-1a-v1 progressive-individual-12p6v-v1 progressive-concurrent-12p6v-v1
node examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/report-motion.mjs progressive-individual-v1 progressive-individual-1a-v1 progressive-individual-12p6v-v1 progressive-concurrent-12p6v-v1
```
