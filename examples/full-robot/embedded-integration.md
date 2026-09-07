# Reduced mechanical integration experiment

The shared `RigidEmbedding::step_midpoint` now advances independent mechanical
coordinates through time. It uses explicit midpoint for positions, velocities
and the original contact model's bristle history, with world-frame quaternion
exponential updates for a floating base. Every stage restores the dependent
coordinates through the original geometric closure equations. The returned
endpoint includes physical accelerations and contact-history rates.

The load callback is a pure force law evaluated at start, midpoint and endpoint.
A controller must sample outside this function and hold commands between its
deadlines; controller, firmware and sensor events must split physics intervals.
Failed steps leave the caller's input untouched. Authored sampled IMUs are
explicitly rejected until their schedule is coupled.

This is an **experimental integration building block, not the promoted robot
training backend**. Midpoint has no adaptive error estimator, implicit stiff-force
update or impact-event treatment. A separate implicit experiment is described
below. A separate adapter now couples the registered motor winding, rotor and
gearbox equations at declared voltage/temperature boundaries. The battery,
driver, thermal network and firmware are not yet coupled. A generalized applied
load is not a realistic servo model.

## Tests and observed limits

Nine tests across `embedding` and `embedded_step` cover the mechanism map and
instantaneous dynamics plus constant-force free-body translation/rotation,
oscillator timestep refinement, contact-memory decay, atomic failure, and
closed-linkage energy/closure through time. The oscillator exhibits second-order
error convergence. The freely accelerating linkage reaches over 35 rad/s;
its energy test uses refined steps of 125, 62.5 and 31.25 microseconds over the
same 0.2 s interval to reach the asymptotic error regime and retain a 1e-7 J
energy-error check. These are numerical tests, not proposed training timesteps.

The initial all-robot experiment applies an explicit zero generalized load,
so it differs from the detailed powered hold-position scene:

| Mechanical diagnostic | Result |
| --- | --- |
| Contact and original joint friction; 0.5 ms step; initial position tolerance 1e-8 scaled | Stops at 17 ms on acceleration-closure check |
| Same experiment, tighter 1e-11 scaled position solve | Stops at 23 ms; unstable motion remains |
| Same tighter solve, 0.25 ms step | Stops at 19 ms; refinement alone does not establish stability |
| Contact disabled, original joint friction; 0.5 ms step | Stops at 11 ms |
| Contact disabled and joint friction zeroed in an explicit diagnostic copy | Completes 100 ms; joint speeds remain below 8e-15 rad/s in free motion |

The original robot/CAD parameters were not changed. The final row isolates the
stiff dissipative terms; it is not an acceptable replacement model. In the
unstable 0.25 ms run, a nearly released floor contact reports roughly 1,149 N
tangential force with only 0.194 N normal force. This is failed-run evidence,
not a calibrated physical prediction. Both the contact-history dynamics and
the velocity-dependent joint friction need stable treatment. Omitting separate
motor inertia from this isolated mechanics experiment also changes the relevant
timescales; the complete actuator coupling must be tested before judging the
training backend's stability or speed.

The tighter position tolerance provides room for small geometric errors to
amplify in velocity/acceleration calculations. The original velocity and
acceleration acceptance checks remain 1e-8 scaled. Tightening closure did not
fix the explicit integrator's stiff-force problem.

## Reproduce and inspect

```sh
cargo test --locked -p sim-domain-robot --test embedding --test embedded_step
cargo test --locked -p sim-domain-robot --no-default-features --test embedding --test embedded_step
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/floating.scene.json examples/full-robot/mechanical-midpoint.json \
  > runs/full-robot/mechanical-midpoint.json
```

Export the scene as described in the parent README. The supplied configuration
expects this robot's 34 full mechanical load coordinates: six base force/moment
coordinates followed by all 28 joint forces/torques in compiled order. It
explicitly supplies zero in every coordinate. The output identifies selected
motor coordinate names and records all original closure rows, link motion,
contact forces/history, the terminal committed frame and any failure. Exit 1
means integration failed even though a useful JSON diagnostic was written.

Local study inputs/outputs are `runs/full-robot/learning/mechanical-*`; they are
ignored diagnostic evidence, not a versioned golden trajectory. The successful
friction-free case is not a speedup claim; it omits essential work. Stable
implicit handling of dissipative forces and explicit actuator coupling require
matched full-trajectory accuracy and total-runtime measurements.

## Implicit mechanical baseline

`RigidEmbedding::step_implicit` solves backward Euler in the independent
velocities. Each trial endpoint updates the base pose and independent joint
positions, restores every original geometric closure equation, and evaluates
the original mass, velocity bias, contact and passive loads. It does not freeze
geometry or replace friction with a gentler law. This is first-order integration;
its numerical damping and contact-event timing must be checked by refinement.

The current contact-memory law is affine and diagonal at fixed pose/velocity:
`zdot = drive - decay*z`. Two simultaneous affine probes of the shared evaluator
extract these coefficients, allowing `z_new = (z_old+h*drive)/(1+h*decay)` without
adding all memory states to Newton's unknowns. The original endpoint law checks
every memory update against a declared absolute residual tolerance. This
condensation must be revised if the shared contact law gains nonlinear or
cross-coupled memory dynamics. Geometry is reused between the two probes only;
the method is a correctness baseline, not an optimized endpoint evaluator.

It uses the existing shared Newton solver, with an opt-in domain-aware line
search that rejects nonfinite trial poses and shortens corrections. Invalid
initial residuals and derivative probes remain errors. The original detailed
solver defaults and residual acceptance criteria are unchanged. Controller,
sensor and separate actuator states remain outside this mechanical diagnostic.

Focused tests cover a strongly damped slider against its exact discrete update,
spring/mass convergence toward the continuous solution, contact-memory decay,
atomic failure, and active sliding contact against an independent scalar
force-balance solve. Shared solver tests cover a root with an invalid full-step
trial, invalid initial states and the case where no finite trial exists.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/floating.scene.json examples/full-robot/mechanical-implicit.json \
  > runs/full-robot/mechanical-implicit.json
