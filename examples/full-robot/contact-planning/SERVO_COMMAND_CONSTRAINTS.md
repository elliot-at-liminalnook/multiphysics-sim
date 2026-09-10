# Executable servo commands in joint speed planning

The recovered sixteen-control motion passed 10,266 combined CAD planning
samples at 0.0252243933 m/s under the previous gates. Its earlier 2,266-frame
reference also passed the existing controller compiler: forward/reverse load
checks, 218 static pause samples and interpolation errors of 7.3027e-6 rad,
0.0005840 rad/s and 0.946799 rad/s² against unchanged limits of 3e-5, 0.015 and 2.
Both initial stopping windows are longer than the required 0.04 s.

Nevertheless, its detailed 0.625 ms runtime screen stopped at **0.72 s**:
`+X | Foot servo output.target` requested **-0.0152497241 rad**, beyond the
existing upper bound **-0.02 rad**. The runtime rejects this request. The eight
second episode is incomplete, so it supplies no qualified speed, slip or
stopping result. The unchanged policy includes tracking gain 0.5 as well as
velocity lead and force feedforward; this rejection alone does not isolate
those contributions.

## Shared constraint

The existing effective servo requests torque

`tau = K * (u - q) - D * qdot`,

subject to its torque-speed envelope. At zero reference tracking error, the
position command needed by a candidate motion/load is therefore

`u = q + (D * qdot + tau) / K`.

`EffectiveServo::reference_target` already implements this law. New shared
`ServoCommandLimits` calls it and checks explicit lower/upper command bounds.
It adds no command clamp or acceptance tolerance. A supplied scale changes
only numerical conditioning; the experiment uses 0.01 rad.

The recipe's optional `servo_command_limits` is supplied from the actual
runtime metadata's software/CAD bound intersection in the same motor order.
Absence preserves historical recipes and serialized output. When present,
uncached CAD evaluation and force-cache reloads both compute commands and
include their violations in physical acceptance. The joint inequality NLP
couples both signed command limits to body motion, foot placement, timing and
contact-force coefficients. Exact force derivatives reuse the servo law;
native sparsity and grouped body derivatives include the additional rows.
Legacy motion planning also receives the same command checks. The force-only
conic solve now also enforces these commands as hard affine inequalities; see
the [conic extension and completed native pilot](SERVO_CONIC.md).

These are necessary **nominal** command constraints. They do not bound extra
feedback/yaw corrections, interpolation error or transition dynamics. Runtime
validation remains required. No robot property, joint range, contact model,
actuator envelope or existing acceptance gate is relaxed.

The analytic servo test demonstrates a pose and torque that are individually
admissible but require an out-of-bounds command, then checks an admissible load
and invalid inputs. The new recipe also retains the four observed IK-failure
phases (0.7505, 0.751125, 0.751375 and 0.7515) in the optimization mesh.

## Completed verification and continuing solve

Three effective-servo tests and eight planner/native-layout tests pass. With
command bounds absent, the rebuilt compiler reproduces the earlier reference
byte for byte. Both selected force-derivative audits check all 366 movable force
columns and both operating clocks. Their maximum scaled derivative error is
2.18316034e-8 against the unchanged 1e-5 criterion, with four independently
recomputed cache comparisons across the two interpolation cases.

The default audit's constant-body fixture fails CAD IK at phase 0.46875;
its error and empty result are preserved. Optional `--case=` selection keeps
the historical default cases and allows the two evaluable fixtures to complete.
This is a fixture-domain rejection, not a passed constant-body derivative test.

The new 274-frame recipe has 13,976 inequalities. Its maximum nominal command
violation is **0.00415777689 rad**, at the +X foot in reverse, phase 0.78125.
The joint position is -0.0307130471 rad and the speed is +0.6401371322 rad/s;
the required torque remains inside its envelope, but the computed target is
-0.0158422231 rad against the -0.02 rad upper command bound.

The first native pilot uses its 600-model budget before completing derivative
work (native status -13). The independently audited returned candidate reduces
command violation to 0.00407176103 rad, at 0.0252074603 m/s, with force error
0.0362233826 N, moment error 0.0128712627 Nm and positive torque margin. It is
still infeasible. The native terminal constraint snapshot is unavailable after
budget exhaustion; the verification explicitly records that limitation.

The second one-iteration pilot has completed after 765 model attempts, with
native final constraints matching its independent CAD report. A subsequent
fixed-motion conic test reports infeasibility with hard command bounds. The
[full joint speed search](SERVO_CONIC.md) is now running with those bounds. The
older eight-control search continues and will also require checking against
command bounds. No new measured gait speed is established.

`joint-servo-command-build.json` records exact input/executable/source identities,
commands, terminal statuses and the raw failed runtime capture archive.
`joint-servo-command-verification.json` records derivative checks, exact runtime
bound matching and pilot limitations. The expanded CI checks have not been
observed remotely.
