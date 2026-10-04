# One robot, end to end: the pipeline plan

Goal (2026-10-04): a novice starts at a blank window, describes a robot to the
AI, watches it appear in CAD, turns it into a simulation model, runs a test
that says plainly whether the controller and the hardware are good enough,
learns about its parts in lessons written for that robot, and gets the files
to make it. All in the native viewer, all over REST too.

Proof instance: a one-joint arm (base, servo, arm, payload) that must lift a
stated load to a stated angle in a stated time without overloading the servo
or overheating it.

## The order of operations (what the user sees)

The bottom strip of every mode shows the open robot and its steps:

    Robot: lift-arm   1 Design ✓  ›  2 Model ✓  ›  3 Test ✗  ›  4 Learn  ›  5 Make

| Step | Mode | Done when |
| --- | --- | --- |
| 1 Design | CAD | the `.rcad` has bodies, at least one joint, a motor on it |
| 2 Model | CAD → export | the simulation model was exported from the current CAD revision and has no unresolved assumption |
| 3 Test | Robot | the latest test run of that model passed every acceptance criterion |
| 4 Learn | Lessons | (always available) suggested and written lessons for this robot |
| 5 Make | CAD export | part files exported from the tested revision |

Each step is a button: it switches to that mode on the project's document.
A step that is not ready says why (and what to do) instead of opening.

## Pieces

1. **Project file** `<dir>/<name>.robot.json` (`sim.robot-project/1`): name,
   CAD file, model file, the test (commanded trajectory, duration,
   acceptance criteria), lessons folder, the design conversation and the
   latest results. The robot's physical definition stays in CAD; the test and
   the acceptance criteria are task conditions and live here (AGENTS.md:
   separate robot, world and policy).
2. **Project context across modes** (`app::project`): one current project
   per window. Opening a project points every mode at it; switching modes
   through the steps carries the project's document. REST: `project_state`,
   `project_new`, `project_open`, `project_close`, `project_step`,
   `project_test`, `project_guide`.
3. **Rust physical export** (`sim_cad::physical`): the `.rcad` to a
   `.simrobot.json` in process, replacing RoboCAD's `export_physical_model`
   in CAD's existing export job: links (fixed joints and mounted motors
   merge), exact mass/COM/inertia, collision surface, joint physics, motors
   from the library, sensors, battery, control, materials with engineering
   properties. Every estimated value is listed in `source.assumptions`.
   Not ported (stated in the file): flexible links (rigid only), SDF
   link-to-link contact.
4. **Qualification** (`sim_domain_robot::qualification`): what in a model is
   missing, defaulted by the loader, or estimated. A test result counts as
   evidence only when nothing is missing; estimates are listed with it.
5. **Acceptance tests** (`sim_runtime::acceptance`): run the model headless
   for the test's duration with its commanded trajectory and judge each
   criterion pass / fail / not assessed. A missing assessment is never a
   pass. Replaces `PhysicalRobot::success` as the verdict (that function's
   substituted margins are fixed too).
6. **Blank-screen start**: a New robot screen (name, folder, a description
   of what it should do) creates the project and an empty `.rcad`, opens CAD
   mode, and hands the description to the design assistant, which works on
   an empty document through a project-level chat (not a pinned comment).
7. **Lessons for this robot**: Lessons mode shows the project's lessons and
   suggestions drawn from its parts and its test results; Write lesson has
   the AI draft one into the project's lessons folder, with figures rendered
   from the CAD model.
8. **Make**: part files (STL/3MF) exported from the tested CAD revision;
   print studies remain unported and are said to be.

## Status (2026-10-04)

Built and exercised end to end over REST (`examples/robot-pipeline/lift_arm.py`
from a blank window; details in its README):

| Piece | Where | State |
| --- | --- | --- |
| Project file and its folders | `sim_runtime::robot_project` | done, tested |
| Project context in every mode, steps, strip chips, project card, `--project`, Start card on a blank launch | `crates/sim-spatial/src/project/` | done; verified over REST (the screen was locked, so the drawn card has not been seen) |
| Following the modes: CAD or Robot opening another project's file adopts that project | `project::actions::follow_documents` | done |
| Rust physical export in CAD's export job and live link | `sim_cad::physical`, `sim_cad::materials` | done; an arm built from an empty archive exports, builds, lifts and passes (test `physical_export`) |
| Engineering material defaults with registry values (also fixes CAD's materials dialog source) | `sim_cad::materials` | done |
| `set_joint_physics` stored where RoboCAD stores it (`robot.physics`) | `sim_cad::ops` | fixed |
| New documents get RoboCAD's stock materials | `sim_cad::edit::empty_archive` | fixed |
| Acceptance tests (pass / fail / not assessed; never a pass when unassessed) | `sim_runtime::acceptance` | done, tested |
| Honest margins in `PhysicalRobot::metrics`/`success` (Monte Carlo) | `sim_runtime::physical` | fixed |
| Printed-part strength from the run's peak loads (layer-aware voxel check) | `sim_runtime::part_strength`, `project::strength` | done |
| Design assistant from an empty design (`project_new` with a description, `project_chat`) | `project::chat` | done; verified live: it lengthened the arm, remade the model, re-ran the test and reported |
| Lessons for this robot (suggestions, AI-written lesson validated by the lesson parser, figure rendered from CAD) | `project::lessons` | done; verified live: a 210-line lesson with the arm's real numbers |
| Make: STL of each printed part and `parts.json` tied to the tested CAD hash | `project::make` | done |
| `robot_open` (a robot file in place, for the Test step) | `robot::actions` | done |
| Mode guides point at `project_guide` | `cad_guide`, `robot_guide` | done |

Not done or not modelled (each said where it shows):

- Flexible links and link-to-link contact in the export (rigid links;
  `source.not_modelled`); bearing inference from cylinder pairs (estimated
  from the motor shaft unless set in CAD).
- Print studies (splitting, plates) remain unported; Make says so.
- The part check's loads are a stated equivalent of the measured peak motor
  torque, not a full load history.
- Lessons for a robot have no live builder system (figures, numbers and
  quizzes); Lessons mode cannot switch folders in place, so a lesson written
  while Lessons shows another folder is opened from the Learn step later.
- The controller under test is the model's servo firmware following a
  commanded trajectory; the project does not yet test a user's own
  controller program or realistic sensing (the robot's seam and drive
  profile paths exist in Robot mode but are not wired to the project test).
