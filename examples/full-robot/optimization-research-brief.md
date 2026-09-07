# Research prompt: robot solver optimizations

Research published methods that could improve our robot simulator's speed and
numerical reliability. Use primary sources—papers, author implementations and
official library documentation—and link each recommendation to its evidence.

## System and measured bottleneck

- Shared Rust physics library, native and WebAssembly execution, Rust/Rhai
  controllers. Python remains on the CAD side.
- Full robot: 29 rigid links, 28 mechanical DOFs, 12 servos, ideal gear/belt
  transmissions, four closed knee linkages, ground contact, and electrical/thermal
  actuator dynamics. 600 compiled solver unknowns.
- Implicit integration, Newton iterations, mostly numerical Jacobians and
  step subdivision on convergence failure. Nominal timestep: 0.5 ms.
- Simulating 20 ms initially took about 20 seconds. Jacobian assembly consumed
  93%; factorization 2.6%; controller callbacks were negligible.
- Ordered parallel derivative columns and dependency-checked contact reuse
  reduced this to 3.9 seconds with an identical final state.
- Experimental recognition of structurally redundant knee angular constraints
  reduced this to 1.6 seconds: 250 versus 470 fresh Jacobians, three versus nine
  subdivisions. It remains opt-in. At the original timestep, independent
  derivative paths disagreed by about 0.9 N of final foot-contact force.
- At 0.125 ms, both derivative paths agreed on all 12,638 checked values over
  20 ms, with zero subdivisions. This is equal-step agreement, not proof of
  timestep convergence, long-trajectory accuracy or real-world fidelity.
- Analytic/hybrid derivatives remain experimental; an earlier full-robot run
  stalled after 2 ms. Faster Jacobian construction alone is insufficient.

## Investigation

1. Efficient derivatives of constrained articulated dynamics and contact.
2. Constraint redundancy, conditioning and stable closed-loop formulations.
3. Newton convergence, Jacobian reuse, preconditioning and timestep/error control.
4. Contact/geometry reuse and parallel approaches suitable for Rust and WASM.

Return a ranked shortlist explaining each method in plain language, why it fits
our measured bottleneck, implementation difficulty, limitations, and a concrete
experiment to validate it. Distinguish published speedups from estimates for our
system. Identify measurements needed before recommending an architectural change.

Prioritize reusable library improvements that preserve the modeled physics.
Require trajectory accuracy, timestep convergence, constraint closure, and total
wall-time checks—not merely faster derivative assembly. Distinguish derivative
errors from divergence caused by different adaptive subdivision/event histories.
Do not propose removing contact or changing physical compliance to pass a speed
benchmark without explicitly treating that as a different physical model.

Implementation pointers, if repository access is available:
`crates/sim-compile/src/island.rs` (component differentiation),
`crates/sim-domain-robot/src/articulated/` (prepared contact, hybrid derivatives,
structural identities), `crates/sim-dynamics/src/lib.rs` (implicit stages/events),
`crates/sim-solve/src/lib.rs` (Newton), and
`crates/sim-runtime/src/validation.rs` (independent trajectory comparisons).
