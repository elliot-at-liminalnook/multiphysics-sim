# Analytic smooth periodic searches

The shared planner now evaluates a C2 periodic cubic B-spline plus horizontal
drift, with analytic velocities and accelerations. Three completed searches
produce smoother, larger hip strokes but **no qualified gait or speed gain**.
All fail force balance, slip and sampled interlink geometry. No controller was
compiled or run; existing browser bundles remain untouched.

## Shared formulation and verification

`sim-domain-control::periodic_drift` wraps the existing periodic trajectory with
an explicit displacement per cycle. The planner's optional
`periodic_cubic_subdivisions` interprets position rows as controls, removes their
linear XY drift, and uses that shared curve for inverse dynamics and geometry.
Rotation-vector derivatives map to spatial angular velocity and acceleration
through the existing robot math. The periodic seam closes velocity and
acceleration; height, rotation and joint controls close exactly. None of this
specifies foot phases or contact timing.

One shared trajectory test checks derivative closure and finite-difference
agreement across knots/seams. Eleven contact-planning tests pass, including
analytic Newton/Euler balance, agreement at shared times after grid refinement,
and exact cached/uncached evaluation for all control probes. The old startup and
discrete-periodic robot audits reproduce their complete previous JSON exactly.
Tests and pre-experiment source/binary identities are retained as `smooth8-*`.

## Matched experiments

All three use eight unique controls, 32 collocation samples, the same .39608749 s
period, .25 m/s +45-degree target, resolved CAD/contact parameters, motor limits,
objective scales and acceptance gates. Each runs four stiffness/smoothing
continuation stages with 40 iterations each. No hardware limit is inferred from
this numerical basis. All terminate at iteration limits.

Uniform and perturbed seeds downsample the earlier 24-control numerical seeds.
The third uses actual recorded `.21` runtime poses from t=1.5 s for one cycle.
The Rust sampler converts root rotations relative to the same CAD base and
selects the recorded independent joints. Linear interpolation and subtraction
of endpoint drift in non-XY coordinates create a closed numerical seed. These
points become spline controls, so this is not exact replay or a feasible motion
claim. The capture has constant forward commands throughout that interval.
CAD identity, floor, coordinate names/units and strict bounds are checked by
`prepare_recorded_periodic_trial.mjs`; regeneration reproduces all optimizer
inputs exactly. A 6.94e-18 m recentering roundoff initially violated the fixed XY
gauge; exact assignment fixes it without relaxing bounds. The rejection is kept.

Independent uncached audits reproduce every final report. Four-times-denser
collocation samples the **same analytic curve**, and all physics fields at shared
times are bit-for-bit equal. The additional times expose missed peaks:

| Seed | Planned travel rate m/s | Force error, 32 / 128 samples N | Moment error, 128 samples Nm | Torque margin, 128 samples Nm | Loaded slip / body path |
| --- | ---: | ---: | ---: | ---: | ---: |
| Uniform | .099845 | 1.430 / 5.821 | 1.930 | -.026585 | 227.9% |
| Perturbed | .120758 | 11.688 / 11.688 | 3.265 | -.274823 | 185.5% |
| Recorded gait | .201908 | 2.771 / 5.142 | 1.827 | -.041335 | 110.9% |

Declared gates remain .05 N, .02 Nm and .001 Nm torque violation. Slip values
are planning diagnostics at 32 samples, not measured runtime slip. Full CAD
geometry at 129 poses finds maximum leg overlaps of 31.48 / 24.33 / 14.02 µm.
Floor penetration is .111 / .121 / .112 mm. Sampled geometry is not continuous
collision certification. `smooth8-trials.summary.json` records the exact values.

Hip strokes now span .235–.368 rad with 2–6 sampled velocity sign changes per
cycle, versus the prior discrete candidate's .050–.071 rad and 16–18 knot
reversals. This addresses the previous tiny-stroke numerical behavior, but the
force and slip failures prevent any walking-performance claim.

## Next decisions

The eight-control basis has 144 free variables after fixing the XY gauge and
adding net displacement, but 32 collocation times already impose 192 base-wrench
components. That count does not prove infeasibility; it does show why simply
enforcing more balance equations on this fixed small basis is not enough.
Refine the spline basis while preserving the existing curve, then improve
constrained base-balance optimization and numerical derivatives. Keep independent
denser audits: a low coarse-grid objective is not continuous feasibility.

This is still weighted-penalty Gauss–Newton, not IDTO's equality-constrained
dogleg solver or online MPC. No finite search failure proves a physical speed
ceiling. The optimistic modeled motor power sum remains 48.645 W; it does not
establish a global speed bound. Stable repetition, explicit orbit entry,
collision/slip checks and responsive WASD runtime validation remain required.

The recorded input is durably covered by the existing contact-planning runtime
archives (v1–v4); source CAD/scene by the earlier speed-ceiling archives. This
turn adds planning evidence only, so contact-implicit runtime archive v3 remains
unchanged. Direct evidence v7 covers the new source and results.
