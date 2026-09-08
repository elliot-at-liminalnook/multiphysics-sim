# Horizontal swing and asymmetric stance development

The 3.75 mm/s command now completes 13 qualified swings in the 24-second
forward/stop case at both 20 ms and 5 ms physics steps. No controller in these
30 trials passes the complete acceptance gate. This is a short-run geometry
milestone, not a validated sustained-speed or browser operating envelope.

The shared planner optionally distributes horizontal foot travel across raise
and lower while retaining the original vertical clearance, landing checks and
support gates. The original trajectory remains the default. The focused Rust
test checks clearance, support feet, phase progression, endpoint identity and
horizontal continuity through the apex.

`plan.json` / `status.json` preserve 18 initial trials. Extending the horizontal
trajectory alone does not solve the forward support or mechanism workspace
limits. `cycle-plan.json` / `cycle-status.json` preserve four rear-support
changes: 3.75 mm/s reaches a later rear gear/pulley reference collision, while
5 mm/s produces observed rear overlap. `asymmetric-plan.json` /
`asymmetric-status.json` preserve eight tests of opposite rear stance shifts
and a rear-first order. Moving the rear stance forward, with the original
order, clears the 3.75 mm/s short case. Rear-first ordering introduces a
pulley/chassis reference collision; 5 mm/s still produces rear overlap.

| 3.75 mm/s original order | 20 ms | 5 ms |
|---|---:|---:|
| Qualified swings | 13/13 | 13/13 |
| Final position error | 3.639 mm | 2.529 mm |
| Final yaw error | 0.001061 rad | 0.000544 rad |
| Position gate | Fail (>1 mm) | Fail (>1 mm) |

The 20 ms run travels 63.70 mm net over the episode. Regression of actual body
motion over the 4.02–16.8 s commanded window gives 3.264 mm/s. The short window
and large cyclic weight shifts make this insufficient to claim sustained
speed. Sampled positive shaft work is 4.009 J; this excludes electrical losses
and is not calibrated hardware energy. Native throughput was 2.65× with
24.29 ms transition p95; native timing is not browser acceptance.

The paired trajectory comparison fails the predeclared numerical screens:
maximum foot difference is 2.202 mm against 1 mm, and maximum body difference
is 2.109 mm against 0.5 mm. Preserving all swings under refinement is useful
but does not establish trajectory accuracy. All original motor bounds,
contact checks, support thresholds and acceptance limits remain unchanged.

The three integrity reports bind all trial recipes, actions, seed 0, captures
and acceptance results. Recipes are reconstructed by the corresponding
`prepare*.mjs`; run each output directory with
`node examples/full-robot/hybrid-speed/run.mjs OUTPUT STATUS_PATH`.
The planner source hash in each plan identifies the required revision.

Next hypothesis: horizontal foot velocity near touchdown contributes to
stopping drift and timestep sensitivity. Test completing horizontal travel
before touchdown while keeping vertical clearance and the accepted support
sequence. This hypothesis has not yet been validated. All profiles retain
uncalibrated physical properties, ideal observations and privileged planning.