```

The output records the full implicit configuration and per-step nonlinear
iterations, endpoint evaluations, velocity residuals and contact-memory
residuals. Total stepping time includes rejected work, although the per-step
diagnostics list contains accepted steps only. Converged Newton iterations do
not establish a calibrated contact model, a complete servo simulation, or a
walking/learning acceptance gate.

## Independent linkage equation solve

`embedding.dependent_solve = "pivoted_qr"` selects an experimental pivoted-QR
least-squares solve for dependent coordinates. It retains every original row
and the SVD rank diagnostic. The default is still `"svd"`. Tests check known
slider-crank geometry under small and large base rotations, loaded acceleration
against an independent full constrained system, rejection of a true toggle,
and implicit trajectory refinement toward a separate explicit reference.

On the contact-free robot with the original joint friction, QR completes the
100 ms test that stopped at 74.5 ms with SVD. Internal joint speed stays below
1.1e-17 rad/s, consistent with rigid free fall. This comparison is evidence of
numerical sensitivity, not a physical travel limit. With original contact, QR
still stops at 37.5 ms with 0.5 ms steps and 65 ms with 0.25 ms steps. No robot
training-model promotion follows from the free-fall success.

## Shared motor coupling

`step_implicit_coupled` solves extra component equations together with the
mechanical endpoint. The adapter supplies explicit auxiliary unknowns and
scaled residuals; its callback is pure and neither input state is committed on
failure. The original load-only method calls this same path with no auxiliaries.

`EmbeddedMotorBank` uses `MotorUnit::residual` through the shared registry. It
does not copy the motor force, winding, loss, backlash or thermal-output laws.
Each motor binds to one exact compiled revolute DOF. Its three additional
unknowns are winding current, rotor speed and gearbox output angle. Equation
scales have declared units (V, N m, rad/s); bad scales, missing/duplicate bindings,
invalid boundaries and unsupported event scheduling fail explicitly.

The detailed adapter and this diagnostic share `cad_motor_unit_parameters`.
That extraction retains the existing parameter floors, damping/friction
estimates and backlash combination. The diagnostic records the resulting maps;
sharing those estimates is not hardware calibration. Initial motor states come
from the registered component declarations.

An independent five-variable linear circuit/mechanics test verifies loaded
motion with gearbox compliance, reflected rotor inertia and back-EMF, including
both finite winding inductance and an algebraic zero-inductance case. It also
checks shaft force balance and copper heating. Failure tests protect both the
mechanical and auxiliary input states. CI includes these adapter checks.

```sh
cargo test --locked -p sim-domain-robot --test embedded_motor
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/floating.scene.json examples/full-robot/mechanical-motor-boundary.json \
  > runs/full-robot/mechanical-motor-boundary.json
```

This configuration declares 0 V and 293.15 K at all twelve motor boundaries,
with QR and 0.5 ms implicit steps. Zero voltage is a **short-circuit boundary**
with electrical braking, not an open circuit or servo position hold. Temperature
is fixed; heat output is recorded but the thermal network is not advanced.
No battery, driver current limiting, firmware, controller or sampled sensors
are supplied. External generalized loads, if present, add to the motor torques.

The initial full-robot run stops at 13 ms on nonlinear convergence; 0.25 ms
steps reach 16.75 ms. A bounded capture of 128 trial endpoints identifies the
**+X foot motor** crossing its backlash boundary: opposite-mode probes differ
by only 4.55e-9 rad in gap, but torque changes from 0.000530 to 0.010605 N m.
Relative speed changes by only 9.10e-6 rad/s. This identifies a force-law jump
inside the failed solve; it does not prove the coupled step has no root or
that this is the only cause. The existing motor's algebraic backlash
damping can be discontinuous, and the detailed library's event-based version
needs a scheduler before this adapter can use it. These are saved failed
diagnostics, not completed episodes or realtime performance results. Next is
failure localization and reuse of shared event handling, followed by the
driver/firmware/thermal coupling and full task-accuracy gates. `trace_trials`
optionally keeps up to 256 trial endpoints in diagnostic output; this adds
measurement overhead and is off by default. The recorded independent joint
indices link these trials to named motor components without guessing ordering.
Enabling the trace reproduced the same committed joint and motor states bit for
bit. Trace data are local diagnostic evidence, not a versioned golden trajectory.

## Located motor events and exact mechanical reuse

The preceding failures describe the unscheduled algebraic-backlash experiment.
`EmbeddedMotorBank::new_with_events` and `advance_with_events` now connect the
registered motor's startup/engagement/release guards and jumps to a reusable
`sim_dynamics::hybrid` scheduler. `new` still rejects event-mode components when
no scheduler is supplied. Event motors include the original fourth mode state;
`state_layout` explicitly records variable per-motor state counts and bindings.

The scheduler and the existing `Trajectory` use the same bracketed root locator.
Every trial advances from an unchanged beginning state; the earliest crossing
is selected, the interval is split, and guards are refreshed after each jump.
Startup and endpoint clocks are processed. Failed intervals leave caller-owned
mechanical and motor states unchanged. Limits return errors rather than skipping
remaining events. Continuous failures can halve the step, which is recovery,
not a local error estimator. Endpoint guard signs can miss multiple intervening
crossings, so step refinement and event-count comparisons remain necessary.
The separate legacy `Trajectory` event-count policy is unchanged.

Tests cover earliest-event ordering, clocks at both interval boundaries,
invalid schedules/layouts, atomic failure, positive/negative engagement against
an independent piecewise linear motor/load solve, release, and repeatability.
Motor jumps preserve all continuous states; no artificial velocity impulse is
introduced to make engagement pass. Existing root-order and scheduled-event
regressions still pass after extracting the shared root function.

`mechanical-motor-events.json` enables this path at the same prescribed 0 V,
293.15 K boundaries. The original contact and joint-friction models remain.
Both the 0.5 ms and 0.25 ms nominal-step runs complete 100 ms, costing 129.026 s
and 168.024 s respectively in single diagnostic runs on the development CPU.
These are not speedup claims against the differently configured detailed servo
hold, nor walking or hardware-transfer evidence. At aligned 2 ms output samples,
coarse/refined foot-tip differences reach 0.576 mm (+X), 0.487 mm (+Y),
1.793 mm (-X), and 0.474 mm (-Y). Active contact body-pair sets differ at 18 of
51 samples, and five motor guards have different event counts. The fine run is
not established as converged. No numerical accuracy promotion follows.

`implicit.reuse_mechanical_endpoint` (default false) reuses the most recent
mapped mechanism and condensed contact memory only when every mechanical
unknown has the same floating-point bits. The cache lives inside one continuous
solve: seed, timestep, geometry, configuration and previous contact history are
fixed. It never crosses a step, event jump, or changed mechanical trial. Auxiliary
motor perturbations still evaluate the original motor equations, forces, and
accelerations afresh. New counters distinguish endpoint evaluations, mechanical
preparations and exact hits; hybrid totals include discarded successful root
trials, but do not yet expose internal work in failed Newton solves. Wall time
includes all work. `mechanical-motor-events-reuse.json` selects the experiment.

The Rust `compare_embedding` example compares aligned completed diagnostic
captures, enforces matching physical metadata/CAD marker provenance, and reuses
the shared local-marker-to-world transform. It reports per-foot Euclidean and
RMS differences, motor telemetry differences, event counts/times, contact-pair
sets, and exact sampled/terminal/event equality. Mixed-unit arrays are labeled;
no mixed-unit maximum is an accuracy gate. Sparse contact snapshots do not
establish contact impulse or energy accuracy.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/solver-performance/bracket-option-off.scene.json \
  examples/full-robot/mechanical-motor-events-reuse.json \
  > runs/full-robot/learning/mechanical-motor-events-reuse.json
cargo run --locked --release -p sim-runtime --example compare_embedding -- \
  runs/full-robot/learning/mechanical-motor-events-reuse.json \
  runs/full-robot/learning/mechanical-motor-events.json \
  examples/full-robot/foot-markers.json
```

