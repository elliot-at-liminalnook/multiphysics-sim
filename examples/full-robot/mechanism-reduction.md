# Independent-coordinate mechanism experiment

`sim-domain-robot::articulated::embedding::RigidEmbedding` implements a local
motion map and an instantaneous rigid dynamics solve for CAD-derived closed
mechanisms. It is a shared library building block for the fast training model,
not yet a complete simulation backend or a new viewer execution path.

The formulation uses independent-coordinate kinematics described in equations
2–4 of [Chignoli et al.](https://arxiv.org/html/2311.13732v2). This implementation
is a small dense coordinate projection, **not** the paper's recursive
constraint-embedding ABA, and does not inherit its published performance claims.

## What is preserved and what changes

The caller explicitly selects independent compiled joint coordinate names.
All other joint coordinates are solved from every original geometric loop and
transmission equation. Rank-checked least squares handles redundant equations;
no pose-local row selection deletes a constraint. A full-rank dependent block
and consistency of all original rows are required. Unit-velocity kinematics
probes supply derivatives without subtracting nearby position evaluations.

The map supplies:

- Full joint positions from nearby branch seeds and independent joint positions.
- Full velocity `v = T u`, retaining free-base world linear/angular motion.
- Full acceleration `a = T udot + bias`, including the acceleration caused by
  moving through a nonlinear linkage at constant independent speed.
- Effective inertia `Tᵀ M T`, using every original rigid link's mass, COM and
  inertia, including cross-coupling and asymmetric source masses.
- Instantaneous acceleration from applied generalized loads plus the existing
  rigid evaluator's gravity, velocity-dependent, contact and passive loads.

This deliberately replaces the detailed evaluator's stabilized/CFM closure
equations with **ideal geometric closure** in the experimental calculation.
The detailed simulator and its defaults are untouched. Full-trajectory accuracy
must decide whether that reduction is suitable for the training task.

Limits are explicit: one connected base, rigid links, finite inputs, and a local
nonsingular coordinate chart. Modal flexibility is rejected rather than silently
discarded. Multiple assembly branches are possible; a nearby seed and small
command increments are needed. A successful local solve does not prove branch
continuity across an arbitrary jump, global reachability, collision clearance,
or compliance with joint/actuator travel limits. The caller must check those.

The instantaneous dynamics function does not integrate time or contact history.
Separate motor rotor/gearbox inertia, electrical/thermal states, servo firmware,
latency, backlash and sensors are not introduced or removed by this helper;
they still need explicit coupling in the fast backend. Ideal transmission closure
does not establish worm self-locking, efficiency or belt elasticity. All source
calibration uncertainties remain relevant.

## Verification and reproduction

```sh
cargo test --locked -p sim-domain-robot --test embedding
cargo test --locked -p sim-domain-robot --no-default-features --test embedding
cargo run --locked --release -p sim-runtime --example audit_embedding -- \
  runs/full-robot/floating.scene.json 41 > runs/full-robot/embedding-audit.json
```

Generate the scene from the versioned CAD with the export command in the parent
README. The diagnostic identifies motor coordinates through their declared CAD
joint bindings, prescribing at most 0.05 rad sinusoidal amplitudes over 41 samples
of a one-second parameterized sweep. It does not advance a one-second physics
episode or execute motor commands. It reports all original closure rows and
link poses, projected inertia positivity, and a separate instantaneous dynamics
check with zero externally applied generalized loads (gravity/contact/passive
loads still apply). The original inverse dynamics independently verifies the
resulting projected force balance.

Five tests cover an 81-pose slider-crank sweep against closed-form geometry,
velocity and curvature; floating-base inertia against inverse dynamics;
invalid/inconsistent coordinate selections; rank loss at a geometric toggle;
and loaded accelerations against an independent full KKT/SVD solve retaining
every original constraint row. Both native feature configurations pass. CI
includes the tests and checks compilation of the diagnostic.

The initial revision-1357 robot kinematic sweep reduced 34 full mechanical
velocity coordinates to 18 (six free-base plus twelve motors), retaining 29
links. All 41 small-sweep configurations closed successfully. This establishes
a usable local motion map; it does not establish walking, timestep accuracy,
calibration, or realtime performance.

The full-robot instantaneous dynamics check also passes at all 41 configurations,
with maximum projected force-balance discrepancy 2.13e-14 against original inverse
dynamics. The shared runtime compiles for WASM. Single-run kernel timings are
diagnostic only; neither timing a prescribed pose sweep nor an instantaneous
acceleration solve establishes simulated-seconds-per-wall-second throughput.

An experimental [time integrator](embedded-integration.md) now advances the
mechanical states and contact history. Smooth-case tests pass; the original
robot's stiff dissipative terms are not yet stable under its explicit update.
Next: couple a documented effective actuator model, handle stiff forces with a
stable integration scheme, and compare complete
motions with the detailed and timestep-refined references using the task gates.
Then implement planner-guided learning on the same runtime contract.
