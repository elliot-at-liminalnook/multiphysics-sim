# Shared motion parameterization

`sim_domain_control::motion_parameters` binds named, typed search coordinates to
reference transformations. `sim_runtime::motion_parameters` materializes them into
ordinary controller parameters and held command rows for `EmbeddedEnvironment`.
Robot names, channel groups, bounds and reference curves belong to configuration.

- `ParameterSpace` declares each parameter's quantity kind, finite bounds and
  optional integer domain. Values bind by name; `named_values` is the explicit
  conversion from an optimizer vector in declaration order. Missing, duplicate,
  unknown, nonfinite, out-of-domain and fractional integer values are errors.
- `TrajectoryTemplate` declares named coordinate channels and their quantity
  kinds, an existing trajectory, and an ordered list of transforms: affine
  amplitude/center/offset, positive reference time scaling, exact integer periodic
  control shifts, and individual control/knot offsets. Periodic closure is
  preserved. Negative and zero amplitudes are allowed. No clipping is introduced.
- Constants use their receiving port's SI unit. Named parameters must have the
  same quantity kind; matching strings or convertible numeric values are insufficient.
  `Scalar::Scaled { value, factor, power }` preserves the value's kind and
  multiplies it by a dimensionless factor raised to an integer power. A shared
  duration factor can therefore scale time by `s`, speed by `1/s`, and acceleration
  by `1/s²`. Nested references remain name checked. Zero denominators, nonfinite
  results and expressions deeper than 64 levels are rejected. Derived integer
  outputs must be exactly representable integers; nothing is rounded.
  With amplitude `a` and time scale `s`, rates scale by `a/s` and accelerations
  by `a/s²`. Periodic shifts permute controls without resampling the curve.
- Runtime trajectory bindings verify the existing reference and its controller
  parameter containing a name-to-index map. Command transforms resolve names and
  check actual controller quantity kinds and bounds using shared input validation.
  Every declared search parameter must appear in a binding. This catches unused
  search dimensions, but does not prove a particular controller responds to them.
- Optional `scalars` bind existing numeric fields through JSON pointers relative
  to `controller.parameters`, including nested arrays. Each binding declares its
  quantity kind, expected original value, optional integer output and scalar
  expression. Missing fields, malformed pointers, reference mismatches and
  overlapping writes (including trajectory/index-map ownership) are rejected.
  No field is created and no robot/world/scheduler property is writable here.
- Optional named `checks` compare two typed controller scalar fields, or a
  controller field with the read-only `scene_period`, using an explicit SI
  absolute tolerance. Checks run after all bindings. They express authored format
  invariants, such as matching curve/body/cycle durations or a wheel integrator
  matching the scheduler period. They introduce no speed, gait or slip penalties.
- Materialization is pure. `MotionVariant` retains the recipe, values, source
  commands, original scalar JSON numbers, resulting scene and commands. Identity
  scalar bindings preserve number types and signed zero; Rhai distinguishes Int
  and Float. `validate` reconstructs its result and
  rejects inconsistent parameter claims before CLI execution. It is not an
  artifact-authentication mechanism.

The registry exposes `control.affine_angle`, `control.affine_length`,
`control.affine_angular_velocity`, `control.affine_linear_velocity`,
`control.affine_time` and `control.affine_dimensionless`. Each declares typed
value/center/offset/result ports and a dimensionless scale. They use the same
algebra as trajectory materialization and the existing affine trajectory helper.
Runtime `metadata()` uses native `ParameterDeclaration` records for search bounds,
units and integer domains. Rhai can call
`parameterized_trajectory(template, space, values)` and use `trajectory_sample`
on the returned ordinary curve.

The same registry also exposes `control.scale_power_<quantity>` for those six
quantities plus linear/angular acceleration, with typed value/result ports,
dimensionless factor and a required integer `power` parameter. Affine components
also cover both acceleration kinds. The registry factory and offline expression
use the same scaling algebra.

For example, a controller cycle can be bound to a dimensionless search coordinate:

```json
{"pointer":"/period_s","kind":"Time","reference":0.4,
 "value":{"source":"scaled","value":{"source":"constant","value":0.4},
          "factor":{"source":"parameter","name":"time_factor"},"power":1}}
```

