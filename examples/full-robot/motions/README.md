# Inspect the robot mechanisms in CAD

These patterns use the completed assembly's joint names. They command the servo
output shafts in degrees; the imported pose is zero. The shared 5:1 transmission
moves the thigh automatically, and closed-knee constraints solve the rigid link
and sliding foot. The belt's pulley ratio is enforced, but belt material is not
animated circulating around the pulleys.

- `worm-thigh.json`: ±150° worm input produces ±30° thigh movement.
- `knee-foot.json`: 0 to −145° crank movement retracts the foot, then returns.
- `leg-cycle.json`: simultaneous hip, thigh and foot inspection.

These modest demonstration envelopes are not validated hardware travel limits.
All tracks use smooth cosine interpolation. Run them from the **Pose** panel or
through the API while the completed robot is open:

```sh
curl -s -X POST http://127.0.0.1:8420/motion/programs \
  -H 'Content-Type: application/json' --data-binary @examples/full-robot/motions/worm-thigh.json
curl -s -X POST http://127.0.0.1:8420/motion/play \
  -H 'Content-Type: application/json' -d '{"program":"+X worm drives thigh"}'
curl -s -X POST http://127.0.0.1:8420/motion/pause \
  -H 'Content-Type: application/json' -d '{}'
```

Use `/motion/seek` with `{"program":"+X knee drives foot","time":4}` to inspect
maximum programmed retraction, and `/motion/stop` to restore the CAD pose.
Saving a pattern is undoable; playback never changes geometry or document revision.

The +X calf interference study found positive-direction contact at
+5.5273–5.5469° (CAD revision 844). The working joint now has a provisional +4.5°
upper stop, preserving just over 1 mm nominal clearance between the curved link
and calf tube. Negative-direction sampled poses through −70° clear that tube;
this does not certify the other collision pairs or a hardware operating range.
See `../knee-clearance.json` for the sampled geometry evidence. Earlier positive
retraction clips are diagnostic examples of the interference, not usable cycles.

The extended demo pauses at maximum retraction from 3 to 5 seconds. It raises
the foot about 146 mm. `../knee-retraction-clearance.json` records the second
interference: curved link / fixed knee housing at −150.3125 to −150.3320°.
The provisional lower stop is −147.5° (about 1 mm clearance); the demonstration
stays at −145°. Constraint-driven sliders obey declared limits, not the manual
slider widget's default 100 mm display range.

## Worm and belt close-up demos

- `belt-hip.json`: ±30° inside-body servo input drives ±30° hip swing through
  the existing 1:1 transmission. The worm assembly moves with the hip while its
  own drive coordinate stays at zero.
- `worm-thigh.json`: the hip stays at zero while the worm turns ±150° and the
  sector/thigh attachment turns ±30°.

The September 5 demo exports are in `runs/full-robot/videos/worm-drive.mp4`,
`belt-drive.mp4`, and the combined `worm-and-belt-demo.mp4`, with copies in `~/`.
Each individual clip is 8 seconds at 30 fps; the combined clip is 16 seconds.
These are kinematic illustrations; the belt surface does not circulate, and
these sweep ranges have not been checked for enclosure collisions.

Capture recipe for the current imported assembly:

- Temporarily isolate `Hip belt drive` (`956cde68c425`), `Thigh worm drive`
  (`028932c744c3`), and `Thigh root yoke` (`50cdcc038fb0`).
- Hide both enclosure parts (`88819d438438`, `2bedd420bf35`) to reveal the mesh.
- Orthographic view: target `[112, 0, 18]`, distance `380`, yaw `-125`, pitch
  `22`, FOV `40`; shaded edges, grid and connector markers off.
- Export each named pattern with `/motion/export`, at 30 fps, 1280 × 1600.
  The capture used a viewport aspect ratio of approximately 1310:1650.
- For the labeled close-up, crop to `1280:1000:0:250`, then pad to
  `1280:1160:0:120` for the title and caption. Check framing again if the
  viewport aspect ratio changes. Restore the previous visibility after capture.

The original CAD part visibility was restored after these exports. The motion
programs remain available from the Pose panel. Decoded video validation is saved
alongside the local clips in `drive-demo-validation.json`.
