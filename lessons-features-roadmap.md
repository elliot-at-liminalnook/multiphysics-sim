# Lesson features roadmap

Twenty improvements to what lessons can do, proposed 2026-09-27 after the
visualization roadmap (`visualization-roadmap.md`) was finished. The aim is
lessons that stay true to the model, adapt to the learner, and let the
learner practise the engineering, not only watch it.

Rules that apply to every item (from `AGENTS.md`):

- Numbers come from the model, the run or the actuator registry, never
  hand-copied. Anything shown as measured says where it was measured.
- One shared library path per idea: `sim-lesson` parses and validates,
  `sim_runtime::lesson` runs and judges, the viewer only presents. The CLI,
  REST and viewer share the same commands.
- Every new block kind fails with an error that names `lesson.md:line`.
- LLM features are manual (the reader or author presses a button), go
  through the existing read-only Codex supervisor (`sim-agent`), and never
  write a file that has not passed `sim-lesson check`.
- Hardware steps never bypass the supervisor, travel windows, watchdogs or
  STOP, and run only with the operator present and the fixture supported.

Status: `todo` · `building` · `done` (with the proving test or check).

| # | Feature | Design | Status | Evidence |
|---:|---|---|---|---|
| 1 | Numbers in prose from the run | Inline `{{value scene=… observe=… reduce=… window=a..b \| 444 rad/s}}` and `{{param system/path.parameter \| 0.012 N·m/A}}`. The shown text is kept in plain Markdown; the viewer prints the computed value; `check` fails when the shown number disagrees with the model | done | `refs` tests; `check` fails a planted wrong number ("the text says 470 rad/s but the model gives 445 rad/s"); seen (k from the system file) |
| 2 | Equations that fill in their values | Block `sim-equation`: `expr` (Rhai), `terms` bound to parameters or observables, `result`; shown as `τ = k·i = 0.012 × 3.29 = 39.5 mN·m` at the playhead. `check` verifies the equation holds on the run (`holds: observable`) | done | `check_equation` catches a wrong `holds`; seen live (`τ = k·i = 0.012 N·m/A × 3.29 A = 39.5 mN·m at t = 0.450 s`) |
| 3 | Measured beside simulated | Block `sim-measured`: a measured data file (`sim.fit-data/1`) with provenance, a system, the parameter swept and the observable reduced; the runtime simulates each measured point and reports the gap (RMS, max). Labels: measured / derived / estimated | done | new lesson `knee-servo-measured` on the real knee campaign data; seen (points, gaps, origins, fitted-to-data notice); `sim-lesson measured`. It found a real one-way offset of ~0.17 rad/s (gravity, set to zero in the model) |
| 4 | Wrong answers linked to misconceptions | Option `remedy: <id>` → block `sim-remedy` (misconception name, short explanation, optional scene and follow-up question), shown only when that option is picked; picks are recorded | done | seen (wrong pick opens "A loaded motor tries harder"); picks recorded and counted by `report` |
| 5 | Fresh numbers on each review | Numeric question `vary: {name: {min, max, step}}`, `{name}` in the text, `answer_expr` (Rhai). Values drawn from a recorded seed per attempt. `check_with` makes `sim-lesson check` confirm the formula against the simulation | done | `varied_questions_keep_their_numbers_until_the_next_review`; `check_with` catches a wrong formula at both corners; gating test answers the drawn 14 V in rpm |
| 6 | Answers with units | `sim_lesson::units`: parse "24 mN·m", "4300 rpm", "3.3 A"; convert to the question's unit; specific feedback for ×1000 slips and rad/s vs rpm | done | `units` tests; gating test answers "36 mN·m" and "2400 mA"; slips named (×1000, rpm, sign) |
| 7 | Worked examples that fade | Question kind `steps`: a list of steps, each `worked` or `blank`; authors fade by blanking later steps in successive questions | done | motor `budget-steps`; seen; steps test |
| 8 | Hints in steps | `hints: [nudge, key idea, worked step]`; one more per request; hints used are recorded and hold back the review box | done | `support_holds_questions_back_…` test; gating test records the hint with the attempt |
| 9 | Confidence ratings | Guess / fairly sure / sure with each answer; confident misses come back the next day; calibration shown to the learner | done | progress tests (confident miss due next day, calibration); seen ("You were sure…") |
| 10 | Pretest and recall prompts | Question `pretest: true` (asked before teaching, never scored as a miss); block `sim-recall` (write what you remember, then tick the key points you had) | done | motor `guess-torque` (pretest) and worm `recall-motor`; both seen |
| 11 | Find-the-fault lessons | Block `sim-task` with `kind: fault`: the sandbox starts with a planted fault (`start` values); the learner fixes it in the builder; `win` claims judge the fix | done | worm `rebuilt-winch`: `check` proves the planted fault fails and the solution passes; card as for 12 |
| 12 | Design tasks | Same block with `kind: design`: a goal, `win` claims, and a `report` table of each attempt's metrics | done | motor `fast-under-load`: seen with its own scene and sandbox, judged (194.8 rad/s against ≥ 420); `check` proves solvable |
| 13 | Hardware lab step | Block `sim-lab`: a bounded test (joint role, duty, duration) run through the characterization `Rig` (SimRig in tests, BusRig on the bench) after the operator confirms the checklist; result compared with the prediction and the simulation | done | `lab_step` on the campaign `Session` (test on SimRig: backs off, runs, stops before the window end); calibration server action `lab_step`; card seen. Not run on the bench |
| 14 | Ask about this moment | "Ask about this moment" on a scene creates a note anchored at the part and time; Codex gets the paragraph, the time and the run's values at that time, and must cite them | done | "Ask about this moment" on scene cards; Codex context gets the run's values at that time. Not exercised with Codex |
| 15 | Feedback on self-explanations | Reflection `key_points` (idea + cue words): after saving, a free, instant check lists what is covered and missing; optional Codex feedback on request; then the model answer | done | `coverage` test; seen (four ideas covered); Codex feedback posts a note. Not exercised with Codex |
| 16 | Concept map | `lessons/concepts.yaml`; front matter `teaches` / `needs`; questions tagged by concept; mastery per concept and a suggested next lesson (viewer panel and `sim-lesson concepts`) | done | `lessons/concepts.yaml`; `mastery_follows_answers…` test; `sim-lesson concepts`; panel seen |
| 17 | Mixed review sessions | One review session queue drawn from all lessons, interleaved by lesson and concept, answered in place | done | `review_sessions_interleave_…` test; outline button and session bar. Not seen (nothing due in the test progress) |
| 18 | Reports for authors | Viewer records per-block time, rewinds and narration skips in progress; `sim-lesson report` aggregates progress files: miss rates, reveals, hints, misconceptions, stalls | done | per-block time, visits, rewinds and narration skips in progress; `report` test; `sim-lesson report` |
| 19 | Drafting help for authors | `sim-lesson draft`: Codex drafts from a system and objectives using `lessons/AUTHORING_PROMPT.md`; the host writes it only into `lessons/drafts/` and iterates on `check` findings | done | `sim-lesson draft`; `drafts_are_checked_revised_and_kept_in_drafts` (a wrong first draft is fixed on round 2). Not run with Codex |
| 20 | Accessibility | Narration transcript, 0.8× / 1.25× speed (time-stretched, pitch kept), replay last sentence, keyboard for questions and sliders, reduced motion, text size; saved per learner | done | WSOLA test (pitch kept at 0.8×/1.25×); "Say that again"; transcript; reduced motion (cuts, no orbit, no blur, instant explode); text size (UiScale); keys 1–9 / Enter / ←→ on a focused slider; settings saved. Transcript and speed not judged by ear |

