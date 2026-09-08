# Resolve competing settled stance corrections

The -24 mm forward front-support teacher completes the revealed 32-second
transition sequence with all 19 swings passing, but stops 2.052 mm from its
body target. At 28/30/32 s all feet carry about 9.7–9.9 N and the inferred
body gain is 1.5, so support weighting and the settling guard are active.
The angular body and foot-position suggestions have cosine about -0.985.
All feet remain 3–3.6 mm ahead of their planned world positions. Correcting
those planted feet through fixed-base joint Jacobians can oppose body motion.
This is a measured conflict, not yet proof of the best remedy.

Reuse the shared Rust body/point correction components and effective servos.
Change only robot policy in Rhai: after the existing stopped/reference-stable
0.4 s guard, optionally scale foot-position feedback to zero. Test the existing
settled body gain 1.5 and a gain of 4.5 in a 2x2 ablation. The larger gain uses
settled_gain_increment=3.75, preserving the existing 0.25 base and 0.5 standing
increment. Retain support weighting, all corrections' existing bounds, and
the final software/CAD command check. There is no direct pose or force override.

Four cases use the same 32-second revealed development inputs, seed 0,
1.25 ms backward Euler, -24 mm support posture and original task gates:
reference (point scale 1 / body gain 1.5), point release (0 / 1.5), higher body
gain (1 / 4.5), and combined (0 / 4.5). Require exact original reference frames
and task transitions, plus exact pre-stop frames through 26 s in each ablation.
The new policy must validate its scale explicitly. Keep failed outcomes and
report full stopping, heading, tilt, collision, work and loaded-marker metrics.

This sequence is development data after the earlier hold-out was revealed.
A passing case needs minute/steering regression, fresh held-out commands and
disturbance/terrain evaluation before promotion. Stronger proportional feedback
may oscillate or saturate; an integral component is a later option if this
bounded causal screen leaves unacceptable steady error. No physics, speed,
accuracy or realtime budget changes follow from this controller experiment.
