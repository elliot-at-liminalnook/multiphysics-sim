# Sampled physical-point tracking

`sim-track` uses shared Rust session physics to capture marker trajectories and
compares them with another simulation or imported measurements. A marker is a
point fixed to a CAD-exported link: `world_point = position + rotation * local_point`.
This makes the comparison meaningful for a foot tip, whose motion differs from
the calf link's origin. A marker is diagnostic configuration, not a new sensor
or a change to CAD physics.

Run the synthetic arithmetic example:

```sh
cargo run --locked -p sim-runtime --bin sim-track -- compare \
  examples/interactive/tracking/candidate.json \
  examples/interactive/tracking/reference.json \
  examples/interactive/tracking/requirements.json
```

Expected: 5 mm maximum/RMS spatial error and 4 mm minimum remaining margin:
20 mm total - 8 mm other-error reserve - 5 mm discrepancy - 1 mm candidate
uncertainty - 2 mm reference uncertainty. All numbers are illustrative;
these fixtures do not establish robot accuracy. Exit 0 means this comparison
passed, 1 means a budget was exceeded, and 2 means invalid/missing evidence.

Capture motion from an existing recording (see the parent interactive example):

```sh
cargo run --locked -p sim-runtime --bin sim-track -- capture \
  runs/interactive/native.recording.json \
  examples/interactive/tracking/pendulum-markers.json > runs/interactive/tip.json
```

The output includes the complete recording, CAD physical source, controller,
seed, actions and marker definitions. It records the initial state and each
reporting frame. `completed` means all recorded actions executed, not that an
episode or walking task succeeded. Change marker names/offsets explicitly for
other CAD models; duplicate link names and missing links are rejected. Simulation
captures can require `expected_cad_sha256` in the marker configuration; a missing
or different source hash is rejected before running. This checks the declared
CAD source hash, not independent integrity of an edited exported model.
Simulation
uncertainty is zero because these are deterministic sampled outputs, not because
the simulated robot is perfectly accurate. Test integration error separately
with a timestep-refined reference and unchanged controller/report periods.

## Import hardware measurements

Use the same JSON layout as `reference.json`, but supply actual registered
measurements and a source such as:

```json
{"kind":"hardware","artifact":"durable raw-data path or content-addressed URI",
 "procedure":"procedure/version, fixture, frame registration, clock alignment, uncertainty derivation and held-out trial ID"}
```

The importer validates the data structure; it does not fetch the artifact or
certify the provenance. Preserve raw files and procedure results alongside the
comparison. Never relabel the synthetic fixture as measured data.

For the first bench experiment:

1. Select the physical foot-tip/marker location in CAD and record its link-local
   offset in metres. Establish the measurement frame with known reference points
   and preserve the transform to the CAD world frame.
2. Record a repeatable motor sweep through the intended operating range in both
   directions, with the same fixture and command timeline as the simulation.
   Record motor feedback, supply voltage, load and temperature with the marker
   measurements. Begin with kinematics; repeat under representative loads to
   distinguish geometric errors from actuator tracking and compliance.
3. Synchronize using a recorded trigger. Preserve latency; do not shift traces
   until they happen to match. State the residual clock error and include its
   maximum spatial effect, frame-registration error and sensor error in
   `uncertainty_m`. This is an additive error bound, not one standard deviation.
4. Export seconds/metres in the declared common frame with the same experiment
   ID, point IDs, and sample times as the simulation. Any resampling belongs in
   the documented measurement processing; bound its error. The comparator does
   no interpolation or alignment. Times must match within 1 ns after registration.
5. Set the required interval, maximum sample gap and budgets before comparing.
   Use separate fitting and held-out trials. Record failed and incomplete trials
   too; an incomplete trace cannot pass.

The gate covers every requested point at every supplied sample. Missing points,
interval truncation, excessive gaps, invalid numbers, wrong frames/experiments
and unaligned samples fail closed. Extra unrequested markers are not assessed.
No continuous-time bound is implied: a between-sample collision can still be
missed. Clearance, contact, balance, actuator capability and walking/recovery
success require additional gates. A simulation-to-simulation pass establishes
agreement only; hardware evidence is separately labeled in the report.
