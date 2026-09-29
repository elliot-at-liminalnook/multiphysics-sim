# Camera turntable

An outward-facing camera on a belt-driven turntable. It turns continuously,
stopping to shoot, so the photos can be stitched into a 3D scene using the
angle the servo reports.

Status (2026-09-28): the CAD model and the drivetrain simulation are done.
Nothing has been printed or measured yet. Every value is labelled as
measured, derived or estimated where it is used.

## Parts to buy

| Part | Qty | Notes |
|---|---|---|
| Adafruit 736 slip ring with flange (22 mm, 6 wires, 2 A) | 1 | ~$15; carries 5 V to the disc. A clamp ring holds its flange, so its hole pattern doesn't matter |
| 6808-2RS ball bearing (40 × 52 × 7 mm) | 2 | Standard size, sold everywhere |
| GT2 closed-loop belt, 610 mm (305 teeth), 6 mm wide, glass fibre | 1 | Common stock length |
| M3 screws (8–10 mm), M4 × 12 screws with nuts, M2 and M2.5 screws | – | Pulley to horn, cap, carrier, clamp; cradle to base; camera; board |

Already owned: HX-30HM servo, Raspberry Pi Camera Module 3 Wide (and a
spare), ESP32-P4-WIFI6-M (and a spare), 150 mm Pi Zero camera cable.

## Parts to print (PLA, from `cad/turntable.rcad`)

- Base and bearing post (one piece, flat)
- Disc with GT2 280-tooth ring and bearing hub (one piece, upside down)
- Servo cradle (slides to tension the belt), GT2 30-tooth drive pulley
- Bearing retainer cap, slip-ring carrier, flange clamp
- Camera mount, ESP32 standoffs

**Measure before printing:** the servo's case and shaft position, and the
horn's screw circle; the slip ring's body length; the ESP32 board's outline
and holes. These are marked `measure_before_print` in the CAD metadata. Print a
pulley-tooth coupon first: the GT2 groove shape is an approximation.

## Layout

- The belt ratio is 280 / 30 = 9.33. The servo sits 123.3 mm from the axis,
  which fits the 610 mm belt exactly.
- The belt wraps 99.6° of the drive pulley, so 8.3 teeth are in mesh. Six or
  more is safe against skipping.
- The camera's optical centre is 84 mm above the base and 156 mm from the
  axis. It faces outward, with a 102° horizontal field of view.

## Rebuilding

```sh
# CAD (live window: one undo step; re-running replaces the previous build)
cad/.venv/bin/python -c "from robocad.client import RoboClient; RoboClient().script('examples/camera-turntable/cad/turntable_model.py')"
# save the window's document to cad/turntable.rcad, then:
cad/.venv/bin/python examples/camera-turntable/cad/derive_physics.py   # *.physics.json from the CAD bodies
cad/.venv/bin/python examples/camera-turntable/cad/export_models.py    # viewer display models (models/)
# system file (from an empty file; the HX-30HM definition comes from library/systems)
target/release/sim-system new examples/camera-turntable/turntable.system.json --title "Camera turntable"
target/release/sim-system library import examples/camera-turntable/turntable.system.json library/systems/hx30hm_knee_gearmotor.definition.json
python3 examples/camera-turntable/build_system.py | target/release/sim-system apply examples/camera-turntable/turntable.system.json -
for x in belt:belt-drive disc:disc-inertia pulley:pulley-inertia bearings:bearing-friction; do
  target/release/sim-system cad-params examples/camera-turntable/turntable.system.json / ${x%%:*} examples/camera-turntable/cad/${x##*:}.physics.json
done
target/release/sim-system run examples/camera-turntable/turntable.system.json 10 --select camera_angle --select servo_angle --select schedule
target/release/sim-spatial --system examples/camera-turntable/turntable.system.json   # viewer
```

## Where the numbers come from

- **The HX-30HM drive train, driver and FPGA controller gains, period, latency
  and encoder resolution** come from the accepted actuator registry (family
  `hx30hm-knee-measured`), referenced by its content hash.
