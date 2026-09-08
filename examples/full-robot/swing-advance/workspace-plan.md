# Preserve the leading foot's forward workspace

The first overlap sweep rejects 3.75 and 5 mm/s at the leading side leg's
gear/pulley geometry. For a transfer period T = 1.38 s and four legs, its landing
reference relative to the starting body is 4Tv plus the configured stance offset.
At 2.5 mm/s with an 8 mm offset this is 21.8 mm; the rejected faster targets are
28.7 and 35.6 mm. Overlapping body movement reduces this relative excursion by
only part of one transfer's body advance.

The next declared development sweep changes the two lateral legs' stance offsets
to `0.008 - 4 * 1.38 * (v - 0.0025)` metres: 1.1 mm at 3.75 mm/s and -5.8 mm at
5 mm/s. This preserves the first leading-foot forward endpoint at 21.8 mm,
while increasing stride travel. It is a controller posture change, not a CAD
geometry, joint-limit or force-law change. It does not prove that the other
legs or the full trajectory fit the mechanism.

Test both speeds with sequential and half-overlap body motion, each at 20 and
5 ms physics over the same 24-second command schedule. The network, support
shifts, motor limits, geometry checks, physical and numerical budgets remain
unchanged. Every outcome is retained. Further tuning or sustained validation
must use a new plan rather than rewriting a failed case.

```sh
node examples/full-robot/swing-advance/prepare_workspace.mjs
node examples/full-robot/hybrid-speed/run.mjs runs/full-robot/learning/swing-workspace examples/full-robot/swing-advance/workspace-status.json
```
