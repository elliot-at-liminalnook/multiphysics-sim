# Shared robot learning and optimization

Objective: generalize the speed experiments into shared Rust components for
CAD-derived robot/actuator/sensor contracts, typed dynamics observations and
action-conditioned trajectory prediction, configurable motion parameterization,
reproducible optimization and experiment execution, explicit fidelity comparison,
and task-specific progress/failure criteria. Keep topology and gait choices in
configuration, expose shared definitions through the registry, and use one runtime
for browser, headless and learning workflows. Validate the same APIs on the current
robot and a structurally different robot, preserving sustained net-speed optimization
without arbitrary gait restrictions and enabling future morphology-conditioned learning.

## Delivery and evidence ledger

| Requirement | Existing foundation | Required work and acceptance evidence |
|---|---|---|
| CAD robot/actuator/sensor contract | `PhysicalModel`, typed ports, registry, embedded coordinate and actuator metadata | A versioned inspection contract with stable identities, topology including closed loops/transmissions, units/frames, physical limits and provenance/uncertainty. Explicitly identify missing CAD properties and legacy defaults. Same contract consumed by authoring, environment and learning. |
| Typed dynamics and action-conditioned prediction | `MotionSnapshot`, `ForecastRecipe`, neural trajectory forecaster | Bind state and actions to the robot contract. Generalize angle-only forecast actions and implicit world/gravity assumptions; preserve causal action horizons. Validate units, timestamps, observation availability and multi-step prediction on both robots. |
| Motion parameterization | Shared trajectory affine transforms, exact periodic shifts, motion clocks and contact phases | Typed configuration-driven parameter bindings and transformations in Rust; no hardcoded leg counts or actuator names. Identity reconstruction and derivative/unit checks, then measured controller behavior on both robots. |
| Reproducible optimization/execution | Rust selectors, PPO, shared environment; JavaScript experiment orchestration | Move experiment state, ask/evaluate/update, durable result identity, replay, cancellation and resume into reusable Rust APIs/CLI. Failed or incomplete episodes cannot become eligible speed scores. Demonstrate interrupted/resumed equivalence and a completed search on both robots. |
| Fidelity | Timestep-conditioned GP and matched-input experiment checks | Reusable typed fidelity identity and comparison reports: physics/command timing, contact options, trajectory error, cost and full task outcomes. Reject accidental context mixing. Browser and detailed models retain measured approximation/performance evidence. |
| Progress/failure separation | Legacy `SpeedMonitor` couples XY distance with upright/ground-clearance checks | Shared registered displacement primitive and optional environment `progress` task with separately authored termination bounds. Preserve old speed behavior and reward arithmetic. Add explicit morphology-appropriate failure rules; no implicit gait/slip/tracking restrictions. |
| Shared registry and one runtime | `BehaviorRegistry`, `EmbeddedEnvironment`, Rust/WASM worker | Expose each new component's parameters/ports/units/validation through the existing registry and each robot's resolved bindings through runtime metadata. Test native/browser/learning use of the same contracts. |
| Two robot forms | Current CAD quadruped and a motorized pendulum regression fixture | Add a durable CAD-authored locomoting robot with different topology, e.g. a two-wheel platform. Run the same inspection, prediction, parameterization, optimizer and fidelity APIs. The pendulum regression alone does not satisfy this requirement. |
| Future morphology-conditioned learning | Explicit physical model and configurable neural channels | Export morphology and dynamics together with stable variable-length entity mappings and observation/action masks. Validate renaming/reordering and changed topology. Universal learned-policy transfer is not established by interface reuse. |

## Implementation sequence

1. Separate progress from morphology-specific failure while preserving existing
   speed measurements. Register the shared algebra and exercise the production
   environment and replay path.
2. Build the strict resolved robot/dynamics contract from existing CAD definitions;
   introduce the second CAD robot to expose missing assumptions early.
3. Bind prediction and motion parameterization to that contract and exercise them
   on both robots. Physics guidance must use each robot's actual capabilities.
4. Consolidate experiment execution and fidelity comparison into Rust, then run
   matched optimization and browser/headless/learning acceptance cases on both.

The existing served browser bundle is immutable during this work. Preserve user
sessions and historical experiment inputs. Existing raw speed experiments are
evidence and training sources; new dedicated gait search batches are not the
primary workstream of this generalization goal.