- **Belt radii, belt stiffness, disc and pulley inertia** are derived from the
  CAD bodies (`cad/*.physics.json`, which include the CAD file's SHA-256).
  - The belt's EA (stiffness) is an estimate, ±50 %.
  - Printed masses are scaled by a fill fraction of 0.6 ± 0.15. Weigh the
    printed disc to replace this.
- **Bearing and slip-ring friction and bearing drag** are estimates.
  Measure them with a coast-down, or with a spring scale on the rim.
- **Supply voltage (12 V) and the shooting schedule** are design choices: 10°
  per shot, a 1.2 s move and a 0.8 s dwell.

## Results so far

From a 10 s run (5 shots); halving the timestep changes angles by ≤ 0.007°:

- **Settling:** each stop settles to within 0.01° 50–125 ms after the move ends.
  The 0.8 s dwell has plenty of margin, so it could be about 0.2 s.
- **Accuracy at the stop:** the camera ends 0.001–0.004° short of the target
  (0.05–0.2 px at about 45 px/°).
- **Servo reading vs. true camera angle:** they agree to 0.0001° during the
  dwell, because belt stretch under the friction load is negligible.
- **Speed:** peak turning speed is 17.6°/s. At that speed, shooting while
  moving would smear the rolling-shutter image, so stop and shoot.
- **What dominates:** the disc inertia seen from the servo (3.3 × 10⁻⁵ kg·m²)
  is tiny next to the gearmotor's reflected rotor inertia. The servo loop and
  the gearbox dominate; the belt does not.

These results are **not calibrated**. The HX-30HM gearbox backlash is
unmeasured (the family estimates 0). Turning in one direction only keeps the
gears loaded on one flank, which should keep backlash from causing error.
Measure backlash anyway.

## Assumptions and gaps

- **The FPGA position loop is single-turn.** The deployed RTL of
  `control.sampled_fixed_pd` refuses positions outside 0–4095 counts.
  - The simulation uses the same integer law with `multi_turn = 1`, which
    feeds it count differences (see `fixed_pd::step_differences`).
  - The hardware needs the equivalent change (a turn counter in the FPGA)
    before it can turn continuously. This is not done and not deployed.
- **Homing:** add a home switch or magnet on the disc to reset the turn count.
  The count is lost if the serial link drops.

## Virtual camera and reconstruction

`target/release/sim-scan examples/camera-turntable/scan.json` runs one full
turn of 36 photos, one every 10°, in about 2.5 minutes. It:

1. **Simulates** `turntable.system.json` on the shared runtime, getting the
   true disc angle and the servo's encoder angle over time.
2. **Renders** each photo from the true angle trajectory in the room
   `scene/room.scene.json`. The camera is a Camera Module 3 Wide as an ideal
   pinhole at 1152 px, with rolling-shutter readout and exposure time. The rig
   geometry comes from CAD (`cad/camera-rig.json`).
3. **Records** the angle the servo would report for each photo: whole encoder
   counts ÷ the belt ratio, both read from the system file.
4. **Refines** those angles from the images, one angle per shot, and keeps the
   servo angles unless the images improve.
5. **Reconstructs** depth by plane-sweep stereo with the true, reported and
   refined poses. It then fuses a point cloud, writes panoramas and a COLMAP
   model, and compares everything against the renderer's exact depth.

Outputs go to `runs/camera-turntable/scan-*/`: `shots/`, `contact-sheet.png`,
`panorama.png`, `cloud.ply`, `colmap/` and `report.json`. `panorama.png` has
three panels: ground truth, 3D reconstruction, and a direction-only stitch.
The code is in `crates/sim-vision`, and the runner is
`crates/sim-runtime/src/bin/sim-scan.rs`.

Results (2026-09-28; an uncalibrated virtual camera, Lambertian scene):

| Scan | Pose error vs truth | Median depth error | Within 5 % |
|---|---|---|---|
| Stop and shoot (`scan.json`), servo angles | 0.004° rms | 0.85 % (oracle poses 0.76 %) | 84 % |
| Same, with injected servo errors of 0.20° rms (`scan-servo-errors.json`), servo angles | 0.20° rms | 12.6 % | 25 % |
| … after image refinement | 0.11° rms (shot-to-shot step 0.58° → 0.08°) | 3.4 % | 66 % |
| Shooting mid-move (`scan-while-moving.json`) | 0.003° | 3.0 % even with oracle poses | 65 % |

Other results from the stop-and-shoot scan:
- **Point cloud:** 384k points, 59 % of pixels reconstructed, median error
  12 mm at 1–3 m.
- **Refinement:** it found no consistency gain and declined, keeping the
  servo angles.

What this says about the hardware:

- **The servo angle is the best pose source.** It is 0.004° from the true
  angle here, below what matching images can resolve (about 0.01° at 576 px).
- **Pose errors ruin depth.** 0.2° of shot-to-shot error turns 0.8 % depth
  error into 12.6 %.
- **Image refinement repairs shot-to-shot errors** such as a skipped tooth,
  backlash or a bad reading. Unit test: 0.21° → 0.008° rms.
- **It cannot repair a smooth once-per-turn angle error.** Neighbouring shots
  cannot see it: it trades exactly against an inverse-depth offset
  (Δ(1/Z) = (dε/dθ)/r). Widening the neighbours to ±45° did not help.
  - A printed ring 0.15 mm off-centre gives about 0.1° of once-per-turn error,
    which is about 3 % depth error at 2.5 m.
  - Measure the ring's runout, or calibrate the angle once against a known
    target, for example the checkerboard at a measured distance.
- **Stop and shoot is about 4× more accurate than shooting while moving.**
  At peak speed the disc turns 1.3° during one rolling-shutter frame.
- **Blank walls and the table top seen edge-on give no matches.**
  Completeness is a trade-off: with a 5 × 5 window and a strict texture
  threshold it is 22 %; with 7 × 7 and a looser one it is 59 %, at 5× the
  compute. The matching settings are in the `reconstruct.mvs` block of the
  scan file.

Limits: the camera is an ideal pinhole, and real photos must be undistorted
with a measured lens calibration first. The scene is procedural and
Lambertian with fixed light, and there is no sensor noise, auto-exposure or
autofocus breathing. On real photos, `colmap/` gives COLMAP the refined poses
as a starting point.

## Place models and AI tools

A **place model** is what an AI (or the simulator) can ask about a scanned
place. `sim-place build` fuses scans into one:

1. Stereo outliers are dropped: a depth has to agree with two other views.
2. Small holes are filled where the neighbouring depths agree.
3. Extra scan stations are registered to the first with level 4-DoF ICP
   (x, y, z, yaw), starting from a tape-measure guess.
4. Everything is fused into a signed-distance volume.

It writes `place.json`, a coloured mesh (`mesh.ply`, `mesh.obj`), the room
layout, a height map and the photos with their poses. Unknown space stays
unknown in every answer.

```sh
target/release/sim-scan examples/camera-turntable/scan.json --out runs/camera-turntable/scan-station-a
target/release/sim-scan examples/camera-turntable/scan-station-b.json --out runs/camera-turntable/scan-station-b
target/release/sim-place build runs/camera-turntable/place runs/camera-turntable/scan-station-a runs/camera-turntable/scan-station-b
target/release/sim-place query runs/camera-turntable/place place_overview '{}' --images /tmp
target/release/sim-place mcp runs/camera-turntable/place        # MCP server (registered in .mcp.json)
target/release/sim-place world runs/camera-turntable/place world.json   # simulator floor + terrain heightfield
target/release/sim-spatial --place runs/camera-turntable/place  # walk through it (WASD, drag to look)
```

**AI tools.** The command line and the MCP server share one set of tools:

| Tool | What it does |
|---|---|
| `place_overview` | Frame, size, floor and ceiling, walls, tables, free floor area, stations; plus a top-down map image |
| `render_view` | A view from any pose: `photo` blends the real photos, `shaded` shows fused colours, `depth` colours by distance |
| `list_photos`, `get_photo` | The real photos; `get_photo` can circle a place point on a photo |
| `photos_of_point` | Which photos show a point, and at which pixel (visibility checked) |
| `raycast`, `measure`, `clearance` | First surface along a ray; distances and line of sight; distance to the nearest surface |
| `surfaces`, `height_at` | The room layout; what is in a vertical column |
| `free_space_map`, `plan_path` | Free, occupied and unknown floor for a height band; A* paths for a round robot. When the start or goal is blocked, it suggests the nearest usable point |

Answers are JSON text, and images come as PNG. The frame is described in
every overview. The tools run in 0.06–0.7 s.

Results (2026-09-28, simulated room, two stations — table, and a stool
1.6 m away and turned 35°):

- **Registration** from a guess 11 cm and 5° off: 8.9 mm, 0.02°.
- **Mesh accuracy:** median 5.5 mm, p90 30 mm (30 mm voxels).
- **Rays from every third photo:** 87 % agree with the true scene within 5 cm
  (median 6.7 mm).
- **Floor height:** found at −0.744 m (truth −0.750).
- **Layout:** 5 walls, the floor and the ceiling found.

**Coverage limits** — the tools report these as unknown:
- A camera 84 mm above the table never sees the tops of nearby objects.
- The table hides the floor out to about 4.5 m (the stool station hides much
  less).
- A higher camera, the spare camera tilted down, or more stations would fill
  this in.

**Objects.** The object-scanning kit (`cad/object_scan_kit.py`) is a platform
on the disc and a fixed camera stand with two detents (15° and 35° down). The
ESP32 and camera move to the stand, so nothing winds up. Its rig files
(`cad/object-rig-*.json`) come from CAD. With the object on the disc, the
camera orbits it the other way in the object's frame.

```sh
target/release/sim-scan examples/camera-turntable/scan-object-low.json --out runs/camera-turntable/scan-object-low
target/release/sim-scan examples/camera-turntable/scan-object-high.json --out runs/camera-turntable/scan-object-high
target/release/sim-place build runs/camera-turntable/object-place runs/camera-turntable/scan-object-low runs/camera-turntable/scan-object-high --voxel 0.002
```

Object result: rings registered within 0.14 mm and 0.01°; surface error
median 0.53 mm; 98.5 % of rays within 5 cm (median 0.66 mm). The model is
`object-place/mesh.ply`. Use a plain backdrop and diffuse light: in the
object's frame the room and a directional light turn with the camera.

**Simulator.** `sim-place world` writes the simrobot `world` block: the floor
height plus a terrain heightfield of the highest surface in each cell.
Never-seen cells are raised to ceiling height, so unknown space counts as an
obstacle, and the file says how many.

**Photoreal views** are image-based: each pixel blends the three real photos
that saw the point from the nearest directions. They are realistic near the
scan positions and fall back to fused colours elsewhere. They are not
Gaussian splatting.

## Printing it

`print/` checks the printed parts against the loads the simulation gives, and plans how to print them (see [cad/PRINTING.md](../../cad/PRINTING.md)):

    cad/.venv/bin/python examples/camera-turntable/print/turntable_study.py      # study: loads from the belt simulation
    target/release/sim-print analyze runs/camera-turntable/print/study.json      # strength as designed
    target/release/sim-print plan runs/camera-turntable/print/study.json         # direction and settings
    cad/.venv/bin/python examples/camera-turntable/print/turntable_study.py --plates   # 3MF plates

**Loads.** From the simulation: the belt's hub load, 50 N, which is twice the 25 N pretension declared on the drive pulley in CAD (an estimate; measure it). This force pulls the ring, the pulley and the cradle, and the bearings share it by lever arm. Assumed for design, not simulated: a 1 kg object on the platform and a 10 N knock on the camera mount.

**Strength as designed** (3 walls, 15 % infill, flat; estimated PLA strengths), as safety factors:

| Part | Safety factor | Governing mode |
|---|---|---|
| Servo cradle | 49 | interlayer shear |
| Base | 10.9 | layer split near the bottom |
| Disc | 37.6 | interlayer shear |
| Camera mount | 32.4 | interlayer shear |

**Plan.** The loads are light, so every part gets 2 walls and 10 % infill. The base, the weakest part, keeps a safety factor of 6.5 at full resolution. The disc prints upside down, with its flat top on the bed. That makes 3 H2C plates, about 12 h and 444 g (estimates; check in the slicer).

**On a smaller printer.** `split_small_printer.py` cuts the disc into 4 quarters for an A1 mini, joined by jigsaw tabs and 8 Ø3 pins. Against the simulated belt load its seams reach safety factors of 27–54. The script also writes an assembly guide and a coupon kit.

**Whole or split?** `stand_strength_split.py` runs the camera stand against a 40 N knock at the top. Verdict: keep it whole. Printed on its side it reaches 2.54; split at the post it reaches only 1.52, because a 20 mm post leaves room for just one M2 screw and a pin.

## Next steps

1. Real photos: calibrate the lens (checkerboard), undistort, and write the
   same `poses.json` and depth maps from real shots plus the logged servo
   angles; `sim-place build` then works unchanged.
2. Calibrate the ring's once-per-turn angle error.
3. Objects in a place (not built): splitting out and labelling objects with a
   vision model, adding them to the place model and the tools.
4. After printing: weigh the parts, measure backlash, friction and belt
   stiffness, and promote the measured values.