That declaration alone does not retime another curve or parameter. Bind each
dependent field explicitly and declare relevant equalities. A parameter called
`period_s` might mean a cycle duration or an integration timestep; the library
does not guess based on its name. See `prepare_coordinated_motion.mjs` for the
current quadruped and wheel recipes. The quadruped recipe coordinates its joint
curve, body/contact period, body keyframe times, phase offsets, pause windows,
velocity leads, reversal settling, nominal/requested velocities and acceleration
settings. Fractional foot phases, physical feedback coefficients and protocol
lease remain unchanged. These are reference transformations, not dynamically
similar solutions: gravity, contact and actuators still determine the motion.

## Execution

Build the ordinary Rust helpers:

```sh
cargo build --locked --release -p sim-runtime \
  --example prepare_motion_experiment --example materialize_motion \
  --example run_environment
```

`prepare_motion_experiment completed-capture.json fresh-directory` extracts the
parsed scene/config/task and reconstructs commands through shared recorded-input
validation. It preserves the original capture's source identity in a sidecar;
executing a new candidate records the current runtime identity. It does not relabel
prediction models or change the episode horizon.

```sh
target/release/examples/materialize_motion scene.json actions.json \
  parameterization.json values.json fresh-motion.json
target/release/examples/run_environment --motion fresh-motion.json \
  config.json task.json > fresh-capture.json
```

The materializer refuses to overwrite its output. The environment CLI uses its
existing seed 0 for new runs and records it; replay uses the recording's seed.
The capture retains the parameter recipe, values and source commands in addition
to the ordinary runtime recording. Python is not involved in control or physics.

WASM exports `materialize_motion`; the worker message of the same name takes
`scene`, `actions`, `recipe` and `values`. It returns the same variant and metadata
as the native materializer without replacing or stepping a loaded environment.
Hosts then load the variant's scene and feed its commands to the ordinary runtime.
Materialization can be expensive for large curves, so browser callers use a worker.

## Evidence and remaining scope

`motion-parameterization-evidence-v1.json` archives the input recipes, native
materializations/captures, browser outputs, binaries and verification logs.
The wheel case varies two angular-velocity commands over 30 ms; the quadruped
varies a 12-channel reference over 100 ms. Both identity candidates reproduce
native physical frames and task transitions exactly. Both changed candidates
affect physical joint motion. These are controller sensitivity cases, not new
speed measurements or a demonstration of wheeled locomotion.

Reference channel units are declared by the controller author. An arbitrary Rhai
program's interpretation of its reference parameters is not statically proved by
its index map. Command units are checked against the scene's input contract.
Reference time scaling does not silently retime firmware, policy, motion-gate,
episode or other controller clocks. Coordinated controller fields are explicit
scalar bindings; checks are author-declared, not static analysis of arbitrary
Rhai. Direct embedded motor-reference materialization, full actuation contracts
and generalized feedback remain. The durable optimizer lifecycle now consumes
these recipes through shared experiments; see `reproducible-experiments.md`.
Discrete phase shifts use integer controls; this is not continuous contact planning.
Search bounds are explicit configuration choices and are not physical speed limits.

`coordinated-motion-evidence-v1.json` retains the coordinated-timing follow-up:
the same durable source scenes, original/identity/changed experiment specs, native
physical trajectories and replay checkpoints, isolated browser captures, binaries
and verification logs. A factor of 0.8 scales the quadruped's 29 explicitly bound
controller scalars and its joint reference; three equalities keep the cycle,
body and joint-curve endpoints aligned. The wheel recipe binds two initial target
angles and inversely scales velocity commands while checking its integration
period against the unchanged scheduler.

Both identity runs reproduce the original physical frames and task transitions
exactly. The changed runs move physical joints by up to 0.025715 rad (quadruped,
100 ms) and 0.0000108585 rad (wheel, 30 ms), with exact same-host replay. The wheel
case includes contact settling; it is not sustained wheeled locomotion. Browser
checks also exercise the pure scalar materializer and reject invalid references
and checks without disturbing the current evaluator. The shared experiment CI
recreates these cases from durable inputs; local results do not imply a remote
CI run. Neither these short trials nor a retimed reference establish a new maximum
speed, physical convergence, learned transfer or realtime browser qualification.
