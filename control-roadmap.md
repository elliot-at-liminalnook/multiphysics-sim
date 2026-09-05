# Closed-loop control roadmap

The purpose of the library is to model advanced multi-component systems —
above all closed-loop control systems such as robots — so that control code
written in any language interacts with the simulated environment as if it
were real. This is the checklist for getting there. It follows the rule of
`domain-roadmap.md`: every item is a library contribution, and every group
ends in a scenario that proves it the way `surprise-tests.md` does — one
knob, a qualitative change, a published number, a falsifier.

What already exists and is relied on below: acausal plants compiled by
`sim-compile` into one `Runtime` with `get`/`set` on signal ids,
`advance_to_event`, sampled controllers as behaviors
(`control.position_pi`, plate 15), composite `MOTOR`/`BATTERY` ports,
planar joints and contact, and the stochastic lane (`Context::add_noise`).

---

## A. The external controller seam

- [x] **A1. `Coupler` trait.** In `sim-core`: `fn sample(&mut self, t: f64,
  sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError>`
  plus a `describe()` returning named, unit-bearing channels. First
  implementation is an in-process closure, so Rust controllers use the
  same seam as everything else.
  *Done: `sim_core::{Coupler, Contract, Channel, CouplerError, FnCoupler}` (`crates/sim-core/src/couple.rs`).*
- [x] **A2. `control.external` behavior** in `sim-domain-control`. Port
  families `sense.*` (signal in) and `act.*` (signal out); parameters
  `sample_period`, `offset`, `input_delay`/`output_delay` in samples.
  Actuator outputs are zero-order-held states; a guard fires at every
  sample instant so the integrator lands on it exactly; the delays are
  ring buffers of past frames, not continuous lags.
  *Done: `control.external` (`crates/sim-domain-control/src/external.rs`) with `sense.*`/`act.*` families, `period`, `offset`, `input_delay`, `output_delay` as frame ring buffers.*
- [x] **A3. Attaching couplers after compile.** `Runtime::attach(behavior,
  Box<dyn Coupler>)`, resolved by behavior id, with a compile-time error
  if an external element is stepped without one. Couplers must be `Send`
  so `Exhibit`s can hold them.
  *Done: `Runtime::attach(behavior, coupler)` derives the contract from the wiring (`Runtime::contract`); `Behavior::couple` / `Behavior::failure` hooks; a seam stepped without a coupler fails by name at its first sample.*
- [x] **A4. Frame protocol.** Lockstep, length-prefixed JSON (msgpack
  later): handshake `{channels: [{name, unit, direction}], period}`, then
  `{seq, t, sensors: [...]}` out and `{seq, actuators: [...]}` back. A
  `ProcessCoupler` (child process over stdin/stdout) and a
  `SocketCoupler` (Unix or TCP socket) implement `Coupler` over it.
  *Done: `sim-couple` — newline-delimited JSON frames (`hello`/`ready`, `sample`/`act`, `close`), `FrameCoupler::{spawn, spawn_command, connect, connect_unix}` with a reader thread and a timeout. Length prefixes were dropped in favour of newline framing.*
- [x] **A5. Client libraries.** Python first (`clients/python/simloop`):
  `for frame in loop: loop.send(cmd)` with `frame.t` in simulation time
  and channels by name. Then a single-header C client for the same
  protocol. Both ship an example PI controller.
  *Done: `clients/python/simloop` (stdlib only; `Loop.stdio/listen/listen_unix`, frames by name) with `examples/pi_controller.py`, `leg_controller.py`, `quadruped_gait.py`; `clients/c/simloop.h` single-header C99 with a P example and the quadruped trot (`examples/quadruped_gait.c`, the fast controller the viewer prefers); both tested against the real coupler.*
- [x] **A6. Failure semantics.** A controller that exits, sends a malformed
  frame, or (in lockstep) exceeds a configurable timeout is a
  `RuntimeError` naming the element — never a silent hold.
  *Done: exit, malformed frame, sequence mismatch and timeout are `CouplerError`s; the runtime reports `RuntimeError::Controller { element, time, message }`. Never a silent hold.*

## B. Time semantics

- [x] **B1. Lockstep is the default.** The runtime blocks at each sample
  until the controller answers; controller wall-clock speed never reaches
  the physics. Test: the same seed and controller give a bit-identical
  trace across runs.
  *Done: lockstep is the only mode of `control.external`; `tests/seam.rs` checks bit-identical traces, plate 27 checks Rust vs Python to the bit.*
- [x] **B2. Simulation time only.** Frames carry `t`; clients expose it and
  no wall-clock. A controller that reads its own clock is the falsifier
  for B1.
  *Done: frames carry simulation time only; plate 27's falsifier is a controller reading `time.perf_counter()`.*
