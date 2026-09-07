# Distilled motor-feedback student

The `robot-distilled-student` preset replaces the teacher's body/foot motor
feedback with a neural policy. It retains the joint reference and local joint
tracking term, and still uses the online planner. This is a real teacher-to-student
initialization experiment, but it is not a deployable hardware controller.

The student sees 45 features: joint reference errors, joint references, joint
velocities, body gravity direction, body angular velocity, and motion commands.
It excludes world body translation, foot geometry, contact forces and the
teacher's computed body/point corrections. All observations currently come from
ideal simulation. CAD declares **no sensors**; encoder differentiation, IMU
availability/mounting, delays, noise and a causal estimator remain unconfirmed.
The planner still uses ideal body/foot state and contact forces. See the explicit
`student-distillation/observation-boundary.json` dependency inventory.

## Training and validation

The shared Rust neural library now supports typed physical demonstrations,
normalized squared-error loss, analytic backpropagation and Adam fitting.
Independent central differences check every parameter of a multi-layer test
network; a separate known-controller test checks held-out predictions. The
normalization and inference path used for fitting is shared with runtime
inference. The optimizer retains the lowest-training-loss checkpoint, with no
validation samples in weight updates or checkpoint selection.

The robot has a 64-unit hidden layer and twelve bounded angle outputs. Labels are
the teacher's complete motor feedback beyond reference plus local joint tracking,
rather than only its tiny learned residual. Training uses 4,200 control samples
from a 24-second forward/reverse episode and a 60-second sustained episode. The
1,200 reverse-first samples remain separate for validation. The selected epoch
is recorded with zero-based indexing in `validation.json`.

The first, short-data student completed 28 qualified swings over a minute but
missed the 1 mm stopping gate at 1.408 mm. Adding the sustained demonstrations
improved that error to **1.127 mm**, which **still fails the gate**. The final
student passes both short sequences (nine qualified swings each), with final
body errors of 0.669 and 0.798 mm. Its held-out imitation error can still reach
about 0.75 degrees on individual motor targets. Imitation loss, physical walking
acceptance and robustness are separate claims.

The student no longer consumes the teacher's body/point motor-feedback
suggestions. `PolicyConfig.feedback_observations = false` omits that optional
work while preserving the planner definitions. Policy-side floor-force
observations are also omitted. The comparison reproduces every physical frame,
retained controller value and task transition exactly over 24 seconds. Physical
contact and the planner's contact queries remain active. This is removal of
unused work, not a changed physics model.

The viewer provides WASD, live learned-output readouts, recording and replay
using the same Rust network artifact. `student-distillation-status.json` records
browser parity/timing and the unresolved physical acceptance gate. The earlier
baseline and neural teacher remain available. The complete active goal remains
open: sensor/estimator integration, meaningful robustness training, wider
commands/terrain and calibration are still required.

On the documented Intel Mac/Chrome host, the rendered minute keeps pace with
realtime; active transition p95 is **22.7 ms**, still above **20 ms**. Rendering's
p95 scheduling interval is 16.67 ms. These measurements do not establish
command-to-visible-response latency or a speedup from the omitted work alone.

## Reproduce

The generator reads the versioned teacher recipes' captures. Recreate those
using the commands in `neural-teacher.md` if the local captures are absent.
Training and validation data are embedded in the versioned distillation recipe.

```sh
node examples/full-robot/prepare_student_distillation.mjs
cargo run --locked --release -p sim-runtime --example distill_policy -- examples/full-robot/student-distillation/experiment.json runs/full-robot/learning/student-distillation/multi-fit
node examples/full-robot/materialize_student.mjs
cargo test --locked -p sim-domain-control --test neural
cargo test --locked -p sim-runtime --test neural_policy --test environment
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/student-distillation/short.config.json examples/full-robot/student-distillation/task.json examples/full-robot/neural-teacher/heldout.actions.json > runs/full-robot/learning/student-distillation/multi-heldout.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/student-distillation/multi-heldout.native.json runs/full-robot/learning/student-distillation/multi-heldout-acceptance
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/student-distillation/config.json examples/full-robot/student-distillation/task.json examples/full-robot/browser-residual-policy/sustained.actions.json > runs/full-robot/learning/student-distillation/multi-sustained.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/student-distillation/multi-sustained.native.json runs/full-robot/learning/student-distillation/multi-sustained-acceptance
```

The final command currently reports failure for stopping error; do not reinterpret
it as a passed walking gate. To check omitted work, copy `short.config.json`, remove
`policy.feedback_observations`, set `policy.task_observations.floor_forces` to true,
run the identical actions, then use `check_unused_feedback.mjs full reduced report`.

```sh
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
node web/build-viewer.mjs runs/interactive/student-distillation/viewer --environment-only
node web/tests/environment.mjs runs/interactive/student-distillation/viewer robot-distilled-student runs/full-robot/learning/student-distillation/multi-sustained.native.json runs/interactive/student-distillation/sustained-parity.json
node web/tests/viewer.mjs runs/interactive/student-distillation/viewer runs/interactive/student-distillation/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/student-distillation/viewer robot-distilled-student runs/interactive/student-distillation/live-performance.json sustained-forward
node web/serve-viewer.mjs runs/interactive/student-distillation/viewer 4183
```

Run performance measurements without competing simulation/build jobs. CI checks
analytic gradients, known-controller distillation, the student's short held-out
walking case, exact removal of unused work, full-minute native/WASM parity and
viewer replay. It does not certify the failed full-minute stopping gate.

Next, improve closed-loop student behavior through further learning and
student-state demonstrations, add bounded perturbation cases, and replace the
remaining ideal observation paths with explicit sensor/estimator contracts.
The optimizer follows [Adam's published update](https://arxiv.org/abs/1412.6980);
this implementation's accuracy is checked locally rather than inferred from
that paper's reported results.
