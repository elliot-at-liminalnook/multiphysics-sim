# Test the front-lift turning support margin

Separate turning postures still predict only 0.3978 N at the rear support
during the front-foot lift. With approximately 39 N total load and a 0.32 m
rear support arm, an additional 6 or 9 mm rearward body shift should add roughly
0.73 or 1.10 N to that support. Test neutral-posture front-lift support X at
-24 and -27 mm, compared with the existing -18 mm. The analytical estimates
are hypotheses; full CAD closure, reach and contact checks decide acceptance.

Use the previous mixed 24-second steering actions and separate posture knots.
Compare the student at 20 ms and teacher at 5 ms for each offset (four cases).
Keep fast forward at +3.75 mm/s, reverse at -1.25 mm/s, yaw at 0.001 rad/s,
all other posture values, walking/standing gains, weights, actuator parameters
and physical budgets unchanged. All cases remain development-only.

Require every swing, geometry, stopping, heading and tilt gate from PLAN.md.
Only a physical pass may proceed to a 2.5 ms teacher comparison or browser
control checks. Moving weight farther backward can violate another mechanism
or support constraint; preserve all such failures without changing thresholds.