## Notes

All twenty are `done` as of 2026-09-28. "Seen" means checked on the
running viewer (REST screenshots and `lesson_frames` contact sheets, on a
scratch progress file).

Also added while building these:

- **Contact sheets** (REST `lesson_frames`): a scene's animation as a
  labelled grid of frames, in seek mode (screen or sim clock) or live mode
  (the scene or a muted narration section playing for real). Used to
  retime and reframe the motor scene.
- **Pacing rule, measured properly.** Slow motion is now decided by the
  time a plotted quantity takes to move by half its range, from each
  sample (ripple no longer counts), eased over screen time, with warnings
  from the content rather than the easing. Script highlights pin a label
  and dim the rest.
- **Playback fix.** The first recorded frame is filled from the
  step-resolution series, so slow motion over the first frame no longer
  shows a frozen view.
- **Narration.** A stale section (text changed since its voice was made)
  played silently; it was regenerated, `check` now warns about stale
  sections and the narration bar says so with a "Make its voice" button.
- **Responsiveness.** The viewer declares a latency-critical activity
  while its REST server runs, so macOS does not nap it in the background.

Limitations: the bench lab step, the Codex features (drafting, reflection
feedback, asking about a moment) and the review-session UI have not been
exercised end to end; narration speed and the transcript were not judged
by ear.
