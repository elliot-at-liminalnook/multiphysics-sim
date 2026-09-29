# Motor control loop: FPGA frame rate versus today's host loop (2026-09-26)

Question: how much of the real leg's tracking lag would an FPGA joint loop
remove, and how fast must it run? Detailed model, 12.5 V study (`study.json`),
three gaits, each under an explicit `controller_override` (gait lab only;
the accepted actuator registry is unchanged). Reports are in `results/`
(current sim) and `results-ctl/`.

The simulated motors already run the FPGA's integer PD + velocity-feed-forward
law (`sim-domain-control::fixed_pd`, gains 4096/4096/4096, 10 ms period, 2 ms
latency), so the gait searches have assumed an FPGA-rate loop all along.

| Controller (period / latency, Q8 gains) | 5027-CmaEs-144 | 5014-CmaEs-088 | 3401-Bayesian-013 (run on the leg) |
|---|---|---|---|
| Current sim: FPGA 10 ms / 2 ms, 4096/4096/4096 | 0.180 m/s, worst RMS 1.5° | 0.167 m/s | 0.094 m/s, worst RMS 1.2° |
| FPGA 10 ms / 5 ms, same gains | 0.181 m/s, 1.6° | 0.170 m/s | 0.092 m/s, 1.2° |
| FPGA 20 ms / 5 ms, same gains | 0.123 m/s, 4.1° | 0.128 m/s | 0.112 m/s, 4.0° |
| Host loop 70 ms / 35 ms, kp 291, kd 275, kv 1068 | rejected at 0.5 s (17° RMS) | rejected at 0.5 s | rejected at 0.5 s (11° RMS) |

Host-loop gains are the tuned host PID mapped to the FPGA law's units
(kp 0.74 duty/rad → 291; kd 0.049 duty·s/rad at 70 ms → 275; feed-forward
3421 counts/s per duty at 70 ms → 1068); the host's integral term has no
counterpart.

Validation against the real leg (gait 013, Sep 23, suspended, host loop):
knee 7.5°/17.9°, worm 5.4°/17.2°, hip 3.2°/7.9° (RMS/peak). The simulated host
loop gives +X hip 8.6°/17.3°, worm 11.0°/21.1°, foot 3.7°/5.5° over its first
0.5 s. Same order of magnitude, different joint ranking. The model is not a
precise match: the real leg was suspended while the sim walks on the floor, the
host runs a PID with integral action, and the sim window is only the start-up.

Conclusions:
- A 10 ms FPGA loop reduces tracking error by roughly 5–10× (to about 1–1.5° RMS)
  and makes the fastest gaits found so far trackable; latency up to 5 ms is
  harmless.
- 20 ms is marginal: about 4° RMS and roughly 30% slower on the fast gaits.
- Target for the hardware image: at most 10 ms frames. Re-validate against a
  measured leg run with the new loop before trusting absolute numbers.
