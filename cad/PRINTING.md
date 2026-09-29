# Printable parts

From a CAD part to prints you can trust. The steps:

1. split the part so the pieces fit the printer, and join them again;
2. check strength under the loads the part really carries, often read from a running simulation;
3. choose the print direction and settings;
4. lay out the plates;
5. write the assembly steps;
6. print test coupons whose breaks replace the estimates.

| Step | Where | Output |
|---|---|---|
| Print registry | `library/printing/registry.json` ([README](../library/printing/README.md)) | Printers, filaments, infill laws, joint geometry, each with provenance |
| Split and join | `robocad/print_split.py` | Pieces under a new group, seams with pins, inserts and screws, or dovetails |
| Strength | `sim-print analyze` (Rust, `crates/sim-print`) | Layer-aware safety factor, failure mode and location, seam checks, colour field |
| Settings and plates | `sim-print plan` + `robocad/print_plan.py` | Direction, walls, infill, layer height per piece; 3MF plates with per-object settings |
| Whole or split? | `robocad/print_strength_split.py` | Verdict comparing the whole part with a split at a junction |
| Assembly | `robocad/print_assembly.py` | Steps, hardware, tools, `assembly.html`, exploded view |
| Coupons | `robocad/print_coupons.py` + `sim-print promote` | Coupon plates, `results.json` template, protocol; measured registry values |

## Using it

**In the CAD window**, use the Print menu:

- *Split selected for printing…*
- *Check strength* and *Plan print settings and plates*, which use the document's print study (see below)
- *Whole or split for strength?*
- *Assembly guide for the selected split…*
- *Test coupons…*
- *Strength overlay on/off*, which colours each part from green (safe) to red (at failure)
- *Print jobs…*, to see progress or cancel

Long work runs off the UI thread, with progress in the status bar. The results are published as one undoable step. Source bodies are never edited: a split adds pieces under a new group and hides the source.

**Over REST** (the window's API, `:8420`):

- `POST /print/split` `{node, printer, joint: auto|pins+screws|dovetail|pins, screw}` runs synchronously; add `background: true` to run it as a job.
- These start background jobs:
  - `POST /print/analyze` `{parts: [{node, fixtures, loads, build_direction, settings}], printer, material, simulation, safety_target, voxels}`
  - `POST /print/plan`, the same body plus `space: {walls, infill, layer_heights, search_voxels}`
  - `POST /print/strength_split` `{node, part, …}`
  - `POST /print/assembly` `{group}`
  - `POST /print/coupons` `{group?, printer, material}`
- `GET /print/jobs/ID` returns state, progress and result; `DELETE /print/jobs/ID` cancels.
- `GET /print/registry` returns the printers, filaments and the file's SHA-256.

A region names where a fixture holds or a load acts. It can be:

- `{contact: other_node}`: the faces touching another body, i.e. the load path through a contact;
- `{bottom: true}`: the faces the part stands on;
- `{faces: [indices]}`;
- a plain `{sphere | box | cylinder | below | slab | points}`.

A load's `magnitude` is newtons, or `{observe, reduce, scale, add, why}`, read from the study's `simulation` (a `sim.system/1` file run once).

**From a script:** `ops.print_split(node, printer=…)`, plus the functions in the modules above. For worked examples, see `examples/camera-turntable/print/`:

- `turntable_study.py`: loads from the belt simulation, then strength, plan and plates;
- `split_small_printer.py`: the disc on an A1 mini, with seams, assembly and coupons;
- `stand_strength_split.py`: whole or split.

**From the command line:** `sim-print registry | analyze STUDY | plan STUDY | promote RESULTS [--write]`. Progress goes to stderr as `progress F message`.

## How the strength check works

- **Voxels.** Each part is turned so its build direction is up, then voxelised. Each voxel is part wall or skin (solid) and part sparse infill, set by the print settings. Infill stiffness and strength scale with density as ρⁿ, with exponents from the registry.
- **Elements.** One trilinear hexahedron per voxel, with a material weaker across the layers than along them. Solved with a matrix-free, Jacobi-preconditioned CG.
- **Failure index.** The highest of:
  - the layer interface, √((σ_z⁺/S_across)² + (τ_z/S_interlayer)²);
  - in-layer von Mises against the in-layer strength;
  - crushing across the layers.

  The safety factor is its inverse. At the surface, stress is extrapolated from the voxel centres and smoothed only along the surface.
- **Section and seam loads.** The force and moment through any plane come from the solved nodal forces. Seam joints share them as a bolt group. Capacities use the same formulas as `part.dowel_pin`, `part.threaded_joint` and `part.dovetail`, or a measured coupon value when there is one.
- **Qualification** (`cargo test -p sim-print`, run in CI):
  - a bar in tension: stretch within 4 % of F·L/(E·A);
  - a cantilever: deflection within 6 % of beam theory (with shear), root safety factor within 10 % of M·c/I;
  - section loads are exact;
  - laying the layers across the bending makes "layer split" govern, with safety falling by the strength ratio;
  - refining the voxels changes results within stated bounds;
  - the planner picks stronger settings for heavier loads;
  - seams held only by pins fail when they are pulled or bent;
  - coupon promotion works as described.

## What to trust, and what not

- **Strengths are estimates** until coupons are broken. The registry marks each value `estimated`, `datasheet`, `derived` or `measured`.
- **Stress.** Voxel stresses near sharp corners, small holes and load patches are approximate. Fixtures are perfectly rigid. Treat the safety factor as a comparison between designs and settings, and confirm finalists with coupons.
- **Time and filament** are estimates from voxel volumes and the registry's flow and overhead values, not a slicer; the slicer's numbers are authoritative.
- **3MF settings.** Plates carry Bambu Studio's per-object keys (wall_loops, sparse_infill_density and pattern, shells, layer_height). These have not been checked by opening the files in Bambu Studio, so check the object settings there. The manifest `plates.json` lists everything as well.
- **Cuts** are axis-aligned planes, and pieces keep the part's up direction. Joints that can't be built at a spot are left out, with a note.
- **Loads.** Loads from a simulation are only as good as the model. Every result records the system file's hash, the observable, the reduction, and any scale or lever arm used.
