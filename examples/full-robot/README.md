# Four-leg robot commissioning

The local CAD assembly has four repeated leg mechanisms, twelve HX-30HM servos,
105 connectors, four closed knee loops, and eight ideal angular transmissions.
Each leg has an internal belt-driven hip, a 5:1 worm/sector thigh drive, and a
crank/rigid-link/sliding-foot mechanism. The lower foot guide remains fixed.

The working CAD archive is `runs/robot-imports/Full_Bot-knee-01.rcad` (revision 841
at this checkpoint). `runs/` is deliberately local and excluded from Git; supply
that archive with `--cad` on another machine. Reference videos are checked in at
`references/robot/2026-09-05/`. `assembly-contract.json` records component IDs,
transmissions and calibration assumptions without duplicating the CAD geometry.

Export runs outside the UI so exact physical derivation cannot block interaction:

```sh
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/export.py --cad /path/to/robot.rcad
```

By default only the exported experiment copy fixes the chassis to a bench; the
saved CAD remains floating. `--free` preserves that floating base in the export.
The detailed imported B-reps make uncached export expensive. At this checkpoint,
full-assembly export/run acceptance is still pending; do not treat this example
as a validated digital twin or a hardware-ready controller.

Once export finishes, a short bench hold can be run with:

```sh
cargo build --bin sim-cad
./target/debug/sim-cad run runs/full-robot/bench.simrobot.json \
  --seconds 0.02 --no-flex --no-contact --step 0.0005 \
  --controller examples/full-robot/controller.py \
  --python cad/.venv/bin/python \
  --controller-arg=--log --controller-arg=runs/full-robot/controller.jsonl \
  --out runs/full-robot/hold.simresult.json
```

`controller.py` checks the twelve-channel contract and holds the imported pose;
`--exercise` requests small smooth input-shaft movements in simulation only.
`controller.rhai` provides the equivalent hold through the Rhai controller
interface, with a twelve-channel contract test in `sim-script`.

Mass, composite layup, nylon grade/process, servo dynamics, transmission losses,
backlash and friction still require measurements. Servo internals are accounted
for once in each motor's declared mass. The two thigh tubes and perpendicular
knee tube carry provisional carbon-fiber properties; pins retain their own
material regions. Ideal transmissions do not model worm self-locking or belt
compliance. Travel limits have not been guessed: the preliminary rod/enclosure
clearance boundary is not yet an operating limit. Full contact, flexibility,
calibrated controllers and sim-to-real validation remain subsequent work.
