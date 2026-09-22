# Direct projected rigid inertia experiment

Opt-in `EmbeddingConfig.direct_projected_inertia` constructs the reduced inertia
from per-link velocity maps, including every nonzero coupling. It also shares
one exact kinematics evaluation with the existing passive/contact force kernel.
The original full mass matrix followed by projection remains the default and
independent reference. No timestep, state, force law, control clock, solver
acceptance tolerance, or CAD physical parameter changes.

The public shared-library `Articulated::rigid_projected_mass_matrix` accepts any
finite dimension-matched tangent, including dense and rank-deficient maps. It
rejects unsupported modal flexibility. Positive-definiteness and original closure
checks remain the responsibility of the existing constrained dynamics caller.

`protocol.json` records source and input hashes. `before-native.bin` preserves
the pre-change executable. `../warm-probes/inertia-before.native.json` is the
fresh sequential pre-change baseline. Any `concurrent-functional` captures are
for correctness only because compilation overlapped the run.

Tests compare against full-coordinate inertia, independent inverse-dynamics
probes, analytic pendulum inertia, kinetic energy, constrained KKT solves, and
fresh contact forces/history. Full robot qualification is recorded separately.

## Screening result

The uncontented native run completed 3 simulated seconds in 30.013 s, compared
with 30.587 s for the immediately preceding warm/probe baseline (1.019x).
This fails the unchanged 1.2x incremental speedup gate. The path is unselected.
Physical comparison passes: motor angle 1.28e-14 rad, link origins 5.90e-15 m,
current 1.22e-13 A, contact force 9.59e-11 N, identical contact identities and
controller counters. Profiling preserves all physical frame fields exactly;
profile metadata itself differs as expected. See `candidate.qualification.json`
and `profile-parity.json`. Browser timing was not run for this rejected candidate.
