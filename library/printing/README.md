# Print registry

`registry.json` (`sim.print-registry/1`) is the one source for printer, filament and printed-joint values. The Rust stress check and planner (`crates/sim-print`) and CAD (`cad/robocad/print_registry.py`) all read this file. Results record its SHA-256, so every number can be traced to the values it used.

## What it holds

- **Printers:**
  - build volume (maker datasheets), edge margin, nozzle and line width;
  - layer heights;
  - travel overhead and layer-change time, which feed the time estimate.
- **Materials:**
  - density;
  - stiffness along and across the layers, and interlayer shear stiffness;
  - Poisson's ratio;
  - strength along and across the layers, compressive, interlayer shear, in-layer shear and pin bearing (all failure values; safety factors are applied on top);
  - glass transition, flow limit and price.

  `cad_material` links a filament to a CAD material id. CAD print materials take their stiffness and strength from here.
- **Infill:** Gibson–Ashby exponents for stiffness and strength against density.
- **Joints:**
  - dowel pin clearances, sizes and depth;
  - dovetail angle, clearance and friction;
  - heat-set insert holes and knurls, and their grip share;
  - screw clearances, heads, counterbores, lengths and proof loads.
- **Test plan:** minimum samples and the coupon kinds.
- **History:** every revision and what changed.

Every value is `{value, unit, provenance, uncertainty, source}`. Provenance is one of:

- `measured`: from coupon tests on your printer, with `evidence`;
- `datasheet`: a maker's specification;
- `derived`;
- `estimated`: typical published data.

Validation refuses out-of-range values, empty sources, and measured values without evidence, and names the JSON path of each problem.

## Changing values

- **Estimates:** edit the file, say why in `source`, and add a `history` entry.
- **Measurements:** only through coupons.
  1. Print the kit from *Print ▸ Test coupons* (or `POST /print/coupons`), break at least 3 of each, and fill in `results.json`.
  2. Run `target/release/sim-print promote results.json` (a dry run).
  3. Run it again with `--write`. This bumps the revision, sets `measured` with the samples, spread, design value (mean − 2·std), coupon geometry and the value it replaced, and records the change in `history`.

Never copy numbers from here into other files: read them through the registry loaders.
