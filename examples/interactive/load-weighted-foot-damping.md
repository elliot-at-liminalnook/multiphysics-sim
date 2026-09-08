# Loaded-foot velocity damping

`control.load_damping` is a stateless shared Rust controller component. Its
typed inputs are velocity in m/s and nonnegative normal load in N; its output
is a signed displacement objective in m:

`delta = -velocity_damping_s * min(normal_force_n / full_support_force_n, 1) * velocity_m_s`

Both parameters are explicit. Seconds must be finite and nonnegative; support
force must be finite and positive. Invalid inputs fail validation. Registry
equations, direct Rust calls and Rhai's
`load_damping_displacement(velocity, normal_force, parameters)` use the same
kernel. This component commands no physical force and provides no actuator
authority of its own.

The runtime's existing point-feedback helper optionally applies this objective:

```json
"floor_velocity_damping": {
  "velocity_damping_s": 0.2,
  "full_support_force_n": 1.0
}
```

This example is a controller parameter, not a calibrated robot property. Omit
the key for the original position-only behavior and serialization. With the
option enabled, the shared articulated evaluation measures each marked link's
normal-force-weighted contact velocity using `v_COM + omega × (contact - COM)`.
The helper adds damping only in world X/Y, attenuates it with the marker's
existing activation, and solves the same bounded angular correction problem.
Unloaded feet get zero damping. Ordinary CAD-derived servo dynamics execute
the resulting target; the helper does not move poses or alter friction.

The current adapter explicitly requires stationary world-Z flat-floor contact.
It excludes internal contacts, rejects terrain, and uses privileged simulated
contact velocities. It does not establish a deployable sensor estimator or
sim-to-real accuracy. Telemetry reports normal loads, contact velocities and
the displacement objectives before activation and angular saturation. The
optional observation currently adds a force evaluation; its computation cost
must be measured along with tracking and contact motion.

Focused tests cover rolling cancellation, unequal contact loads, internal
contact exclusion, unloaded/zero-gain behavior, correction direction and cap,
registry units, and transactional Rhai input errors. Robot-level improvements
and stability require separate recorded experiments.