- [x] **B3. Real-time mode** in `sim-app`: wall-clock pacing; a controller
  that answers late keeps the held command and increments a missed-deadline
  counter the viewer displays. Off by default; for hardware-in-the-loop and
  for driving the live viewer from outside.
  *Done as a coupler, not a viewer switch: `sim_couple::RealTime` wraps any coupler with a wall-clock deadline per sample, holds the command and counts missed deadlines when the controller is late; plate 30's exhibit shows the counter live in the viewer.*

## C. The boundary as if it were real

- [x] **C1. Sensor elements** (`sim-domain-control`, or a new
  `sim-domain-sensing`): `sensor.encoder` (quantisation to counts, optional
  index pulse), `sensor.imu` (accelerometer and gyro read from a planar
  frame, with bias and noise from the stochastic lane), `sensor.current`,
  `sensor.voltage`, `sensor.force`. Each has bandwidth (first-order),
  noise intensity, latency, and a sample-and-hold output.
  *Done: `sim-domain-sensing` — `sensor.{encoder, tachometer, imu, current, voltage, force}` on a shared chain of bandwidth, Erlang latency, sample-and-hold, quantum, seeded noise.*
- [x] **C2. Actuator elements**: `actuator.pwm_driver` (duty → voltage
  with dead time and supply limit, on the electrical lane of `MOTOR`),
  `actuator.servo` (a `MOTOR` composite wrapper with current and torque
  limits), and command quantisation on every `act.*` channel.
  *Done: `actuator.pwm_driver` (supply, on-resistance, dead band), `actuator.servo` (bandwidth, torque and current limits, current readout), `actuator.quantiser`.*
- [x] **C3. Faults as knobs**: dropout, stuck value, latency spike, via the
  guard/jump machinery so they are events, not parameter hacks.
  *Done: `fault.mode` 1 stuck / 2 dropout / 3 latency spike at `fault.time`, as guard events on the sensor chain.*
- [x] **C4. Units in the handshake.** Every channel carries its
  `QuantityKind` so a client sees "knee_angle (rad)" and nothing about
  states, lanes or islands.
  *Done: each channel's `QuantityKind` comes from the port wired to it; the hello frame carries names and units.*

## D. Robot plants

- [~] **D1. Finish `domain-roadmap.md` item 12**: minimal-coordinate
  elimination pass for joint chains, 3-D joints (spherical, universal),
  and the quadruped re-authored on library joints. Multiplier joints work
  today but a dozen of them make a slow robot.
  *Partly: `multibody.chain` is the elimination pass as an element (planar serial chains in minimal coordinates, recursive Newton–Euler, joints as rotational ports, owned tip frame). 3-D joints and compiler-level elimination between separately authored bodies are not done.*
- [x] **D2. Leg on the seam.** Build the leg from reusable elements with `control.external` and a Python
  PI controller. The specialized leg runtime has been removed.
  *Done: plate 31 rebuilds the leg from library parts and drives it with `leg_controller.py`; the old harness's checks pass and the hold currents match gravity torque / (N·kt) to 1 %.*
- [x] **D3. Quadruped closed loop** with an external gait controller and
  C1/C2 sensors and actuators; the phenomena viewer drives it.
  *See plate 32: body, four two-link chains, servos, encoders, tachometers, point contacts, and `quadruped_gait.py` trotting through the seam.*

## E. Proof scenarios (plates 27–30)

- [x] **E1. Language independence.** Knob: which coupler runs the same PI
  law (in-process Rust, Python child process). Number: traces identical to
  1e-12. Falsifier: a controller that uses wall-clock time diverges
  between runs.
  *Done: plate 27 `language-independence`.*
- [x] **E2. Latency-induced instability.** Knob: transport latency in
  samples. Change: a stable position loop goes unstable. Number: the
  boundary where the sampled loop's phase margin reaches zero (Jury
  criterion on the discretised plant).
  *Done: plate 28 `latency-instability`.*
- [x] **E3. Quantisation limit cycle.** Knob: encoder counts per turn.
  Change: a quiet loop begins to hunt. Number: the describing-function
  amplitude of about one count, and the frequency it predicts.
  *Done: plate 29 `quantisation-hunt` (posed at a count edge, where the quantiser is a relay; amplitude to 0.2 %, frequency within 35 %).*
- [x] **E4. Missed deadlines.** In real-time mode, knob: controller
  compute time. Change: a walking gait degrades to a stumble as the
  missed-deadline count climbs. Number: the fraction of missed samples at
  which the gait's Floquet multiplier crosses one.
  *Re-posed: plate 30 `missed-deadlines` uses the motor speed loop rather than a gait, so the number is the deadline boundary itself (compute time = sample period) and the sign flip of the loop's growth rate, not a Floquet multiplier.*

Each plate gets an `Exhibit` and a row in `surprise-tests.md`, and the
gallery is regenerated as before.

## Sequencing

A1–A3 and B1 first (an in-process coupler already proves the seam), then
A4–A5 with E1 as the acceptance test, then C1–C2 with E2–E3, then D1–D3,
and B3 with E4 last.
