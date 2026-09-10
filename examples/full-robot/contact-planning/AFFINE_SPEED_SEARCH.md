# Expanded actuator-reference search

The five-minute baseline is the original teacher: 154.3012690273 m in 300 s,
or 0.5143375634 m/s, without a sampled fall. The previously leading minute-long
neural actor is slower over 300 s. This experiment therefore optimizes the
teacher's commanded motion directly, through the same Rust physics and net-speed
task. No new sustained-speed result is available at this snapshot.

The earlier Bayesian teacher search varied command speed, tracking gain and one
common derivative-lead factor. This search has eight independent parameters:

| Parameter | Initial numerical search interval |
| --- | --- |
| Signed command speed | −0.8 to 0.8 m/s |
| Tracking gain | −2 to 2 |
| Belt, worm, and foot amplitude factors, independently | −1 to 3 each |
| Belt, worm, and foot derivative-lead factors, independently | −1 to 3 each |

These boxes define this initial experiment, not physical limits or final search
boundaries. A promising boundary requires expansion. Four actuators of each
type share a factor in this representation; the underlying reference timing and
curve shape are retained. Per-leg timing, mean offsets, more general trajectory
shapes and neural policies remain additional search directions. This finite
representation does not exhaust gait space, and actual physical contact is not
constrained to match the reference's contact phases.

`Trajectory::affine_values` supplies the shared transformation
`q_new = center + scale * (q_reference - center)`. For the teacher's uniform
periodic cubic B-spline, the center is the mean of the 4,000 unique controls;
partition of unity makes it the period-average reference value. The repeated
closing control is excluded from that mean. The stored centers are in the
recorded actuator coordinates, in radians. Negative factors invert a channel's
motion around its center; zero factors hold the center. Rates and accelerations
scale consistently through the ordinary shared trajectory sampler. Identity
factors preserve the original values exactly, including floating-point bits.

The Rust CLI prepares each transformed curve. The orchestration script only
maps declared actuator groups, writes scene/action configurations and invokes
the Rust transform, selector and evaluator. The robot, terrain, transmissions,
actuator torque law, initial physical state and final target bounds stay
unchanged. Existing command saturation remains in the Rhai adapter. Changing a
commanded reference does not directly move a physical joint or inject forces.

The unused zero-output actor and trajectory-forecast calculations are removed
from this teacher-search configuration. A two-second comparison reproduces all
101 sampled physical frames and 100 applied command packets exactly. It covers
poses, joint positions/velocities, servo targets, motor state/readings and
contacts. This is an initialization check. Before searching, the driver also
requires its complete 300-second baseline to reproduce 154.3012690273 m within
1e-8 m. That tolerance applies to experiment setup, not candidate acceptance.

The production recipe uses seed 1901, ten Latin-hypercube initial experiments
and twelve subsequent proposal slots. Initial candidates run two at a time;
observations are ordered by assigned trial index before fitting, independently
of process completion order. The shared Rust Bayesian selector uses normalized
inputs, a constant-mean Matérn-5/2 Gaussian process and constrained LogEI. It is
the existing selector, not an implementation of SCBO. Every candidate is
evaluated from startup over the full 300 seconds. Only complete nonfalling
episodes supply speed scores. Physical falls and numerical errors retain their
records, but the present backend excludes failed rows from fitting rather than
inventing an objective. If fewer than dimension+1 complete observations remain,
a proposal slot collects another seeded initial-design point instead of making
an unsupported GP fit. This limitation of the current failure model is explicit.

The short workflow tests exercise baseline checking, concurrent initial
evaluations, sampled falls, data refill, the actual eight-dimensional Bayesian
proposal and its physical execution. The first direct selector probe correctly
rejects insufficient completed observations. Subsequent independent short runs
provide nine complete rows; the selector excludes four fallen rows and proposes
a new reference which completes its two-second execution. None of these short
scores is promoted as sustained-speed evidence.

Fourteen Rust checks pass: the new affine test covers three interpolation laws,
derivative consistency, exact identity, negative/zero scales and invalid inputs;
the thirteen existing trajectory tests cover their analytic and traversal cases.
The full five-minute baseline/search process is running. Two independent
five-minute fidelity evaluations also run: one halves the timestep to 0.3125 ms,
the other enables inter-link contact at the nominal 0.625 ms timestep. They will
measure approximation sensitivity, not count as controller speed gains.

`affine-speed-search-v1.json` records the completed checks and live-process
snapshot. `affine-speed-search-evidence-v1.json` preserves sources, executables,
specifications, transformed curves, closed short results and pending long-run
inputs. Unfinished outputs are excluded. Placing a `STOP` file in the production
output directory requests cancellation after already running evaluations finish;
the full speed goal has no time limit and remains unproven.

## Full-horizon baseline and independent leg phases

The production baseline now completes all 15,000 intervals and exactly
reproduces 154.3012690273 m (0.5143375634 m/s). The first two expanded-amplitude
initial-design candidates fall at 1.08 and 0.48 s and receive no completed speed
score. Their physical failure records are preserved; the next pair is running.

The shared `Trajectory::shifted_periodic_controls` adds another representation:
independent leg timing. It cyclically permutes each channel's control points by
an integer number of grid intervals. For a uniform periodic cubic B-spline this
is an exact time translation, preserving the original signal's shape, rates and
accelerations. It does not sample the spline as replacement control points.
Zero shifts and whole-cycle shifts preserve values exactly; negative and large
integer shifts wrap without integer overflow. A new analytic test checks those
properties and inverse shifts. Together with the existing affine and trajectory
tests, all 15 checks pass.

The generic preparation helper maps four independently variable leg phases onto
the 12 declared motor channels. It shifts the reference and all three
feedforward curves together; their 4,000-interval grids match exactly. Requested
phases are rounded to the nearest grid interval, with ties toward positive
infinity. The resolution is 0.00025 cycles, or 99.0219 microseconds of reference
time, and the rounding error is recorded. This refers to the reference clock;
elapsed simulation time also depends on the commanded traversal rate. Integer
grid resolution is a numerical representation that can be refined, not a
physical limit.

All four leg phases vary over an entire cycle, including their common initial
phase. No leg anchors the cycle origin. Four seeded Latin-hypercube starts
(seed 2203) are prepared and are being evaluated sequentially for 300 seconds
each. The tested waveform shapes, amplitudes and commanded speed stay at the
teacher values in this branch. Physical contact emerges from ordinary physics;
no particular contact sequence earns acceptance. This initial design is not
an exhaustive phase search or a new measured speed improvement.

The zero-phase preparation reproduces all 101 physical/controller frames and
task transitions of the verified two-second baseline exactly. The first parity
assertion also compared `stepping_wall_s` and failed on elapsed compute time;
the corrected check excludes only that wall-time diagnostic and retains every
simulation-time, physical, policy and task field. The original assertion/log
are retained. The nonzero variants are now physical experiments, with their
full inputs preserved before execution.

These phase trials use a continuous motion request. The original stop/yaw phase
metadata is retained, so they do not establish WASD qualification. Their
feedforward waveforms are translated commands derived from the original gait;
the coupled loads for a new phase combination are determined by the runtime,
not certified by those original feedforward estimates. Neither limitation adds
a gait-quality penalty to the net-speed objective.

`phase-speed-search-v1.json` and `phase-speed-search-evidence-v1.json` preserve
this update, including the completed long baseline and first two fallen
amplitude trials. Pending search, phase-trial and fidelity outputs are excluded.
