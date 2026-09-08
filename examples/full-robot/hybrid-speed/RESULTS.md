# Executed hybrid-feedback sweep

All 14 declared cases ran. No hybrid is promoted.

| Gain | 24 s, 20 / 5 ms | 60 s, 20 / 5 ms |
| --- | --- | --- |
| 0 | Existing selected short baseline | Both fail sustained heading; 20 ms also fails final position |
| 0.1 | Both pass the physical audit | 7 / 5 failed supported swings; heading error 0.00902 / 0.00917 rad |
| 0.25 | 20 ms passes; 5 ms hits observed geometry overlap | Both hit observed geometry overlap |
| 0.5 | Both hit observed geometry overlap | Both hit observed geometry overlap |

The low gain's minute ends within the 1 mm position gate, but heading and swing
qualification deteriorate. Its maximum 20/5 ms sampled foot difference is
1.553 mm, beyond the unchanged 1 mm screen. The short pair differs by 1.216 mm,
also outside that screen. A short physical pass is therefore insufficient.
The reported tiny penetrations are the actual retained rejection diagnostics;
this sweep does not alter the geometry tolerance to admit them.

`status.json` records every case, input/capture hashes, completed audits and
the exact early errors. `point-0.1-*-refinement.json` compares whole sampled
trajectories at common controller times. The declared recipe remains in
`plan.json`; large captures can be regenerated with `prepare.mjs` and `run.mjs`.

`baseline-motion.json` uses the new shared capture-analysis script to measure
the selected student's actual motion. A least-squares steady-forward fit gives
**2.477 mm/s**; net advance over the full minute is **141.1 mm**. The body travels
**1.631 m** horizontally through its weight shifts. Raise/lower occupy 31.16 s
without planned body movement; shift/return occupy 21.32 s and 1.432 m of body
path. No reference support waits occur in this baseline. Loaded marker paths
are also reported, but include rolling/rocking and are not contact-patch slip
certificates. Sampled positive shaft work is 12.32 J, not electrical energy.

These measurements motivate the separate `../swing-advance/` experiment: overlap
body progression with swinging instead of increasing foot correction gain.
The original selected faster student is now packaged as an explicitly
experimental browser preset; its sustained failures remain visible.
