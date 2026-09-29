# Project handbook

Working knowledge for this repository. Rules live in AGENTS.md (loaded
automatically); this is how to get things done quickly. Verify against the
code when something looks stale, and say so in coordination_notes.

## Build and test

- Rust 2024 workspace, 45 crates under `crates/`. `cargo` is on PATH.
- You share the user's `target/` build cache, which is usually warm. Never
  `cargo clean` it or change profiles or RUSTFLAGS just to retry a build.
- Default loop: `cargo check -p <crate>` for compile questions,
  `cargo test -p <crate> --lib` or `cargo test -p <crate> <test_name>` for
  behavior. Build release only when you need the speed, such as long simulations.
- `cargo run --release -p sim-phenomena -- all` regenerates the full phenomena
  suite and takes about 50 minutes. Run single scenarios instead unless the
  whole suite is the point.
- The gait-lab runtime fingerprint hashes every crate's source, so any code edit
  invalidates gait qualification (see the gait-lab README before evaluating gaits).

## The native viewer: `sim-spatial` (Bevy)

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
- `ui_capture.py` (path in "This run" below) wraps all of that: it launches the
  viewer, runs a JSON script of commands, saves PNGs and a `capture.json`
  receipt, and exits nonzero on any failure. Use it for evidence and as a check.
  Put captures under `$PAIR_CAPTURES/<task-id>/` and look at them with Read.
  A binary older than the source may lack newer commands. Rebuild it first
  (`target/debug/sim-spatial` was last built before the `screenshot` command existed).

## Other surfaces

- `sim-app`: `--scene phenomena [--exhibit N]` (live exhibits) and
  `--scene cad --model <file.simrobot.json>`.
- `web/`: browser viewers (Node build scripts, `web/README.md`).
- `cad/`: robocad, a Python/OCCT/Qt CAD tool (`cad/README.md`, `cad/ARCHITECTURE.md`).
  Tests: `.venv/bin/pytest -q tests` from `cad/` (`cad/run.sh` creates the venv
  if it is missing). A GUI window serves REST on port 8420.

## Data and history

- `runs/` is ignored and very large; `EXPLORATION_DATA.md` says what must be
  preserved. Experiments, recordings and calibration data are never disposable.
- Root progress documents (`systems-builder-progress.md`, `builder-roadmap.md`,
  `systems-viewer-plan.md`, …) are leads. Code is the truth.
