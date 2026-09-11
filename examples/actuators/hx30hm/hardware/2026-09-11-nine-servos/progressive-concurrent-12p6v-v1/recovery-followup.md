# Stop and subsequent read-only verification

The all-nine 1 A / 12.6 V position trial passed the first three stages, then stopped during the first ±60° movement when ID 6 reported 8.9 V. Recovery acknowledged hold-position commands for all nine. Those acknowledgments establish command receipt, not instantaneous mechanical stopping.

The immediately sampled final states still include nonzero speed registers on seven servos and a lowest voltage reading of 5.4 V on ID 4. These are retained unchanged in run.json. A separate later [three-second read-only check](../post-concurrent-stop-check/run.json) obtained 489 clean replies: **every speed reading was zero on all nine** and final voltages were 11.9–12.2 V. There was no other motion process owning the serial port.

The supply current limit was last reported as 1 A. The voltage collapse is consistent with inadequate transient current headroom and/or power-distribution drop; neither the supply CV/CC indicator nor actual supply current was captured by software. Do not claim its cause was independently isolated. The user has been asked to increase the current limit to 2 A before further all-nine concurrent tests. Small single-servo PWM pulses are a separate experiment.
