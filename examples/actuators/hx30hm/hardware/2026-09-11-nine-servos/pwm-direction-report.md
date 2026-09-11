# PWM direction verification

**All nine servos physically reversed with direction bit 10 (mask 0x0400).** A +100 pulse followed by a −100 pulse, each 50 ms with a zero-drive rest between them, produced opposite encoder displacement on every servo.

| ID | Positive pulse, counts | Negative pulse, counts |
|---|---:|---:|
| 4 | 11 | -10 |
| 5 | 14 | -11 |
| 6 | 11 | -7 |
| 7 | 12 | -9 |
| 8 | 11 | -10 |
| 9 | 10 | -7 |
| 10 | 10 | -8 |
| 11 | 12 | -11 |
| 12 | 13 | -11 |

Bits 11 and 15 were separately rejected on ID 12: those candidate commands still moved forward. The Hiwonder-maintained generic Feetech software comment was not a verified HX-30HM wire specification. The corrected shared Rust codec exposes the measured bit explicitly.

All nine finished at zero PWM, torque disabled, mode 2, and NVS locked, with zero reported speed and status flags. Only the intended mode was previously changed; these trials skipped mode writes when mode 2 was already active.

Pulse displacement includes coast and host-timing variation. Differences between positive and negative displacement are not sufficient to identify friction or backlash independently.

Next requirements remain FPGA-side timeout, increasing PWM magnitude/duration, repeatability, concurrent tests with sufficient current headroom, known loads, and held-out simulation validation. The full goal remains incomplete.
