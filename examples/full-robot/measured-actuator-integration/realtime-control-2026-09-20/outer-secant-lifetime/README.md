# Bounded outer secant lifetime experiment

Status: tests pass; timing and browser comparisons are pending. Selected recipe unchanged.

This follows `../outer-secant-only/`: outer-only good-Broyden updates reduce
mechanical Newton iterations, but some updates request fresh derivatives. The
shared Newton solver now exposes `broyden_max_updates: Option<usize>` and counts
cap-triggered refreshes separately. Omitted means eight, preserving prior
behavior. Zero requests fresh derivatives at every update attempt. Inner motor
secants remain disabled in all experimental recipes.

Only numerical correction proposals change. Real residual evaluation, scaled
correction acceptance, backtracking, finite checks, bounded derivative changes,
singular-update rejection, cold fallback, physical timestep and the original
qualification gates remain in force. The candidates combine the existing
analytic mechanism motion option with caps 8, 16, and 32. A fourth cap-32 candidate
also tests the existing 256-visit matrix lifetime. This interaction is explicitly
recorded rather than attributed to the cap alone.

`protocol.json` pins source snapshots, binary and input hashes. All experiments
use the same three-second command schedule. `run-native.mjs` measures the default
and four candidates sequentially, both unprofiled and profiled. `analyze.mjs`
checks exact default parity, explicit-eight parity against the prior combined
recipe, all saved fields across profiling, controller equality, solver counters,
and unchanged physical/performance gates. Browser runs use a fresh SIMD/LTO
build containing this option; older modules can silently ignore unknown Newton
fields and are not valid for testing the new cap.

Validation so far: 44 shared solver tests and 56 coupled robot/motor/power/servo/
sensor tests pass. New tests cover cap exhaustion, unsafe updates, immutable
cache snapshots, nonlinear roots and default serialization compatibility. The
initial profile-bucket array-length compile error is preserved separately; it
was fixed before passing tests and builds.

These numerical comparisons do not establish timestep convergence or measured
hardware accuracy. The selected BE6400 recipe already fails its refinement
comparison; this experiment does not change or reinterpret that result.
