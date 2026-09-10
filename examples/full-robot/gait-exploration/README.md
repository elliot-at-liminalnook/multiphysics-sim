# Fast gait exploration — review checkpoint

**A command-driven paired gait now travels about 25 mm/s and executes walking
turns, reversal and stopping in the shared Rust simulator. It is experimental,
not a completed goal.** The first checkpoint was delivered within 30 minutes;
this iteration stops for review within the requested hour.

The 20 s steering reference measured 26.96 mm/s forward and 24.51 mm/s reverse.
The 5 ms browser profile measured 27.07/24.86 mm/s, turned 0.104 rad under the
walking-turn command and drifted 0.236 mm during the late stop window. Its body
trajectory differed from the 1.25 ms reference by at most 1.46 mm. All 1001
recorded poses passed independent sampled inter-link inspection. The first
24 lifts passed sampled clearance/unloading/support checks; later turns and
reverse lifts have not received that phase-by-phase qualification.

**Remaining failures:** worst loaded material motion / total body path was
10.9% during steering, above the 5% screen. Rendered browser execution reached
0.778x realtime with 73.3 ms p95 transitions, missing both required gates.
Worker-only execution reached 1.130x; it is not the rendered result. Browser
and native state/transition comparison passed, including exact replay/reset.
The leaderboard entry is explicitly unranked, with failed and missing gates.

## What changed

- A reusable Rust batch configuration inspector measures marker Jacobians,
  closure, authored limits and sampled collision geometry. The offline marker
  planner now audits inter-link geometry even when that force profile is omitted.
- 829 broad and 1288 fine poses explore hips, worm inputs and foot retraction.
  Most hip ranges exceed the old +/-11.5 degree controller bounds substantially.
  Coordinated collisions and asymmetric mount constraints remain explicit.
- CAD mass, inertia, Jacobians, friction and centroidal equations inform three
  gait families. Crouching with foot cranks at -60 degrees adds roughly 54 mm
  extension reserve. The paired gait alternates opposite support lines.
- The wave and 50/100 mm/s candidates encountered sampled grid rejections.
  Exact CAD checks show four rejected points are outside the solids, about
  0.261 mm from their surfaces. This disproves those points, not entire-path
  collisions. A full 1 mm grid rebuild was stopped after its first link took
  about five minutes; no collision correction or exclusion waiver was adopted.
- A Rhai controller executes the physics-generated cycle with signed phase rate,
  all-stance stop/reversal handling, and stance hip adjustments for walking turns.
  It consumes joint feedback and motion commands without an offline world path.
- A 33-feature, 12-output student was distilled through the existing Rust learner
  with full target authority and no planner references in its inputs. Its closed
  loop exceeded the tilt limit at 3.28 s. Teacher PPO and on-policy student
  robustness training remain outstanding; supervised fit is not stability proof.

## Review and reproduce

The active preset is `experimental-fast-paired-gait`. Press Reset then Play;
hold W/S for forward/reverse and combine W or S with A/D for walking turns.
Release to finish the airborne transfer and stop. This preset runs for 20 s.
No lateral motion, turn-in-place, hardware sensing or sim-to-real claim is made.

From the repository root:

```sh
cat examples/full-robot/gait-exploration/evidence.part-* > examples/full-robot/gait-exploration/evidence.tar.gz
tar -xzf examples/full-robot/gait-exploration/evidence.tar.gz -C examples/full-robot/gait-exploration
node examples/full-robot/gait-exploration/package_browser.mjs
node web/serve-viewer.mjs runs/gait-exploration/viewer 4173
```

Open `http://127.0.0.1:4173/?preset=experimental-fast-paired-gait`.
The versioned browser template contains the exact tested WASM and static assets;
source scripts replace the viewer/command mapping and package the exact recipe.
No ignored run directory is required after archive restoration. Raw artifact
hashes are in evidence-index.json and archive hashes in archive-sha256.json.
The saved UI recording is browser-live.json.recording.json; it restores with
the evidence archive. All rejected plans, failed student results and tests remain.

Validation: 10 Rust planning/inspection tests passed, including floating-base
coordinate counts and collision detection with omitted forces; typed UI command
mapping checks passed. Native/browser parity, exact replay/reset and live
keyboard steering were exercised. Relevant evidence is in CHECKPOINT.md,
physics-derivation.json, steering-summary.json, browser-parity.json,
browser-live.json, leaderboard-entry.json and student-observation-boundary.json.

Next work: correct and validate the CAD collision sampling, reduce loaded slip
through contact-aware phase/force control, profile slow physics transitions,
then on-policy teacher/student improvement and minute-long robustness testing.
The original checkout and unfinished heading work remain preserved separately.
