# Realtime formulation research — 2026-09-06

Research findings, not a demonstrated speedup for this project. No engine,
contact law, actuator model, or numerical tolerance was changed by this review.

## Evidence relevant to this robot

- **Local constraint embedding:** Chignoli et al. (2024 revision) address geared
  motors, differential drives and four-bars using recursive dynamics over groups
  of bodies. [Paper](https://arxiv.org/html/2311.13732v2).
  Their [C++ implementation](https://github.com/ROAM-Lab-ND/generalized_rbda)
  provides a useful independent reference; it is MIT licensed.
- **General loop-aware dynamics:** Sathya and Carpentier's LCABA/proxBBO work
  handles internal loops and singular cases. Its reported greater-than-sixfold
  gains concern selected high-DOF dynamics benchmarks, not our implicit
  electromechanical timestep. [Author page](https://simple-robotics.github.io/publications/lcaba/).
- **GPU closed-loop learning:** Kamino demonstrates training a robot with 12
  actuated joints and six loops using 4096 environments. This is batched
  throughput evidence; it does not prove single-robot latency or our physical
  fidelity. [2026 paper](https://arxiv.org/html/2603.16536v1).
  The [current implementation](https://newton-physics.github.io/newton/stable/api/_generated/newton.solvers.SolverKamino.html)
  is an experimental/beta Newton solver.
- **Contact convergence:** Drake's Irrotational Contact Fields introduces convex
  approximations with interactive demonstrations and reusable derivative
  factorizations. They change the contact formulation and have documented
  approximation artifacts. [Paper](https://arxiv.org/html/2312.03908v3).
  Drake's [discrete-model documentation](https://drake.mit.edu/doxygen_cxx/group__mbp__discrete.html)
  also states that its velocity-level discrete models ignore the static friction
  coefficient. They cannot silently replace our bristle-state friction law.
- **Stiff actuator integration:** MuJoCo's
  [computation documentation](https://mujoco.readthedocs.io/en/latest/computation.html)
  describes structured velocity-implicit integration and a discrete effective
  inertia incorporating damping and positive stiffness. These are alternatives
  to repeatedly solving every coupled state with a generic Newton method.

## Proposed next experiment (engineering assessment)

Prioritize a shared Rust representation of local drivetrain constraints, with
the existing CAD model supplying its physical parameters. First address the
ideal transmission coordinates; then the closed knee. The worm drive already
uses a 5:1 transmission equation, so removing tooth geometry would not remove
the measured derivative work. Derive dependent motions instead of introducing
independent unknowns plus equations to enforce their relationship.

Retain full link motion reconstruction, motor/rotor inertia and load transfer.
Any reduction must explicitly identify which existing compliance, backlash,
friction, electrical and thermal states remain. Do not treat a fitted PD motor
or a new contact law as equation-preserving reduction. Singular poses and
configuration branches need explicit validity checks and a safe fallback.

Validate in stages: imposed torques without contact; loaded joints; floor
contact; then all four legs. Compare accelerations and physical reactions at
saved states, full position/velocity closure, motor load/current, complete
trajectories, contact impulses and timestep sensitivity. Establish a refined
reference instead of assuming the present baseline is accurate. Measure total
runtime and rejected work at matched physical error. Only then benchmark GPU
batch execution separately from single-environment latency and browser costs.

This is a change in formulation to investigate, not an authorization to replace
the Rust runtime with a Python simulator. External implementations serve as
references; reusable algorithms belong in the shared Rust library. Existing
analytic derivative flags remain experimental until their trajectory and
runtime gates pass.

## Local eligibility evidence

Audited all 240 committed stages of the existing 100 ms hold capture. The eight
transmission rows use phi_ddot + 200 phi_dot + 10000 phi + 1e-6 lambda = 0.
Maximum angle mismatch is 4.70064e-11 rad, but the reaction-dependent term reaches
1.54014e-6 rad/s². Thus the geometric ratio is extremely close on this recording,
yet deleting its regularization term changes the discrete equations. An ideal
coordinate map must be compared explicitly rather than labeled an exact
reformulation of the current model. No new physics, solver option, or runtime
optimization was introduced in this audit. Reproduction and measured rows are
in `runs/full-robot/solver-performance/transmission-regularization-audit.py/.json`.
This hold-only evidence does not establish behavior under commanded motion.

## First implementation checkpoint

The shared Rust `articulated::transmission_coordinates` module now implements
constant-ratio coordinate projection and reaction recovery for a transmission
forest. `cargo run --release -p sim-runtime --example compare_transmission_block
-- CAPTURE.json all 100` compares every committed saved mechanical state with an
independently assembled full acceleration/constraint system. This is a building
block for embedding, not an implementation of the paper's recursive algorithm.

On the existing 240-stage hold fixture, dimension falls 62→46 and mean solve
time falls 33.55→27.33 microseconds. Both references use ideal transmissions:
finite production transmission regularization is not silently removed. The
default coupled timestep is unchanged. Prepared contact evaluation reproduces
all block results exactly while reducing diagnostic mass-assembly time from
3.606 to 1.243 ms in separate preliminary runs. Assembly still dominates this
experiment. Direct recursive dynamics, nonlinear knee maps, moving trajectories,
contact impulses, and matched-accuracy total runtime remain validation work.

## Direct rigid inertia checkpoint

`Articulated::rigid_mass_matrix` now constructs the rigid mechanical inertia
from link motion maps, preserving coupling instead of repeatedly probing
accelerations through loaded inverse dynamics. It supports the constructor's
joint parameterizations, including ball and compliant fixed joints, with
multiple grounded/floating bases. Modal flexibility is rejected explicitly.
This does not implement recursive ABA or replace the coupled integrator.

Analytic pendulum, inverse-dynamics and kinetic-energy tests pass in native and
no-default-feature builds; WASM compilation passes. Across the same 240 saved
states, direct and loaded-probe mass entries differ by at most 1.38293e-13.
The projected direct-mass solution differs from the full probe-mass ideal
solution by at most 2.28065e-8 in mixed acceleration units. Ten alternating
timing pairs per state average 492.45 µs for prepared loaded probes and 23.43 µs
for direct construction (21.0× for mass construction only). Default timestep
behavior and analytic flags are unchanged. Experimental derivative integration
and full trajectory/contact-impulse comparisons remain the next gates.

## Integration gate: convergence still fails

A prototype substituted direct inertia and link acceleration maps into the
existing experimental rate-derivative hook. Independent loaded rigid/contact
and loop-reaction checks passed, while modal models retained the old evaluator.
However, neither the old nor the new rate-partial path completed the 100 ms
hold replay at 0.5 ms nominal steps: the old path failed near 35.6 ms and the
candidate near 34.0 ms, with hundreds of subdivisions. Their contact sets also
differ at the common 34 ms snapshot. The integration was removed and the new
loaded rigid-rate regression retained. Direct mass construction remains an
available building block; its isolated speedup has not earned timestep
promotion. Investigate convergence at identical contact-transition states next.
