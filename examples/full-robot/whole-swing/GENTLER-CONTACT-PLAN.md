# Contact smoothing under the gentler gait

The fixed-stride 2× shift/return gait passed the development minute at 2.741 mm/s,
but its worst foot accumulated loaded tangential motion equal to 29.2% of body
advance. Prior smoothing experiments used the faster body shifts, so they do not
isolate regularization creep under this gentler motion.

Freeze three 60-second development cases before evaluating them: regularized
Coulomb speed scales **1, 0.1, and 0.03 mm/s**. Repeat the 1 mm/s baseline using
the same native binary as both alternatives. Keep the versioned CAD robot,
material friction coefficients, effective servo model, controller gains, seed,
development push, 1.25 ms backward Euler settings, and input sequence identical.
Only `scene.options.floor_friction.slip_speed_m_s` differs between cases.

Reconstruct the existing 2× gait from the versioned integral-minute recipe:
double Shift and Return durations to [0.56, 0.38, 0.38, 0.48, 0.1] seconds, and
set forward speed to 5.175 mm / 1.90 s. Adjust its positive command-posture
anchor and software forward-command ceiling to that same speed. These are
controller settings, not changes to physical actuator limits. All three cases
use the same reconstructed gait. No ignored prior run is needed to prepare it.

Retain complete failures and runtime-error prefixes. Reuse the original task
gates and previously declared **5%** maximum per-foot loaded contact-motion /
net horizontal body-advance screen. Report sustained speed, stopping, heading,
qualified swings, work, native computation, and contact motion by phase. Audit
each fidelity profile independently and assert that all parsed robot, controller,
configuration, inputs and other physics options match across profiles.

Smoothing is an explicit numerical constitutive approximation, not a measured
material property or a controller improvement. A pass needs separate timestep,
command, fresh held-out, terrain and browser checks. Sampled contact velocities
omit birth/death intervals and between-sample peaks; the screen is provisional
and does not establish calibrated hardware traction. Keep the live profiles
unchanged during this experiment.

Prepare with `node examples/full-robot/whole-swing/prepare_gentler_contact.mjs`.
Pass a fresh directory argument to reconstruct the same authored recipes without
overwriting the committed plan or depending on prior ignored captures.
