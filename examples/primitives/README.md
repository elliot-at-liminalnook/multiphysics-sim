# Reusable physics-informed exploration primitives

Status: implemented in the shared Rust library, 2026-09-19. The original
primitive checks below are analytic/unit tests and synthetic request examples.
The subsequent [reduced exploration workflow](reduced-exploration.md) adds
explicit matched simulation qualification. Gait search and hardware operations
remain paused.

## Available building blocks

| Family | Native library APIs | Registry operations added |
| --- | --- | --- |
| Mechanism reduction | `sim_domain_robot::reduction::LocalReduction`; `EmbeddedMotion::local_reduction()` | `mechanics.local_reduction` |
| Actuator limits and reductions | `sim_domain_robot::actuator_envelope::{Config, prepare_reduction, winding_error_retention}` | `actuation.dc_envelope`, `actuation.prepare_reduction` |
| Contact and load feasibility | `sim_domain_robot::contact_feasibility::{required_wrench, evaluate}` | `mechanics.centroidal_wrench`, `contact.force_feasibility` |
| Motion references | `sim_domain_control::motion_primitives::{ContactTemplate, govern}` | `motion.contact_template`, `motion.contact_sample`, `motion.govern_angles` |
| Offline experiment evaluation | `sim_runtime::evaluation_primitives::{score, rank, tracking}`; existing `fidelity::compare` | `experiment.score`, `experiment.rank`, `experiment.tracking_statistics`, `experiment.compare_fidelity` |

These twelve operations have versioned identities (currently version 1), shared
input/output descriptions, units, assumptions, and typed Rust requests. They
are registered in `BehaviorRegistry` as explicit functions, separately from
acausal physical elements. Scalar quantity references are checked against the
same physical definitions registry. Mixed-coordinate records describe their
units explicitly. The typed request structures are the authoritative schema;
descriptor shapes are discovery metadata, not a generated JSON Schema.

The mechanisms, motor equations, sampled controller, trajectory interpolation,
contact allocation, environment, search, and replay remain shared components.
There is no quadruped-specific runtime or alternate browser physics here.

### Mechanism reduction

At a solved CAD configuration, the existing constraint embedding supplies the
tangent `J` and acceleration bias `b`. The primitive computes velocity `J u`,
acceleration `J a + b`, generalized force `Jᵀ f`, and inertia `Jᵀ M J`.
This preserves instantaneous virtual work and kinetic energy. The API validates
dimensions, finite values, full column rank, and positive definite inertia.

Recompute the embedding when configuration or velocity changes. A local tangent
does not authorize freezing a nonlinear linkage over a gait. Full inverse
dynamics and point-force mapping remain in `PreparedEmbeddedDynamics` and
`motion_capability`; this primitive does not replace their bias-force handling.

### Actuator dynamics and power

The DC envelope evaluates voltage, temperature-dependent resistance, back EMF,
current, torque, gearbox efficiency, sliding friction, and electrical/mechanical
power. It reports current violations without clipping them away. Its optional
torque interval becomes absent when voltage and current constraints have no
common steady operating point. Electrical power and bus current may be negative
during regeneration; the power system must model whether it can accept that.

Dynamic behavior reuses `robot.motor_unit` and its existing `MotorDynamics`
levels: detailed, quasistatic winding, quasistatic rotor, or both. The reduction
preparer preserves authored physical parameters and changes only the explicit
storage-omission flags. Its winding decay diagnostic assumes held voltage and
speed, at reference temperature; it does not justify eliminating rotor inertia.
The existing sampled controller owns command/sensor delay and quantization;
the existing motor/gearbox owns compliance, backlash, friction, and heat.
`robot.h_bridge` and `robot.battery` remain the power-system components.

The envelope is an instantaneous screen, not a replacement dynamic motor.
Friction at rest, load-dependent behavior, and the measured accuracy of any
reduced model still require validation. No CAD values, controller gains, or
runtime fidelity defaults were changed. The existing `cad_fixed_pd` guard
rejects the separate effective-servo path; it does not reject storage reductions
within `MotorUnit`. Those reductions already reach the ordinary runtime through
`scene.options.motor_dynamics`. The subsequent [reduced exploration workflow](reduced-exploration.md)
packages these choices with accuracy/performance gates and retained quadruped
evidence. Storage/timestep reductions failed that case; solver-work reduction
preserves the full motor model.

### Contact feasibility

The centroidal helper computes the required contact force and moment from
explicit mass, gravity, acceleration, inertia, angular motion, and other loads.
It uses a locked-body inertia approximation: relative limb momentum is omitted.

The contact checker evaluates a **supplied** allocation against force/moment
balance, arbitrary-normal Coulomb friction, unilateral support, normal-force
capacity, and supplied actuator torque bounds. It retains signed constraint
margins. Use the existing `motion_capability` allocation and inverse-load tools
to construct forces and motor loads. A failed allocation does not prove every
allocation fails; a passing snapshot does not prove a stable gait.

### Motion generation