Raw diagnostic captures under `runs/` remain ignored local evidence, not durable
golden tests. The compact event-integration status records their hashes and
explicit limitations. Driver/battery/current limiting, sampled firmware/sensors,
and thermal-network integration still precede a full actuator/learning gate.

The completed reuse comparisons preserve saved frames, terminal frame and
located event schedule exactly at both 0.5 and 0.25 ms. Single-run times become
53.065 s and 70.137 s (about 2.4x faster). The coarse run avoids 53,296 of 83,330
mechanical preparations in successful trials. This does not establish realtime
or repeat-run performance. The option remains disabled by default.

A further cached 0.125 ms test completes in 73.518 s. Comparing 0.25/0.125 ms
still gives maximum foot differences of 0.869 mm (+X), 0.481 mm (+Y),
1.698 mm (-X), and 0.476 mm (-Y), with different contact sets at 19/51 samples
and seven guards with differing event counts. Errors do not consistently
contract across the three resolutions. Before about 16 ms the foot differences
generally decrease; the first sampled 0.5/0.25 ms contact-set disagreement is
at 18 ms at the +X foot, during lift-off. This localizes a useful interval for
accepted-step contact/impulse diagnostics; it is not proof that contact is the
only source of discrepancy. Subsequent backlash events also differ.

Compact evidence, raw artifact hashes and limitations:
`examples/full-robot/event-integration-status.json`. Tests passed: 19 combined
embedding/step/motor cases in both native default and no-default builds, 21
shared dynamics/root/schedule/hybrid cases, 18 runtime session cases, and the
comparison-tool fixture. WASM compilation passes (the pre-existing unused
`rotor_speed` warning remains). CI runs the hybrid scheduler and comparison
fixtures. This is compile portability, not full-robot browser performance.

## Accepted contact stages and impulse comparison

`EmbeddedMotorBank::set_contact_step_audit(true)` enables an optional trace of
accepted continuous segments, including segments without contact. Each pure
trial carries a persistent diagnostic chain; rejected candidates and event-root
probes lose their private nodes. The returned trace belongs only to the fully
successful interval. A later failure returns no partial trace. Trace destruction
is iterative to avoid deep recursion in large diagnostic intervals.

Each record contains its beginning time, duration, and the original contact-law
samples from the accepted backward-Euler endpoint: body pair, world force,
world point and penetration. The force is from the continuous endpoint before
any motor-mode jump. It is not a fresh evaluation at a reporting frame. The
number of records must equal the scheduler's accepted-segment count.

`sim_runtime::contact_audit::embedded_contact_impulses` reuses the detailed
runtime's stage quadrature and complete-coverage checks. For each body pair it
sums all contact point forces and integrates F_endpoint * dt in N s. Missing,
overlapping, partial or nonfinite coverage fails. Intervals with no contact must
still appear. `compare_impulse_reports` compares equal windows in shared world
axes and body numbering, treating an absent pair as zero impulse. Internal
contacts are retained separately from floor contacts; their equal/opposite
reactions must not be mistaken for external support.

The example's `audit_contact_steps: true` selects this capture. The comparison
example uses it when both inputs contain a trace, reporting whole-run and aligned
report-window impulse differences. The trace is optional and does not alter
forces, event location, residual tolerances or accepted state. There is no
continuous contact-event locator, torsional contact impulse, force moment or
work/energy audit in these results. The contact-presence durations are stage
samples, not exact touchdown/lift-off times. Integration quadrature accuracy
still requires refinement.

The 100 ms runs at 0.5/0.25/0.125 ms record 248/465/870 accepted segments, with
complete coverage. Enabling the audit preserves all saved frames, terminal
frames and motor event histories exactly at each resolution. The two finer
runs give:

| Contact quantity over 100 ms | 0.25 ms | 0.125 ms |
| --- | ---: | ---: |
| +X floor vertical impulse, N s | 2.065163 | 2.048521 |
| -X floor vertical impulse, N s | 1.813736 | 1.816774 |
| -X floor world-X impulse, N s | 0.249471 | 0.196277 |

Total upward floor impulse differs by about 0.35%, while -X world-X impulse
changes by about 21% relative to the 0.25 ms value. Whole-vector per-foot
impulse differences are 0.022107 N s (+X) and 0.053281 N s (-X). Differences
in totals do not bound timing error: the -X vector difference over 70–72 ms
is 0.262235 N s. The first 2 ms window exceeding an illustrative 0.01 N s
vector difference is 22–24 ms, at +X recontact. That cutoff is localization,
not a task-accuracy requirement.

