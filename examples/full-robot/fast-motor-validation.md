# First fast-motor reduction screen

This is a physical-model experiment, not an accepted training model. It tests
whether eliminating fast internal motor dynamics permits cheaper execution
without materially changing the task. The detailed, validated final-refresh
model and browser build remain available and unchanged.

## Shared implementation and provenance

`robot.motor_unit` now exposes two explicit registry parameters, both defaulting
to zero: `dynamics.quasistatic_winding` and `dynamics.quasistatic_rotor` (unit 1,
integer 0/1). `BuildOptions.motor_dynamics` selects detailed, winding-only,
rotor-only, or combined quasistatic behavior through the same component in the
compiled and embedded Rust runtimes. Default serialization remains unchanged.

The winding reduction omits magnetic storage and enforces instantaneous voltage
balance. The rotor reduction omits internal rotor/gear inertia and enforces
instantaneous torque balance. Gear angle evolution, compliance, backlash laws
and events, friction, efficiency, temperature effects and heat-flow laws remain.
Original CAD inductance and inertia values stay in the parameter map; flags
record the approximation instead of rewriting those physical values to zero.
Rigid-body mass/inertia, linkage geometry, transmissions, driver limits, sampled
firmware, latency, world and policy are unchanged in the prepared experiments.

These reductions can change current spikes, reversal behavior and heat. They
require operating-envelope and hardware validation before any physical claim.
Algebraic current/rotor consistency at initial conditions and events must also
be considered; passing the current task screen alone would not resolve that.

The independent circuit/rotor test checks all four modes over voltage reversals
and three steps against a separate linear backward-Euler calculation. A second
test derives the combined model's compliant transmission relaxation law and
checks timestep convergence to its analytic solution. Six derivative tests
include reduced state-rate terms and engaged/free gearbox regions. Runtime
tests verify recorded mode flags, preserved source CAD and exact replay.
WASM compilation passes; these experiments have not replaced the browser's
validated controller preset.

## Measured results

All five robot runs complete 2.8 s. The screening thresholds were written before
the first run: maximum/RMS foot differences 0.5/0.2 mm, maximum motor-angle
difference 0.005 rad, per-foot impulse difference at most 1% of reference
impulse with a 0.001 N·s floor, and sampled supported lift. These are provisional
simulation-only screens, not hardware acceptance thresholds.

| Comparison | Maximum foot difference | Maximum current difference | Maximum contact-impulse difference |
|---|---:|---:|---:|
| Combined reduction, 1 ms vs detailed 0.125 ms | 4.098 mm | 0.583 A | 1.544 N·s |
| Combined reduction, 0.5 ms vs detailed 0.125 ms | 2.811 mm | 0.553 A | 0.809 N·s |
| Detailed 1 ms vs detailed 0.125 ms | 2.884 mm | 0.542 A | 0.816 N·s |
| Combined reduction vs detailed, both 1 ms | 1.529 mm | 0.279 A | 0.746 N·s |
| Winding-only reduction vs detailed, both 1 ms | 1.529 mm | 0.279 A | 0.746 N·s |
| Rotor-only reduction vs detailed, both 1 ms | 0.0000388 mm | 0.0000756 A | 0.0000132 N·s |

The combined model's sampled supported lift passes at both steps, despite
failing the accuracy screen. It is not suitable for promotion on that basis.
The near equality of winding-only and combined errors at the same step
identifies winding dynamics as the material physical reduction here. Rotor-only
differences are small, but it provides no measured cost advantage in this path.
The detailed 1 ms discrepancy independently shows that integration accuracy
also prevents simply increasing the step.

At the same 1 ms step, detailed/winding-only/rotor-only runs take about
52.46/54.19/53.45 s and build 2,277/2,363/2,372 Jacobians. The combined run takes
62.15 s with 2,366 builds. These are concurrent development measurements, not
isolated repeated benchmarks; no speedup from the reductions is established.
The large improvement relative to earlier small-step runs mostly comes from
fewer nominal steps, which currently fails the accuracy screen.

Current and heat differences are sampled at 10 ms and can miss fast switching
transients. They are not electrical-energy or thermal-accuracy validation.
Different backlash event counts are reported rather than silently paired.
No settings, tolerances or source values were loosened to pass the screen.

## Reproduce and compare

Prepare the previously validated point-feedback/final-refresh inputs, then:

```sh
node examples/full-robot/prepare_fast_motor.mjs
cargo test --locked -p sim-domain-robot --test motor_reduction --test motor_jacobian
cargo test --locked -p sim-runtime --test embedded_session --example compare_embedding
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
```

Run each prepared scene with `config.json`; use the original point-feedback
scene for the detailed control at the same step. The combined refined run uses
`refined.config.json`. Capture all outputs without changing CAD or the policy.

`compare_embedding candidate reference markers --motor-reduction` explicitly
acknowledges the physical comparison. It verifies that the selected mode agrees
with every motor's reduction flags and permits only those flags to differ.
Other captured parameters, world, controller and experiment metadata must
match. Without the option, differing motor dynamics are rejected. Tests cover
misdeclared flags, changed resistance/inductance and changed world conditions.
The preparation manifest additionally hashes the actual scene inputs.

`summarize_fast_motor.mjs` records same-step and refined-reference comparisons
separately, computes the motor-angle screen on motor DOFs rather than mixed
joint/state arrays, and hashes the captures, source snapshot and executable.

## Next decision

Keep winding dynamics. A useful next computational experiment is to solve the
motor's internal equations locally, preserving those equations while reducing
the unknowns and repeated geometry work in the global mechanical solve. This
must support the original driver/firmware boundaries and be validated against
the detailed path. Larger-step accuracy remains a separate problem. The fast
training model, accepted stepping and learning pipeline are still unfinished.
