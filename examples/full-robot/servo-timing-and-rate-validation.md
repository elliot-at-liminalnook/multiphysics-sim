# Servo timing and short-step precision

The follow-up tracking tests exposed two numerical defects, distinct from real
motor tracking error. Fixes belong to shared library components; CAD parameters,
firmware gains, backlash, inertia and force laws were not retuned.

## Periodic firmware deadline drift

`ServoFirmware` previously advanced its stored deadline by adding one period.
After 1,354 additions to a 0.001 s initial deadline, the stored value was
1.3549999999999616 s. The nominal 0.25 ms physics grid reached the same intended
boundary through different arithmetic. The hybrid scheduler then attempted a
tiny leftover physical interval; the 1.4 s slow-motion test failed in its final
hold. Increasing an arbitrary clock tolerance would only postpone that drift.

Firmware now derives the next tick index from transactional deadline state and
computes its time from the declared initial phase and period. No hidden mutable
counter is introduced, so rollback and checkpoint restoration preserve schedule.
The existing firmware state layout, PID, quantization, deadband, delay queue and
saturation are retained. Other independently implemented component clocks are
outside this focused change.

A regression checks one million updates for each of three rates/phases, including
state restoration. The full slow robot motion now completes 1.4 s. On the
original one-second trajectory, clock-only changes preserve all event counts
and sampled contact identities; maximum world-foot difference is 4.58e-16 m.
Motor states and some located backlash event times differ at roundoff scale;
captures are not bit-identical.

## Auxiliary rate coordinates

The no-gravity/no-contact diagnostic originally failed during backlash guard
location near 0.24045 s. Additional failure reporting identifies auxiliary row
18: the +X worm motor's gearbox-angle rate equation. A trial changed angle from
approximately -0.2761420885415331 to -0.2761420885448044 rad. Forming
`(new_angle - old_angle) / short_step` loses precision, preventing the original
rate residual from reaching its tolerance. Halving the interval does not repair
that cancellation.

The shared implicit integrator now has an opt-in `auxiliary_rate_unknowns` mode.
It solves for rates `r`, constructs physical endpoint states as `old + h*r`,
and passes the rates directly to registered component residuals. In exact
arithmetic this is the same backward-Euler relation. Floating-point endpoint
states still round normally, but rates are no longer recovered by subtracting
those rounded states. Original residual units, scales, Newton tolerances, motor
equations and event guards remain unchanged.

`step_implicit_coupled_with_rates` and `EmbeddedMotorBank::evaluate_with_rates`
provide the reusable interfaces. Existing state-based interfaces remain available;
the experimental option defaults to false. Changing coordinate representation
invalidates the correction-matrix cache. Rates and candidate states participate
in the same atomic hybrid trial; failed attempts cannot advance accepted state.

Focused tests cover an independent coupled linear motor/circuit solution with
and without inductance, piecewise-linear backlash engagement in both directions,
short-step angle precision, rollback and cache invalidation when switching
coordinate representation. Existing mechanics/contact integration checks and
native/non-default-feature suites pass; the WASM target compiles. These tests
establish numerical behavior, not calibrated hardware response.

## Full-robot results

All five rate-coordinate runs complete: loaded and zero-gravity/no-contact
at 0.25 and 0.125 ms, plus the slower loaded 1.4 s program at 0.25 ms. All 12
servos retain exactly 1,000 ticks per simulated second. The smallest accepted
unloaded event segment is about 55 ns; this is an event-location subdivision,
not a requirement to run the whole robot at that timestep. Maximum auxiliary
residual in successful trials stays below 4.7e-13 in these runs.

| Experiment | -Y foot-crank lag at 45% of return | Maximum body-relative foot tracking error |
| --- | ---: | ---: |
| Loaded, 0.25 ms | 3.7034 degrees | 1.7657 mm |
| Loaded, 0.125 ms | 3.7065 degrees | 1.8046 mm |
| Zero gravity/contact, 0.25 ms | 2.8803 degrees | 1.2110 mm |
| Zero gravity/contact, 0.125 ms | 2.8814 degrees | 1.2110 mm |
| Loaded, transitions twice as long, 0.25 ms | 2.8916 degrees | 1.5592 mm |

The command is -6.7967 degrees at that return phase in every case. Slowing the
command or removing external loading reduces the gap, but neither eliminates
the modeled tracking error. This is evidence that command timing and loading
matter. The combined gravity/contact removal does not isolate their individual
effects; inertia, internal losses and the whole robot's coupled motion remain.
These are simulation observations, not measured servo calibration.

At 0.125 ms, rate- versus state-coordinate loaded runs agree within 4.2e-14 m
in sampled world-foot position, with matching event counts. At 0.25 ms that
difference reaches 1.605 micrometres and 0.01759 duty; event counts and sampled
contact identities still match. Firmware quantization makes command comparisons
particularly relevant; close foot positions alone are not an acceptance gate.

Loaded timestep refinement changes sampled world-foot position by 0.2261 mm and
duty by 0.09065. Unloaded refinement changes foot position by 2.028 micrometres,
but some backlash guard counts differ: guard 15 fires 1 versus 3 times, and
guards 24/33 each fire 13 versus 15 times (coarse versus refined). Those transition
differences remain to be classified before promoting a training-model timestep.
No accepted internal contacts are reported in loaded runs; collision checking
was deliberately disabled in the unloaded diagnostic.

Single-run physics wall times are about 65.2/71.1 s for loaded coarse/refined,
39.3/39.6 s for unloaded coarse/refined, and 70.6 s for the 1.4 s slow run.
Some runs overlap builds. These are robustness experiments, not controlled
performance comparisons or a realtime result. The rate option remains opt-in.

`servo-timing-and-rate-status.json` records exact values, inputs, source snapshots,
complete capture hashes, process results, comparisons and test evidence. The
full planning/teacher/student/robust-learning/viewable-policy goal remains
unfinished. Next classify the near-boundary event differences and connect the
validated mechanism/actuator interface to feasible stepping references; hardware
calibration, collision geometry and the operating envelope remain open work.

## Reproduction

Use the scene derivation in `embedded-integration.md`. Diagnostic loading overrides
are explicit: a separate scene sets `robot.gravity` to `[0,0,0]` and
`options.contact` to `false`; robot inertia and internal forces remain present.
The source CAD is not modified.

```sh
cargo test --locked -p sim-domain-robot --test servo_clock --test embedded_motor --test embedded_step
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-knee-motion-rates-coarse.json
```

The `rates-refined` recipe halves the physics step while retaining firmware
sampling. The `rates-slow` recipe doubles commanded transition durations while
retaining amplitude and hold durations. Exact source snapshots and captures are
recorded separately from the older failed diagnostic; no failed capture is
relabelled as complete.
