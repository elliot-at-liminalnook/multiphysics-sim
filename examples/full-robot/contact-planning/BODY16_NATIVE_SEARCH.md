# Sixteen-control joint body search

This experiment doubles the body spline's independent controls from eight to
sixteen using the existing shared exact knot-insertion implementation. It starts
from the completed timing-aware AL motion, as the eight-control warm Ipopt
comparison does. Body-value/rate/acceleration errors over 4,097 samples are
2.78e-17, 2.22e-16 and 6.99e-15 respectively (channels retain their original
metre/radian units). No gait phase, foot path, force coefficient, actuator,
physical tolerance, direction, speed objective or channel bound is changed.

There are now 491 variables: 125 motion variables (96 body controls and 29 other
motion decisions) plus 366 force coefficients. The target remains the existing
experimental 0.30 m/s value, not a demonstrated physical maximum. The body curve
can evolve more freely during optimization; it is not manually prescribed to
a new shape.

All 250 original CAD frames are retained. Their maximum normalized physical
residual difference is 2.22e-12. Sixteen extra body-knot frames increase the
mesh to 266 frames and 7,192 inequalities. These additional checks expose
force error of 0.21247341 N and moment error of 0.03011423 Nm in the unchanged
starting motion, compared with 0.07916036 N and 0.02406937 Nm on the old mesh.
This is newly observed between-sample error, not a changed trajectory or a
relaxed acceptance rule. The initial motion remains infeasible.

The existing body-refinement preparer now accepts source/audit/output arguments
and validates original and new control counts, unchanged non-body recipe fields,
per-channel search bounds, curve fidelity and preservation of old frame checks.
Its original four-to-eight-control recipe and initial-report outputs replay
byte-identically. The new sixteen-control recipe also replays byte-identically
after the independent channel-bound checks.

Grouped and ordinary native one-iteration pilots use the same refined recipe,
initial bound distances of 1e-8 and full physical acceptance. The grouped
calculation must reproduce the ordinary native result before it is used for
a longer speed search. Its expected probe count remains 48 body probes per
Jacobian; ordinary body probes rise to 192. Native structural sparsity remains
unchanged between these two pilots. No claim of overall wall-time speedup,
feasible gait, physical ceiling or browser qualification follows from refinement.

## Completed pilots and live search

The grouped and ordinary pilots reproduce the same native search result,
returned candidate and complete final CAD report after signed-zero
canonicalization. The native final objective and all 7,192 inequalities match
the independent uncached audit. Grouping uses **220 model attempts versus 508**
for ordinary derivatives, with 96 grouped probes across two Jacobian requests
and no fallbacks. The permanent Jacobian has 2,033,504 declared entries in both
pilots (3,531,272 if dense). This is a measured reduction in model attempts,
not a claimed overall wall-time speedup.

The pilot ends at 0.02603803 m/s, force error 0.21058861 N, moment error
0.02994923 Nm and torque margin +0.68204939 Nm. Maximum normalized inequality
is 3.21177212. Both pilots remain infeasible despite increasing speed and
slightly reducing balance error relative to their starting point.

The longer sixteen-control search is now launched with grouped derivatives,
8,000 model attempts, 100 native iterations and 1,000 callbacks. Its speed
target, robot, physical bounds and acceptance thresholds are unchanged. The
additional collocation checks remain enabled. The eight-control warm search
continues independently with its unchanged executable. No candidate has been
promoted to a controller or browser gait.

## Completed native search and force repair

The full run has now exhausted 8,000 model attempts with native status -13
(callback budget error), 35 derivative callbacks and no feasible candidate.
The independently audited final reference is 0.025224397 m/s, with force error
0.045422063 N, moment error 0.015216872 Nm, torque margin +1.270958066 Nm and
cone violation 1.071195961 N. See `joint-body16-speed.summary.json`.
The [shared conic follow-up](CONIC_PATTERN_SCREEN.md) repairs the forces on
this mesh, but its dense audit stops at a bounded inverse-kinematics failure.
Neither solver termination establishes physical infeasibility or a speed ceiling.

The [bounded domain recovery](JOINT_DOMAIN_RESTORATION.md) subsequently finds
a 2,266-frame physical pass after a small algorithmic retreat. A finer audit
still rejects it at a joint bound, so it remains an unqualified initializer.

The combined 10,266-sample recovery now passes the prior planning gates. Its
[runtime command rejection](SERVO_COMMAND_CONSTRAINTS.md) identifies an additional
actuator-command constraint that is being included in the joint speed search.
