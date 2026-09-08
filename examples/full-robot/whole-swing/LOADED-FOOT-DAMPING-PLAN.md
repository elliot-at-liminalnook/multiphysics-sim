# Loaded-foot velocity damping development ablation

The direct-transfer minute preserves 3.696 mm/s travel but fails heading and
the previously declared contact-motion screen: worst-foot integrated loaded
contact motion is 45.2% of net body advance, against a 5% limit. Shift accounts
for most remaining contact motion. Position-only marker feedback does not
explicitly oppose the observed tangential contact velocity.

Freeze four 60-second native development cases before observing their outcomes:
the unchanged direct-transfer minute with no new option, then velocity damping
of **0.05, 0.2 and 0.5 seconds** with full support at **1 N**. At 20 mm/s contact
velocity these correspond to 1, 4 and 10 mm displacement objectives before
existing activation and the 0.04 rad angular correction cap. This is a broad
controller-gain ablation, not a change to physical friction or motor damping.

Keep the exact versioned CAD-derived scene, direct-support gait, time steps,
controller source, other feedback gains, actuator limits, environment, seed
and complete command sequence fixed. The new objective uses only horizontal
material-contact velocity and disappears when unloaded. It remains privileged
teacher feedback. No physical robot property is inferred or overridden.

Retain the original task gates and the prospective **5%** contact-motion screen.
Report speed, swing/support qualification, heading, stop error, contact motion,
shaft work, runtime cost and phase attribution for every completed run. Retain
runtime failures and accepted failed prefixes without granting them complete
minute metrics. Verify that the rebuilt option-absent baseline matches the
previous direct minute's physical frames, transitions and recording exactly,
excluding only wall-clock telemetry. Do not call a gain successful because it
reduces sliding while failing the task or sacrificing the useful speed.

This is development data. Any promising case still needs steering, timestep
refinement, fresh held-out commands/disturbances, terrain and rendered browser
validation. The current added contact observation may increase computation
cost; it is not a realtime performance improvement by itself.
