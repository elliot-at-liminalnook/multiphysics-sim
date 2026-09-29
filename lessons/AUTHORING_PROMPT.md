# Prompt: write a bespoke lesson

Use this prompt, as-is, whenever you (a person or an assistant) write a new
lesson for this repository. `sim-lesson draft` sends it to Codex as its
instructions. It is kept up to date with what lessons can do. The block
reference is `lessons/README.md`, and every rule below is enforced or
measured by `sim-lesson check`.

---

You are writing one lesson: a `lesson.md` file that teaches an engineer
with a live, simulated system. The reader learns by predicting, watching,
answering and changing things, not by reading alone. Write for a curious
engineer who is new to this topic.

## Before you write

1. **Name 2–4 objectives** as things the reader will be able to *do*
   ("predict a motor's speed for a load", not "understand motors").
2. **Find the system and read its model.** Run
   `sim-lesson model LESSON.md` (or read the context you were given). Note
   each instance path, each parameter with its unit and origin
   (measured / derived / estimated), and each observable key. You may only
   name what exists there.
3. **Find the concepts.** Use IDs from `lessons/concepts.yaml` in
   `teaches` and `needs`. Add a concept there only if none fits.
4. **Plan the ideas as a chain**, one new idea per part, each needing only
   the ones before it. If an idea needs two new things, teach them
   separately first (pre-training).

## Shape of the lesson

- Front matter: `title`, `summary` (one sentence on the payoff),
  `order`, `minutes`, `category` (an id from `lessons/categories.yaml`),
  `requires` (lesson slugs), `teaches`, `needs`, `systems` (name → path
  relative to the lesson folder).
- `# Title`, then **By the end of this lesson you will be able to:** and
  the objectives.
- One short hook paragraph with the real system and a question the lesson
  will answer. If it follows another lesson, open with a `sim-recall`
  of that lesson's key ideas.
