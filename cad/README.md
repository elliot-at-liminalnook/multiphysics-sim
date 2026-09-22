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

## Crash diagnostics

When the editor crashes, check `~/Library/Logs/RoboCAD/` on macOS first.
`latest-editor.json` points to the newest editor's `session.log`. Each session
keeps Python tracebacks, native stderr/Qt messages, recent UI actions and model
path/revision, plus `session.json` with its PID and start time. Logs survive the
background launcher's exit. **Help → Open diagnostics folder** opens the folder.

From the repository root, collect matching macOS native crash reports and print
the latest log path:

```sh
PYTHONPATH=cad cad/.venv/bin/python -m robocad.diagnostics
```

Collection also runs on startup. Apple's `.ips` reports arrive asynchronously;
the collector matches both PID and launch time and leaves the originals intact.
The log records fatal-signal Python stacks immediately when possible. Power loss,
SIGKILL, or failures before the logger starts cannot produce a final traceback;
a session without `process_exit` indicates an interrupted process, not proof of
a particular cause. Logs are separate from CAD saves and autosaves.

Windows uses `%LOCALAPPDATA%/RoboCAD/Logs`; Linux uses
`${XDG_STATE_HOME:-~/.local/state}/robocad/logs`. `ROBOCAD_LOG_DIR` overrides the
location for tests or custom installations. Session logs are retained until
removed by the user; they can include local paths and diagnostic error text.
