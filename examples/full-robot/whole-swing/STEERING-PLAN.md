# Separate forward, turning and reverse postures

The existing posture table ends at zero forward speed, so its final knot is
used for both pure turning and every positive speed. The asymmetric experiment
therefore applied its fast-forward support shifts to pure turning, which loses
static support in the live case. Keep the fast posture as an explicit +3.75
mm/s knot and restore the original zero-speed posture from the 2.5 mm/s source
configuration. This preserves the tested full-forward posture while assigning
turning its own existing support posture.

Use the existing reverse posture at an explicit -1.25 mm/s command limit, rather
than requesting the untested -3.75 mm/s reverse. This is a declared asymmetric
controller envelope, not a change to CAD motor limits. Keep yaw limits at
0.001 rad/s, half body overlap, 85% horizontal finish and all physical budgets.

Evaluate four 24-second cases: student at 20 ms forward/turn/reverse/stop;
teacher at 5 and 2.5 ms with the same scenario; and the student forward-only
case as a preservation check. Mixed commands are +3.75 mm/s until 8.4 s,
+0.001 rad/s until 16.8 s, -1.25 mm/s until 20 s, then stop. Forward preservation
uses the existing 16.8 s forward/stop actions. Teacher standing gain stays at
the original 0.5 increment; the stronger standing attempt failed and is not used.

Every task must pass the unchanged swing, geometry, tilt, position and heading
gates. Paired teacher trajectories retain the 1/0.5 mm foot/body screen.
No result certifies arbitrary keyboard sequences, turning at faster yaw rates,
fast reverse, browser timing or hardware transfer.
