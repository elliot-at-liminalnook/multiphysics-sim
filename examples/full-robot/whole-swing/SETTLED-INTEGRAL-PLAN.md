# Bounded integral stance bias after the proportional ablation

The point-release teacher stops at 1.776 mm error, while gain 4.5 produces
large command alternation and 127–194 mm drift. Do not increase proportional
gain further or relax the task. Reuse the stable gain 1.5 and investigate a
slow bias that can persist when body error becomes small.

First add a reusable Rust sampled angle-integral component in sim-domain-control.
Its scalar state/output is an angle bias, input is angular correction, gain is
1/s, period is seconds, bounds are radians, and slew limit is rad/s. Expose the
same parameter validation and typed ports in the shared registry. Rhai should
call the Rust update through a small native binding, with ordinary numeric
state in its existing replayable controller map. Do not duplicate the update
in robot scripts or introduce a robot-specific runtime.

For a held input e, form desired=(bias + period*integral_gain*e)/
(1 + period*leak_rate), then bound the change by maximum_rate*period and the
result by +/-maximum_bias. When disabled, desired is zero and the same slew
limit releases the bias gradually. Reject nonfinite inputs/parameters and
out-of-bound prior state. Zero integral gain is allowed for exact reference
identity; all other scale constraints must be explicit. Output remains a
servo target correction, subject to existing software/CAD limits and actual
servo force/speed dynamics.

Test saturation and reversal (no hidden windup), leak and disabled release,
invalid inputs, deterministic replay, registry units/parameters, and Rhai/Rust
agreement. Check rejection and unchanged caller state on failed updates.
Include a sampled first-order disturbed plant demonstrating bias rejection at
the declared timestep; label it an analytic controller test, not robot evidence.

Then use the same revealed 32-second development case, -24 mm support posture,
1.25 ms backward Euler and stable settled point-release controller. Integrate
only after the existing stopped/reference-stable guard, scaling its input by
the existing support weight. Disable and slew toward zero while moving.
Predeclare gains 0, 0.5 and 1.0 /s, leak 0 /s, maximum bias 0.04 rad,
maximum rate 0.01 rad/s, period 0.02 s. Require exact gain-zero reference and
pre-stop frames. Preserve all outcomes and original task/accuracy budgets.

A passing case still needs minute/steering regression, stop-to-motion checks,
fresh held-out commands, disturbances, terrain and browser evidence. This new
native Rhai binding requires a new isolated WASM build before any browser
recipe uses it; the existing live 14-entry bundle must remain intact.
