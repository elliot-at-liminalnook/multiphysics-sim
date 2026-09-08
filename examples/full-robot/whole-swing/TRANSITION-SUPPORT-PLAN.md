# Repair the demonstrated turn-to-forward support limit

The predeclared mirrored-turn episode fails at 14.84 s for both the fine
teacher and fitted student, with identical planned support forces. The next
front-foot transfer starts after the forward command is already latched;
this is not a command change during the support shift. As body advance
continues during swing, the weak planned support falls from about 0.76 N
at 14.38 s to 0.510 N at 14.80 s, then below the unchanged 0.5 N gate.
The shared runtime computes these forces from the fitted reference pose and
CAD mass geometry, independently of learned motor feedback.

Treat this revealed episode as development evidence for any further tuning;
preserve its original held-out failure and never reuse it as untouched final
validation. With the teacher frozen, test the forward-command front-foot
support x offset at -22.5 and -24 mm, versus the existing -21.45 mm. These
changes move the planned body farther into support before advancing during
swing. They are policy offsets, not changes to CAD geometry, mass, friction,
actuator authority or acceptance thresholds. The -24 mm endpoint is the
existing neutral-turn posture, but forward-motion collision and dynamic
tracking still require their own checks.

Use the exact 32-second revealed command schedule and 1.25 ms teacher physics.
Retain both candidate results. Require the original static support, full
supported-swing, internal-collision, tilt, stopping and heading gates. For any
passing offset, also check the original minute and mixed steering so improved
transition support cannot hide a speed or collision regression. Select the
smallest added shift satisfying all development cases. If neither works,
inspect the complete swing's reference support and mechanism limits before
adding further parameter guesses.

Before further learning, reserve a new complete command/perturbation schedule
that has not informed tuning. Keep robustness-training inputs separate from
that evaluation. The separate timestep sensitivity of the student and browser
latency remain unresolved; a support-posture repair cannot qualify them.