Contact templates resolve typed parameters into existing smooth body and foot
references, with any number of independently phased contacts. Robot topology
and specific gait choices belong in configuration. Templates currently generate
one stance/swing pair per foot per period; the existing `ContactPhaseConfig`
and `motion.contact_sample` also support explicit additional step sequences.
Body rotation-vector derivatives must not be confused with angular velocity.

The named angular governor bank uses the existing speed/acceleration governor,
explicit initial states, and authored angle bounds. It retains the raw request
and reports target clipping. Invalid or infeasible boundary states fail as a
whole without changing caller state. Conditioning a reference is not evidence
that a motor tracks the original unconditioned request.

### Evaluation, search infrastructure, and replay

Scoring requires explicit physical quantities and acceptance gates. Incomplete,
failed, or missing-metric outcomes cannot win. Offline ranking retains rejected
outcomes and requires unique candidate IDs in one declared comparison context.
Callers must derive that context from recorded model/world/policy/input identity;
an arbitrary matching string is not independent provenance verification.

Tracking statistics use matched timestamps without alignment or lag fitting.
RMS weights samples equally after the explicit startup cutoff. Command rates
and accelerations use actual sample intervals and include startup. Fidelity
comparison delegates to the existing matched-capture comparator with explicit
context differences and tolerances.

Reuse `sim_runtime::experiment::{Experiment, Journal}` for durable proposals,
checkpoints, failure records, and resume; `experiment_search` for the optional
Bayesian proposal adapter; and existing environment recordings for replay.
Those APIs were not exercised in this implementation step. No new candidate
was proposed or evaluated, and no saved result was promoted.

## Using the primitives

Native Rust callers can call the typed functions directly. Registry clients
(including future inspectors) and Rhai call the same function and validation:

```rust,ignore
let registry = sim_runtime::registry();
let descriptors = registry.primitive_descriptors();
let output = registry.call_primitive(
    &sim_core::definitions::DefinitionId::new("mechanics.local_reduction", 1),
    serde_json::from_str(include_str!("mechanism.json"))?,
)?;
```

Rhai exposes `primitive_catalogue()` and
`primitive_call("mechanics.local_reduction", 1, request)`. Runtime-created Rhai
controllers receive the host registry. Standalone `RhaiController::with_seed`
includes the motion primitives; hosts can supply additional domains using
`with_seed_and_registry`. The serialized bridge is for scripting/inspection;
native typed calls avoid JSON overhead in performance-sensitive code.

The CLI below only discovers or calls primitives; it has no environment-run or
optimizer entry point. The checked-in requests are **synthetic examples**, not
Hiwonder calibration or quadruped parameters:

```sh
cargo run --locked -p sim-runtime --example inspect_primitives
cargo run --locked -p sim-runtime --example inspect_primitives -- mechanics.local_reduction 1 < examples/primitives/mechanism.json
cargo run --locked -p sim-runtime --example inspect_primitives -- actuation.dc_envelope 1 < examples/primitives/actuator.json
cargo run --locked -p sim-runtime --example inspect_primitives -- contact.force_feasibility 1 < examples/primitives/contact.json
cargo run --locked -p sim-runtime --example inspect_primitives -- motion.govern_angles 1 < examples/primitives/motion.json
cargo run --locked -p sim-runtime --example inspect_primitives -- experiment.score 1 < examples/primitives/evaluation.json
```

## Validation and stopping point

Focused tests cover work/energy preservation; invalid reductions; drive,
braking, and regenerative power balance; voltage/current/temperature envelopes;
support, tipping, slip, and overload; gyro moments; preservation of authored
motor properties; parameter units and independent phases; governor equivalence
and boundary errors; rejection-aware scoring; nonuniform sample derivatives;
registry discovery/version checks; and a captured Rhai callback.

```sh
cargo test --locked -p sim-core --test primitives \
  -p sim-domain-control --test motion_primitives \
  -p sim-domain-robot --test exploration_primitives \
  -p sim-script --test primitives \
  -p sim-runtime --test evaluation_primitives
```

These sixteen tests pass. The CLI builds, discovers all twelve operations, and
all five checked-in requests execute successfully with these analytic results:

| Example | Verified result |
| --- | --- |
| Mechanism | Full velocity `[6, -3]`, acceleration `[8.1, -4.2]`, reduced force `4`, inertia `11` |
| Actuator | Winding current `2.75 A`; electrical `16.5 W = 1.2275 W` mechanical `+ 15.2725 W` dissipation |
| Contact | Zero wrench residual; supplied allocation passes |
| Motion | Next angle `0.002 rad`, speed `0.1 rad/s`; original request `2 rad` retained and clipping reported |
| Evaluation | Eligible objective `0.1 m/s`, no rejection reasons |

They establish primitive behavior on focused cases,
not gait performance, speedup, or sim-to-real accuracy. The subsequent reduced
exploration workflow adds `experiment.prepare_reduced` to the registry and
explicit profile qualification; see its separate guide and measured evidence.
No gait search or hardware promotion follows automatically from qualification.
