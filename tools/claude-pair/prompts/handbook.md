# Project handbook

Working knowledge for this repository. Rules live in AGENTS.md (loaded
automatically); this is how to get things done quickly. Verify against the
code when something looks stale, and say so in coordination_notes.

## Build and test

- Rust 2024 workspace, 45 crates under `crates/`. `cargo` is on PATH.
- You share the user's `target/` build cache, which is usually warm. Never
  `cargo clean` it or change profiles or RUSTFLAGS just to retry a build.
- **The dev profile keeps line tables only** (`debug = "line-tables-only"` in the
  workspace `Cargo.toml`, since 2026-10-02): backtraces still show files and
  lines, links are much faster and the cache far smaller. The first build after
  that change rebuilds everything once (about 20 to 30 minutes for the
  workspace); later builds are incremental. For a debugger session with full
  variable info, build with `CARGO_PROFILE_DEV_DEBUG=true` (a separate
  rebuild); don't change the profile itself.
- **One cargo command at a time.** The `cargo` on agents' PATH queues each
  command until the previous one finishes, because check, test and build of the
  same crate undo each other's work. Subagents never run cargo.
- **Don't build or test during normal work.** Cold or even incremental cargo
  commands here take minutes (a `sim-runtime` or `sim-spatial` test binary
  takes 2 to 4). Claude Code stops every shell command after 10 s, and
  background commands are off. Nothing is built or tested: verification is by
  reading the code.
- The gait-lab runtime fingerprint hashes every crate's source, so any code edit
  invalidates gait qualification (see the gait-lab README before evaluating gaits).

## The native viewer: `sim-spatial` (Bevy)

- Target shape: `docs/architecture/native-viewer.md`. Bevy is pinned in the
  workspace `Cargo.toml` (0.19.1); check that version's docs (docs.rs/bevy/<version>) and
  the migration guides on bevy.org rather than recalling APIs.
- Bevy practice for this codebase: `tools/claude-pair/prompts/bevy.md`.

- Build mode (System Builder): `cargo run -p sim-spatial -- --system examples/systems-builder/motor-driver-board/board.system.json`
  (other examples: `examples/systems-builder/*/*.system.json`).
- Lessons: `--lessons lessons [--lesson <slug>]`. Scanned places: `--place DIR`.
  `--validate-only` checks inputs without a window. `--headless` has no window
  (and so no screenshots).
- Every window serves a loopback REST API (`--api-port`, default 8421).
  `GET /v1/capabilities` lists every command with an argument example.
  `POST /v1/batch {"commands":[{"command":"…","args":{…}}]}` returns a job URL to poll.
  Useful commands: `system_ui` (discover and activate live controls through the
  same handlers as a click), `system_state`, `state`, `display`, `camera`,
  `fit`, `screenshot` (the window exactly as drawn).
- CAD mode, for agents: start with `cad_guide` (also `GET /v1/cad_guide`), then
  `cad_state`; edit with `cad_model`, `cad_op` (any RoboCAD operation by
  name), `cad_sketch`, `cad_patch`; atomic multi-step edits with `cad_model
  {op: batch}`; talk in `cad_threads` (`author_kind: agent`, `watch`, the
  in-window assistant via `ask` / `ai`); show things with `cad_views` (named,
  sliced views) and `cad_render`. Every edit is one undo step; nothing is
  written until `cad_save`.
- **Screenshots are off** (unless "This run" says they're on). `ui_capture.py`
  can drive the viewer and save screenshots, but don't run it: the binary isn't
  rebuilt, so screenshots would show stale code. Read the code instead.

## Other surfaces

- `sim-app`: `--scene phenomena [--exhibit N]` (live exhibits) and
  `--scene cad --model <file.simrobot.json>`.
- `web/`: browser viewers (Node build scripts, `web/README.md`).
- `cad/`: robocad, the Python/OCCT/Qt CAD tool the Rust CAD mode replaces
  (`cad/README.md`, `cad/ARCHITECTURE.md`). **Read it as the reference for
  behaviour; never start it or call its REST service** (user decision
  2026-10-03: everything in Rust, in one process).
- `crates/sim-runtime/examples/serve_actuator_calibration.rs` and
  `serve_motor_bench.rs`: the hardware servers the in-process leg driver
  replaces. Read them as the reference, including every safety rule.

## Lifecycle starting points (design to delivery)

Nothing models suppliers, a BOM, assembly sequences, inventory or shipping
yet (2026-10-13). Build on these rather than beside them:

- **Robot project:** `crates/sim-runtime/src/robot_project.rs`
  (`sim.robot-project/2`) and `crates/sim-spatial/src/project/`; the step
  strip and plan are in `docs/robot-pipeline.md`. Make (part files tied to
  the tested CAD hash) is the last step today; new stages extend the strip.
- **Printed parts:** `sim-print` (registry of printer, filament and joint
  values, `plan`, voxel stress, `promote` for coupon results) and CAD print
  in `crates/sim-spatial/src/cad/print/` (studies, checks, jobs tracker,
  fastener tool).
- **One accepted source with provenance:** the actuator registry,
  `crates/sim-runtime/src/actuator_registry.rs` over
  `examples/actuators/hx30hm/accepted/registry.json`, with promotion. A parts
  catalog should follow its pattern.
- **What CAD already knows:** `sim_cad::physical` (bodies, mass, materials,
  motors from the library, assumptions) and the component registry behind
  CAD components.
- **Selection and budgets:** datasheets (`crates/sim-runtime/tests/datasheets.rs`)
  and `examples/full-robot/measured-actuator-integration/battery-selection/`.
- **Robots to try it on:** `examples/robot-pipeline/`, `examples/robot-kit/`
  (rover, drone, gripper, belt axis, arm), `examples/wheeled-robot/`.

## Data and history

- `runs/` is ignored and very large; `EXPLORATION_DATA.md` says what must be
  preserved. Experiments, recordings and calibration data are never disposable.
- Root progress documents (`systems-builder-progress.md`, `builder-roadmap.md`,
  `systems-viewer-plan.md`, …) are leads. Code is the truth.
