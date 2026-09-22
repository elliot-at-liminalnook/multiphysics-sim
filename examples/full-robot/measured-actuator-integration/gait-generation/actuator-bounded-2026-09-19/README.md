# Actuator-bounded gait generation — 2026-09-19

The initial simulation campaign is complete: eight short search trials, three
ten-second candidate captures, an exact three-second replay, and stopping and
half-timestep checks. Hardware is disconnected. No gait has been promoted to the
browser or to hardware.

The shared Rust reference governor now runs inside the Rhai walking policy on
all twelve actuated joints in every trial. It limits the applied reference to
80 degrees/s and 400 degrees/s² at the existing 20 ms policy cadence. These are
explicit experimental command limits, not measured motor ratings. The detailed
CAD motor models retain their 10 ms integer-controller cadence, encoder model,
electrical/mechanical dynamics, and full experimental controller authority.

The separate `current-controller/robot.rcad` declares Kp/Kd/Kv Q8 gains
4096/4096/4096 and a provisional 2 ms delay. Its controller fingerprint matches
the current shared Rust implementation. The first CAD revision and failed
`legacy-gains-screen` run retain evidence of a stale controller fingerprint;
they are not campaign candidates.

`current-controller/robot.simrobot.json` explicitly reuses the pinned parent
physical derivation after verifying that every archive entry and all manifest
content except actuator profiles and save metadata are unchanged. Its receipt
preserves the old derivation identity. An independent fresh physical export of
the first profile revision also equals the parent physical definition exactly.
No geometry, contact, material, mass, inertia, or transmission was retuned.

## Search and acceptance

The frozen `validation-protocol.json` predates trials. The initial search uses
seed 2301, eight three-second trials, and actual net travel speed. It varies
pace, trajectory amplitude, joint feedback, and velocity lead. Every candidate
executes the bounded policy during simulation; no accepted gait is filtered
only afterward.

Ten-second finalists must stay upright, travel at least 0.1 m, keep each motor
below 2 degrees RMS/8 degrees peak tracking error after startup, and obey the
command limits. Every foot must lift at least 3 mm on two excursions. Sampled
inter-link penetration must stay below 1 mm. Exact replay, half-step numerical
comparison, and seven-seconds-walk/three-seconds-stop are separate checks.
Short trials cannot qualify a gait. Failed bounds are not relaxed.

## Results

| Ten-second candidate | Travel | Speed | Worst motor RMS | All feet lift | Tracking gate |
|---|---:|---:|---:|---|---|
| Bounded baseline (fastest short search) | 0.399 m | 0.0399 m/s | 2.162 degrees | Fail | Fail |
| Slower search candidate | 0.288 m | 0.0288 m/s | 2.379 degrees | Fail | Fail |
| Slow full stride (additional authored candidate) | 0.327 m | 0.0327 m/s | 2.423 degrees | Pass | Fail |

All three ten-second candidates completed without falling, remained upright,
obeyed the reference bounds, and passed sampled inter-link collision checks.
The first two largely shuffle two feet, which speed-only ranking would miss.
The full-stride candidate lifts all four feet 4.8–20.5 mm, with four or five
clearance excursions per foot. It is an **experimental walking recipe**, not an
accepted controller or a calibrated real-robot gait. See its
[detailed joint/foot report](slow-full-stride-10s/README.md).

The full-stride candidate passes the frozen half-timestep comparison: net travel
differs by 0.313 mm, final body position by 0.394 mm, and final actuated angles by
at most 0.0458 degrees. In the seven-second-walk/three-second-stop test, maximum
displacement after the stop request is 5.92 mm and final-second drift is 0.264 mm.
No fall occurred. Exact replay of every frame/transition was verified for the
three-second bounded baseline, and its independent capture matches the search
endpoint exactly. A full-stride ten-second replay has not been run.

**No promotion:** worst tracking remains above the predeclared 2-degree RMS gate.
The retired 0.264-degree physical-fit criterion was not reinstated. The two gates
measure different things; no simulation result establishes physical accuracy.

Machine-readable evidence is in [results.json](results.json), each candidate's
geometry-report.json, slow-full-stride-stop/check-detailed.json,
slow-full-stride-half-step/check.json, and search/replay-check.json.

## What changed and verification

- A Rhai binding calls the existing shared Rust reference governor; controller
  math is not duplicated in the viewer or configuration tools.
- The capture CLI accepts the actual experiment specification, including its
  seed, parameterization, and command schedule. Search/capture parity passed.
- Profile-only CAD export reuse proves archive equality except profiles and save
  metadata, retains pinned derivation provenance, and runs native validation.
- Search checkpoint reads/writes are now buffered. Checksums, atomic replacement,
  file fsync, and directory fsync remain intact. A 12,750,635-byte initial checkpoint
  is byte-identical to the previous writer. The search resumed from committed
  revision 14 and completed revision 16 with the same runtime identity. No partial
  checkpoint was treated as a committed result.
- Focused checks passed: two Rust/Rhai governor parity/rollback tests, one governor
  bounds/reversal test, and one CAD reuse test covering accepted metadata/profile
  changes and rejection of mass, geometry, and archive-inventory changes.

The original Rust source snapshot is in rust-source.tar.gz with hashes in
rust-source-manifest.json. The later I/O-only CLI change is retained as
verification/search_motion-buffered.rs. Runtime identity is unchanged by that
example-host edit. Test/build logs are retained under verification/.

## Remaining work

1. Search leg phasing, stance posture, and foot trajectory shape, with foot-lift
   and tracking feasibility influencing selection. This first search only changes
   pace, amplitude, feedback, and lead around the historical trajectory.
2. Separate sustained loaded tracking bias from reversal/velocity error and
   evaluate controller/load compensation through the shared controller path.
   The current diagnostics show both components; their physical causes remain
   uncalibrated. Do not lower the acceptance gate to promote this recipe.
3. Require every gate to pass together, then run longer walking/turning/reversal,
   voltage/load robustness, candidate-specific replay, and realtime browser checks
   before replacing the current walking recipe.
4. When hardware returns, verify per-motor tracking and loaded-leg behavior.
   Real battery/current and physical-unit calibration are still missing.

## Reproduction

Shared executables: `search_motion`, `run_environment --experiment`, and
`audit_capture_geometry --pairs` from `sim-runtime` with `--features bayesian`.
Use `RAYON_NUM_THREADS=1` for the retained native runs.

Configuration/statistics helpers are one directory above:

- `prepare_bounded_search.mjs`: binds the shared governor and search parameters.
- `prepare_bounded_validation.mjs`: extends a candidate with the original command
  schedule and seed, or creates half-step and stop scenarios.
- `summarize_bounded_capture.mjs`: tracking, command derivative, travel, and
  sampled geometry gates from actual saved Rust observations.
- `check_bounded_validation.mjs`: replay, numerical, and stop comparisons.

`search/journal.json/` holds immutable search revisions, including proposals
saved before execution. Captures retain the complete replay recipe and actual
observations. CAD artifacts, protocol, failures, and results are durable here,
not only in ignored `runs/`.

## Limits of the result

The actuator family remains provisional and identical across all twelve motors.
Voltage is fixed at 11.1 V and temperature at 293.15 K. Load torque, current,
battery behavior, backlash, and real timing have not been calibrated. The
policy consumes ideal simulated body/joint feedback. An offline controller fit
and a successful simulation gait cannot establish real-world tracking or
hardware readiness. No FPGA bitstream or browser bundle is updated by this
campaign.
