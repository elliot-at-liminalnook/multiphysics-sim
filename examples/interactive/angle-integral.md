# Bounded sampled angular bias

`control.angle_integral` is a reusable Rust controller component. Its registry
descriptor supplies the same typed ports and parameter validation used by CAD
and model authoring. `angle_integral_update` exposes its pure update to Rhai;
the example is `angle-integral.controller.rhai`.

The inputs are `error` (rad) and `enabled` (dimensionless; values at least one
enable the registry adapter). Output `bias` is in radians. Explicit parameters:

| Parameter | Unit | Example |
| --- | --- | --- |
| `period_s` | s | 0.02 |
| `integral_gain_per_s` | 1/s | 0.5 |
| `leak_rate_per_s` | 1/s | 0 |
| `maximum_bias_rad` | rad | 0.04 |
| `maximum_rate_rad_s` | rad/s | 0.01 |

Sample once per declared period. Each enabled update forms
`desired = (bias + period * gain * error) / (1 + period * leak)`, limits
the bias change by `period * maximum_rate`, and bounds the resulting bias.
Disabled updates slew toward zero. The bounded bias is the only integral
memory, so saturation does not conceal additional accumulated error.

Rhai retains the bias as ordinary numeric controller state. Opening/resetting
a controller clears that state; replay starts from the same initial state and
inputs. Failed native updates leave prior Rhai commands and state unchanged.
The helper accepts integer or floating-point numeric parameters, rejects
invalid types/unknown fields, and uses the Rust constructor's scale checks.

The component changes commands, never robot poses or forces. Keep actuator
command limits and plant dynamics downstream. The sampled first-order plant
in its tests is an analytic control example, not an actuator calibration.

Run `cargo test --locked -p sim-domain-control --test angle_integral` and
`cargo test --locked -p sim-script --test angle_integral` for the kernel,
registry adapter, binding, replay and numeric-boundary checks.
