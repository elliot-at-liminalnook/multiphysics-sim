# Nested parametric component acceptance example

Open `assembly.rcad` in RoboCAD and select the Components panel.

One **Hinged leg** definition contains two child components (**Mount block** and
**Rectangular link**) and one revolute joint. Four occurrences have independent
body/joint IDs. The parent parameter `leg_length` drives the child link's `length`.
The shared default is 90 mm; Leg 1 overrides it to 100 mm. Reset that override to
return to 90 mm. Editing the shared link width affects all four occurrences.

`hinged-leg.rcomp` includes all three definitions; only two source B-reps are
stored. `verification.json` records the expected dimensions and exported counts.
The existing CAD-to-physics exporter produces eight links and four independent
joints. Dimensions are estimated and material is declared PETG. This is a CAD
acceptance example, not a calibrated walking robot or controller benchmark.

Regenerate from the repository root:

```sh
PYTHONPATH=cad cad/.venv/bin/python cad/scripts/parametric_components_example.py
```
