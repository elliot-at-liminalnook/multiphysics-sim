# Sampled dynamics and heading feedback

The speed search found fast prefixes whose small mean turn rates strongly affect
net displacement over 300 seconds. Fixed steering corrections improve those
prefixes, but their residual drift changes with numerical resolution. Before
using the existing body gyro channels for continuous steering feedback, this
study checks whether their sampled history represents actual heading changes.

The task observer provides body-frame angular velocity and gravity direction at
the controller deadline. Telemetry in an environment endpoint frame belongs to
the preceding policy sample: a frame at 0.02 s contains policy observations at
0 s. The diagnostic aligns those explicit timestamps. All 3,000 compared gyro
components exactly equal the preceding pose's recorded world angular velocity
rotated into body axes. This rules out a conversion/timestamp mismatch in this
comparison, not every possible observation or dynamics error.

For world-Z gravity, let `g` be its unit direction in body axes and `w` the
body-frame angular velocity. The heading rate of the body X axis is

`yaw_rate = -(g_y*w_y + g_z*w_z) / (g_y*g_y + g_z*g_z)`.

This is the derivative of `atan2(R_yx, R_xx)` and is undefined when that body
axis is vertical. No singularity is encountered in these captures. Analytic
Euler-angle cases check the formula independently. Integrating it with the
trapezoidal rule at the recorded 20 ms intervals produces the following errors:

| Source | Physics step | Fitted rate error, 5–19.98 s | Heading error at 19.98 s |
| --- | ---: | ---: | ---: |
| Original coarse steering root | 0.625 ms | 0.0460803 rad/s | 0.907998 rad |
| Same command, fine prefix | 0.3125 ms | 0.0469379 rad/s | 0.924921 rad |
| Fine response root, quarter prefix | 0.15625 ms | 0.0466524 rad/s | 0.918569 rad |

These errors are much larger than the true mean turning being corrected. A
controller that simply integrates these 20 ms gyro observations could therefore
apply a substantial erroneous steering correction. The recorded instantaneous
values and a finite-interval mean derivative are different quantities.

`runs/gyro-window-v1` uses the existing Rust `capture_embedded_window` example to
observe the original coarse root during 2–5 seconds at every 0.625 ms physics
step. It retains the exact runtime recipe, seed and held command schedule. The
reducer checks shared-time physical/controller state equality, then subsamples
the same trajectory at multiple periods and all 32 phases of a 20 ms sample.
This distinguishes sampling effects without changing the robot or controller.

The initial attempt used the environment wrapper at a period shorter than the
controller period and was correctly rejected before simulation. The subsequent
window capture uses the existing runtime inspection API rather than changing
the environment contract. Both the failed attempt and corrected inputs are
retained. The first coarse diagnostic also rejected the empty initial policy
telemetry; the corrected version explicitly aligns policy sample timestamps.

The observations remain ideal simulation diagnostics, not identified hardware
sensors. This study adds no gait or slip penalty and claims no improved neural
learning or control until physical speed measurements establish one.

## Dense-capture result

The dense capture reproduced all **151 shared-time physical/controller frames
exactly**, including link poses and velocities, joint states, motor states,
applied targets and policy telemetry. The analytic heading-rate formula check
has maximum error 1.11e-16 rad/s. Over the same 2–5-second physical trajectory:

| Observation period | Gyro integration mean-rate error |
| ---: | ---: |
| 0.625 ms | -0.00005095 rad/s |
| 1.25 ms | +0.00010977 rad/s |
| 2.5 ms | +0.00077225 rad/s |
| 5 ms | +0.00378217 rad/s |
| 10 ms | +0.01551133 rad/s |
| 20 ms | +0.04777809 rad/s |

Changing the phase of the 20 ms samples gives errors ranging from -0.03415984
to +0.04777809 rad/s. This is direct evidence of sampling error in the sparse
integral on this trajectory, not evidence that the instantaneous gyro channels
are wrong. The dense residual is small but nonzero; this is not a global
kinematic-consistency or hardware validation certificate.

The next speed experiment therefore opts into an ideal world-Z heading channel
in the existing task observer. The default observation contract stays unchanged.
The new channel is typed as angle, carries the existing CAD reference-link
provenance, requires world-Z gravity and rejects the mathematically undefined
vertical-X-axis projection. It is explicitly privileged simulation orientation.

