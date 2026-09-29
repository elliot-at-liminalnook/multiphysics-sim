# Printable parts: to-do and status

Goal: turn CAD parts into printable pieces with an honest strength check.
The system finds loads in the running simulation, splits parts to fit the
printer (adding joints), checks layer-aware strength, chooses print
settings, writes assembly steps and generates test coupons whose
measurements replace the estimates.

Ownership (AGENTS.md): **CAD (Python) owns geometry**: splitting, joints,
coupons and assembly drawings. **Rust owns the analysis**: the registry
loader, voxel FE stress check, settings search, simulation loads and
promotion. **One source for print values**: `library/printing/registry.json`.

Proof cases, from `examples/camera-turntable`:
- servo cradle and base post: belt hub load from the turntable simulation;
- camera stand: split so each piece prints in its strong orientation;
- the whole turntable on a small printer (A1 mini, 180 mm): pieces must be split to fit.

Legend: [ ] to do · [~] in progress · [x] done (with the check that proved it)

## 1. Print material registry
- [x] `library/printing/registry.json` (`sim.print-registry/1`) holds:
  - printers (H2C, P1S, A1 mini): build volume, nozzle, layer heights, flow;
  - materials (PLA Basic, PETG HF), with directional stiffness and strength (in the layers / across them / interlayer shear), density and temperatures;
  - infill scaling laws;
  - joint clearances (pins, dovetails, inserts, screws).

  Every value records unit, provenance (estimated / derived / measured), uncertainty and source, plus a revision history.
- [x] Rust `crates/sim-print` registry: typed load, validation that names the JSON path, content hash.
- [x] Python `cad/robocad/print_registry.py`: same file and same hash; CAD print props (`Material.props()['print']`, `printing.METRIC` insert holes) come from it.
- [x] Tests (Rust and Python): load, validate, reject bad values; hashes agree. (`cargo test -p sim-print`; `cad/tests/test_print_registry.py` 3 pass; full CAD suite 303 pass with the registry wired in.)

## 3. Strength from the simulation (Rust stress check)
Built before step 2 because splitting uses the seam loads.
- [x] Belt part exposes `transmitted` and `hub_load` (pretension is an estimate) so the simulation gives the loads.
- [x] Voxel mesher: welded triangle mesh to solid voxels; skin depth splits perimeter/skin from infill.
- [x] Hex8 linear elastic FE, transversely isotropic about the build direction; infill scaled by density; matrix-free PCG, multithreaded.
- [x] Study file `sim.print-study/1`:
  - regions: faces, points, box, sphere, cylinder;
  - fixtures and loads, where a load can come from a simulation run (`system`, `observe`, `reduce`) times a CAD direction;
  - gravity and acceleration;
  - clear errors for missing supports.
- [x] Results:
  - layer-aware failure index (layer split, interlayer shear, in-plane);
  - safety factor with location and mode;
  - a field file for colouring;
  - loads through any cut plane (force and moment).
- [x] `sim-print analyze STUDY` CLI with progress lines, for CAD to run as a subprocess.
- [x] Qualification in CI:
  - cantilever deflection and stress against beam theory;
  - uniform bar exact;
  - orientation flips the governing mode;
  - timestep-free, so a mesh-refinement check instead.

## 2. Splitting and joints (CAD)
- [x] `robocad/print_split.py`:
  - pick cut planes so pieces fit the printer (with margin);
  - score candidate planes by section area, seam load from step 3 and features cut through;
  - recurse until every piece fits;
  - the source body is untouched (pieces are new nodes).
- [x] Joints at each seam, built by booleans:
  - dowel pins (slip on one side, press fit on the other);
  - heat-set insert and screw, where the screw can reach;
  - a sliding dovetail rail.

  Capacities come from the same models as `part.dowel_pin`, `part.threaded_joint` and `part.dovetail`, checked against the seam load.
- [x] Split for strength: an optional cut that lets a piece print in its strong orientation (camera stand).
- [x] Ops, REST and UI: `ops.print_split`, `POST /print/split` job (off the UI thread, progress, cancel), Print menu command; pieces recorded in `robot['print_piece']`.
- [x] Tests: the turntable disc on an A1 mini, where pieces fit, joints exist, pieces rejoin to the original volume and meshes are closed.

## 4. Print settings and export
- [x] Rust `sim-print plan`: for each piece, try orientations × walls × infill × layer height:
  - strength by FE (safety factor ≥ target);
  - time, filament and support estimates from volumes and the registry flow (labelled estimates);
  - bed contact area.

  It picks the fastest candidate that is strong enough and keeps the alternatives table.
- [x] Plates: orient and lay pieces flat, pack them on build plates (grouped by material and layer height), and export 3MF with Bambu per-object settings (`Metadata/model_settings.config`) plus a manifest.
- [x] CAD `POST /print/plan` job, a Print panel summary and a stress colour overlay (a preview, not source geometry).
- [x] Tests: the plan chooses stronger settings for higher loads; the 3MF reopens with the settings.

