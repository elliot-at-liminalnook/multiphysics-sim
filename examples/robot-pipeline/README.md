# One robot through the whole pipeline

`lift_arm.py` takes a one-joint lifting arm from a blank window to a tested
design and its part files, entirely through the viewer's REST API: the same
steps a person takes through the bottom strip (`◆ Lift arm  1 Design ›
2 Model › 3 Test › 4 Learn › 5 Make`) and the project card.

```sh
target/debug/sim-spatial                         # a blank window: the Start card offers a new robot
python3 examples/robot-pipeline/lift_arm.py /tmp/lift-arm
target/debug/sim-spatial --project /tmp/lift-arm # reopen it later, every mode on this robot
```

What happens:

1. **Design.** `project_new` makes `lift-arm.robot.json`, an empty
   `lift-arm.rcad` (RoboCAD's stock materials) and the results, lessons and
   make folders, and opens CAD mode on the design. The script then models
   the arm through CAD's own commands (each one undo step), as the design
   assistant does when given a description: a grounded PLA base, an MG996R
   servo mounted on it, a PLA arm on the servo's revolute joint, a steel
   payload bolted to the arm's tip, an encoder, and an estimated drive
   backlash (the simulator refuses an unmeasured one).
2. **Model.** `project_step model` saves the design and exports it in
   process (`sim_cad::physical`) to `lift-arm.simrobot.json`. Every estimate
   is listed in the model (servo constants, catalogue material values, the
   backlash estimate…); none may block.
3. **Test.** `project_set_test` states what the arm must do: lift the
   payload to 60° in a second and hold it, keeping 30 % of the servo's stall
   torque and 20 °C of winding temperature in reserve, within its limits,
   with every printed part at least twice as strong as the run's peak loads
   need (the print registry's layer-aware stress check). `project_test` runs
   it headless and writes `results/<stamp>.test.json`: each criterion pass,
   fail or not assessed.
4. **Learn.** `project_lessons suggest` lists lessons for this robot (its
   servo's holding torque and heating, its joint's play, printed-part
   strength, its encoder, what the simulation proves). `--lesson TOPIC`
   has the AI write one into `lessons/<topic>/lesson.md`, with this robot's
   numbers and a render of it; Lessons mode opens it.
5. **Make.** `project_make` writes each printed part's STL and
   `make/parts.json` (the tested CAD file's SHA-256, materials, masses, the
   motors to buy).

The design assistant does step 1 from words: `project_new {name,
description}` hands the description to it, and `project_chat {text}` asks
for changes ("make the arm 20 mm longer and run the test again"). It needs
the Codex CLI (`sim_agent`).

What a passed test does not show: the model's estimates are estimates (the
servo's electrical and thermal constants, the drive backlash, catalogue
material values), flexible links and link-to-link contact are not modelled,
and the part check's loads are a stated equivalent of the measured peak
torque. Measure the servo and the joint on the bench before trusting the
margins.
