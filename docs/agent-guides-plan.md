# Agent guides for every mode — plan (2026-10-08)

Status: done 2026-10-08, all four steps. Results and the test run are in
`docs/architecture/native-viewer.md`, "Agent guides for every mode".

AGENTS.md: every screen is usable by an AI, and each mode's functionality is
"covered by that mode's guide command (like `cad_guide`, also
`GET /v1/<mode>_guide`)". Four of the viewer's seven modes have one (Build:
`system_guide`, CAD: `cad_guide`, Robot: `robot_guide`, plus the cross-mode
`project_guide`). Inspect, Lessons, Place and Phenomena have none.

## 1. Four guide commands

`inspect_guide`, `lesson_guide`, `place_guide`, `phenomena_guide`, built like
the existing ones (`builder/guide.rs`, `robot/guide.rs`):

- One `guide(topic)` function per mode returning `about`, `how_to_call`,
  `concepts`, `workflows` (in order), `commands` (every command of the mode,
  each with a working example) and `rules`; `topic` narrows it to one section
  and an unknown topic is refused naming the valid ones.
- A variant on the mode's own action type (`InspectAction`, `LessonCommand`,
  `PlaceAction`, `PhenomenaAction`), so the guide goes through the same
  action layer, is listed in `GET /v1/capabilities` with its mode, and is
  refused by name in other modes.
- Published as `GET /v1/<mode>_guide` from the mode's existing publishing
  system (non-blocking read for an agent).

| Mode | Guide covers |
|---|---|
| Inspect | the 12 spatial commands (state, description, spatial, animation, measurements, render, select, display, camera, fit, panels, annotations); descriptions vs frames vs presentation; provenance; the headless server |
| Lessons | listing and opening, scenes and claims, quizzes, reflections, tasks, labs, compares, notes, edits with hashes and undo, narration (paid, ceiling), contact sheets, progress |
| Place | `state` and `camera`; stations and photo views; the viewer frame (metres, Y up); `sim-place` and its MCP tools for queries the view does not answer |
| Phenomena | the 8 `phenomena_*` commands and `system_ui`; exhibits, knob, readouts, verdict; relation to the `sim-phenomena` acceptance suite |

## 2. Close the "see" gap the guides expose

Reading the code changed this step. `screenshot` already works in every mode,
and Phenomena publishes a full `phenomena_state`, so neither mode is blind.
The real gap is Place:

- `state` reports only the number of photo views, though the mode holds each
  photo's position and viewing direction, and `camera` can jump only to
  stations. Add `photo_views` (position, direction) to `state` and a
  `camera {view: i}` jump to a photo's viewpoint.
- While checking the camera maths for that jump: the fly camera's `pitch`
  rolls the view around its forward axis instead of tilting it (forward stays
  `(1, 0, 0)` for every pitch at yaw 0; checked numerically). Fix
  `camera::fly::orientation` so pitch tilts, keeping yaw's meaning, and add a
  test.

## 3. Tests so the rule holds from now on

- Every `ViewerMode` has a mode-specific `*_guide` command registered for it
  (`project_guide`, which every mode accepts, does not count).
- Each new guide lists exactly its mode's commands, and every example parses
  as the command it documents.
- The fly camera: pitch tilts forward up/down; yaw 0 pitch 0 still looks
  along +X.
- `copy_guard_tests.rs`: the four guide files join `DESCRIPTION_FILES`.

## 4. Verify and record

- `CARGO_INCREMENTAL=0 cargo test -p sim-spatial --lib`.
- Live: `sim-spatial --headless` for `inspect_guide`; the windowed viewer over
  REST for the other three (`viewer_mode` into each mode, then the guide
  command and `GET /v1/<mode>_guide`), and `camera {view}` in Place.
- Record in `docs/architecture/native-viewer.md` and update the guide row of
  `docs/architecture/repository-map.html`.
