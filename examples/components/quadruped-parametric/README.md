# Full quadruped in the parametric component system

Open **[model/robot.rcad](model/robot.rcad)**. This is the working converted model.

The conversion contains 23 definitions and 22 nested occurrences. Every one of
its 114 physical bodies and 105 joints belongs to a linked component. The chassis
is a component; the four legs are occurrences of one **Quadruped leg** family,
with nested mechanical subassemblies.

## Shared parameters and retained variants

Select **Quadruped leg** in the Components library and choose **Edit defaults**.
These defaults propagate through the family and nested subassemblies to all four
legs. Select a leg and use **Occurrence** to override only that leg. Resetting an
override restores inheritance.

| Parameters | Units | Meaning |
| --- | --- | --- |
| `hip_drive_ratio`, `worm_drive_ratio`, `foot_drive_ratio` | dimensionless | Existing CAD actuator-output-to-joint coupling ratios. These are separate from each motor's internal gearbox. |
| `hip_home_deg`, `worm_home_deg`, `foot_home_deg` | degrees | Existing CAD joint home/reference angles. |

Select **Quadruped** to edit `position_x`, `position_y`, `position_z` (mm) and
`heading_deg` (degrees). They move the entire assembly coherently, including joint
frames, actuator coordinates and declared mass-property frames.

The leg family retains four explicit source variants: `minus_y`, `plus_x`,
`plus_y`, and `minus_x`. Their body counts are 25, 33, 28 and 27 respectively.
They have distinct imported geometry/topology; this conversion preserves those
engineering differences. Family parameter bindings are visible under **Family
variants**. New occurrences can select a variant in the **Place** dialog.

Solid geometry remains exact imported B-rep source data. Original solid feature
histories, such as design dimensions for custom housings and gears, have not been
recovered. These parts are linked and reusable, with explicit assembly and joint
parameters; adding a new shape dimension requires an explicit construction recipe.
The source model's calibration uncertainty and provisional physical data remain.

## Verification and backups

[model/verification.json](model/verification.json) records the source/result
hashes and checks. All 114 geometry payloads are byte-identical to the source.
Original physical node IDs, names, materials, joint records, actuator metadata,
robot settings, annotations and the exporter's 29 rigid-link groups are preserved.
Save/reload and parameter propagation after reload passed.

On the full model, a shared hip drive ratio of 1.25 propagated to three legs while
an explicit +X override stayed at 1.5. Undo restored every original joint record.
A 1 mm whole-assembly translation moved every joint pivot by precisely 1 mm and
was also undone. The saved model contains the original values.

Verified pre-conversion copies of both the baseline and the previously linked
model are under [backups/20260915T163535Z](backups/20260915T163535Z), with a
`backup.json` SHA-256 manifest. The original source files were retained. A rejected
first archive is kept in `validation-failures`, documenting the source-byte cache
issue caught by the strict audit and fixed before the accepted conversion.

`model/quadruped-leg.rcomp` contains the leg family and all nested dependencies.
`model/chassis.rcomp` contains the chassis component. Whole-robot settings and
experimental provenance remain in the authoritative `.rcad` document.

Reproduce into a **new empty destination** using the verified backup:

```sh
PYTHONPATH=cad cad/.venv/bin/python cad/scripts/convert_quadruped_components.py \
  examples/components/quadruped-parametric/backups/20260915T163535Z \
  /path/to/new/output-directory
```