For +X, the first accepted-stage contact span ends at 18.5/17.5/17.25 ms across
the three resolutions; the next active spans begin at 20/22.5/23.875 ms. These
are endpoint-quadrature spans, not continuously located collision times.
Early contact and subsequent backlash sequences interact; this evidence does
not isolate friction or one gearbox as the sole cause. The experiment remains
unpowered short-circuit braking with fixed winding temperature, not standing or
walking control. No hardware-transfer or learning gate has passed.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/solver-performance/bracket-option-off.scene.json \
  examples/full-robot/mechanical-contact-audit-coarse.json \
  > runs/full-robot/learning/mechanical-contact-audit-coarse.json
# Repeat with mechanical-contact-audit-refined.json and -quarter.json.
cargo run --locked --release -p sim-runtime --example compare_embedding -- \
  runs/full-robot/learning/mechanical-contact-audit-refined.json \
  runs/full-robot/learning/mechanical-contact-audit-quarter.json \
  examples/full-robot/foot-markers.json
```

Tests verify accepted history through event searches and a failed interval,
actual articulated contact samples against the original endpoint evaluator,
audit-on/off state equality, point-force aggregation, zero-contact coverage,
window matching and missing-stage rejection. Compact evidence and hashes are
in `contact-integration-status.json`; raw captures and binaries remain local
ignored artifacts, not versioned golden tests.

## Registered driver feedback

`EmbeddedDriverBank` now connects the registered `robot.h_bridge` to each
motor's trial current. Kirchhoff current balance fixes the bridge's absorbed
output current to minus the motor current; the original bridge voltage residual
then determines the motor terminal voltage. No driver law is copied and no
extra Newton unknown is introduced. The current-dependent boundary is evaluated
inside the coupled solve, including every derivative/event-search trial.
Using a lagged current here would change the equations.

`EmbeddedMotorBank::advance_with_boundary_law` accepts a pure boundary callback
of time, trial mechanics and motor states. The fixed-boundary method delegates
to it. Boundary errors and later interval failures remain atomic. Driver inputs
are explicit supply voltage, signed duty in [-1,1], and winding temperature.
The driver validates exact named motor ordering and current-state layout; a
binding check is available when reusing a bank. Invalid inputs fail rather than
silently clamping a policy command. Original H-bridge foldback is **not an ideal
hard current cap**. Its clamping/regeneration limitations and averaged nature
remain; switching electronics and a driver heat network are not added.

Detailed and reduced adapters share `cad_h_bridge_parameters`. This preserves
existing CAD parameter floors and current-limit selection, including provisional
values. Outputs record motor voltage/current, supply current, and supply power
minus motor electrical input power. That last value is diagnostic; it does not
integrate temperature or establish a calibrated loss model.

In `integrate_embedding`, `motors.drivers` replaces `motors.boundaries` for a
driven experiment; exactly one is required. Each list follows named motor order.
`driver_components` records the resulting CAD-derived parameter maps, and
`driver_readings` appears on driven snapshots. The comparison tool checks driver
metadata and telemetry dimensions before comparing matched-input experiments.
An intentionally changed command is not a matched-input model-validation run.

Tests compare loaded motor/bridge motion to independent five-variable linear
backward-Euler equations in both rotation directions, with ordinary series
voltage drop and above the foldback threshold. They verify current/power signs,
invalid bindings/inputs and recovery after a failed boundary callback. Existing
motor events, contact traces and runtime session checks still pass.

Two 100 ms robot experiments at 0.25 ms complete with original contact and
backlash events. Twelve zero-duty drivers reproduce the direct zero-voltage
braking case exactly in mechanical/motor frames, terminal state, event history,
and accepted contact trace (driver-only telemetry is excluded from that explicit
algebraic-equivalence comparison). A 5% duty command on the +X worm motor gives
sampled motor voltage 0.526068–0.555 V and a maximum sampled joint-path difference
of 0.0124225 rad (about 0.71 degrees) from zero duty. At the terminal frame,
that motor has 0.114426 A and 0.261971 N m output torque, while the joint remains
moving under load; this is not a position-hold or walking controller. Supply is
imposed at the CAD value 11.1 V and winding temperature at 293.15 K.

Single diagnostic times are 70.913 s (zero duty) and 61.142 s (5% worm duty).
The commands differ, so the timing difference is not an optimization speedup.
This driver connection does not resolve the prior timestep/contact sensitivity.
At this stage sampled servo firmware, battery sag, thermal coupling, deployable
observations, controlled task gates, and the planning/learning pipeline were
outstanding. The firmware connection is described below.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/solver-performance/bracket-option-off.scene.json \
  examples/full-robot/mechanical-driver-zero.json \
  > runs/full-robot/learning/mechanical-driver-zero.json
# Repeat with mechanical-driver-worm.json for 5% +X worm duty.
```

Compact driver evidence is in `driver-integration-status.json`. Raw captures
and the source/executable snapshot are ignored local artifacts, not versioned
performance or physics golden tests.

## Scheduled servo feedback

`EmbeddedMotorBank::advance_with_control` adds held discrete controller states
to the same transactional event scheduler as the registered motor modes.
`SampledMotorControl` supplies pure trial boundaries, guards, deadlines and
state jumps. Continuous electrical/mechanical states remain in the coupled
solve. A failed interval returns no partially advanced controller state; callers
must keep all mutable history in the supplied state, not in callback captures.
The original fixed-voltage and current-dependent boundary methods delegate to
this path with no additional controller events.

`EmbeddedServoBank` reuses the registered `ServoFirmware` component, including
quantized angle measurement, deadband, filtered speed feedback, anti-windup,
saturation, sample-rounded delay queue and next-sample clock. No PID or queue
formula is copied into the adapter. Between ticks the output is held. At a tick
the accepted mechanical state supplies the measurements, and the new held
command affects subsequent continuous motion. Simultaneous motor and servo
events have explicit guard identities: `control_guard_offset` separates the
motor guards from one clock per servo in exact named motor order.

