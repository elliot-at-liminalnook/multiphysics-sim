# Lessons

Each folder is one lesson: `<slug>/lesson.md` plus the Rhai scripts (and,
optionally, system files) it uses. Read them in the physical viewer:

```sh
cargo run --release -p sim-spatial -- --lessons lessons [--lesson worm-self-locking]
```

Check every lesson (systems load, references resolve, scripts evaluate and
every `expect` claim holds on the recorded run):

```sh
cargo run --release -p sim-runtime --bin sim-lesson -- check lessons
```

## Format

`lesson.md` is Markdown with YAML front matter (`title` required; `summary`,
`order`, `minutes`, `requires`, `authors`, `category`, and `systems`: names
for system files relative to the lesson folder). Special blocks are fenced with a
`sim-` info string and a YAML body:

- `sim-scene` — `id`, `system`, optional `title`, `caption`, `level`,
  `camera {preset: iso|front|side|top, focus, zoom, yaw, pitch}`,
  `set {instance/path.parameter: value}` (applied before the run),
  `run {duration_s, frame_rate}`, `script` (Rhai), `cues` (YAML cues),
  `plots`, `phase` (operating-point plots: `[{x, y, title}]`, y against x
  with a dot at the playhead), `show` (physics layers: `power`, `forces`,
  `current`, `heat`, `trails`), `expect` (claims checked in CI), `height`,
  `fidelity` (`detailed` or `realtime`), `autoplay`, `magnify` (draw
  sliding motion this many times larger, 1–100000: for tenths of a
  millimetre that would otherwise be invisible; display only, the pace
  line says "motion ×N (display only)", and values and plots stay true).
  The camera starts from `preset` (plus `yaw`/`pitch`); a script's
  `zoom(part, …)` keeps that direction and frames the part.
  Interaction (all optional):
  - `sliders: [{parameter, label, min, max, step, unit}]` — the reader
    drags one, lets go, and the scene is recorded again with that value
    (cached by hash). The claims are then hidden with a note, because they
    are written for the lesson's values.
  - `hints: [text]` — "Things to try", shown in free explore.
  - `companion: {label, set, mode: split|ghost}` — a second run with `set`
    applied, drawn beside this one (`split`) or as a faint wireframe over
    it (`ghost`). Its curve is added to each chart in purple.
  - `challenge: {goal, win, hint}` — a goal reached with the sliders.
    `win` is a list of claims in the `expect` form, judged on every run
    with the reader's values; a met challenge is saved to progress. Needs
    `sliders`.