- Parts (`##`), each shaped the same way:
  1. a concrete picture of the idea (a figure, a scene or a physical
     image);
  2. at most **one** equation, in a ```` ```text ```` block;
  3. a **worked example** with the system's real numbers;
  4. a question that uses the idea (retrieval practice). It gates the
     next part.
- Before the first scene that shows the key result, a `kind: predict` or
  `kind: sketch` question: commit before watching.
- Then the scene, a question about what it showed (with `moment:`),
  and a `sim-equation` if the scene makes an equation visible.
- A `sim-reflect` "in your own words", with `key_points`.
- Optionally a `sim-task` (find the fault or design something) and
  **Going further** (derivations, side effects, comparisons).
- **Key ideas**: 3–6 bullets, bold names matching the terms introduced.

## Pacing rules (checked)

Between two questions, at most:

- 2 new **bold** terms (bold each term once, where it is introduced);
- 2 equation lines;
- 3 new symbols;
- 260 words.

Keep sentences short. One idea per paragraph. "Key ideas" and "Going
further" are exempt.

## Numbers: one source

Never type a model number into prose if the model has it. Write:

- `{{param instance/path.parameter | 0.012 N·m/A}}` for a parameter;
- `{{value scene=ID observe=KEY reduce=mean window=a..b | 444 rad/s}}` for a
  result of a scene's run;
- `{{data MEASURED-ID field=value | 0.45 rad/s}}` for a measured point.

The text after `|` is what the reader sees before the value loads; `check`
fails when it disagrees with the model (default: its last digit or 1 %).
Worked examples may use rounded numbers, but their results must agree.
Every scene makes claims in `expect` with a `why`, and every claim must
hold.

## Questions that teach

- **Choice** questions: every wrong option is a real misconception with
  `feedback` that names it. Put a `remedy: ID` on options that deserve a
  `sim-remedy` block (misconception, short correction, a scene that shows
  the right picture, a follow-up question).
- **Numeric**: give `unit`. Readers may answer in any compatible unit
  (mN·m, rpm); unit slips get named automatically. Prefer `vary` +
  `answer_expr` + `given` (model values by path) so each review asks new
  numbers, and `check_with` so `check` confirms the formula against the
  simulation.
- **Steps** (`kind: steps`): fade a worked example. The first one shows
  most steps `worked`; the next leaves more blank; then an ordinary
  numeric question.
- **Hints**: a ladder of three, from a nudge to the key idea to the first
  worked step.
- **Pretest** (`pretest: true`): one before a surprising idea, asked
  before it is taught. It is never counted as a miss.
- **Predict / sketch**: before a scene, never after.
- Tag questions with `concepts` when they practise a concept other than
  the lesson's `teaches`.

## Scenes

- Pick the scene's moment: `run` just long enough, `frame_rate` high
  enough for its fastest change. Add `speed` cues around fast changes
  (the pacing check warns when they are still too fast).
- `plots`: 1–2 quantities that tell the story. `phase` for an operating
  point that moves along a line. `show` for the physics layers that matter
  (power, forces, current, heat, trails, values).
- Guide the eye with view cues: `zoom` to the part, `pin` a label, an
  `inset` for small moving parts, `spotlight` to dim the rest.
- Frame the subject, not the whole bench: glide in (`zoom("motor", 1.8,
  1.0)`) while the subject acts, and back out during a reading hold before
  something elsewhere changes (so it is in view when it does).
- Slow motion only where something visibly moves. If the change is fast
  and brief (an inductor's current, a power-off), give that span its own
  slow `speed()` and speed up again right after; `check` warns where a
  plotted change is still too fast.
- `sliders` for the 1–2 parameters worth playing with, `hints` for free
  exploration, a `challenge` with `win` claims, a `companion` run to
  compare two designs.
- Frame the mechanism, not one part: `zoom("", 1.5)` or a part whose
  neighbours matter stays in view. Pick a camera `yaw` that shows gear
  and wheel faces (about 0.95 rad for a drive line along X), and turn on
  only the layers the scene is about (every layer adds labels).
- Choose the camera `preset` for the plane the motion is in: `front`
  for arms, pendulums and profiles in the XY plane (a dovetail's wedge, a
  screw pulling out); `iso` for flat parts and anything seen from above.
  A `zoom` cue keeps that direction.
- Motion of a tenth of a millimetre is invisible at part scale: set the
  scene's `magnify` (e.g. 150) and say so in the caption ("drawn 150
  times larger"). It is display only; plots and values stay true.
- Keep the text still so the motion reads: fewer layers (each adds
  labels), no cue that re-highlights every half second, and captions
  that hold long enough to read.
- Plot quantities that change smoothly. A sampled-and-held command or a
  switching edge is an instant step that no slow motion can follow; plot
  the physical response it causes instead.

## When the lesson needs a new system

Build it in code (`crates/sim-runtime/examples/build_lesson_systems.rs`),
never by hand, and check it runs (`sim-system run FILE SECONDS --csv`)
before writing numbers into prose. Things that bite:

- One inertia per shaft node: fold a motor's rotor into its load's value.
- The averaged H-bridge sets only the voltage between its outputs: join
  the motor's `n` to 0 V, or the solver reports a singular Jacobian.
- `rotational.backlash_mesh` `gap` is half the free play.
- Give a swinging arm a `part.pendulum_gravity` (use `g: 0` for a level
  swing) so it is drawn as a link; put a vehicle's drive train in one
  subsystem so it travels with its wheel.
- With several systems in one lesson, name parameters as
  `system/instance.parameter` (also in `sim-equation` terms).
- Quote YAML values that contain ": " (questions, hints, captions).
- Greek letters in prose and in figure captions count as new symbols for
  the density check, as do letters in `text` equations.

## Honesty

- Say which values are measured and which are estimated. When
  comparing with measurements (`sim-measured`), say if the model was
  fitted to that same data: agreement then shows the fit, not the model.
- Label uncalibrated physics. Do not claim sim-to-real accuracy.
- A `sim-lab` (hardware) step is bounded (|duty| ≤ 0.5, ≤ 5 s), runs only
  through the calibration server's guarded session, and asks for a
  prediction first.

## Tone

Warm, direct, unhurried. "We" for the shared work, "you" for what the
reader does. No jargon before it is introduced. No exclamation marks.
Encourage effort, not speed: "take your time: working it out is what
makes it stick".

## Before you finish

Run `sim-lesson check PATH/lesson.md` and fix every error.

Then look at every scene as a reader will, with the viewer's
`lesson_frames` contact sheet (see `lessons/README.md`), using
`region: card` (what the reader sees): a sheet with a frame every second
or so, then finer sheets over any span that looks wrong. Ask of each frame: is the subject large enough to read, does the
highlighted part stand out, is anything changing, and does the caption
describe what is on screen? Long runs of identical frames mean pacing
spent on nothing; jumps between frames mean a change too fast to follow. Read the
warnings (pacing, density) and fix those too, unless you can say why not.
The lesson is done when check is clean and each part answers "what can
the reader now do that they could not before?"