`cad_servo_firmware_parameters` is shared by detailed and reduced adapters. It
preserves the existing CAD-to-duty gain normalization and parameter floors;
this is not a newly calibrated model of the physical Hiwonder firmware. The
robot's recorded rate is 1000 Hz, command latency 1 ms and angle quantum
0.0015339807878856412 rad. Internal firmware feedback currently receives joint
angle and speed, as in the detailed adapter. This does not establish deployed
policy observations or model the external bus/sensor delay. Supply voltage and
winding temperature remain imposed; battery sag and thermal evolution are
still absent from this reduced diagnostic.

Configure exactly one of `motors.boundaries`, `motors.drivers`, or
`motors.servos`. Servo entries contain `target_rad`, `supply_voltage_v` and
`winding_temperature_k`; servo mode requires an explicit `motors.events` object.
Targets use the CAD joint coordinates (zero is the assembled reference pose),
not an assumed hardware encoder zero. Output records CAD-derived servo
parameters, state layout, held commands and state snapshots. Matched-input
comparison checks those parameters/layouts and reports command/state differences
alongside mechanical, motor, contact and event diagnostics. Aggregate servo-state
differences mix quantities including clock time and are not a physical accuracy
threshold.

Independent component tests check exact quantization/deadband/delay/saturation
behavior and compare a clocked proportional controller plus loaded motor against
separately assembled linear backward-Euler equations. Failure injection after
controller ticks verifies rollback and deterministic retry. A shared scheduler
regression reproduces a roundoff-sized step caused by repeated clock additions;
ulp-level boundary handling fixes it without changing physical guard-location
tolerances. The new cases run in the existing motor and hybrid CI test suites.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/solver-performance/bracket-option-off.scene.json \
  examples/full-robot/mechanical-servo-hold.json \
  > runs/full-robot/learning/mechanical-servo-hold.json
# Repeat with mechanical-servo-hold-refined.json at half the physics timestep,
# or mechanical-servo-worm-target.json for a +0.02 rad +X worm-motor target.
cargo run --locked --release -p sim-runtime --example compare_embedding -- \
  runs/full-robot/learning/mechanical-servo-hold.json \
  runs/full-robot/learning/mechanical-servo-hold-refined.json \
  examples/full-robot/foot-markers.json
```

The 100 ms hold runs complete at 0.25/0.125 ms with 452/862 accepted continuous
segments. All twelve servos tick 100 times, with identical clock times across
resolutions. Single diagnostic stepping times are 74.675/89.308 s. Maximum
sampled foot-position differences are +X 0.867 mm, +Y 0.486 mm, -X 1.146 mm and
-Y 0.496 mm. Contact pair sets differ in 32 of 51 samples. The largest held-duty
difference is 0.949 at the -X worm motor at 48 ms: -0.050996 versus -1.0. The
largest 2 ms floor-impulse vector difference is 0.169579 N s at -X over 48–50 ms.
Identical controller timing does not imply converged mechanical/contact response
or reliable learned actions; these results do not pass a loaded-control gate.

A separate +0.02 rad +X worm-motor target run completes in 68.553 s of stepping
work. Its joint path differs from zero-target hold by up to 0.0164312 rad; the
terminal joint angle is -0.00987333 rad versus -0.0224974 rad for hold. The motor
responds in the requested direction relative to hold but does not attain its
target over this short loaded run. Different inputs make this a response
diagnostic, not model equivalence or an optimization comparison.

`servo-integration-status.json` records source/configuration/capture hashes,
sampled closure, controller clock counts, refinement and changed-target
diagnostics. Raw captures and the exact integrator executable/source snapshot
are ignored local evidence. Component/scheduler/comparison tests pass natively,
the motor tests pass without default features, and the runtime compiles for
WASM (existing unused `rotor_speed` warning). Full-robot browser execution and
realtime performance are not established by that compilation check.

## Contact-coupled feedback diagnosis and local factor reuse

For a matched +0.02 rad +X worm-motor target, halving the step from 0.25 to
0.125 ms changes maximum held duty by 0.547612 with contact enabled versus
0.0182297 with only `scene.options.contact` disabled. Maximum sampled foot
position changes are 0.797/0.0618 mm. The contact-off diagnostic retains gravity,
all masses/linkages and servo/driver/motor states; it removes both floor and
internal contact. It cannot support the robot and is not a candidate walking
model. Timings are 68.553/82.459 s with contact and 11.267/21.060 s without for
100 ms of simulation. Changes in loads, mode sequences and solver work prevent
attributing that cost difference solely to geometry queries.

The original hold capture also shows large transient tangential loads during
unloading. At 37.5 ms in the finer hold run, the -X foot patch has 56.475 N
sideways force with 0.989779 N normal load. The material's declared world
coefficients are 0.3 static and 0.25 kinetic, but the bristle law's transient
force is not constrained to those multiples of normal load. Bristle damping,
stored history and the evolving normal load require targeted investigation.
This evidence does not alone prove which contact term causes trajectory
sensitivity, nor justify silently clamping a force or removing contact.

`contact_audit::embedded_floor_force_ratios` now sums all floor-contact points
on each body at each accepted stage, then reports the largest |F_xy|/F_z above
an explicit positive normal-load cutoff. Internal contacts are excluded. This
uses the current articulated floor law's world +Z normal; it does not infer a
normal from arbitrary terrain data. The comparison example includes it using
0.1 N as a reporting cutoff, not a physical tolerance. Tests cover point-force
cancellation, internal-contact exclusion, cutoffs and invalid/incomplete traces.

The linkage coordinate map also shares one final Jacobian factorization between
velocity mapping and acceleration curvature. Position iterations still build
fresh matrices, all SVD rank checks remain, and nothing is reused across poses
or force trials. A full 100 ms hold run matches all prior frames, terminal
state, accepted contact trace, event schedule and solver counters exactly.
Single timings are 73.527 s after versus 74.675 s before: a small difference,
not an established realtime improvement. Analytic linkage, toggle-rejection,
implicit integration and motor tests pass in native and no-default builds.

A separate reporting fix stores actual floor depth in `ContactPoint.penetration`.
Previously normal damping contaminated that value because it was calculated as
normal force/stiffness. At 0.1 mm actual overlap and -0.5 m/s vertical speed,
a regression records the old incorrect value of 0.11 mm; the corrected output
stays at 0.1 mm for descending, stationary and ascending motion while force
changes appropriately. Forces and integration equations are unchanged. Old raw
captures retain the prior penetration field and are not corrected retroactively.

Reproduce the contact ablation by copying the recorded scene with only
`options.contact` set to false, then running the same two target configurations:

```sh
node <<'JS'
const fs = require('fs');
const scene = JSON.parse(fs.readFileSync('runs/full-robot/solver-performance/bracket-option-off.scene.json'));
scene.options.contact = false;
fs.writeFileSync('runs/full-robot/learning/servo-no-contact.scene.json', JSON.stringify(scene));
JS
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/learning/servo-no-contact.scene.json \
  examples/full-robot/mechanical-servo-worm-target.json \
  > runs/full-robot/learning/servo-no-contact-coarse.json