## 5. Assembly
- [x] Assembly order from the seam graph (largest piece first); steps name the pieces, hardware and direction to join.
- [x] Hardware list: pins, inserts and screws, with standard screw lengths chosen from the reach.
- [x] Assembly guide (HTML with drawings of each step) and an exploded view offset per piece.
- [x] Tests: steps cover every piece; the hardware count matches the joints.

## 6. Test coupons and promotion
- [x] Coupons from the registry test plan:
  - tensile bars printed flat and standing (in-layer and interlayer);
  - pin shear, insert pull-out and dovetail pull coupons copying the real seams.

  They are laid out on a 3MF plate with the parts' settings.
- [x] Test protocol plus a results template (`sim.print-test/1`).
- [x] `sim-print promote RESULTS`: statistics (n ≥ 3, mean, spread, a lower design value) produce a new registry revision with `measured` provenance, the evidence hash and the previous value recorded.
- [x] Tests: synthetic results promote; too few samples refused; the history keeps the old value.

## Wrap-up
- [x] Run everything on the camera turntable (loads from its simulation) and record the results here.
- [x] Docs: `cad/PRINTING.md` (workflow, REST, limits and honesty labels); README pointer.
- [x] Memory note.

## Log
- 2026-09-29: plan written.
- 2026-09-29, step 1 done:
  - Registry revision 1: H2C, P1S and A1 mini volumes from maker specifications (`datasheet`); the H2C uses its single-nozzle box, 325×320×320. PLA and PETG are `estimated`.
  - The bearing value is a failure pressure (45 MPa for PLA); CAD's allowable is that ÷ 3.
  - CAD print materials read their values from the registry (for PLA, the stiffness-anisotropy factor goes 0.6 → 0.81 and the layer-strength factor 0.7 → 0.58).
