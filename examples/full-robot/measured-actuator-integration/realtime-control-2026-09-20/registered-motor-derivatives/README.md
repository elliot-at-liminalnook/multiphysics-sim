# Registered motor derivative proposals

This opt-in experiment adds `supplied_auxiliary_jacobian` for condensed
component solves. Motor state/rate partials come from the registered
`Behavior::jacobian`. Independent motor boundary adapters supply voltage and
temperature through their existing functions; grouped numerical probes obtain
the boundary chain rule. State-coordinate and rate-coordinate factors are
explicit, including the SDIRK stage duration. Complete coupled residuals,
correction bounds, event scheduling and closure checks remain unchanged.

Unknown/shared-power adapters and components declining derivatives retain
numerical Jacobians. Invalid or unsuccessful supplied matrices cause a cold
retry with original numerical derivatives; no tolerance is relaxed. The
embedded runtime now honors the existing scene `analytic_motor_jacobian`
option instead of hardcoding it off. That numerical selection never changes
the residual law or CAD properties.

70 tests pass: 31 robot library, 20 embedded-motor, 19 embedded-step. Tests
include nonlinear residual/derivative comparisons in both directions, winding
and rotor differential/algebraic variants, nonlinear voltage/temperature
chains, two motors with real driver foldback, BE/SDIRK and state/rate
coordinates, unknown-adapter fallback, and cold recovery from invalid/singular
proposals. The WASM runtime compiles. Earlier fixture/annotation failures remain
in their logs; test corrections are recorded in `protocol.json`.

Three-second paired times: prior binary 27.392042 s, current numerical path
27.240909 s, supplied path 28.997610 s. The new route is **slower** (0.9394x)
and remains disabled. It passes numerical agreement: peak motor-angle change
1.13e-14 rad, link-origin change 3.00e-15 m, winding-current change 1.08e-13 A,
and contact-force change 1.32e-10 N, with equal contact identities and clocks.
The disabled path matches all 151 original physical frames exactly.
Profiled/unprofiled candidate frames also match exactly.

Profile: 20390 supplied matrices are actually used. Component calls fall from
835596 to 773878, but derivative proposals add 0.613 s; total profiled time is
28.943518 s. The 7.4% component-call reduction does not produce a throughput
win. Matrix count and physical agreement do not justify selecting this route. Profile counters distinguish supplied matrices
from component residual calls and boundary-derivative preparation.
The old binary rejects the newly introduced configuration key even when false;
`numeric/config.json` therefore omits the key (its default is false). The
initial pre-simulation rejection is preserved as `numeric/unsupported-flag.*`.
