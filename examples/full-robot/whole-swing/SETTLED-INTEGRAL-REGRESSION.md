# Frozen integral teacher: sustained, refinement and fresh commands

Freeze gain 1.0 /s from the predeclared integral ablation: it passes the revealed
32-second case with 0.425 mm final body error, versus 0.758 mm at gain 0.5,
under identical bias/rate bounds. Both retain 19 passing swings and similar
work/command variation. Selection is based only on that development evidence.
Use the same -24 mm forward support posture, settled point release, stable
proportional gain 1.5, maximum bias 0.04 rad and rate 0.01 rad/s.

Predeclare five sequential cases before evaluating any:

- Original 60-second sustained-forward development inputs at 1.25 ms.
- Original 24-second steering/reverse/stop development inputs at 1.25 ms.
- The same minute at 0.625 ms, with unchanged equations and tolerances.
- Fresh 48-second command sequence, unforced: idle until 4 s, forward 3.75 mm/s
  until 12 s, stop until 16 s, reverse 1.25 mm/s until 24 s, stop until 28 s,
  forward 3.75 mm/s with +0.001 rad/s yaw until 36 s, in-place -0.001 rad/s yaw
  until 42 s, then stop through 48 s.
- That fresh sequence with -0.75 N lateral force at 19.2 s for 0.24 s, and
  -0.5 N forward-axis force at 33.1 s for 0.2 s. These are hypothetical bounded
  validation pushes, not a calibrated hardware distribution.

The two fresh episodes are held out from this candidate's selection. They
exercise initial idle and motion resumption while an integral bias releases.
Do not tune against their results while continuing to call them held out.
Keep both outcomes even if an earlier development case fails. The original
development pulse remains in the minute and steering cases; fresh unforced
and pushed cases explicitly replace that load schedule.

Keep the original swing, support, tilt, heading, stopping and collision gates.
Compare minute 1.25/0.625 ms trajectories at the unchanged 1 mm foot / 0.5 mm
body limits. Measure sustained displacement/speed, work and sampled marker
travel. For each fresh stop, additionally report body/yaw error and reference
phase at the last sample before the next motion command (16/28 s), and at the
final sample. Intermediate stops use the same 1 mm / 0.005 rad / idle criteria.
These cases do not prove terrain robustness, stochastic reliability, browser
performance or hardware accuracy.

Version the selected authored scene/config independently of ignored run files.
Use a fresh isolated WASM build before publishing this native-function-dependent
controller in the browser. The existing live bundle must remain intact.
