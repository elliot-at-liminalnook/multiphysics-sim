# HX-30HM PWM commissioning

PWM control was explicitly authorized by the user after position-mode measurements. Each servo was tested separately at 25, 50, 100, 150, and 200 drive counts out of 1000, for 150 ms per pulse with 800 ms zero-drive intervals. Only positive-direction encoding was exercised.

| ID | Run complete | Total displacement ° | Peak reported speed °/s | Minimum reported V | Maximum reported °C | Final mode / PWM / torque enable |
|---|---|---:|---:|---:|---:|---|
| 12 | Yes | 17.84 | 52.7 | 12.0 | 52 | [2] / [0,0] / [0] |
| 4 | Yes | 17.14 | 57.1 | 11.9 | 55 | [2] / [0,0] / [0] |
| 5 | Yes | 18.11 | 52.7 | 12.0 | 52 | [2] / [0,0] / [0] |
| 6 | Yes | 15.56 | 52.7 | 11.9 | 53 | [2] / [0,0] / [0] |
| 7 | Yes | 17.67 | 52.7 | 12.0 | 53 | [2] / [0,0] / [0] |
| 8 | Yes | 14.77 | 48.3 | 12.2 | 53 | [2] / [0,0] / [0] |
| 9 | Yes | 15.82 | 48.3 | 12.1 | 53 | [2] / [0,0] / [0] |
| 10 | Yes | 16.61 | 52.7 | 12.1 | 55 | [2] / [0,0] / [0] |
| 11 | Yes | 17.84 | 57.1 | 12.3 | 54 | [2] / [0,0] / [0] |

Mode 2 is PWM. A final PWM readback of [0,0] and torque-enable [0] means output drive is disabled. Mode writes use one byte at 0x21 and the NVS bank is relocked; the selected mode remains configured after the test. Exact preflight and final register blocks are retained.

The Rust loop sent serial duty commands through the existing FPGA release bridge and monitored encoder travel, voltage, temperature, and fault status. The drive command is persistent until replaced; software deadlines and cleanup are not an independent hardware watchdog. Before fast closed-loop or unattended PWM operation, an FPGA watchdog and deterministic feedback scheduling remain to be implemented and validated.

These short pulses establish mode switching, motion response, and zero-drive stopping. They do not establish maximum PWM-mode speed, calibrated torque/current control, reverse-drive encoding, or all-nine simultaneous PWM performance. The prior all-nine position test stopped on a voltage collapse at the last confirmed 1 A supply limit; a 2 A setting has been requested for further concurrent tests.

Raw measurements:

- [pwm-pilot-id12-v1](pwm-pilot-id12-v1/run.json), [analysis](pwm-pilot-id12-v1/analysis.json), [supply conditions](pwm-pilot-id12-v1/supply-observation.json)
- [pwm-individual-ids4-11-v1](pwm-individual-ids4-11-v1/run.json), [analysis](pwm-individual-ids4-11-v1/analysis.json), [supply conditions](pwm-individual-ids4-11-v1/supply-observation.json)