- 2026-09-29, step 3 core done:
  - `part.belt_drive` gained a `pretension` parameter (declared on the pulley in CAD: `belt_pulley.pretension_n`, 25 N, estimated) and outputs `transmitted` and `hub_load`. Turntable run: hub load 50 N; transmitted force at most 0.63 N.
  - Qualification: bar stretch within 4 % of F·L/(E·A); cantilever within 6 % of Euler–Bernoulli plus Timoshenko; section loads exact; laying the layers across the bending stress makes "layer split" govern with safety falling by the strength ratio (±12 %); 2 mm vs 1 mm voxels within 12 % (deflection) and 25 % (safety factor); bolt-group sharing and the insert pull-out of 852 N (the lesson's number).
  - Turntable study (`examples/camera-turntable/print/turntable_study.py`), 91 s for 4 parts at about 60k voxels each. Safety factors:
    - cradle: 47 (interlayer shear);
    - base: 13.7 (layer split near the bottom);
    - disc: 40;
    - camera mount (10 N knock): 33.

    Equilibrium residual about 1e-7.
  - Known limit: the thin 300 mm disc needs about 15k Jacobi-CG iterations (61 s). A better preconditioner is a later improvement.
- 2026-09-29, step 2 done:
  - **Splitter:** `print_split.py`.
    - Cuts are axis-aligned planes scored by cross-section (one region, no holes, even pieces), planned against the printer box less a joint allowance, then checked after the joints are built; pieces keep their modelled up direction.
    - Joints: dowels (falling back to smaller registry diameters); inserts with screws (counterbore tunnel up to 30 mm, else a side screw pocket; M3 → M2.5 → M2 for small sections); dovetails (jigsaw tabs through plates, snapped to the part axes, or one rail for thick sections).
    - A joint that can't be built is left out with a note.
  - **Seam checks:** by `sim-print` (`seams` in the study) against the simulation loads. A seam with tension or bending and nothing holding it closed fails with safety 0, and so does one whose joints can't resist the bending.
  - **Interfaces:**
    - `/print/split` (optionally a background job), `/print/analyze`, `/print/plan` and `/print/strength_split`;
    - `/print/jobs` (state and cancel), `/print/registry`;
    - Print menu: Split selected, Check strength, Plan, Whole or split?, Strength overlay, Print jobs.
  - **Strength overlay:** `node.results.section = print` colours the voxel failure field, green to red.
  - **Proof:**
    - 250 mm bar on an A1 mini: 2 pieces, 2 pins, 2 M3 screws in side pockets;
    - turntable disc on an A1 mini: 4 upright quarters, jigsaw tabs on every seam plus 8 Ø3 pins; seam safety factors 27–54 against the simulated belt load (`split_small_printer.py`).
  - **Split for strength** (`print_strength_split.py`, substructuring: the seam force and moment from the whole-part solution go onto the held piece): the camera stand verdict is **whole**. Lying on its side, 4 walls and 60 % infill give safety 2.54 in 3.4 h; the best post/base split reaches 1.52, since a 20 mm post leaves room for one M2 screw and a pin only.
  - Tests: `cad/tests/test_print_split.py` (8 pass).
- 2026-09-29, step 4 done:
  - **Planner:** `sim-print plan`.
    - Cheap estimates first: time from voxel volumes and the registry flow (walls and skins at half flow), layer changes and travel overhead; support by the 45° rule; bed contact.
    - Candidates are then solved in time order until one passes at 1.15 × the target on the coarse grid. The pick is re-checked at full resolution; if it falls short, the next candidates are tried.
  - **Stress accuracy fix:** surface stresses are extrapolated from voxel centres and smoothed only along the surface. Before this, coarse grids read bending stress low; the root stress is now within 10 % of beam theory, and a test covers it.
  - **Plates:** `print_plan.py` orients each part, turns it to its smallest footprint, shelf-packs by layer height, and writes a 3MF with Bambu `Metadata/model_settings.config` (wall_loops, sparse_infill_density and pattern, shells, layer_height) plus `plates.json`.
  - **Turntable on the H2C:** 3 plates, about 12.0 h and 444 g (estimates). Every part gets 2 walls and 10 % infill: the loads are light.
  - Tests: Rust planner test; CAD plan job and packing tests.
  - Limits:
    - Bambu's per-object keys are written by name but not yet checked by opening the files in Bambu Studio.
    - Time is not calibrated against a slicer.
- 2026-09-29, step 5 done:
  - `print_assembly.py`:
    - assembly order (largest piece first, then neighbours through the seams);
    - preparation first (inserts at 220–240 °C, pins pressed into their press-fit holes);
    - join steps with a direction (drop in for jigsaw tabs, slide along a rail, else straight on along the seam normal) and screws with the right hex key;
    - hardware and tool lists;
    - a self-contained `assembly.html` with a picture of each step (the new piece in colour, raised along its way on);
    - an exploded view as instances of the pieces (one undo step; the pieces don't move).
  - The split summary now carries each joint's spec and hardware and each seam's plane, so nothing is parsed out of the notes.
  - Access: `/print/assembly` job and Print ▸ Assembly guide.
  - Proof: `runs/camera-turntable/print-a1-mini/assembly/assembly.html` (disc for the A1 mini: 3 pin steps, then 3 join steps).
  - Test: every piece is added once; the steps' hardware matches the hardware list; the exploded view moves all but the first piece and undoes cleanly.
- 2026-09-29, step 6 done:
  - **Coupons** (`print_coupons.py`):
    - tensile bars printed flat and standing (solid, 40 and 60 mm² gauges);
    - a pin lap coupon, an insert boss pulled along the layers, and a dovetail tab and slot, each copying the split's real joint dimensions;
    - loading holes at both ends;
    - output: 3MF plates, `results.json` (`sim.print-test/1` template) and `PROTOCOL.md` (luggage scale or water bucket, reading the peak, leaving out grip breaks, safety).
  - **Promotion** (`sim-print promote`):
    - needs at least 3 breaks per test;
    - conversions: F/A; pin bearing F/(d·L); insert pull-out back-computed as τ = F/(π·D·L·η); dovetail capacity in N;
    - the value becomes the mean with provenance `measured`; uncertainty is the coefficient of variation;
    - evidence: the samples, mean − 2·std, the results file's SHA-256, the coupon geometry and the previous value;
    - the registry revision goes up with a history entry; dry run unless `--write`.
  - A measured dovetail capacity replaces the model in seam checks, scaled by neck area × thickness.
  - Access: `/print/coupons` job and Print ▸ Test coupons.
  - Proof:
    - the disc split's coupons: `runs/camera-turntable/print-a1-mini/coupons` (3 A1 mini plates);
    - synthetic breaks promoted into a registry copy (revision 1 → 2, e.g. across-layer 26 → 25.2 MPa measured).
  - Tests:
    - Rust: promotion with evidence and history; too few breaks refused; measured tab scaling;
    - CAD: kit → plates → filled template → `sim-print promote --write` into a registry copy.
  - The real registry is unchanged: nothing is measured until you break coupons.
- 2026-09-29, wrap-up:
  - Re-ran the turntable with the final code. Strength as designed:
    - cradle 49 (interlayer shear);
    - base 10.9 (layer split near the bottom);
    - disc 37.6;
    - camera mount 32.4.
  - Plan: every part at 2 walls and 10 % infill, the base checked at 6.5; 3 H2C plates, about 12.0 h and 444 g (estimates).
  - Docs: `cad/PRINTING.md`, `library/printing/README.md`, a README pointer and a "Printing it" section in the turntable README.
  - Memory: `printable-parts.md`.
  - Found and fixed on the way: `Document` shared its default Material objects across documents, so editing one changed all of them. Each document now gets copies.
  - Checks: `cargo test -p sim-print` (11 qualification tests + 3 unit tests); CAD suite; `sim-print` rebuilt.
  - Open items:
    - check the Bambu per-object keys by opening a plate in Bambu Studio;
    - calibrate time estimates against a slice;
    - a stronger preconditioner (the thin disc takes about 60 s);
    - a socket joint for small posts (the stand split lost only for lack of joint room);
    - break real coupons to replace the estimates.
