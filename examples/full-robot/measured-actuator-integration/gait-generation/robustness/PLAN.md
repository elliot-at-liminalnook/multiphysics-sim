# Gaits under nonideal actuator conditions

The user requested that supply, timing, backlash, temperature and motor variation
be represented in gait evaluation. Preserve the existing full-authority result
as an optimistic comparison, not the sole learning environment.

Use shared Rust components and separately saved CAD profile scenarios. Every
assumption carries units and provenance. The initial values below are sensitivity
probes, not measured bounds or a probability distribution. No bench fit is promoted.

1. **Command-delivery delay:** compare 0, 4 and 10 ms while retaining 100 Hz
   feedback. Four milliseconds is a probe near the observed three-axis transaction
   duration; ten milliseconds is one full control interval. Neither establishes
   encoder sample age. Internal stale-feedback behavior needs separate treatment.
2. **Internal gearbox backlash:** compare 0, 0.25, 0.5 and 1 degree at the motor
   output. These are unmeasured probes. Preserve external belt/worm transmission
   overrides separately; do not count internal play twice. First run the 0.5-degree
   case and check event behavior and timestep sensitivity before longer searches.
3. **Shared supply:** author a provisional CAD battery, common feed and four
   three-motor power branches. Actual pack choice is pending user input. Test
   multiple internal/wiring resistances and states of charge; do not infer them
   uniquely from servo voltage telemetry without calibrated current. Check terminal
   voltage/current/power and operating-envelope failures, not just robot speed.
4. **Motor variation:** use explicitly synthetic family variants and recorded seeds
   until physical units are assigned. Preserve correlated electrical parameters.
   Do not invent physical motor IDs to bypass per-unit provenance validation.
5. **Temperature:** first compare fixed-temperature scenarios as sensitivity tests.
   Then integrate the shared thermal components into the same Rust actuator state
   and CAD schema so loss power changes temperature and future motor behavior.
   Heating/cooling rates remain estimates until measured; constant hot temperature
   is not a substitute for the dynamic thermal model.

Run one change at a time to diagnose effects, then combine them. Compare the same
gait, seed, initial state, terrain and horizon. Score longer episodes: the prior
two-second winner regressed badly over ten seconds. Keep fall/numerical failures,
net progress, foot clearance, tracking, saturation and electrical results.

After basic correctness and numerical checks, evaluate candidate gaits across the
same declared scenario suite, including unfavorable combinations. Report both
nominal performance and degradation. Broader robustness across assumed scenarios
is useful, but does not establish a real-world accuracy bound.
