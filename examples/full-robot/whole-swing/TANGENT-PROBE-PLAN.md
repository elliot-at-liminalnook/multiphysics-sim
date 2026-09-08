# Reduce closure work in numerical derivative probes

After bounded secants, the 20 ms steering profile still takes 26.10 ms active
browser p95. Native derivative assembly uses 4.285 of 7.984 seconds; closure
mapping uses 3.400 seconds across 56,399 calls (these timers overlap).
Small-correction secants reduce Newton iterations but increase fresh Jacobians
from 1,996 to 2,018 and provide no measured speed improvement.

Next test a shared, default-off numerical Jacobian approximation using the
existing closure tangent. At the exact Jacobian base point, prepare one exact
EmbeddedMotion. For each tiny finite-difference velocity perturbation, predict
joint displacement using its tangent times h*delta_u, retain the exact base
quaternion integration, and map perturbed velocities with the frozen tangent.
Recompute inertia, gravity, passive loads, contact geometry, forces and contact
history at each probe. Freeze only the tangent/curvature data inside derivative
construction. This omits their configuration derivatives; it is an approximate
correction matrix, not an accepted physical endpoint or frozen force model.

Every ordinary residual, backtrack, convergence verification and final endpoint
must use the full original closure solve. Probe-only caches must be cleared
before returning from Jacobian construction and cannot enter a committed
workspace or physical observation. A failed approximate solve must restart
from its original state with exact numerical derivatives. Record probe counts
and fallback reasons; invalidate reused matrices when this option changes.

Before any promotion, check finite-difference accuracy on independent analytic
mechanisms, fixed/rotating-base mapping, closure at every accepted endpoint,
analytic oscillator/contact behavior, and transactional failure recovery.
Then compare the exact current 24-second 20 ms steering case off/on with
unchanged tolerances and physical task gates. Require full sampled differences
within 1 mm foot / 0.5 mm body, native/WASM agreement and exact replay/reset.
Only then measure rendered forward/turn/reverse/stop without concurrent heavy
work. Keep active >=1x and p95 <=20 ms. Preserve the detailed and earlier
browser profiles and all failed trials.
