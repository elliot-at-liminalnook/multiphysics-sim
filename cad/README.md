# robocad

Direct-modeling CAD for 3D-printable mechanical parts, built on Open
CASCADE with a PySide6/OpenGL desktop UI, and linked to the physics
simulator in this repository so a robot modelled here runs there.

    ./run.sh                       # macOS / Linux (creates .venv on first run)
    .\run.ps1                      # Windows
    .venv/bin/pytest -q tests      # kernel, document, export, parser, bridge and UI tests
    .venv/bin/python scripts/acceptance.py out   # the two-part robot torso, end to end
    .venv/bin/python scripts/robot_leg_demo.py out && ../target/release/sim-cad out/leg.simrobot.json 2

Read `ARCHITECTURE.md` for the design and `USER_GUIDE.md` for the
workflow (Tab-to-type, live dimensions, planes, print helpers, export,
the simulation loop). The Blender add-on is `blender_addon/robocad_link.py`.

For the CI-enforced CAD → motor → controller → measured result workflow,
see the [motorized pendulum acceptance example](../examples/motorized-pendulum/README.md).
