# Exact periodic boundaries and first cycle searches

This extends commit `01073c4`. The shared planner can now optimize a translating
periodic orbit, including its starting pose. Three retained searches close their
boundary exactly but fail physical planning tolerances and slip quality. No new
controller run, browser gait, validated speed or physical speed maximum.

## Boundary formulation

`ContactImplicitConfig.periodic_horizontal_translation` defaults to false,
preserving the existing fixed-initial-state startup problem. When true, the
caller must supply an empty `initial_velocity`; a supplied velocity is rejected
rather than ignored. The final pose must equal the first in height, orientation
and every independent joint coordinate. Only XY translation differs. Initial
spatial velocity is the final backward-difference velocity, including the same
rotation-vector mapping as every other knot. This closes the discrete state
across the cycle seam on the translation-invariant flat world.

All unique poses are optimization variables. For K intervals and reduced pose
dimension n, the optimization vector has K*n pose values followed by dx and dy.
The repeated endpoint is reconstructed from the first pose plus [dx,dy,0,...].
`bounds` therefore has K full n-entry rows and a final two-entry displacement
row. Any spatial gauge must be fixed explicitly by the caller's bounds; these
experiments fix the first XY coordinates. Height, orientation and joint closure
are structural, not weighted penalties. The first physical pose is free to move
within the authored experimental bounds. This permits gait discovery without a
particular startup pose or per-foot contact sequence.

The cache keys include the preceding spatial velocity, so changing the last
unique knot correctly invalidates the first frame across the periodic seam.
`periodic_boundary` in the report records the displacement and derived initial
velocity. Exact closure does not prove a stable orbit, viable entry from rest,
or continuous/interpolated feasibility.

Nine contact-implicit tests pass. New analytic tests check boundary-velocity
closure, dependence across the seam, exact rejection of a tiny closure error,
identical inverse dynamics over two translated repetitions, and optimization of
the first pose to reach gravity balance. A fixed at-rest initialization produces
a different, correctly nonzero initial force requirement in that same case.
The retained .168 startup path's complete uncached audit is bit-for-bit unchanged
(`periodic-startup-regression.audit.json`).

The existing `compile_contact_implicit` diagnostic deliberately requires a fixed
at-rest start and now explicitly rejects periodic inputs. Its rejection is
verified in `periodic-at-rest-rejection.log`. The runtime currently resets initial
coordinates at zero reduced velocity; executing a periodic orbit requires an
explicit moving-state diagnostic or a separately verified entry controller.
There is no silent substitution of zero velocity for the planned boundary state.

## Matched searches

`prepare_periodic_trials.mjs` generates two guesses for the same problem:
24 unique poses, .3960874905 s period, 432 free variables after fixing the XY
gauge, and the existing .25 m/s +45-degree velocity target. Body/joint bounds,
resolved material properties, actuator envelopes, work cost and tolerances are
retained. The free displacement box is an experimental search domain, not a
physical speed bound. All twelve actuators remain free in every unique pose.

The uniform guess has constant joint positions and translating body coordinates.
The perturbed guess adds deterministic smooth numerical noise to every actuator
coordinate: xorshift seed271828183, three Fourier harmonics, amplitude bounded by
1% of its software-bound width. No leg phase, stance flag, touchdown order or
swing curve is supplied. This breaks time symmetry only in the initial guess;
all knots subsequently remain free. Both initial guesses have zero sampled
interlink overlap; the perturbed guess penetrates the floor .652 mm and has
unbalanced forces, explicitly an optimization seed rather than a valid motion.

Both receive the same four contact-continuation stages: stiffness2k/10k/50k/200k
N/m per point and smoothing3/1/.1/.01 mm, 40 iterations each. The lower-cost
perturbed solution then receives100 more iterations at the final physical model.
No physical acceptance criterion is relaxed.

| Final result | Uniform | Perturbed | Perturbed refinement |
| --- | ---: | ---: | ---: |
| Planned displacement rate, m/s | .243885 | .250670 | .250676 |
| Maximum force error, N | 4.48041 | 1.46622 | 1.44434 |
| Maximum moment error, Nm | .501264 | .838626 | .826844 |
| Minimum torque margin, Nm | -.016144 | -.007659 | -.007500 |
| Loaded slip / body path | 196.11% | 107.09% | 107.09% |
| Final least-squares cost | 1739.24 | 136.882 | 131.211 |
| Sampled interlink overlap | 0 | 0 | 0 |

All fail the declared .05 N / .02 Nm / .001 Nm force, moment and torque gates.
All fail the development5% slip criterion. All terminate at their iteration
budgets, not a demonstrated physical limit. Each independent uncached audit
exactly matches the corresponding final planner report. Each full CAD geometry
audit samples121 poses; maximum floor penetration is .13042/.17889/.17893 mm.
The higher planned speed is **not** a measured robot speed. The best retained
startup measurement remains .16837 m/s, itself still unqualified for slip.

## Actuator utilization and the next formulation

The model's optimistic sum of motor peak mechanical powers is **48.645 W**,
using sum(stall_torque * no_load_speed / 4). This follows the existing effective
servo's linear motoring envelope; it is not thermal capacity or a global m/s
ceiling. In the infeasible perturbed candidate, individual peak use reaches98.76%
of that motor's envelope. Positive discrete actuator work is5.772 J per cycle,
while signed work is5.286 J and planning sliding work is1.119 J. These endpoint
work sums do not certify continuous energy balance.

Small position ranges do not imply unused torque or power. The refined candidate's
four hips span only .0497–.0707 rad but reverse direction16–18 times per .396 s
cycle. `periodic-motion-variation.json` records their knot-to-knot variation.
These rapid reversals explain how small strokes coexist with high motor loads.
They do not prove those motions are trackable or that the robot has reached a
physical limit. No hardware bandwidth or hidden actuator limit was invented.

Next, use the shared periodic trajectory components to express smooth position,
velocity and acceleration consistently, then check contact/dynamic balance at
additional points between controls and in the detailed runtime. Refine the
trajectory representation to measure approximation error rather than treating
its bandwidth as a new physical limit. Retain exact periodic closure and free
contact patterns. Equality-constrained base balance and the established numerical
derivative diagnostics remain needed. The current derivative-audit example
assumes the startup coordinate layout; share the periodic parameterization before
using that tool on these new optimization variables.

Only an admissible cycle should progress to explicit moving-state tracking,
a verified transition from rest, sustained repetition, slip/collision/timestep
checks, and responsive WASD control. CAD geometry and existing browser bundles
remain untouched. A numerically closed or slowly improving cycle is not goal
completion.