# Repeat with mechanical-servo-worm-target-refined.json, then compare the pair
# with compare_embedding and examples/full-robot/foot-markers.json.
```

`servo-contact-diagnosis-status.json` records the exact original source/binary,
scene/configuration/capture hashes and local factor-reuse source overlay.
Comparisons and raw captures are local evidence, not versioned golden tests.
The next physical experiment is an explicitly selected load-bounded friction
model checked against independent physical cases and the full loaded trajectory;
controller gains or contact parameters must not be tuned just to hide numerical
sensitivity.

## Explicit kinetic-Coulomb floor experiment

`BuildOptions.floor_friction` selects a shared Rust contact formulation. The
absent/default value is `{"kind":"bristle"}` and is omitted when serializing
legacy options. The explicit alternative is:

```json
{"kind":"regularized_coulomb","slip_speed_m_s":0.001}
```

This changes floor friction only. It retains CAD material kinetic coefficients,
normal contact stiffness/damping, collision geometry, internal body contacts,
motor/driver/servo behavior and clocks. The registry exposes
`floor.regularized_slip_speed` in m/s: zero retains bristle friction, a positive
finite value selects the alternative. Negative/nonfinite values are rejected.
The recorded scene carries the choice through native execution, replay,
numerical-reference comparison and WASM compilation. This is an environment
contact-formulation override, not a measured material-property change.

The existing compliant point-contact law in `sim-domain-multibody` now shares
a scalar regularized Coulomb kernel with its vector extension. For a foot patch,
let N be total normal load, mu_k the material's kinetic coefficient, r the
normal-force-weighted RMS tangential contact radius, and
u = (v_x, v_y, r * omega_z). The scaled wrench is

    w = -mu_k * N * tanh(|u| / v_s) * u / |u|, with w = 0 at u = 0.

Apply (w_x,w_y) at the patch centroid and torsional moment r*w_z. The combined
force/twist norm is bounded by mu_k*N and wrench power is nonpositive. A single
point has r=0 and no independent torsional capacity. This is a patch
approximation, not a reconstruction of every distributed friction force.
Original bristle state slots remain for layout compatibility and decay at the
existing 200/s rate, but do not contribute to forces in this memoryless model.

The approximation omits exact static stiction, the distinct static coefficient
and stored tangential elastic energy. A steady horizontal load of half the
kinetic capacity requires slip v_s*atanh(0.5): about 0.549 mm/s with v_s=1 mm/s,
or 0.0549 mm/s with v_s=0.1 mm/s. Smaller regularization can reduce creep while
increasing numerical stiffness; this general tradeoff is also discussed in
[Drake's contact defaults](https://drake.mit.edu/doxygen_cxx/group__contact__defaults.html).
Our radial-tanh patch law is not Drake's TAMSI/SAP algorithm and makes no claim
to those solvers' performance.

Tests verify combined force/twist capacity and dissipation across loads and
slip directions, zero independent point-contact twist, independence from stale
bristle memory, predicted creep under a subcapacity load, and decreasing
stopping-distance error with monotone kinetic energy. Registered and reduced
sliding dynamics agree. A recorded contact session replays exactly and agrees
with the independent numerical-derivative reference. These are component and
integration tests, not full-robot hardware calibration or an energy audit of the
entire robot.

With v_s=1 mm/s, 100 ms zero-target robot captures at 0.25/0.125 ms both complete,
using 448/869 accepted segments. All twelve servo clocks tick 100 times and
floor tangential/normal force ratios remain <=0.25 (up to roundoff). Maximum
sampled foot-position differences are +X 0.603 mm, +Y 0.717 mm, -X 0.856 mm and
-Y 0.765 mm. Held servo duty differs by up to 1.062785: -X worm at 66 ms commands
-0.062785 versus +1.0. The largest 2 ms floor-impulse vector difference is
0.192263 N s over 66–68 ms. Contact pairs differ at 12 of 51 samples. Native
stepping takes 73.502/100.985 s. The force-capacity test passes; full-robot
convergence, loaded-control and realtime gates do not. The option remains
experimental and does not replace the default model.

The exported starting geometry is also not a four-foot stance: the +X foot's
lowest collision vertex is 0.0181 mm above the floor, while the other three are
about 8.018 mm above it. Its provisional mass is 0.22144 kg versus 0.07689 kg.
No source geometry or mass was changed. These runs include an uneven landing;
a physically feasible starting stance and settling procedure are needed before
interpreting them as standing/stepping-control tests. The hardware symmetry
question is pending user clarification, but stance initialization can proceed
using the explicit authored geometry.

`regularized-floor-experiment.json` records the scene override and paired
configuration paths. Reproduce by copying the baseline scene and applying only
that `floor_friction` option, then use the existing hold configurations:

```sh
node <<'JS'
const fs = require('fs');
const spec = JSON.parse(fs.readFileSync('examples/full-robot/regularized-floor-experiment.json'));
const scene = JSON.parse(fs.readFileSync(spec.source_scene));
Object.assign(scene.options, spec.scene_option_overrides);
fs.writeFileSync('runs/full-robot/learning/servo-regularized-floor.scene.json', JSON.stringify(scene));
JS
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/learning/servo-regularized-floor.scene.json \
  examples/full-robot/mechanical-servo-hold.json \
  > runs/full-robot/learning/servo-regularized-floor-coarse.json
