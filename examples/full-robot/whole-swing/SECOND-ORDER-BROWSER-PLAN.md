# Test a shared second-order stiff integration profile

The learned student now reaches approximately 3.75 mm/s and passes fine
development walking/steering, but its 20/5 ms trajectory difference is about
2.6 mm and both coarse minute endpoints fail. Geometry-cache, display and
transport changes have not supplied sufficient realtime headroom for simply
running the fine timestep. Investigate integration accuracy per solve before
more small presentation optimizations.

Evaluate a default-off BDF2 mechanical integration profile in the shared Rust
embedding runtime, retaining backward Euler as the reference. BDF2 offers a
single implicit endpoint solve per step and second-order accuracy on smooth
problems; contact transitions do not automatically inherit that order. Derive
the residual for reduced coordinates, velocities, floating-body rotations and
contact/auxiliary history explicitly. Do not implement it by changing only a
velocity guess or silently treating an extrapolated state as a physical one.
Respect real simulation time in force callbacks and controller/event sampling.

History must contain accepted physical states only and be transactional across
rejected Newton trials, subdivisions and replay. Start/restart with the original
method where history, step ratio or an event makes the second-order stencil
invalid. Define and test the restart policy before robot evaluation. Keep all
residual, closure, contact-history, CAD/actuator and task limits unchanged.
Default-off serialization and trajectories must remain exact.

Require analytic constant-force and damped/stiff-motion cases, observed
second-order convergence for smooth motion, floating closed-linkage closure,
contact dissipation/bounded transients, event timing, failed-trial rollback and
configuration invalidation. BDF2 is not an exact energy-conserving integrator;
measure the relevant energy error instead of claiming conservation by design.
Use existing shared components and the same native/WASM execution contract.

Only after these checks, run predeclared 20/10/5 ms teacher and student cases
with the original full-trajectory and task budgets. Retain failures and compare
actual solves/time against backward Euler. A profile needs fixed native/WASM
parity, exact reset/replay, supported walking and measured rendered >=1 pace /
<=20 ms p95 before a realtime claim. The separate revealed support-transition
and held-out robustness failures still require controller/planner work.
