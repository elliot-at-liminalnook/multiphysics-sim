# Motor evaluation storage

The shared motor adapter now fills each motor's residual slice directly in
the already allocated result. A force-only entry point omits creation of the
unused telemetry vector, while evaluating and validating all registered outputs.
The full telemetry API remains available; no motor or control law changed.

38 release tests and the wasm32 no-default-feature compile check pass. Before and after whole-robot captures agree exactly in
all 151 physical frames, excluding the observation wall clock; profiled and
unprofiled candidate frames match exactly too. Paired native timings are
28.222062 s before and 27.295874 s after, a 1.03393x ratio for three simulated
seconds. This small single-pair gain is below the unchanged 1.2x acceptance
gate, so it is not a newly qualified performance profile. Realtime still fails
at 0.10991x with 218.09 ms p95 policy transitions.

The current browser bundle was built before this storage change. Its separate
SIMD/LTO verification is recorded in `../shared-rigid-kinematics/`; do not claim
that the browser timing includes this final native change.
