# Bounded secants in the shared Newton solver

Native profiling of the existing 20 ms steering student took 10.064 seconds:
6.211 seconds in 2,908 Jacobian assemblies, versus 0.631 seconds in online
planning and 0.037 seconds in factorization. Nested closure/dynamics timers
must not be added to the total. Scalar and SIMD/LTO browser p95 remained
32.9–37.4 ms, above the unchanged 20 ms target.

Test an opt-in shared solver update, not a robot-specific equation change.
For dense systems up to 64 unknowns, decreasing full steps supply scaled
good-Broyden secants. Rebuild after eight updates, tiny/nonfinite secants,
singular factors, or a scaled update greater than twice the matrix norm.
Failed full steps retain the fresh-Jacobian retry. Keep raw residual bounds,
correction bounds, line search, timestep, iteration cap and physics unchanged.
Default serialization and behavior remain unchanged.

First compare the same 24-second 20 ms steering student with the flag off/on,
seed 0 and identical actions. Require independent physical task acceptance
and compare entire sampled trajectories within the existing 1 mm foot and
0.5 mm body budgets. Report raw solver costs and residual acceptance tests.
Only proceed to browser timing after native/WASM parity, exact replay/reset
and physical checks pass. Browser targets remain active >=1 simulated second
per wall second and p95 <=20 ms, measured without concurrent heavy work.
Compiler profile must be identical between solver comparisons.

This experiment addresses computation. It cannot by itself resolve timestep
accuracy, model calibration, terrain robustness or untested steering inputs.
