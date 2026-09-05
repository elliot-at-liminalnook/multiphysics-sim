# Physical robot reference videos

Original MP4s copied unchanged from `~/Downloads` on 2026-09-05. SHA-256 hashes and extracted frame paths are recorded in [index.json](index.json). Original video audio is preserved; this inspection used video frames only.

| Clip | Length | Visual evidence |
| --- | --- | --- |
| [1000034220.mp4](1000034220.mp4) | about 9.2 s | Bench-mounted leg assembly sweeps relative to its mount, particularly in the latter part of the clip. |
| [1000034223.mp4](1000034223.mp4) | about 5.2 s | Knee servo rotates its perforated crank; the curved rigid connecting link changes orientation and drives foot extension/retraction through the fixed lower guide. |

Frame overviews: [first clip](1000034220-contact-sheet.jpg), [second clip](1000034223-contact-sheet.jpg). Frames were sampled at 2 fps; displayed times are approximate sampling times, not synchronized controller measurements.

## Observations and modeling implications

- The second clip shows a long exposed foot rod near the start and a much shorter extension around 2.5 seconds. The lower guide remains at the end of the thick tube. This supports the existing prismatic foot joint and fixed guide arrangement.
- The curved perforated connecting link changes orientation as a rigid linkage. Visible motion does not suggest a spring element; the user separately confirmed it is rigid.
- The knee housing and tube structure appear to maintain their relative arrangement as the assembly sweeps. The user explicitly confirmed the knee frame is fixed to the thigh rods.
- The enclosed proximal worm/gear and internal belt transmission are not sufficiently visible to establish their ratios or internal couplings from these clips alone.
- Neither clip establishes the maximum permitted range of travel or a collision stop. The separately computed ±64.4° candidate rod/enclosure contact boundary comes from the CAD geometry, not these videos.
- These clips can provide motion-path validation targets once camera perspective, scale and time are calibrated. They do not contain synchronized encoder commands, joint torque or contact-force measurements.

## User-provided materials

The two long thigh tubes and the perpendicular knee tube are light carbon fiber. Most other structural material is nylon. Exact nylon grade/process, composite layup, measured masses and per-part exceptions remain uncalibrated. Retaining pins must retain separate material identity when grouped with carbon tubes.

## CAD discussion links

Working model: `runs/robot-imports/Full_Bot-knee-01.rcad`.

- Annotation 1 (`f203411ed6ea`): knee crank, curved connecting rod and sliding foot.
- Annotation 3 (`6948b16256d6`): rigid thigh assembly, material identification and preliminary enclosure-clearance sweep.
- Clearance evidence: `runs/robot-imports/thigh-clearance-refined.json`.
- Assembly verification: `runs/robot-imports/rigid-thigh-verification.json`.

## Re-extracting frames

`extract_frames.py` requires Pillow and either `imageio-ffmpeg` or an `FFMPEG_EXE` environment variable pointing to FFmpeg. The session used the existing CAD Python environment plus an isolated installation of `imageio-ffmpeg` at `/tmp/robocad-video-tools`:

```sh
PYTHONPATH=/tmp/robocad-video-tools cad/.venv/bin/python references/robot/2026-09-05/extract_frames.py
```
