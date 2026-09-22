# Guarded numerical matrix lifetime

ImplicitStepConfig.step_jacobian_max_uses is an optional positive limit on Newton
iteration visits to a correction matrix, including convergence checks. Omitted
values retain 64. The limit is checked between continuous stages/steps; an active
solve may exceed it before the next boundary. Setting a limit requires cross-step
Jacobian reuse. Changing the effective limit invalidates the proposal workspace.

Only numerical correction matrices are reused. Physical endpoints are recomputed;
original residual/correction bounds, stale-matrix refresh and backtracking remain
unchanged. Contact, mode, layout, timestep and time-continuity invalidation remain
active. Workspace changes commit only after successful physical acceptance.

56 focused tests pass across mechanical/motor integration, power boundaries,
sampled servo clocks and sensors. Two new tests check the independent analytic
backward-Euler spring-mass solution for five lifetime settings, exact omitted/
explicit-default behavior, expected cold/reused policy, invalid-configuration
rollback, and configuration-change invalidation. An initial test compilation
error used unwrap_err on a non-Debug success type; the error extractor was fixed
without changing assertions. The original compiler log is retained.

Native release build and full sim-web wasm32 checks pass. The selected physical
recipe remains unchanged. Longer limits are experiments, not selected profiles.
run-native.mjs preserves and compares the original binary, the unchanged default,
and limits 256, 1024 and 4096. analyze.mjs checks all saved default/profile physical
fields, task transitions, retained counters, scheduler accounting, input/binary
hashes, and the unchanged physical/performance qualification gates. Timed runs
are ordered single trials; they are not repeated-machine estimates.

## Completed screen

After the first long-lifetime screen, profiler evidence motivated a second
screen at 16, 32 and 128 visits. The commands are preserved in run-shorter.mjs;
all profiles and qualification receipts are included in analysis.json.

| Lifetime | Native wall seconds | Outer Newton iterations | Endpoint evaluations | Full closure mappings |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 26.801611 | 78154 | 244275 | 116613 |
| 32 | 24.186997 | 84143 | 203142 | 122608 |
| 64 | 22.389637 | 91123 | 179489 | 129594 |
| 128 | 22.122599 | 97635 | 170015 | 136102 |
| 256 | 22.275917 | 102890 | 168374 | 141353 |
| 1024 | 23.983131 | 107097 | 169739 | 145564 |
| 4096 | 22.780078 | 107097 | 169739 | 145564 |

The best alternative (128) is only 1.01207x the same-build default. No candidate
passes the unchanged 1.2x promotion or realtime gates. The default 64 remains
selected. Longer lifetime saves linearized derivative probes but increases
Newton iterations and full nonlinear closure mappings. Shorter lifetime reduces
iterations but creates too many probe endpoints. There is no large gain here.

The 1024/4096 routes produce exactly equal physical frames, complete task
transitions, motor-solver counters and interval diagnostics, despite wall times
23.983/22.780 s. This demonstrates appreciable timing variability; tiny changes
must not be described as robust speedups. All timings are single ordered trials.

All alternatives stay within existing physical gates, preserve contact identities
and exact saved FPGA states/commands, and have zero rejected trials. Worst
pairwise differences are 1.122e-8 rad, 4.036e-9 m, 1.174e-7 A and 4.340e-5 N.
These are same-timestep comparisons, not timestep convergence or hardware error.
Every route's profiled/unprofiled physical frames and task transitions match
exactly. The omitted-default source change also preserves every saved physical
frame, task transition, motor statistic and interval diagnostic exactly.

No new browser artifact was built for these unqualified policies. The previous
force-output bundle remains at port 4192 and predates the optional lifetime field;
its 0.11262x result still describes that preserved artifact. No benchmark/build
is pending. Known controlled-robot timestep sensitivity and calibration gaps
remain. The goal remains active; maximum attainable speed is unproven.