# Repeat with mechanical-servo-hold-refined.json and compare using the same
# foot-markers.json. The comparison now includes floor force-ratio diagnostics.
```

`regularized-floor-status.json` records exact source/binary, scene/configuration
and capture hashes, paired results and initial-geometry measurements. Ignored
raw runs remain local evidence, not versioned golden tests or promoted physical
calibration. CI exercises the shared kernel, articulated regularized-friction
cases, session replay/comparison and default-model regressions.
# Bounded support placement and explicit startup poses

`RigidEmbedding::point_jacobians` composes exact rigid point velocities with
the verified mechanism tangent. `place_points_on_planes` uses those derivatives
for local damped point-to-plane placement with explicit coordinate intervals,
step scales, line search and authored joint-limit checks (including dependent
joints). The base pose stays fixed during placement. Every accepted candidate
passes the original linkage closure checks. Failure returns no partially changed
input state. This is kinematics, not force equilibrium or a swept-collision test.

Three independently checked cases cover a slider-crank's analytic inverse
position, point derivatives under a translated/rotated base, unreachable and
fixed-coordinate targets, dependent-joint limits, and base-pose preservation.
They run with the existing embedding suite in native and no-default CI.

The `place_supports` example uses named CAD support links and the runtime's
lowest contact vertices. It reselects vertices after rotation and records
the point derivatives, all link poses, original closure rows and instantaneous
contact reports. Its CAD hash must match the scene. The recipes are:

- `support-placement-narrow.json`: initial +/-0.25 rad experiment. It cannot
  lower the three shorter feet to the original floor within those bounds.
- `support-placement.json`: +/-0.4 rad worm coordinates, other motors fixed.
  About +0.31052 rad on three worm outputs levels the feet geometrically, but
  produces a reported 0.6403 mm internal contact between the +Y worm/input
  spindle and hip shaft/pulley, with about 128 N contact force. Do not use this
  pose as a clean support reference. The collision representation versus actual
  hardware interference remains to be investigated.
- `support-placement-retracted.json`: raise the target plane by 8 mm, primarily
  retracting the longer +X foot with -0.276128 rad at its worm motor. Then the
  integration configuration translates the entire floating base down 8 mm.
  All four exported collision-vertex minima are about 20 micrometres above the
  unchanged floor, and the initial runtime reports no internal contacts. This
  is a common-height release pose, not a preloaded or settled stance.

The +/-0.4 rad interval is a local experiment bound, not a hardware range
certificate. No CAD geometry, mass, physical stop, transmission or material is
changed. The +X foot's assigned mass/geometry asymmetry remains unresolved.

`integrate_embedding` accepts optional `initial_coordinates` in its recorded CAD
motor order and `initial_base_translation_m` in world metres. It solves closure
at rest and rejects authored joint-limit violations. Registered `initial.angle`
parameters align each motor's gearbox output to its joint, with zero initial
current, rotor speed and elastic preload; backlash mode initialization remains
the original component's event. Servo history starts from the original firmware
defaults, and targets are explicit experiment inputs. The comparison tool rejects
different initial-coordinate or base-translation metadata.

Reproduce from the scene generated by the regularized-floor recipe:

```sh
cargo run --locked --release -p sim-runtime --example place_supports -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/support-placement-retracted.json > runs/full-robot/learning/support-placement-retracted.json
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-retracted-100ms-coarse.json > runs/full-robot/learning/servo-retracted-100ms-coarse.json
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-retracted-100ms-refined.json > runs/full-robot/learning/servo-retracted-100ms-refined.json
cargo run --locked --release -p sim-runtime --example compare_embedding -- runs/full-robot/learning/servo-retracted-100ms-coarse.json runs/full-robot/learning/servo-retracted-100ms-refined.json examples/full-robot/foot-markers.json > runs/full-robot/learning/servo-retracted-100ms-comparison.json
```

The supplied initial coordinates are the accepted placement output; regenerating
them requires explicitly copying that output and matching servo targets into both
integration configurations. Do not interpret a successful placement as a motor
command that teleports a running robot. Initializer, loaded tracking, collision
and timestep checks answer different questions. Compact outcome evidence is in
`support-placement-status.json`; full source/binaries are snapshotted under
`runs/full-robot/learning/support-placement-source`.

## Exact mechanical dynamics preparation

`RigidEmbedding::prepare_dynamics` returns an immutable, owned mechanical
snapshot tied to its model. It prepares the reduced inertia factorization,
original mechanical passive/contact loads, velocity and closure curvature.
Calling its `accelerations` method supplies fresh generalized component loads;
it does not reuse a finished acceleration or motor response. Projection keeps
the original subtraction/multiplication order and original balance check.

The optional `implicit.reuse_mechanical_dynamics` requires
`reuse_mechanical_endpoint`. A local cache holds preparation only for the same
mapped endpoint object, whose key is the exact bit pattern of the mechanical
velocity unknowns. Seed, timestep, time, contact history and model are immutable
within that continuous solve. A changed mechanical trial rebuilds preparation;
every new continuous solve, event interval or rejected-step retry owns a new
cache. Auxiliary component residuals and applied loads are always reevaluated.
The default remains disabled. Counts distinguish mechanical mapping preparation,
dynamics preparation and reuse; motor aggregate counts cover successful trials,
including discarded event-location probes, but not the internals of failed
Newton solves. Wall time still includes rejected work.

Tests compare coupled electrical/mechanical motion with an independently
assembled linear system, require bitwise cached/uncached states and accelerations,
and retain backlash event/rollback comparisons. A contact fixture independently
checks the force-to-acceleration increment, immutable preparation ownership,
changed-pose re-preparation and invalid inputs. Existing closure, contact-memory
and integration tests remain in CI. Full-robot recipes select the optimization in
`mechanical-servo-prepared-coarse.json` and `mechanical-servo-prepared-refined.json`;
all initial poses, servo targets, physical equations and timestep settings match
the corresponding `mechanical-servo-retracted-100ms-*` recipes. Compare outputs
with `compare_embedding` and additionally check the complete accepted contact
trace. See `prepared-dynamics-status.json` for measured results and limitations.

For optional phase timings, use `mechanical-servo-dynamics-profile-off.json`
and `mechanical-servo-dynamics-profile-on.json`. They set `profile_solver=true`
in a standalone process. The JSON `solver_profile` includes exact bucket call
counts and wall seconds for closure mapping, contact-history condensation,
dynamics preparation/application and coupled component equations, alongside the
existing Newton/Jacobian/factorization timers. Timers start after initialization.
Mapping and other phase times overlap the enclosing Jacobian/residual timers;
do not add those two views together. Compare profile-on trajectories against
unprofiled captures, and use separate unprofiled repeats for runtime claims.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-dynamics-profile-on.json > runs/full-robot/learning/dynamics-profile-on.json
```

