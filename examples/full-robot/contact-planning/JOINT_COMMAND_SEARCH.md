# Joint command and trajectory speed search

The fine steering winner travels 164.0674820674 m in 300 seconds, or
0.5468916069 m/s, without a sampled fall. Its yaw command was calibrated after
selecting the trajectory. The next search includes that command alongside the
existing speed, tracking gain, belt/worm/foot amplitudes and derivative leads,
so the optimizer can discover their interactions in one nine-dimensional space.

`run_affine_speed_search.mjs` now accepts optional `extra_command_parameters`.
Each binding names an existing controller input and a search parameter. Its
quantity kind and unit must match the exported runtime channel schema. Duplicate
bindings and the command heartbeat are rejected. `materialize` applies the
candidate value throughout the existing action schedule while retaining every
other input. Old specifications follow the same eight-parameter path.

Two focused Node tests pass and are added to the Bayesian CI workflow. Checks
using the actual shared Rust affine transformer show exact agreement with the
previous legacy materialization, an identity new baseline, and changes confined
to requested yaw for all 15,000 actions. The first verification incorrectly
compared legacy output with its unmaterialized template, whose speed/gain input
bounds were wider; comparison with the actual previous materialization passes.
That failed diagnostic is retained. The new baseline's first 101 physical frames
and task transitions exactly match the fine winner, excluding wall-clock timing.

The new yaw search domain is ±1 rad/s, expanded from the template's ±0.25 rad/s
policy input range. Other numerical domains remain speed ±0.8 m/s, tracking gain
±2, and amplitude/lead factors -1 to 3. These are initial optimizer domains, not
physical bounds or acceptance restrictions. Final CAD/runtime actuator limits
remain unchanged. The inherited 0.05 rad steering-offset policy setting is also
unchanged; the amplitude variables can still explore the broader joint range.
Evidence at a numerical boundary should motivate further domain exploration.

## Seed data and active selection

`runs/joint-command-search-inputs-v1` contains a fresh fine-resolution context.
Four existing captures differ only in yaw request; their scene, configuration,
task, seed and every non-yaw input match exactly. Their previously fitted
requests are checked against the first 20 seconds of actual frames. No future
300-second endpoint or coarse-step result is used as a training score.

| Requested yaw | Predicted 300 s net speed from first 20 s |
| ---: | ---: |
| 0.0552386367 rad/s | 0.4439793653 m/s |
| 0.0752386367 rad/s | 0.5171081766 m/s |
| 0.0952386367 rad/s | 0.5440455383 m/s |
| 0.0963787796 rad/s | 0.5443029374 m/s |

The shared Rust GP selector requires at least dimension+1 completed observations.
`run_local_global_planar_speed.mjs` therefore supports a seeded local design
phase before that threshold. Rust generates each design point; JavaScript only
prepares its declared numerical box and executes the existing runtime. The
normalized identification radius is 0.01 around the best observed prediction.
Once sufficient complete rows exist, the shared adaptive local/global selector
takes over, with periodic proposals over the full parameter domain. Failed
prefixes remain failed observations, never fabricated speeds.

The live batch is `runs/joint-command-planar-v1`, initially twelve further
proposals. This count is a checkpoint, not a limit on the speed goal. The fit
windows remain 5–20 and 10–20 seconds; the larger predicted full-distance speed
is an optimistic proposal score, not a calibrated bound. Every promising gait
still requires a complete physical run and relevant fidelity checks.

The first four new prefixes complete without sampled falls. Candidate 002
reaches 0.5412294105 m/s over its actual 20 seconds and predicts
0.5463296714 / 0.5490613651 m/s over 300 seconds from the two fit windows.
The next identification box follows that new best observation in all nine
dimensions; the recorded selection-loop check verifies the three preceding
outcomes entered the history. This is still the initial design stage, before
GP acquisition. The other three forecasts are lower than the seed incumbent.
`runs/joint-command-sustained-v1/candidate002` now tests the promising candidate
for the full 300 seconds with its unchanged controller and authored action
schedule. Extending the recipe and restoring its prefix reproduces the exact
original input JSON; physical prefix parity remains to be checked when it ends.

## Related research

[Bayesian Optimization of Composite Functions](https://proceedings.mlr.press/v97/astudillo19a.html)
models the expensive vector-valued intermediate response and evaluates expected
improvement through a cheap outer function. A relevant extension here would
model translational motion and turning separately, then propagate uncertainty
through the existing displacement calculation. That is an inference about our
problem, not an implemented result: the current selector still fits scalar
predicted scores. A composite acquisition would need its own tests and measured
comparison before any sample-efficiency benefit is claimed.

## Concurrent physical validation

`runs/steering-fine-contact300-v1/sustained300` enables inter-link contact for the
fine winner while preserving its entire controller, input schedule, timestep,
CAD and world. Its source scene differs in exactly one explicit omission flag.
This checks the current candidate rather than assuming an older teacher's
contact result transfers. The fixed-controller third-resolution full run also
continues.

The slower heading-feedback controller has now completed 300 seconds at the
0.3125 ms timestep: 164.0530593690 m, or 0.5468435312 m/s, without a sampled
fall. The verification checks all 960,000 steps, 15,000 authored input events,
the exact scene and all 15,001 sampled transitions. It essentially matches
the constant-steering winner's speed; it does not establish a speed gain.
The first evidence-check script expected the runtime recording inside another
wrapper and failed before checking scene/input parity. The corrected check uses
the actual direct recording and retains that failed diagnostic.

The heading-feedback third-resolution prefix has completed: 0.5366618554 m/s
over 20 seconds without a sampled fall, with fitted mean turning 0.0021800683
rad/s over 10–20 seconds. It does not qualify a full-duration speed.
`joint-command-search-evidence-v1.json` preserves the code/checks, seed data,
closed physical evidence and immutable inputs for live work. No new neural
learning gain, complete numerical convergence, hardware accuracy or physical
speed maximum is asserted.
