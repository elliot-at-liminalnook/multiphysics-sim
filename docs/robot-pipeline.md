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

## Status

Tracked below as it lands.
