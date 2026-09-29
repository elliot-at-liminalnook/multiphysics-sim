# Lessons roadmap: teaching with live systems

**Goal:** a Learn mode in the physical viewer where pre-written, editable
Markdown lessons explain concepts with live, scriptable system scenes
embedded in the text. Any scene opens in the system builder and returns to
the lesson. Lessons, scenes and systems share one annotation system.

**Final test:** open `lessons/`, read the worm self-locking lesson, watch
the power-off scene hold the load, scrub its timeline, open the scene in
the builder, change the lead angle in a sandbox, come back, annotate a
sentence and a part in the scene, edit the Markdown and see the note stay
attached. `sim-lesson check lessons/` proves every claim the lessons make.

## Decisions (2026-09-26)

- A lesson is a folder: `lessons/<slug>/lesson.md` plus its system files,
  Rhai scripts and images. Front matter (YAML) holds title, order,
  prerequisites and named systems.
- Special content: fenced `sim-scene`, `sim-component` and `sim-compare`
  blocks (YAML bodies) and `part:<system>/<path>` links. Everything else is
  ordinary Markdown rendered by `sim-markdown`.
- Scene claims are `expect` lines checked by `sim-lesson check` (CI).
- Opening a scene in the builder edits a **sandbox copy**
  (`runs/lessons/<slug>/<scene>/`), with Reset and Save-to-lesson. The
  authored lesson never changes silently. (Recommended default; not yet
  confirmed by the user.)
- Editing: lessons hot-reload on file change; in-app editing is one block
  at a time plus "Open in editor". A full source pane is later.
- Lesson annotations are shared discussions in a sidecar
  `lesson.annotations.json` (threads with comments); a private note is a
  thread nobody replies to.
- One live scene at a time (the one nearest the reading position); others
  show a still. Bevy 0.16 has no viewport node, so scenes render to a
  texture shown in the card.
- Scene runs go through `SystemSession` / `run_history` like the builder
  and are cached by a hash of system, overrides, script and settings.
- Presentation cues from scripts never change physics; physics cues
  (`set`) are recorded run inputs.

## Milestones

### L0 · Shared annotations
- [x] Crate `sim-annotate`: threads, comments, commands, revisions, bounded
  undo, limits, sidecar store (lock + atomic write), `Anchor` trait
- [x] Anchors: system (lineage), selection (schematic), text (quote +
  context + hint), embed (scene id + part + time)
- [x] `sim-system` discussions and `sim-inspect` annotations built on it,
  file formats unchanged
- **Done:** existing tests pass; existing files round-trip byte-for-byte in
  meaning

### L1 · Lesson format
- [x] Crate `sim-lesson`: front matter, embed blocks, `part:` links,
  validation with `lesson.md:line` errors, lesson index
- [x] CLI `sim-lesson list|check`
- **Done:** malformed lessons fail with errors that name the line

### L2 · Learn screen
- [x] Inspect | Build | Learn mode switch; `sim-spatial --lessons DIR`
- [x] Outline, reading column, annotation margin, component cards,
  `part:` links that highlight
- **Done:** a lesson reads end to end with no UI-thread stalls

### L3 · Live scenes
- [x] Scene cards: live view, timeline scrubber, plots, fidelity label
- [x] Presentation script module (Rhai over Rust): `at`, `set`, `caption`,
  `highlight`, `camera`, `plot`, `pause`
- [x] Run cache by hash; Open in builder (sandbox) and Back to lesson
- **Done:** same hash gives the same trace; the builder round trip keeps
  scroll position

### L4 · Lesson annotations
- [x] Text and scene anchors in the margin, re-anchoring after edits,
  detached notes kept
- [x] Codex answers on lesson threads with lesson context
- **Done:** notes survive edits to surrounding text

### L5 · Editing and automation
- [x] Block edits through a `sim-lesson` command layer with revision
  checks and undo; hot reload; Open in editor
- [x] REST `lesson_list|lesson_open|lesson_state|lesson_goto|lesson_play|
  lesson_edit|lesson_annotations`
- **Done:** UI and REST share commands and undo

### L6 · Starter lessons
- [x] Worm self-locking, motor torque–speed, motor-driver board, each with
  `expect` checks