## Direct original-closure Jacobian from shared rigid motion maps

`Articulated::rigid_closure_velocity_jacobian` now reuses the same rigid motion
columns as `rigid_mass_matrix`. One kinematic geometry pass propagates each
body's linear/angular motion columns; endpoint lever arms and axis-alignment
terms assemble every original loop row. Signed transmission derivatives are
explicit. No row is removed or set to zero using pose-local rank. Modal
flexibility and unsupported mixed joint parameterizations fail explicitly.
The existing inertia arithmetic and coordinate ordering are retained.

`embedding.direct_closure_jacobian=true` selects this implementation for the
experimental mechanism chart. The default still uses original unit-velocity
probes. SVD/QR selection, rank thresholds, position/velocity/acceleration closure
checks and all physical equations are unchanged. The comparator allows this
single implementation option to differ, reports both choices, and still rejects
changes to the other embedding settings.

Independent checks cover a moving planar four-bar and its toggle, misaligned
spatial loops, ball/prismatic/compliant joints, multiple floating bases and signed
transmissions. Matrix entries match independent original velocity probes, with
position differences and analytic slider-crank/KKT cases checking the reference
and direct paths. Direct-chart dynamics retain the independent integration
refinement tests. The rigidity/input rejection checks cover both shared users.

`audit_embedding` accepts an optional third sweep-config file. It checks the
direct closure matrix against the independent velocity-probe rank audit at every
sample, using the recorded row/column scales and a 1e-10 maximum-error rejection
threshold. Derivative verification has its own timer, excluded from kernel
performance timings. `direct-closure-sweep.json` prescribes 0.05 rad amplitudes
around the retracted startup coordinates; it is a kinematic check, not a
collision-free trajectory or hardware travel certificate.

```sh
cargo run --locked --release -p sim-runtime --example audit_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json 41 examples/full-robot/direct-closure-sweep.json > runs/full-robot/learning/direct-closure-sweep.json
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-direct-closure-coarse.json > runs/full-robot/learning/direct-closure-coarse.json
cargo run --locked --release -p sim-runtime --example compare_embedding -- runs/full-robot/learning/direct-closure-coarse.json runs/full-robot/learning/dynamics-reuse-on-coarse.json examples/full-robot/foot-markers.json > runs/full-robot/learning/direct-closure-coarse-comparison.json
```

Repeat with the `-refined` recipe/captures, and compare the direct coarse/refined
pair. Separate `mechanical-servo-closure-profile-off/on.json` inputs enable phase
timers. Closure Jacobian and factorization buckets sit inside mapping time; SVD
sits inside closure factorization, so these nested times must not be added.
The main Newton factorization is a separate calculation. Timing repeats should
use the unprofiled configurations and the same executable, alternating run order.
Results and source/recipe hashes are in `direct-closure-status.json`.

## Experimental Jacobian reuse across accepted steps

`ImplicitStepConfig.reuse_step_jacobian` is opt-in and defaults to false.
`ImplicitSolverWorkspace` carries only a numerical correction matrix, its
factorization and reuse metadata. `EmbeddedMotorBank::advance_with_control_cached`
accepts this caller-owned workspace; ordinary APIs create a fresh workspace for
each interval. `integrate_embedding` carries one workspace across its sampled
servo intervals. Changing this cache lifetime does not freeze motion, actuator
loads, contact geometry, contact memory or the endpoint residual.

The shared `sim_solve::solve_newton_numeric_cached` retains the reference forward
perturbation and Newton acceptance/refresh rules. `JacobianCache::clone` shares an
immutable factorization while keeping mutable counters independent. Both the
hybrid trial state and the outer interval own speculative workspace snapshots;
a failure, rejected root-location trial or failed final endpoint check cannot
commit their cache to the caller. A dimension mismatch starts a fresh matrix.

The reduced adapter clears reuse after every motor/firmware jump, a changed
accepted contact-pair list, noncontiguous time, a timestep change greater than
1e-10 relative, or 64 factor uses. Those last two values are numerical reuse
heuristics, not physical tolerances or timestep-error acceptance. A contact
transition occurring within a trial still evaluates the current contact law;
the existing stale-matrix convergence checks refresh as needed. Callers must
also clear the workspace after model edits, external state resets, coordinate
changes or changed external force-law definitions. The workspace is deliberately
not serialized as robot state.

Reproduce the loaded robot experiment with:

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-jacobian-reuse-coarse.json > runs/full-robot/learning/jacobian-reuse-on-coarse.json
cargo run --locked --release -p sim-runtime --example compare_embedding -- runs/full-robot/learning/jacobian-reuse-on-coarse.json runs/full-robot/learning/jacobian-reuse-off-coarse.json examples/full-robot/foot-markers.json > runs/full-robot/learning/jacobian-reuse-coarse-comparison.json
```

The off recipe is `mechanical-servo-direct-closure-coarse.json`. Repeat at the
`-refined` timestep and compare both against their corresponding fresh-matrix
reference; also retain the coarse/refined accuracy comparison. The paired
`mechanical-servo-jacobian-profile-off/on.json` recipes enable nested timers.
Use separate unprofiled alternating runs for timing. Successful-trial counters
include discarded event-location solves but omit the internal work of failed
Newton solves; wall time and global profile buckets include that rejected work.
The reproducibility and measurement record is `jacobian-reuse-status.json`.
