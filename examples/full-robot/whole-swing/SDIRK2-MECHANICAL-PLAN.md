# Two-stage held-command mechanical experiment

The scalar held-input screen rejects the proposed BDF2 shortcut for 20 ms
control/physics alignment. At decay rate 50/s, maximum error is 0.01968 for
backward Euler, 0.02962 for BDF2 retaining history across updates, and 0.01968
for BDF2 restarted each update. SDIRK2 improves that case to 0.002401, but at
500/s its 0.02284 error is worse than backward Euler's 0.01057. These are
analytic scalar tests, not robot accuracy claims. The stiff overshoot remains
a risk to test; second order is not an unconditional improvement.

Use gamma = 1 - 1/sqrt(2). Stage one solves Y1 = y0 + gamma*h*f1. Stage two
uses the affine anchor y0 + (1-gamma)*h*f1 and solves Y2 = anchor + gamma*h*f2.
The shared vector primitive uses the existing residual, Jacobian and Newton
acceptance code. The mechanical adapter applies these same weights to reduced
position/velocity and contact memory. Floating orientations compose world-frame
exponential increments; the difference of the two stage angular velocities is
O(h) in smooth motion, so the neglected commutator is O(h^3). Both stage forces
see their exact physical evaluation times. The anchor is never reported as an
accepted physical state.

Only pure mechanics is supported initially; detailed coupled auxiliary motor
states reject this option. Keep all CAD properties, solver/history/closure
tolerances and controller sampling unchanged. The default remains backward
Euler, with omitted false serialization. SDIRK2 starts fresh at every macrostep
and each internal solve, with no history across controller updates. Existing
hybrid subdivision retries a whole two-stage macrostep atomically. Record both
stage solves as work, without counting them as two physical control intervals.

Require analytic acceleration and oscillator convergence, bounded stiff decay,
independent sliding-contact force/history equations, fixed/floating closed
linkages, exact force clocks, invalid configuration and failed-stage rollback.
Run the existing backward-Euler regression suite unchanged. Then evaluate the
fixed fine teacher and fitted student on the same 24-second steering commands
at 20/10/5 ms, plus a rebuilt default-off 20 ms student identity reference.
Preserve every outcome before considering a minute, browser or promotion test.

The existing supported-swing/stop/tilt/heading/collision gates and 1 mm foot /
0.5 mm body trajectory screens remain fixed. No realtime claim follows from a
native result; the original browser pace and p95 gates still apply. This work
does not solve the separate revealed support-transition or robustness failures.