- **Done:** `sim-lesson check lessons/` passes in CI

**Order:** L0 → L6. Code first; tests are written alongside but run after
L6 (user's requested cadence, 2026-09-26).

## Later

- Annotation widget in the schematic (egui) viewer
- Lessons in the browser (crates stay wasm-compatible)
- Equation typesetting (v1 shows equations as monospace)
- Several live scenes at once

## Evidence (2026-09-26)

| Milestone | Proof |
|---|---|
| L0 | `sim-annotate` unit tests (thread undo/redo, messages, text re-anchoring); `sim-inspect` `committed_sidecars_round_trip_unchanged`; `sim-system` `discussion_format_is_unchanged_and_follows_renames`; builder agent/discussion tests still pass |
| L1 | `sim-lesson` tests: blocks/sections/links, `errors_name_the_line`, block edits with undo and stale/invalid refusal, anchors |
| L2–L3 | `sim-spatial` 33/33 (incl. `learn_edits_undo_and_notes_share_the_command_layers`); `sim-runtime --test lessons`: deterministic runs, cache round trip, script cue applied at 0.3 s, sandbox isolation, false claim fails at its line |
| L4 | Text anchors follow their paragraph after inserts above (sim-lesson + sim-spatial tests); scene anchors check part paths |
| L5 | REST smoke on the live app: seek, note create, edit, undo, builder round trip all succeeded; same `LessonStore` journal as the CLI |
| L6 | `sim-lesson check lessons --compares`: 3 lessons, 4 scenes, 8/8 claims, 3 comparisons, exit 0 (10.7 s) |

Simulated vs analytic: motor 721.2 rad/s (722 predicted), 444.8 after the
load step (444.4), 3.33 A (3.33); worm winch 26.67 rad/s lift, 0.24 mrad
creep after power-off; four-start worm back-drives at 33 rad/s, matching
η_back ≈ 0.69 at the 7.5:1 ratio; board rail 5.003 V.

## Remaining

- The Learn screen has not been inspected on screen (the display was
  asleep during the test pass); layout, the embedded viewport's clipping
  while scrolling and scrubbing were exercised only through REST and tests.
- Text editing is basic: append-only typing with Backspace, no cursor
  movement, selection or paste (same as build-mode drafts).
- Notes anchor to whole paragraphs or scene parts; selecting part of a
  sentence needs text selection in the UI.
- Scene charts are static images with a moving playhead; no hover values.
- Lesson Codex answers are manual only (no auto-answer, by design).

# Narrated explainers (requested 2026-09-26)

**Goal:** an audio explainer over each lesson, written as plain text beside
it (`explainer.md`), in sections. Narration drives the lesson: cues placed
between words highlight text, box or point at paragraphs, parts, plot
regions and scenes, run and pause simulations and move the camera. Each
section is generated (and regenerated) on its own.

## Decisions

- Voice: Gemini 3.8 Flash TTS (`google/gemini-3.8-flash-tts`) through
  OpenRouter `/audio/speech` with the `openrouter-rs` crate; 24 kHz mono PCM
  stored as WAV. Tone in `speech_metadata.style` (file-wide, per-section
  override); inline Gemini tags (`<short pause>`, `<sigh>`…) and CAPS
  emphasis pass through to the voice and are hidden from subtitles.
- Sync: each section's audio is transcribed with word timestamps
  (`openai/whisper-1`) and aligned to the script's words; cue times come
  from the words they precede. Without alignment, times are estimated from
  the audio's length (labelled "estimated"). Without audio, the explainer
  runs silently with subtitles at an estimated reading pace.
- Cache: a section's audio is keyed by a hash of model, voice, style and
  spoken text; `narration/narration.json` records hashes, durations, word
  timings, costs and generation IDs. Editing cues never regenerates audio.
- Spend: only on request (CLI or per-section button), with a dollar ceiling
  per run (default $1), estimated before and recorded after. Key from
  `OPENROUTER_API_KEY` or `~/OPENROUTER_API_KEY`; never written to files.

## Milestones

- [x] N1 · Explainer format in `sim-lesson::narration`: sections, cues,
  errors with lines, spoken text, hashing, alignment, cue timeline
- [x] N2 · `sim-voice`: synthesis, transcription, alignment, cache
  manifest, budget; CLI `sim-narrate plan|generate|status`
- [x] N3 · Viewer: Explain player (audio, subtitles, sections), cue
  execution, overlays (highlight, box, arrow), wait-scene pacing
- [x] N4 · Per-section generate/regenerate from the viewer and REST
- [x] N5 · Explainers for the starter lessons, generated and checked

## Evidence (2026-09-26)

- `sim-lesson` narration tests (parsing, line-numbered errors, cue vs word
  hashing, alignment with gaps, marks over time) and `sim-voice` tests
  against a local OpenRouter stand-in (requests carry the verbatim text,
  voice and styles; per-section regeneration; budget refusal before any
  request; failed alignment falls back to estimated timing).
- `sim-lesson check lessons`: explainers checked (quotes, parts, plots,
  scenes, blocks); 0 errors. `sim-narrate check`: motor 26 cues, worm 32.
- Real generation with Gemini 3.8 Flash TTS + whisper-1: 11 sections,
  270 s of audio, measured $0.056 (estimated $0.10).
- Live app (REST): section plays, arrow on the rotor at 10.7 s, subtitle
  follows the words, `play-until 0.3` stopped the scene at 0.3 s.

## Remaining

- Overlays (boxes, arrows, highlights) not yet seen on screen (Mac locked).
- The motor-driver-board lesson has no explainer yet.
- Narration WAVs are git-ignored (1.3 MB per ~27 s section); each machine
  regenerates them (~$0.03 per lesson) unless you decide to commit them.

# Diagrams, practice and pacing (requested 2026-09-26)

**Why:** the lessons read too fast. The best-supported techniques are
practice testing and spacing (retrieval spread over time), then
pretesting, self-explanation, worked examples, dual coding and multimedia
design that limits overload (signalling, segmenting).

- [x] D1 · Figures: Markdown images (`![caption](file.svg)`), SVG
  rasterized with `resvg` (IBM Plex fonts bundled), PNG/JPEG; checked in CI;
  narration can box or point at a figure or a region of it
  (`figure:id@x,y,w,h` in the SVG's own units)
- [x] D2 · `sim-quiz` blocks: choice (per-option feedback on the
  misconception), numeric (tolerance, unit) and predict (commit to a
  prediction before a scene unlocks, then compare with the run);
  `sim-reflect` self-explanation prompts
- [x] D3 · Mastery pacing: content after a gating question stays locked
  until it is answered correctly (answer shown after two tries, and noted);
  narration holds at `[[quiz id]]`
- [x] D4 · Spaced review: answered questions return after 1, 3, 7, 21
  days (Leitner boxes); a Review list across lessons. Learner progress in
  `runs/lessons/progress.json` (per machine, not content)
- [x] D5 · Lessons rewritten slower: diagrams, a prediction before each
  scene, checks after each idea, a self-explanation prompt and key ideas;
  explainers updated (changed sections re-voiced)

## Evidence (2026-09-26)

- Tests: `sim-lesson` 14 (quiz judging with tolerance and per-option
  feedback, Leitner schedule, gates and prediction order, figure
  rasterizing with bundled fonts, image path rules, figure/quiz cues),
  `sim-markdown` images; `sim-spatial` 34 incl.
  `questions_gate_the_lesson_and_predictions_unlock_scenes` (wrong answer
  keeps the lock, right answer opens the next part, prediction unlocks the
  scene and is saved, figures rasterize off the calling thread).
- `sim-lesson check lessons --compares`: 3 lessons, 10/10 claims (new:
  the board's rail stays within 4.9–5.1 V, backing the "tiny ripple"
  answer); figures and explainer cues checked.
- 5 diagrams, each rendered with `sim-lesson figure` and inspected;
  3 explainers (17 sections) re-voiced for $0.114 measured; the "lift"
  section kept its audio because only its cues changed.
- Seen on screen: figures, locked outline, question card, wrong-answer
  feedback with hint, lock card, narration strip.

## Remaining

- Arrows and boxes from narration not yet watched on screen end to end.
- Text entry is basic (no cursor or paste) for numeric answers and
  reflections.
- Progress is per machine (`runs/lessons/progress.json`), one learner.
