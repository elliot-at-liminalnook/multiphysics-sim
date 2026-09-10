# First decision checkpoint

2026-09-08, before 12:15:36 UTC. Hard review stop remains 12:45:36 UTC.

**A new opposite-pair gait achieved 24.983 mm/s in a short physical trial.**
This is about 20 times the original 1.25 mm/s crawl. It is not a qualified
sustained, learned, steerable or browser-realtime controller yet.

## Unused capability and geometry

The 829-pose coarse screen completed in 0.184 s: 349 poses passed sampled
closure/limits/inter-link geometry, 480 had overlap, no closure solve failed.
Hips were explored to +/-60 degrees, worm inputs to +/-150 degrees, foot
cranks to -140 degrees. Bounds are hypotheses, not hardware certification.
A finer 1-degree scan confirms substantially wider connected sampled hip
intervals for most legs; the -X hip has an asymmetric pulley/chassis overlap
boundary. Gear/shaft overlap appears in several separated worm intervals.
Both same-leg and cross-leg overlaps are retained; none has been waived.
Positive exact CAD clearance and between-sample clearance remain unproven.

At the starting stance, side-leg hips provide about 0.217 m/rad forward
motion; front/rear worm inputs provide about 0.085 m/rad. Retracting all
foot cranks to -60 degrees creates about 54 mm more extension reserve when
the body is lowered accordingly. This allows larger coordinated motions.

CAD-derived mass is 3.976 kg. The crouched whole-robot CoM is about 0.346 m
above the floor. Equal paired support would require about 19.5 N per foot.
The shared Jacobians, CAD inertia aggregation, unilateral/friction bounds
and centroidal momentum equations are recorded in physics-derivation.json.
These are screening estimates, not a force-allocation stability proof.

## Three strategies

| Strategy | Schedule prediction | Evidence |
|---|---:|---|
| Long-stride wave, four-contact body transfer | 12.5 mm/s | Rejected at 0.76 s by sampled gear/pulley overlap |
| Continuous-body-advance wave | 25 mm/s | Rejected at 0.54 s by sampled gear/pulley overlap |
| Alternating opposite-leg pairs | 25 mm/s | Completed 6 s physical trial, including 4.8 s travel and stop |

The paired trial measured 24.983 mm/s over 1.2–5.2 s, maximum heading error
0.000195 rad, tilt 0.00293 rad, stop drift 0.592 mm, and 1.900 J positive
sampled shaft work. All 24 individual lifts passed an exploratory sampled
2 mm clearance / 0.2 N unloading / 1 N stance / 40 ms dwell check. All 301
recorded poses passed independent sampled inter-link geometry. These lift
criteria were declared after the first trial and before follow-up validation.

Worst loaded material slip/body advance was 8.08%, above the prospective 5%
screen. Lower contact smoothing reduced it to 7.10%; this does not qualify.
Halving physics step from 1.25 to 0.625 ms changed speed by only 0.10%.

## Cost and next decision

The 6 s trial took 9.76 s native wall time (0.615x, p95 transition 55.1 ms).
A 2.5 ms step reached 0.932x, p95 45.7 ms; browser realtime is still unproven.
At current cost, a million 50 Hz transitions would take roughly nine hours
on one sequential instance, excluding training. No large learning job started.

Continue bounded work on foot-flight timing, 50/100 mm/s geometric screens,
and an adjusted wave support path. Investigate narrow gear-grid rejections
against CAD before calling them physical travel limits. Then retain exact
recipes, captures and comparisons for review. Teacher learning, student
distillation/robustness, steering, long-horizon validation and browser timing
remain outstanding. Do not promote a short-trial result as full success.
