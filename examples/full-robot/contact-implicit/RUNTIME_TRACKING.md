# Detailed runtime tracking and contact-model corrections

Previous goal turn made progress in `cb380a7`. This turn executes the new
contact-implicit references in the shared Rust/Rhai runtime. It improves measured
startup motion from .131 to .158 m/s, but **none of these paths qualifies as a
walking gait**: loaded-foot sliding is excessive and coarse-knot feasibility
does not survive time-grid refinement. No new browser bundle or speed record.

## Controller and execution

The shared `EffectiveServo::reference_target` converts position, velocity and
torque references into the existing servo target, preserving its gains and
torque-speed saturation. Tests verify feedforward, feedback, saturation and
nonfinite rejection. No force is injected around the actuators.

`compile_contact_implicit` independently checks the planner and sampled geometry,
requires an at-rest initial state, and compiles linear joint references with
interval-held inverse torque and velocity feedforward. A known failing plan is
rejected (`rejected-surface25.compile.log`). `finite-plan.rhai` requests those
targets through the normal environment contract. This is a timed diagnostic,
not a periodic or command-responsive controller. Nothing certifies endpoint hold.

The .39608749 s trials use 97 reporting/controller endpoints, 4.125911 ms policy
period, and physics steps .515739 or .257869 ms. Existing tilt/height task bounds
are retained. All seven executed trials complete; their sampled collision audits
find zero interlink overlap. `audit_capture_geometry` calls shared CAD geometry
checks and preserves whether a capture is complete or partial.

The retained source profile uses **regularized Coulomb patch friction at .001
m/s**, sampled CAD surfaces, and omitted interlink force response. Full sampled
interlink geometry is audited separately. Earlier descriptions calling this
particular validation profile a bristle model were incorrect. The runtime has
a bristle option, but it is not selected in these source scenes.

Two setup attempts fail before integration: the first had inconsistent scene
construction/embedded clocks; the second requested a per-step contact log that
currently supports scheduled motors, not effective servos. Their inputs, empty
captures and error logs remain archived. Version `v3` names are the actual runs.
Contact-motion evidence is at report endpoints, not every accepted physics step.
An initial analysis also incorrectly compared raw export JSON with typed capture
JSON; schema defaults and key order differ. The corrected analysis checks source
identity, declared options, input hashes and equal typed models across timesteps.

## Resolved contact properties

The reusable `floor_contact_profile` API and `inspect_floor_contact` example read
the properties actually resolved by the runtime. `resolved-floor-contact.json`
records the four nylon foot links, their 24 samples each, and these values:

| Quantity | Earlier planner | Resolved runtime / corrected planner |
| --- | ---: | ---: |
| Kinetic foot/world friction | .317241 | .25 |
| Static material-pair friction | Not used by planner | .30 (not used by selected kinetic patch law) |
| Normal dissipation, s/m | 5 | .2 |
| IDTO reciprocal dissipation velocity, m/s | .2 | 5 |
| Slip regularization, m/s | .02 | .001 |
| Stiffness per sample, N/m | 200,000 | 200,000 |

The .317241 world scalar does **not** determine the compiled nylon/world pair.
Earlier contact-implicit recipes copied the wrong field. They remain reproducible
historical experiments, not correctly matched runtime models. The resolved
dissipation includes the existing runtime's compiled default/override logic;
none of these values is newly calibrated hardware data. Pointwise planning
friction still differs from the runtime's coupled friction patch, and 10 µm
planning smoothing still permits force at separation.

For the four nylon feet supporting 3.976239 kg on level ground with negligible
vertical acceleration, the conditional traction bound is .25 mg = 9.751726 N,
or 2.4525 m/s² horizontal acceleration. This is **not a speed ceiling**. Previous
calculations using .317241 as the actual foot coefficient need this correction;
previous measured runtime speeds are not invalidated by a planner-input mistake.

## Sliding work objective and outcomes

The original plan itself slides its loaded feet roughly one body-travel distance.
The new optional `contact_sliding_work_scale_j` adds dissipated tangential work
`sum(dt * -f_t dot v_t) / scale` to the objective. It leaves unloaded motion free
and supplies no contact timing or stance flags. An analytic constant-velocity
test verifies the work, unchanged forces and invalid-scale rejection. The chosen
scale, .04828171 J, is explicitly .05 mu mg times requested travel distance. This
normalization does not guarantee a 5% per-foot slip ratio and is not a speed limit.

The corrected-parameter control and work-cost searches start from exactly the
same previous plan with the same task, bounds and 100-iteration budget. The control
ends at a damping limit after 48 iterations; the work case hits the iteration
limit. Both meet the declared planning tolerances (including the -.01 Nm torque
margin allowance) and sampled geometry gates. Neither proves stationarity.
Predicted sliding work falls from .872024 to .660639 J with the added cost.

| Runtime variant | Planned mean m/s | Measured mean m/s, fine step | Max body error at plan knots | Max loaded-foot slip/body-path ratio |
| --- | ---: | ---: | ---: | ---: |
| Original plan, feedforward | .183028 | .131644 | 20.49 mm | 148.48% |
| Original plan, position/velocity only (coarser step) | .183028 | .099582 | 34.19 mm | 112.77% |
| Corrected contact parameters | .189191 | .141752 | 19.16 mm | 127.50% |
| Corrected parameters + sliding work | .184967 | .158038 | 13.02 mm | 99.88% |

The work variant has .04327 rad maximum joint error at the 13 plan knots,
.07702 rad maximum tilt, zero sampled interlink overlap and .296773 mm maximum
floor penetration. Halving the runtime timestep changes its body path by at most
.297991 mm and mean speed by .0554%. Numerical integration error therefore does
not explain the much larger plan/tracking discrepancy. All slip ratios fail the
existing 5% development quality criterion; this is not a physical speed limit.

## Time-grid consistency failure and next work

Linearly subdividing the retained work plan at half its planning timestep, without
reoptimizing, produces 45.2815 N force error, 10.5474 Nm moment error and -1.71333
Nm torque margin. Its 121-pose geometry still passes. This is an inverse-dynamics
consistency check of the interpolated path, not an alternate runtime simulation.
It shows why coarse endpoint feasibility cannot certify the executed reference.

Next, make finer and longer time-grid optimization practical by caching unchanged
local three-knot evaluations with exact uncached-equivalence checks. Reoptimize
the finer grid; do not merely interpolate and call it feasible. Include the
resolved contact properties and sliding-work objective from initialization, use
multiple generic seeds, and address repeatability/terminal viability. Retain
actual steering, reverse, stop, command-loss, collision/slip and rendered-browser
requirements. The existing .21 browser remains immutable and experimental.

## Reproduction

Recipes, compiled references, summaries, tests and input hashes are direct files
beside this report. `runtime-tracking-binaries.json` identifies the executables
used for the first controller comparison, before later source additions.
`runtime-evidence/evidence-v1-index.json` plus the incremental v2 archive restore
all runtime inputs, captures, logs and geometry outputs with verified hashes.
Direct manifest v3 is commit-scoped; preserve earlier source/evidence manifests.
No CAD values or files in the original user worktree were changed.
