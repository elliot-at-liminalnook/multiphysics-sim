# Passive coordinates and scheduled sensors

`embedded::Config.independent_coordinates` explicitly selects the mechanical
coordinate chart. Its order is separate from CAD motor order. Passive joints can
be independent, and transmission constraints can make actuated joints dependent.
The shared embedding validates the selected chart; no motor is synthesized for a
passive coordinate. Omitting the field preserves the previous selection.

Motor, driver, firmware, effective-servo and policy bindings continue to use named
motor DOFs. Explicit initial coordinates use the selected mechanical order, in
radians or metres according to the joint. Metadata records both the independent
joint indices and motor joint indices.

Authored IMUs now participate in the existing hybrid scheduler for coupled motor
advancement and backward-Euler mechanical subdivision. Sensor deadlines can fall
between nominal physics endpoints. The adapter uses the same sampling equations
as the registered articulated component, including axes, gravity, velocity
differencing, keyed noise, bias walk, quantization and range. Held samples are
returned as `imu_samples` with sampling times and SI units in field names. These
are not automatically appended to policy inputs. Policies can explicitly select
authored IMUs through the shared adapter in `imu-policy-observations.md`.

Unscheduled implicit stepping, explicit midpoint and SDIRK sensor support remain
outside this change. Use an explicit scheduled adapter; raw implicit calls with
IMUs continue to reject unsupported scheduling. Hybrid retries and replay retain
sensor state in the same transaction as mechanics and actuator state.

## Evidence

- Seven new runtime tests cover passive motion, independent-coordinate ordering,
  replay, invalid charts, dependent motor coordinates, detailed and effective
  actuator bindings, nonaligned IMU deadlines and stationary specific force in
  rotated sensor axes.
- Existing embedded/environment tests: 21 passed. Shared mechanical, motor and
  servo-clock tests: 35 passed.
- The wheeled CAD model retains its passive axle and IMU in native and WASM.
  The 15 ms sensor case matches exactly; reset, replay and rejected-edit lifecycle
  checks pass. The previous quadruped's 0.1 s native replay also remains exact,
  excluding wall time.
- Contact-enabled 20 ms runs complete. The 0.25 ms timestep diverges between native
  and WASM by up to 0.583 N in a contact-force entry. At 0.125 ms and 0.0625 ms,
  native/WASM entry differences are below 3.2e-14, with exact browser replay.
- Those finer timesteps are **not converged against each other**: maximum link
  position difference is 49.3 µm, joint velocity difference 0.988 rad/s, and held
  IMU specific-force difference 3.78 m/s² over the same 20 ms horizon. This remains
  an accuracy issue for later contact/controller validation.

`passive-sensor-evidence-v1.json` durably references all inputs, builds, tests and
captures, including failures and timestep comparisons. CI runs the sensor case
and the 0.125 ms contact portability case. These are short API/host checks, not
locomotion, sustained speed, realtime performance or hardware qualification.
