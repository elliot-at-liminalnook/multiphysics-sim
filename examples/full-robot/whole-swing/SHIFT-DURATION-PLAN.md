# Reduce shift acceleration at fixed stride geometry

The contact-phase audit attributes 77.7% of summed loaded contact motion to
shift and return. Lowering friction smoothing 10x/33x leaves the worst-foot
motion ratio near 87.5% and increases compute cost. Test the controller's
traction demand next, retaining the original 1 mm/s contact profile.

Freeze two 60-second development runs before evaluating them:

- Double shift/return durations from 0.28/0.24 to 0.56/0.48 seconds.
- Quadruple shift/return durations to 1.12/0.96 seconds.

Raise, lower and settle remain 0.38/0.38/0.10 seconds. The shared Rust sequence
and body-advance fraction remain unchanged. The same normalized shift/return
path at 2x/4x duration has 1/4 and 1/16 the reference acceleration demand;
this is a reference-kinematics prediction, not an actual force guarantee.

The original 1.38-second nominal transfer at 3.75 mm/s advances 5.175 mm.
Set the new forward commands to 5.175 mm divided by the new nominal period:
**2.723684 mm/s** at 1.90 seconds and **1.760204 mm/s** at 2.94 seconds.
Move the positive command-posture anchor to the new speed so the same forward
support/stance posture is used. Set the software command maximum consistently.
The resulting planned spatial step remains unchanged; record and verify its
completed-transfer body reference endpoints. The slower branch explores the
speed/reliability tradeoff and is not an acceptable speed ceiling by assertion.

Keep the CAD robot, force coefficients, motor limits, fine timestep, solver
tolerances, body/point/integral gains, development pulse and stop time fixed.
Preserve every runtime/task outcome. Use the original supported-swing, tilt,
heading, collision and final stop gates. Apply the already declared prospective
anti-sliding screen: maximum per-foot integrated load-weighted contact speed
at most 5% of net horizontal body advance over the minute. Measure actual
sustained speed, work, contact motion by phase, and native compute separately.
Do not change thresholds after seeing results or infer timestep, browser,
terrain, fresh held-out or calibrated hardware qualification.
