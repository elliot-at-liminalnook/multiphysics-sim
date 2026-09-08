# Combine controlled turning with delayed standing feedback

The delayed standing teacher passes both minute task checks and changes no
physical frame before 57.12 s; all 145 observed boosted frames are idle. The
-24 mm turning posture passes the 5 ms short task, while its 2.5 ms counterpart
misses stopping at 1.112 mm. Combine the two independently motivated changes:
separate posture knots with the explicit +3.75/-1.25 mm/s envelope, -24 mm
neutral front-lift support, and reference-history-gated standing feedback.

Evaluate four cases: minute forward/stop at 2.5 and 1.25 ms, and 24-second
forward/turn/reverse/stop at both timesteps. Use the existing exact action
sequences, 50 Hz control, seed 0, 0.5 N development push, teacher gains,
zero network outputs, physical model, target bounds and acceptance budgets.
No weights or physical properties change. This is further development, not
held-out robustness or hardware validation.

Require all swings, sampled geometry, tilt <=0.01 rad, final position <=1 mm,
yaw error <=0.005 rad and idle completion. Require full paired trajectories
within 1 mm foot and 0.5 mm body difference. Confirm that combined minute
physics at 2.5 ms exactly preserves the original delayed-standing minute,
since the added posture knots affect only non-forward motion. Verify added
gain remains confined to idle frames in mixed motion as well.

The finer reference addresses the minute-long 5/2.5 ms body difference; it
does not claim calibrated physics. Browser realtime is assessed separately
after compiler experiments; these finer native runs must not overlap timing.
