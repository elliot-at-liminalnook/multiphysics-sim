# Visualization and interaction roadmap

Twenty improvements to how the 3D view, charts and lessons show physics and
invite the learner in. Motion is always purposeful: it is driven by lesson
cues or by the learner, the learner can always take over or pause, and every
moving thing comes from simulated values (see `lessons/README.md`).

Status: `todo` · `building` · `done` (with the proving test or check).
All twenty are `done` as of 2026-09-27. "Seen" means checked in a
screenshot of the running lesson viewer (REST `screenshot` command on a
scratch progress file). "Not seen" means covered by code and tests only.

| # | Improvement | Where it lives | Status | Evidence |
|---:|---|---|---|---|
| 1 | Smooth camera moves (eased glides, no cuts) | `sim-spatial` `view.rs`: `Orbit` glide; all framing goes through it | done | test `glides_ease_to_the_pose_the_short_way_and_stop_on_input`; seen (zoom cue glides in) |
| 2 | Slow orbit around the subject; stops on learner drag | `view.rs` orbit spin; cue `orbit` | done | `view_directives_build_a_view_state`, narration `orbit 0.15` in the motor explainer; motion not seen (stills only) |
| 3 | Continuous zoom (dolly) cues | cue `zoom` (focus, factor, seconds) | done | seen on the winch; fixed: cues seen before the card is framed now re-apply over the framing |
| 4 | Zoom to component (click a part or a `part:` link; F key) | `view.rs` + builder + lesson links | done | glide to `frame_pose(part)`, H returns home; not seen this round |
| 5 | Picture-in-picture inset following a part | inset camera, cue `inset` | done | seen (close-up of the turning gearbox); physics labels keep clear of it |
| 6 | Split, synchronised views of two variants | companion run, `split` in a scene | done | seen (four starts beside one start, companion curve on the chart); labels follow the halved view |
| 7 | X-ray view (see-through everything but the focus) | view state `xray`, cue `xray` | done | seen |
| 8 | Animated exploded view | eased explode factor | done | seen; test `explode_fallback_stays_near_each_part_despite_an_outlier` (fallback is median-centred, at most two part sizes) |
| 9 | 3D arrows and labels anchored to parts | cue `pin` (world-space arrow + label) | done | seen; the arrow comes from the side facing the middle of the view and its label is placed first |
| 10 | Spotlight: dim everything but the subject | cue `spotlight` | done | seen (motor and supply dimmed around the gearbox) |
| 11 | Graph ↔ scene links (hover a chart to preview that moment; hover a part to light its charts) | lesson charts | done | hover, click-to-commit and part hover wired; not seen (needs a real pointer) |
| 12 | Value readouts on parts | overlay layer `values` | done | seen (rad/s, rpm, A per part; grounds skipped; labels step clear of each other) |
| 13 | Live sliders that re-simulate | scene `sliders`, re-record with overrides | done | test `sliders_snap_to_their_steps_and_range`; seen (9.3 V re-recorded, claims hidden with a note) |
| 14 | Grab and push: drag a shaft or slide to load it | builder live run, load components | done | builder Alt+drag varies the part's load through `RunControl::Swap`; not seen; lessons reach it through "Open in builder" |
| 15 | Predict by drawing a curve | quiz kind `sketch` | done | test `sketches_score_by_their_gap_to_the_run`; seen (sketch over the regulator rail, 5 % gap); fixed: locking a sketch no longer asks for a number, and the locked curve survives a restart |
| 16 | Scrubbing with event markers and step keys | timebar ticks, ←/→ | done | seen (marks on the timebar) |
| 17 | Ghost runs: a faint earlier run over the current one | companion run, ghost mode | done | seen (one-start load as a purple wireframe); no shipped lesson uses ghost mode yet |
| 18 | Strobe or motion blur for fast parts | view setting `strobe`, motion blur | done | Strobe chip (five crisp spokes) and motion blur; strobe not judged by eye |
| 19 | Guided tour or free explore, with "show me" moments | scene `hints`, question `moment` | done | test `exploring_drops_holds_but_keeps_slow_motion`; seen (hints under the sliders in free explore) |
| 20 | Challenges with goals checked by the simulation | scene `challenge` (goal, win, hint) | done | judged by `runtime::check_claims`; seen (met at 9.3 V, recorded in progress) |

## Shared pieces

- **View directives.** One definition of camera and emphasis actions
  (`glide`, `orbit`, `zoom`, `spotlight`, `pin`, `inset`, `xray`,
  `explode`) in `sim_script::presentation`. Scene scripts, YAML cues and
  narration cues all use it.
- **Companion runs.** One mechanism for a second recorded run drawn beside
  the first (split view) or over it (ghost).
- **Parameter overrides.** One path for re-recording a scene with changed
  parameters, used by sliders and challenges.

## Notes

- **Layer switches.** One chip builder (`physics_view::LayerChips`) fills a
  floating bar over the builder's 3D view and a "Show" row under a lesson
  card's controls, so the switches never cover a lesson scene.
- **Labels.** Physics and pin labels are projected through NDC (correct in a
  lesson card's partial view and in split view). They are placed nearest-free-slot
  first, keep clear of the top strip and the inset, and read once when repeated.
- **Background windows.** A REST command wakes the viewer's event loop, which
  keeps drawing while a job runs. Before this, an unfocused window could take
  seconds to answer.
- **Limitations.** The inset and split cameras do not clip to a partly
  scrolled lesson card. Views seen only in stills (orbit, strobe, blur)
  have not been judged in motion. Hover links and grab-and-push were not
  exercised with a real pointer.