The experimental controller reuses the shared `AngleIntegral` component with
equal input/leak gains as a first-order low-pass on wrapped heading error.
Its time constant is one commanded gait cycle. Dividing the filtered angle
error by the measured steering response gain and a four-cycle response time
proposes a yaw correction through the existing command/actuator path. This is
feedback initialization from measured response, not a new trajectory penalty.
The same controller is prepared at two timesteps; a disabled-feedback run first
checks physical parity before speed comparisons.

For the local response `heading_rate ≈ disturbance + G * command_correction`,
a filtered proportional correction `command_correction = -filtered_error/(G*T)`
with filter time `tau` has linearized characteristic equation
`lambda² + lambda/tau + 1/(tau*T) = 0`. Choosing `T = 4*tau` gives critical
damping in this reduced model. This motivates the initial four-cycle response
rather than proving stability of the nonlinear contact dynamics. The actual
filter is the existing sampled backward-Euler component, with 20 ms updates.

Three task-observer tests pass, covering the prior moving-frame derivative and
contact behavior plus the new opt-in heading/units/default-contract checks. The
first new test used an invalid empty marker list and failed the existing marker
validation; correcting that fixture produced the passing run. The new release
runtime with feedback disabled reproduces all **101 frames and transitions** of
the original 2-second physical prefix, excluding only wall timing and the newly
requested observation key. Enabled nominal/fine 20-second runs remain active.

## First feedback comparison and state replay

Both 20-second feedback runs completed without a sampled fall. Nominal speed was
0.5364065085 m/s and fine speed was 0.5362985781 m/s. However, the finer run's
fitted yaw increased to 0.0136772621 rad/s over 10–20 seconds, versus
0.0001609800 at the nominal step. The initial reduced-model feedback design did
not transfer reliably across these resolutions and is not a qualified speed gain.

The shared `replay_policy_state` example now accepts a completely captured
environment prefix, while preserving the distinction from a complete episode.
It keeps the per-sample timing, unit and actuator-command checks. Replaying both
captures recovers all 1,000 controller states with **zero command error**. The
coarse raw yaw command ranged from -0.0620 to +0.1879 rad/s. The fine command
ranged from -0.0308 to +0.4964 rad/s before its existing ±0.25 rad/s policy bound;
170 samples saturated. These excursions substantially exceed the response
calibration region. This is evidence that the local response model was being
used outside its tested range, not a complete identification of the nonlinear
closed-loop failure mechanism.

The largest filtered heading excursion during the first five seconds was
0.0326979461 rad. Using the measured gain magnitude 0.1907512256 and a correction
span of 0.02 rad/s gives `T = error / (abs(G) * span) = 8.5708351278 s`.
`runs/heading-feedback-slow-v1` tests this slower response at the same two
resolutions. Only response time changes; actual command limits, filters, CAD,
world, task and actuator properties remain identical. This calculation is a
proposal based on a measured transient, not a guarantee about future excursions.

`heading-observation-evidence-v1.json` preserves the nominal 300-second steering
result, third-timestep prefix, sampling audit, observer changes/tests, exact
state replays and immutable inputs for the next feedback comparison.

## Slower feedback result

The 8.5708351278-second response completed both 20-second runs without falling:
0.5369030986 m/s at 0.625 ms and 0.5368330315 m/s at 0.3125 ms. The fine
10–20-second mean turn rate fell from 0.0136772621 to 0.0016489903 rad/s.
Exact 1,000-state replays again have zero actuator-command error. No yaw-command
sample saturates; its observed ranges are 0.0546921–0.0848278 rad/s nominally and
0.0625518–0.1019389 rad/s at the finer timestep. These are measured ranges, not
new policy bounds, and do not certify long-run stability or a speed gain.

`runs/heading-feedback-sustained-v1` now validates this unchanged controller over
300 seconds at 0.3125 ms and for a 20-second prefix at 0.15625 ms. In parallel,
the faster measured constant-steering controller undergoes its own third-step
full-duration check. `steering-fine-sustained-evidence-v1.json` preserves the
slower-feedback results and immutable live inputs.

## Completed full-duration feedback refinement

The slower ideal-heading feedback controller now completes 300 seconds without
a sampled fall at both 0.3125 and 0.15625 ms: 0.5468435312 and 0.5468228713 m/s.
The 0.003778% speed difference is much smaller than the fixed-steering result's
1.9579% change, but its endpoints still differ by 3.7133 m. This supports the
feedback controller's sustained-speed behavior at these resolutions, not
full-trajectory convergence or a deployable heading-estimation claim. Full
input/scene/prefix checks and raw captures are in
`joint-sustained-late-evidence-v1.json`; the result remains separate from the
constant-steering search context.
