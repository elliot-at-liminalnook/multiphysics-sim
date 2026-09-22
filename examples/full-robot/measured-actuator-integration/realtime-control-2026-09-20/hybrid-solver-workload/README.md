# Retained hybrid motor workload counters

Read-only EmbeddedSession and EmbeddedEnvironment getters now expose the motor
statistics already retained by diagnostic mode. The standalone profiler writes
them after its timed run. There is no new work or state in stepping, and ordinary
runs do not retain this history. Native release and full sim-web wasm32 checks
pass. All 151 saved physical frames and all task transitions are exactly equal
to the preceding executable. Run `node analyze.mjs` to reproduce the receipt.

The three-second selected BE6400 replay contains 19,200 outer intervals and
19,200 successful continuous trials, with zero rejected trials. It processes
28,812 guard events (counts are per guard, not unique event times). Existing
accepted_implicit_steps is empty for this separate hybrid path; the new motor
statistics cover all outer intervals.

- 17,311 stages start with a reused outer Jacobian (90.16%).
- 91,123 outer Newton iterations: 4.746 per trial.
- 179,489 endpoint evaluations and auxiliary solves: 9.348 per trial.
- 835,596 component evaluations: 43.521 per trial.
- 414,653 auxiliary Newton iterations: 21.597 per trial.
- 160,289 dynamics preparations and 19,200 dynamics cache hits.
- 160,308 mechanical preparations and 21,954 mechanical cache hits.
- No stage fresh restarts and no supplied auxiliary Jacobians.

Maximum reported scaled velocity residual is 1.018e-6 and maximum auxiliary
residual is 6.188e-10; these are diagnostic maxima, not timestep or hardware
error estimates. Profiled wall time is 24.485446 s and is not an unprofiled
performance-acceptance measurement. Counts include successful discarded
location trials in general; failed-solve internal work is unavailable. This
replay has no rejected trials. Bucket timers can nest and are not additive.

The high exact endpoint/dynamics workload remains a target. Outer matrix reuse
already covers most stages, so changing its lifetime alone cannot be assumed to
remove the dominant repeated work. No solver or physical recipe was changed to
obtain these counters. Original gates and known timestep-convergence limitations
remain in force. Source snapshots and binary/input hashes are retained.