- `sim-quiz` — `id`, `question`, and either `options` (choice) or `kind`:
  `numeric` (`answer`, `tolerance`), `predict` (commit before the scene
  runs; judged against the run), `sketch` (the reader draws the curve of
  `observe` over `range: [lo, hi]` before the `scene` runs, then sees it
  over the simulated curve with the typical gap), or `steps` (a worked
  example: each step `worked` or left blank with `answer`, `unit`,
  `tolerance`; fade support by blanking more steps in later questions).
  `moment` adds a "Show me in the scene" button that jumps the scene to
  that time. Any question may add:
  - `hints: [nudge, key idea, first step]` — revealed one at a time; hints
    used hold the question back in the review schedule;
  - `pretest: true` — asked before the teaching; never counts as a miss;
  - `concepts: [id…]` — what it practises (default: the lesson's `teaches`);
  - on an option, `remedy: ID` — the `sim-remedy` shown when it is picked.

  Numeric answers are read with their unit ("24 mN·m", "4300 rpm") and
  converted; slips (a factor of 1000, rpm for rad/s, a sign) are named.
  Numbers can change per attempt: `vary: {V: {min, max, step}}` with
  `{V}` in the question, `answer_expr: V / k`, `given: {k:
  motor.torque_constant}` (model values, never copied), and `check_with:
  {scene, observe, reduce, window, set: {supply.voltage: V}}` so
  `sim-lesson check` confirms the formula against the simulation.
  Readers say how sure they are; a confident miss comes back the next day.
- `sim-reflect` — `id`, `prompt`, optional `gate`, `model_answer`,
  `key_points: [{idea, cues: [words…]}]` (after writing, the reader sees
  which ideas their text covers; "Ask Codex for feedback" posts it as a note).
- `sim-recall` — the same fields: "write what you remember" of another
  lesson (`of`), then tick the key points you had.
- `sim-equation` — `id`, `scene`, `expr` (`k * i`), `result {symbol, unit}`,
  `terms {name: {symbol, unit, param | observe | value}}`, optional `show`
  and `holds {observe, window, tolerance}`: shown as `τ = k·i = 0.012 ×
  3.29 = 39.5 mN·m` at the playhead; `check` confirms it holds on the run.
- `sim-measured` — `id`, `system`, `data` (a `sim.fit-data/1` file), `x
  {field}`, `y {field, observe, reduce, window}`, `set` (parameters per
  point: `supply.voltage: duty * supply_v`), `only`, `run`, `max_rms`,
  `max_gap`: each measured point beside its simulation, the gaps, where
  every parameter came from, and a warning when the model was fitted to
  this same data.
- `sim-remedy` — `id`, `misconception`, `body`, optional `scene` and
  `then` (a follow-up question). Hidden until a wrong option names it.
- `sim-task` — `id`, `kind: fault|design`, `scene`, `goal`, `start`
  (values the sandbox starts with), `win` (claims), `report` (metrics per
  attempt), `hints`, `solution` (proves solvability in `check`; never
  shown). The task runs its own scene on its own builder copy.
- `sim-lab` — `id`, `joint`, `test {duty, seconds}` (at most ±0.5 and 5
  s), `compare` (a `sim-measured` block), `predict`, `unit`, `notes`: a
  bounded bench test through the calibration server (`SIM_BENCH_URL`),
  after the operator checklist; compared with the prediction, the model
  and the recorded measurement.
- `sim-component` — `component` (registry type), `show` (summary,
  explanation, equations, tradeoffs, limits, parameters).
- `sim-compare` — `id`, `system`, `study` (a saved study in the system file).

Numbers in prose come from the model: `{{param motor.torque_constant |
0.012 N·m/A}}`, `{{value scene=ID observe=KEY reduce=mean window=a..b |
444 rad/s}}` or `{{data MEASURED-ID duty=0.15 | 0.45 rad/s}}`. The text
after `|` is what plain Markdown shows; `check` fails when it disagrees
(`tol=2%` to widen). Front matter `teaches` and `needs` name concepts from
`concepts.yaml`, which drive the concept map and the next-lesson
suggestion.

Links `[text](part:<system>/<instance path>)` highlight a part in the live
scene. Clicking a part (or pressing F on a selection) glides the camera to
it; H glides home. Scripts use `at`, `wait`, `caption`, `highlight`, `camera`, `plot`,
`set`, `pause` and `speed` (see `sim_script::presentation`). Only `set`
changes physics, and it is recorded with the run.

**View directives** move the camera and set emphasis; none changes
physics. The same definitions (`sim_script::presentation::View`) serve
three places:

| Directive | Rhai | YAML cue (`view: [...]`) | Narration |
|---|---|---|---|
| Glide to frame a part | `zoom("motor", 1.6, 1.5)` | `{view: zoom, focus: motor, zoom: 1.6, seconds: 1.5}` | `[[zoom motor 1.6 over=1.5]]`, `[[zoom all 1]]` |
| Circle slowly (rad/s) | `orbit(0.15)` | `{view: orbit, rate: 0.15}` | `[[orbit 0.15]]`, `[[orbit off]]` |
| Dim all but | `spotlight(["gearbox"])` | `{view: spotlight, paths: [gearbox]}` | `[[spotlight gearbox]]`, `[[spotlight off]]` |
| Arrow + label on a part | `pin("rotor", "the rotor")` | `{view: pin, path: rotor, label: "the rotor"}` | `[[pin part:rotor "the rotor"]]`, `[[unpin]]` |
| Close-up in a corner | `inset("gearbox", 2.5)` | `{view: inset, path: gearbox, zoom: 2.5}` | `[[inset gearbox 2.5]]`, `[[inset off]]` |
| See-through | `xray(true)` | `{view: xray, on: true}` | `[[xray on]]` |
| Exploded | `explode(true)` | `{view: explode, on: true}` | `[[explode on]]` |

The learner's drag or zoom always interrupts a glide or orbit. While the
narration speaks, its directives win; otherwise the scene's apply.

An `expect` is `{observe, reduce: final|mean|max|min|peak|change|integral,
window: [t0, t1], min, max, why}` — the same reductions as saved studies.

Errors always name `lesson.md:line`. Notes on a lesson live beside it in
`lesson.md.annotations.json`; they re-attach after the text is edited.
Opening a scene in the builder edits a sandbox copy under
`runs/lessons/sandbox/`, never the lesson's system file.

## Categories

`categories.yaml` lists the groups the lesson list shows, in order, each
with an `id`, `title` and `summary`. A lesson joins one with `category:
<id>` in its front matter; lessons without one are listed last under
"Other". An unknown id is an error in `sim-lesson check`. Readers fold and
unfold groups in the sidebar (REST: `lesson_categories`, `lesson_fold`).

## Pacing

Scenes play on the shared pacing rules (`sim_script::pacing`), so a reader
can take in each moment. They follow the multimedia-learning principles:
reading time, signaling, segmenting and apprehension.

- **Captions hold for reading.** A new caption holds the scene still for a
  short orientation beat. If the motion that follows is too short to read
  the caption during it, the scene holds for the remaining reading time
  (about 170 words per minute).
- **Highlights and camera moves hold for looking.** Each gets about 2 s
  (1.2 s for a camera move) before the next change.
- **Every stretch of motion lasts at least 1.5 s on screen.**
- **Fast changes slow down automatically.** From every sample, the time
  until a plotted quantity has moved by half its range is measured; where
  that is under 1.5 s on screen, playback slows (up to 8× beyond the
  authored `speed`) over just that move. Ripple smaller than half the range
  never triggers it. Speed eases by at most 1.25× per 0.2 s of screen time,
  so deep slow motion recovers within a second or two.
- **Highlights are visible.** A script's `highlight` pins an arrow with the
  part's name and dims the rest of the scene a little.
- **Narration boxes and arrows stay at least 2 s,** even if `[[unmark]]`
  comes sooner.
- **The viewer always shows the pace** ("slow motion, 1/100 speed",
  "holding to read"). Charts draw up to the playhead in step with the scene.
  Playback interpolates the step-resolution recording, so slow motion stays
  smooth and exact.

`sim-lesson check` prints how long each scene plays on screen. It warns
where a plotted change is still too fast even at 8× automatic slow motion.
Add a slower `speed()` around that moment, starting a little before it so
the slow-down can ease in. Put `pause()` at event boundaries you want the
reader to predict or discuss. Physics is never changed by pacing.

**See the animation before you ship it.** The viewer's REST command
`lesson_frames` draws a contact sheet: many frames of a scene as one
labelled grid (index, screen time, sim time; metadata adds caption and
pace). `seek` mode places frames on the screen clock (holds and slow
motion show as runs of similar frames) or the sim clock; `live` mode
captures every `interval_s` while the scene or a narration section plays
for real (muted), camera glides included. Start with 64 frames over the
whole scene, then zoom into a span with `from`/`to` or explicit `times`:

```sh
curl -s -X POST localhost:PORT/v1/commands -H 'content-type: application/json' \
  -d '{"command":"lesson_frames","args":{"scene":"load-step","mode":"live","interval_s":1.0,"count":16,"region":"view","path":"/tmp/sheet.png"}}'
```

## Pacing the ideas

Lessons introduce **one idea at a time** (cognitive-load theory: working
memory holds only a few new elements at once). Each part follows the same
shape:

1. A concrete picture of the idea.
2. At most one equation.
3. A worked example with real numbers.
4. A short question that uses the idea and opens the next part (retrieval
   practice).

Meet the pieces before combining them (pre-training). For example, the
motor lesson covers torque from current and back-EMF from speed before the
voltage budget that joins them. Derivations, side effects and design
trade-offs go in an optional **Going further** section after the core.

Write each new term in **bold** where it is introduced. `sim-lesson check`
counts, between two questions:

- new bold terms (at most 2);
- equation lines (at most 2);
- new symbols (at most 3);
- words (at most 260).

It warns where a stretch asks too much (`sim_lesson::density`). It also
estimates how long the lesson takes: reading at 150 words per minute, a
minute per question, two per reflection, plus the scenes' time on screen.
"Key ideas" and "Going further" are exempt.

## What the 3D view shows

Everything moving in the view is driven by the recorded measurements
(`sim_runtime::system_display` derives the bindings from the compiled
model; `sim_spatial::physics_view` draws them):

- **Parts.** Single shafts (rotors, worms, drums) turn with their angle.
  Housings with two or more shafts (motors, couplings, gear stages) are
  drawn see-through, with their moving pieces inside: an armature, two
  coupling halves twisting against the spring, or a pinion, idler and gear
  for ratios up to 5. The idler's angle is gear kinematics.
- **Links.** A `part.pendulum_gravity` (a leg, an arm, a pointer) is
  drawn as a rod from its joint to its centre of mass, with a bob there,
  hanging straight down at angle zero. Parts on the same shaft placed
  away from the joint (an IMU down a leg) turn about the joint with it.
  Give an arm swinging level `g: 0`: it is then only its drawing.
- **Sliding parts.** A part on a single linear port moves with its
  position. A rack-and-pinion or belt draws its rope or belt from the drum
  to the load. A `part.drive_wheel` rolls: it turns with its axle and
  slides with the chassis it pushes, and so does every part in the same
  subsystem (put a rover's motor, gearbox, hub and wheel in one
  subsystem so they travel together).
- **Framing.** `zoom` frames a part together with the pieces drawn
  inside it (an arm, a gear train), and the split view's copy shows those
  pieces posed by the companion run.
- **Layers** (the "Show" row under a lesson scene's controls, or chips at
  the top right of the builder's view; `show` in a scene):
  - Values: speed (rad/s and rpm), travel from the start, temperature and
    current, on each part.
  - Power: what each part gives or takes, and the sum over all parts.
  - Forces: the torque each part applies to its shaft and the force it
    applies to a slide; orange drives, blue resists.
  - Current: dots along wires, speed ∝ current.
  - Heat: temperature glow and heat-flow chevrons.
  - Trails: fading copies of moving parts.

  Layer sizes are relative to the largest value on screen; labels give the
  values.
- **Steady labels.** Labels stay put so the motion is what moves: values
  show three significant figures, change at most four times a second
  while playing (at once after a seek or pause), and each label keeps its
  place, easing after its part instead of jumping. Its box is sized for
  the name plus a fixed allowance, so a changing number never moves it.
- **View switches** in the same row: X-ray, Explode (eased; parts without
  an authored offset move about two of their own sizes away from the
  middle of their assembly) and Strobe (crisp spokes instead of motion
  blur for fast shafts).

## Robot physics lessons

Twenty lessons (orders 10–29) build the physics behind a small robot one
phenomenon at a time. Their systems are generated by
`cargo run --release -p sim-runtime --example build_lesson_systems [SLUG…]`
(representative hobby-robot values, each recorded as an estimate with its
reason); edit the example, not the JSON.

| Lesson | Phenomenon | New library pieces it uses |
|---|---|---|
| `inertia-acceleration` | τ = J·α; where the mass sits | `part.torque_command` |
| `gear-ratio` | torque ×N, speed ÷N, reflected inertia J/N² | |
| `friction-deadband` | dry and viscous friction, the dead band | |
| `series-elastic` | spring torque, cushioning impacts | |
| `resonance` | natural frequency, damping ratio, a frequency sweep | `torque_command` chirp |
| `gravity-torque` | m·g·r·sin φ, servo sag, stall | link drawing |
| `backlash` | lost motion on reversal | |
| `linear-drives` | belts and screws as ratios in rad/m | |
| `wheel-traction` | μ·N, wheelspin and skids | rolling wheel, riding module |
| `battery-sag` | internal resistance, brownout | |
| `motor-heating` | I²·R, thermal time constant, ratings | |
| `back-driving` | generator braking | |
| `wiring-decoupling` | lead R and L, bus capacitors | |
| `h-bridge-modes` | drive, brake, coast, plug | |
| `stepper-steps` | synchronism and lost steps | `part.trapezoid_move` |
| `encoder-resolution` | counts and speed estimates | `part.speed_estimate` |
| `pid-joint` | P, I and D on a gravity-loaded joint | |
| `loop-rate` | sampling and latency | |
| `imu-tilt` | accelerometer, gyro drift, complementary filter | `part.tilt_imu`, `part.complementary_tilt` |
| `current-control` | torque through a PI current loop | `part.pi_current` |

All values are representative estimates, not measured parts, and the
models are labelled with their fidelity in each scene. Nothing here claims
to match a particular robot's hardware; the knee lesson
(`knee-servo-measured`) is the one comparing a model with measurements.

## Printed joints

Three lessons (orders 30–32, category `printed-joints`) cover joining 3D
printed parts. Their systems come from the same `build_lesson_systems`
example, and the parts on screen are CAD-built display models
(`cad/scripts/component_models.py`, exported to `library/models`).

| Lesson | Phenomenon | Library parts |
|---|---|---|
| `alignment-pins` | clearance and contact stiffness; one pin locates, two over-constrain | `part.dowel_pin` |
| `dovetails` | the wedge: flank force and wall spreading; preload and sliding friction | `part.dovetail`, `part.force_command` |
| `heat-set-inserts` | pull-out as shear of a plastic cylinder, τ·π·D·L·η; boss design | `part.threaded_joint` |

PLA strengths and stiffnesses are estimates, recorded with their reasons;
printed parts vary widely with settings and layer direction.
