# Joint CAD-domain restoration

The sixteen-control force repair passed its 266 planning frames but failed the
independent dense IK check. The planner now includes phase, time and operating
clock in point-tracking errors. The failure is at phase 0.7505 in forward travel,
with independent coordinate 5 (+X foot servo) at its -0.02 rad upper bound.
The reported maximum point-plane error is 5.300800798 nm. This is a bounded IK
solver failure, not a proof that the motion is physically impossible.

The shared `sim-solve::domain_restore::bisect_evaluation_domain` evaluates both
endpoints, then bisects a known accepted/rejected parameter bracket. It preserves
an evaluated accepted point and every attempted fraction/rejection. Fatal
callback errors are distinguished from domain rejections. Nonmonotone domains
are supported without claiming that the returned fraction is globally maximal
or that the path between endpoints is valid.

`ContactPlanner::restore_joint_evaluation_domain` reuses the shared joint
decision decoder and uncached CAD evaluator. The eight-control reference is
exactly refined to sixteen controls before interpolation. Only specified
bounded decisions change; the target's fixed fields, force timing bindings,
robot and physical limits are retained. Acceptance requires successful CAD
evaluation, zero sampled inter-link penetration and the recipe's original
0.1 mm floor-penetration gate. Force and actuator feasibility are reported
independently, not used to hide a rejected geometry evaluation.

## Completed experiment

Inputs are `joint-conic-feasible-speed.recipe.json` (reference) and
`joint-body16-conic-audit.recipe.json` (target). Their robot definitions and
force timing bindings match exactly. The CLI adds 1,000 uniform phases while
retaining the old uniform phases, body/contact events and both clocks.

Twenty-four bisections plus both endpoints give 26 CAD attempts. The accepted
fraction is **0.9999452233314514**, bracketed above by rejected fraction
0.9999452829360962. The selected reference passes all **2,266** sampled physical
checks:

| Quantity | Result |
| --- | ---: |
| Planned speed | 0.02522439347 m/s |
| Maximum force error | 0.04419977283 N |
| Maximum moment error | 0.01347873672 Nm |
| Minimum torque margin | +1.26014453239 Nm |
| Force-cone violation | 0 N |
| Maximum normalized inequality | 0 |

This is a planning result, not measured robot walking. The subsequent
4,000-uniform-phase audit **fails** at phase 0.751125 in the same forward clock
and at the same joint bound (reported point-plane error 0.748918039 nm).
Its exit code 1, empty result and diagnostic log are retained. Thus even the
2,266-frame pass is insufficient for promotion.

The finer restoration completed 26 evaluations and passes all **10,266**
combined samples at 0.025224393293 m/s. Force and moment errors are
0.044205384684 N and 0.013717458637 Nm; torque margin is +1.260143237319 Nm
and cone violation is zero. The accepted interpolation fraction is
0.9999974966049194, relative to the first restored target. The four observed
IK-failure phases are retained in the next optimization mesh.

The raw 60,228,918-byte result is stored losslessly as
`joint-body16-domain4000.result.json.gz`; its byte-identical round trip and hashes
are recorded. This pass covers the previous planner gates. A subsequent
[controller screen](SERVO_COMMAND_CONSTRAINTS.md) fails a servo-command bound
at 0.72 s, so it remains unqualified for runtime walking. The original
eight-control speed search continues independently.

## Verification and reproduction

All 38 shared solver tests and six planner tests pass. The bisection tests
cover disconnected domains, valid target shortcuts, rejected references,
fatal callback errors and nonfinite inputs. The final wrapper also validates
body trajectories before decoding; an empty-body CLI case exits with a clear
validation error. The earlier successful CAD measurements use the explicitly
archived initial build; this input guard does not alter valid trajectory math.

`check_joint_domain_restore.mjs` verifies the accepted/rejected bracket,
original decision bounds, fixed recipe fields, timing bindings, audit mesh,
geometry gates and complete selected report. It writes the summary and
restored recipes with exclusive creation. `record_joint_domain_restore.mjs`
records source overlays, exact executable/input hashes, build/test commands,
CAD evidence references and the finer-run launch. The first source/binary
manifest is `joint-domain-restore-initial-build.json`; the final build is
`joint-domain-restore-build.json`. CI builds the example and runs the shared
tests; the remote CI run has not been observed here.

No joint bound, IK tolerance, friction coefficient, motor limit or acceptance
threshold was relaxed. Local optimizer exhaustion and sampled domain recovery
are not continuous-motion or global-speed certificates.
