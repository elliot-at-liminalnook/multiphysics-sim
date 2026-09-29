# Robot physics lessons

Twenty full lessons, written 2026-09-28, on the physics a robot builder
meets: how joints accelerate and hold loads, what gearboxes, belts,
screws and wheels trade, how batteries, wiring, drivers and motors behave
electrically and thermally, and how sensors and controllers close the
loop. Each teaches one or a few concepts from `lessons/concepts.yaml`
and builds on the four earlier lessons (motor, worm, driver board, knee).

Every lesson has objectives, a hook, parts shaped as picture → equation →
worked example → question, a prediction or sketch before its key scene,
scenes with checked claims, live `sim-equation` readouts, sliders with a
challenge or a find-the-fault / design task where it fits, a reflection
with key points, key ideas, one SVG figure, a narrated explainer, and a
system generated in code
(`crates/sim-runtime/examples/build_lesson_systems.rs`).

Checked: `sim-lesson check lessons --compares`: 24 lessons, 0 errors, no
warnings. Seen: contact sheets (`lesson_frames`, card region) of every
scene in a test viewer. Voiced: every explainer (OpenRouter TTS,
$0.58 measured in all).

| # | Lesson | Teaches | Scenes (claims) | Interaction |
|---:|---|---|---|---|
| 10 | `inertia-acceleration` | rotational-inertia, inertia-distribution | spin-up (3), ring (1) | sketch, challenge |
| 11 | `gear-ratio` | gear-ratio, reflected-inertia | race (2) | challenge, study |
| 12 | `friction-deadband` | coulomb-friction, dead-band, viscous-friction | sweep (3) | sketch, remedy |
| 13 | `series-elastic` | spring-torque, series-elastic | wall (3) | challenge |
| 14 | `resonance` | natural-frequency, resonance, damping-ratio | tap (2), sweep (3) | challenge, study |
| 15 | `gravity-torque` | gravity-torque, holding-torque | lift (3) | fault task, study |
| 16 | `backlash` | backlash | rock (3) | phase plot |
| 17 | `linear-drives` | linear-ratio | belt-lift (2), screw-lift (2) | design task |
| 18 | `wheel-traction` | traction-limit, wheel-slip | launch (3) | study |
| 19 | `battery-sag` | internal-resistance, brownout | start (3) | pretest, challenge, study |
| 20 | `motor-heating` | joule-heating, thermal-time-constant, continuous-rating | five-minutes (3) | sketch, challenge, study |
| 21 | `back-driving` | generator-braking | brake (2) | challenge, study |
| 22 | `wiring-decoupling` | wiring-drop, decoupling | switching (3) | pretest, challenge, study |
| 23 | `h-bridge-modes` | bridge-modes | stop (2) | |
| 24 | `stepper-steps` | stepper-sync, acceleration-limit | move (2) | design task, study |
| 25 | `encoder-resolution` | quantization, speed-estimation | counts (3) | challenge |
| 26 | `pid-joint` | pid-control | p-only (2), add-d (2), add-i (1) | challenge |
| 27 | `loop-rate` | loop-latency | slow-loop (2) | challenge |
| 28 | `imu-tilt` | accelerometer-tilt, gyro-drift, complementary-filter | swing (4), blend (1) | challenge |
| 29 | `current-control` | current-loop | spin-up (3) | challenge |

## Library work done for them

Shared pieces, per the project rule to extend the library first:

- **New parts** (`library/parts`): `torque_command` (step, sine, chirp),
  `trapezoid_move` (limited-acceleration moves), `speed_estimate`
  (filtered derivative of an angle), `pi_current` (torque control),
  `tilt_imu` (accelerometer and gyro on a swinging link) and
  `complementary_tilt`. The part language gained the
  `LinearAcceleration` and `AngularAcceleration` quantities.
- **Display**: a `part.pendulum_gravity` is drawn as a swinging link
  (`InternalElement::Arm`), and parts on its shaft ride it; a
  `part.drive_wheel` rolls and carries its drive module; camera framing
  includes internal pieces; the split companion view now draws internal
  pieces (it showed only housings before, in older lessons too).
- **Fixes found on the way**:
  - `robot.effective_servo` read its target and wrote its torque at the
    wrong signal index, so it crashed in any system file (its unit test
    used the same wrong convention and was corrected);
  - the averaged H-bridge's floating output is now documented in its
    notes;
  - scene-run cache keys now include the authored parts a system uses,
    so editing a `.part` no longer replays a stale recording;
  - contact-sheet capture ignores prediction locks (as it already
    ignored gating).

## Accepted notes and limits

- `loop-rate` no longer plots the firmware's held command (its instant
  steps cannot be paced); the arm's angle tells the story.
- `wiring-decoupling` plays about 70 s: showing 160 kHz ringing at a
  followable pace needs it.
- `resonance/sweep` plays about 83 s for the same reason (a 10 Hz swing).
- All parameters are representative estimates for hobby parts, recorded
  with their reasons; none is measured. The lessons say so, and none
  claims to match this repository's robot.
- The IMU and complementary filter are planar and deterministic
  (vibration is two sines, not noise); the stepper model has no
  speed-dependent torque drop; tyre grip does not fall when sliding. Each
  lesson states the limit where it matters.
- Narration was checked for cue validity and generated, not judged by ear.