Completion requires direct evidence for every ledger row. Short analytic examples,
a compiling API, or tests on one robot do not establish the whole objective.

## Current checkpoints

- Shared progress extraction: `shared-progress.md` and its durable evidence.
- Authored model inspection: `robot-contract.md`; native/WASM share the same Rust
  inspector and registry descriptions. Episode binding and complete typed dynamics
  remain pending.
- A durable second CAD topology exists in `examples/wheeled-robot/baseline` with
  analytic wheel inertia checks. Passive independent coordinates and scheduled
  IMUs now run through shared runtime components; see
  `passive-coordinates-and-sensors.md`. Short native/WASM sensor and contact cases
  pass at the recorded fine timestep. Contact trajectories are not timestep
  converged, and this is not yet a locomotion/controller acceptance case.
- Authored held IMUs now reach Rhai/neural policies and causal forecast inputs;
  environment transitions and motion snapshots retain typed sensor samples and
  optional sample timestamps. See `imu-policy-observations.md`. Coordinate and
  actuator initialization/reference bindings account for passive chart order and
  linear units. Full robot/action contract binding and passive-aware geometric
  feedback remain pending; these sensor tests do not qualify learned dynamics.
- Controller-conditioned forecasting now has complete typed command bindings,
  explicit program/policy/reference context, recorded held-action reconstruction
  and a read-only native/WASM environment query. See `controller-action-forecasts.md`.
  Internal servo planning remains angular; full physical action/fidelity identities
  and validated prediction accuracy remain required.
- Shared fidelity comparisons now retain complete parsed execution contexts,
  exact declared configuration changes, matched physical-time controller inputs,
  per-channel trajectory errors, final task outcomes and explicitly scoped costs.
  Native and WASM expose the same read-only comparator; see `fidelity-comparison.md`.
  Strict authored/resolved contract binding,
  hidden actuator-state comparison, realtime qualification and physical convergence
  remain required; software parity is not physical validation.
- Version 2 trajectory models now bind recorded/executing physical profiles and
  library source identities. The shared runtime caches profiles, controller queries
  return their checked binding, and embedded actuator forecasts can opt in to the
  same validation. See `physics-bound-forecasts.md`. This does not qualify prediction
  accuracy, complete the observation state, or establish morphology transfer.
- Named Rust motion parameterization now produces ordinary controller references
  and commands with typed parameters, explicit transforms, registry descriptions,
  saved-result validation and native/WASM preparation. Identity reconstruction
  and changed controller behavior are checked on both CAD forms; see
  `motion-parameterization.md`. Direct embedded reference integration and complete
  CAD unit binding remain required. The short wheel case is still not locomotion
  acceptance.
- Shared experiment contexts, journals and bounded evaluators now connect motion
  proposals to ordinary environment execution, exact checkpoint replay and eligible
  observed scores. A native Bayesian adapter and durable CLI preserve selection
  and partial progress across process restarts; browser workers use the same
  evaluator. See `reproducible-experiments.md`. Short searches/replay cases do not
  qualify sustained locomotion or physical convergence. Discrete selector
  integration, efficient complete-state restore and broader
  morphology/learning validation remain required.
- Typed scalar expressions and existing-controller-field bindings now coordinate
  gait timing, velocities and accelerations through the same motion materializer.
  Authored equality checks distinguish reference cycle periods from scheduler
  timesteps. The two CAD forms exercise identity, changed physical motion, replay
  and native/browser preparation; see `motion-parameterization.md`. These checks
  validate authored controller invariants, not arbitrary Rhai semantics or
  physically similar motion under retiming. Strict CAD contract binding and
  sustained locomotion/fidelity acceptance remain pending.
- Identified actuator values now reach incremental motor, driver and firmware
  construction through the same resolved model as the detailed plant. The original
  scene and fit remain replay inputs; see `identified-actuator-resolution.md`.
  The prior motor torque-constant mismatch is reproduced and corrected. Complete
  explicit derivation provenance and strict resolved robot/sensor/actuator
  contracts remain required.
- Scene-based episodes now retain original robot input presence and metadata,
  distinguish parsed-only input, and carry checked override receipts through
  serialization and replay. Inspection and environment metadata expose the
  original claims separately from edits; see `robot-input-provenance.md`.
  This boundary evidence does not recover upstream-lost CAD metadata or prove
  physical validity. Complete resolved contracts and CAD promotion remain pending.
