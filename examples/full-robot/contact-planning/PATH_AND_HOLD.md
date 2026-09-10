# Faster diagonal walking with measured foot clearance

The new `xhip-lift-v230` development candidate measures **.233740 m/s forward
and .223767 m/s reverse** in the detailed eight-second runtime test. All 42
planned foot-clearance checks pass, all 401 recorded poses have zero sampled
inter-link overlap, and short speed/heading/tilt/stop checks pass. Loaded-foot
slip is **13.46%**, so this remains an experimental gait and fails the unchanged
5% contact-quality gate. No sustained or browser promotion is claimed.

At .3125 ms physics, the same candidate measures .233531/.223355 m/s with
13.52% slip, again passing 42/42 lifts, all 401 sampled geometry poses, and
the short control checks. Maximum chassis-path difference from .625 ms is
.678 mm; forward speed differs by .090%. This is one timestep refinement,
not continuous-time collision coverage or a converged physical certificate.

## Changes that led to this candidate

The existing .23 candidate overlaps only between the -X hip motor pulley and
the chassis. The shared Rust capture geometry CLI now accepts `--pairs`,
exposing the already-computed pair witnesses and link names. Removing those
optional fields exactly reproduces its default physical output. The existing
clearance audit independently reports the same 250 affected poses and maximum
12.374 µm overlap. Invalid flags are rejected before file reading. No contact
forces, exclusions, shapes or physical tolerances change.

Earlier exact-CAD work on this same region found a small interference witness;
see [the retained CAD investigation](../contact-implicit/COUPLED_SLIP.md).
The new runtime witness has not itself received an exact-CAD check. Its sampled
overlap is treated as a failure rather than dismissed because it is small.

The parent runtime's observed -X hip angle spans approximately -.125 to .143 rad.
A new foothold comes from a shared CAD kinematic inspection with a +10 degree
-X hip seed; only its horizontal marker displacement is applied. Shared IK and
inverse dynamics then rederive the complete periodic joint reference and
forward/reverse/static feedforward. The resulting runtime hip observations
span .065 to .314 rad, and its 401 poses have no sampled overlap. These angle
ranges are controller observations, not continuous extrema or new hardware
limits. This changes the operating posture; the physical CAD model is intact.

That first collision-free candidate reaches .233035/.224012 m/s, with 12.62%
slip and 37/42 lifts. All five failed lifts belong to the +X foot. Its worst
failed peak clearance is .4885 mm against the unchanged 2 mm criterion.
The next reference adds the measured 1.5115 mm deficit plus an explicit 1 mm
planning margin to that foot's swing lift: **3.9404 → 6.4519 mm**. Treating lift
response as linear is only a candidate-sizing heuristic; actual replay is the
test. Recompiled dynamics and runtime measurements produce the all-42 result
above. All other foot paths, timings, body motion and physical gates remain fixed.

## Rejected alternatives and useful limits

| Candidate at .23 command unless shown | Forward / reverse m/s | Slip | Lifts | Overlap µm |
| --- | --- | ---: | --- | ---: |
| Parent | .229225 / .226941 | 9.04% | 41/42 | 12.374 |
| Three hips shifted 15° | .227331 / .215450 | 11.40% | 41/42 | 12.325 |
| Same, nominal -X foot extension change | .227331 / .215450 | 11.40% | 41/42 | 12.325 |
| Half-hold lead, .23 | .232008 / .230391 | 14.22% | 40/42 | 17.333 |
| Half-hold lead, .25 | .262488 / .260305 | 19.22% | 38/46 | 18.974 |
| Half-hold lead, .28 | .309528 / .301844 | 30.88% | 38/50 | 20.397 |
| -X foothold clearance change | .233035 / .224012 | 12.62% | 37/42 | 0 |
| Clearance change plus +X lift | .233740 / .223767 | 13.46% | 42/42 | 0 |

The three-hip choice increases the frozen-posture projection of belt motion
onto diagonal travel. The desired negative -X rotation was excluded because
its sampled initial posture already overlaps the chassis by 55.782 µm. But
larger belt projection at one posture does not rank full gaits: the three-hip
path reduces -Y/+X worm peaks while increasing +Y's peak. Its conditional
whole-reference rate budget falls from .212496 to .208803 m/s, and nominal
inverse-wrench errors grow to 3.025 N / .968 Nm. Runtime reverse tracking fails.
It is rejected rather than promoted based on belt usage alone.

The nominal -X foot extension experiment is effectively a negative control:
changing the CAD foot seed from -60° to -45° moves that marker vertically by
24.278 mm but horizontally by only .371 µm. Because the reference preparation
retains floor height and applies only horizontal offsets, it creates almost
the same path. Do not interpret its similar runtime result as an exploration
of materially different leg reach.

The half-hold correction adds T/2=.010 s to the existing D/K=.020 s velocity
lead, the mean advance of a linear reference during a 20 ms held command.
It preserves all other inputs and is only first-order compensation. Here it
raises speed mostly alongside worse slip, missed lifts and overlap. The .28
case also fails speed tracking. None of these lead changes is retained.

Moving the -X foothold away from interference increases its reference worm
peak to 7.218 rad/s, lowering that path's conditional no-load rate budget to
.161670 m/s. Faster actual runtime motion therefore does not establish exact
reference tracking or invalidate motor physics: tracking error, load-dependent
motion and slip matter. The theoretical global maximum remains unproved.
Next work must reduce slip and worm demand while preserving the newly measured
foot clearance and collision results, and must extend the candidate to sustained
WASD and browser validation.

## Reproduction

All named `*-reference.recipe.json` files run through `compile_contact_reference`
with the original diagonal scene and workspace markers. `prepare_controller_trial.mjs`
assembles their existing Rhai/Rust runtime; `prepare_cadence_screen.mjs` sets the
same declared speed schedule. `prepare_hold_lead_trial.mjs` produces the matched
lead experiments. `run_environment` records seed-zero episodes using the original
task, actuator/contact model and .625 ms physics. The standard screen analyzer
and exact-policy clearance audit measure results independently.

`summarize_path_and_hold.mjs` reproduces `path-and-hold-summary.json`, verifies
input hashes, unchanged robot/world/controller code and command schedules,
declared initial-condition changes, the one-foot-only lift edit, capture hashes,
exact policy replay and detailed/compact pair-output equivalence. The evidence
archive index identifies raw inputs, captures, audits, source and binary hashes
and its required previous cadence archive. No runtime or controller library
physics changed; the only Rust source edit adds optional geometry reporting.
