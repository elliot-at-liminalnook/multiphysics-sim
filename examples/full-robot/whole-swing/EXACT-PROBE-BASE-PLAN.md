# Reuse the exact base of a tangent Jacobian

Guarded velocity prediction reduces Newton/Jacobian counts but increases total
mapping work and native p95; leave it off. The current tangent-Jacobian builder
unconditionally discards its exact endpoint cache before preparing the base
point. Its preceding residual often prepared that same bitwise-identical point.

Test an optional default-off reuse of that exact cached base. Require existing
exact endpoint reuse and tangent probes. At Jacobian entry, the probe context
must be absent. The ordinary cache key still checks every mechanical unknown
bitwise; a miss performs the existing full mapping. Keep the immutable exact
base motion for constructing probes, then clear geometry/dynamics caches before
probe context starts and again when it ends. Fresh component forces, endpoint
closure, fallback, Newton tolerances, controller inputs and physical definitions
remain unchanged. Record how many bases are reused. Invalidate reused solver
matrices when the option changes.

Require analytic fixed/rotating linkage and contact/rollback tests, actual base
reuse, and identical accepted results. Compare the same 24-second steering task
with the option off/on and the velocity predictor off. Require all physical
gates and bitwise equality of every accepted physical/task frame and recording
(apart from the declared solver option and measured wall time). Profile both
cases sequentially without expensive concurrent work and retain every outcome.
Only after demonstrated savings, build an isolated WASM artifact and require
full fixed-tolerance host parity and exact replay/reset before same-binary
rendered off/on steering and candidate forward/stop timing. Keep original
>=1 pace / <=20 ms p95 gates. This is reuse of exact numerical work, not a
physical approximation or an answer to coarse-timestep accuracy and robustness.
