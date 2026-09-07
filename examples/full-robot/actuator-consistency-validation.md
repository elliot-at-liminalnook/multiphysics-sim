# Actuator consistency before loaded tracking

The existing CAD motor definition declares 3 A stall current and 2.941995 N m
output torque, but its 11.1 V / 7.59852 ohm parameters imply only 1.46081 A
even before the driver voltage drop. The catalog derivation updated the torque
constant from declared stall current without recomputing its previously estimated
resistance. This is a model consistency defect, not a measured hardware weakness.

`motor_physics` now uses V / declared stall current when resistance is missing.
Explicit catalog resistance stays authoritative. Inductance retains the existing,
unmeasured 0.4 ms L/R estimate. This changes newly created catalog components;
existing CAD documents and controller gains are not silently updated.

The reusable Rust `actuator_audit::stall_operating_point` solves the registered
motor and H-bridge equations at a locked shaft, fixed supply and winding
temperature. It does not substitute a separate torque formula for the runtime.
Independent analytic tests cover both torque signs, electrical and mechanical
balance, heating, temperature effects and the driver's finite-slope current
foldback. The audit CLI retains optional raw CAD stall-current metadata; the
runtime's current limit is a separate field, not a substitute for that rating.

| At 11.1 V and 293.15 K, all twelve motors | Original export | Recorded correction |
| --- | ---: | ---: |
| Resistance, ohm | 7.59851999 | 3.7 |
| Inductance, H | 0.003039408 | 0.00148 |
| Registered locked-shaft current, A | 1.451261 | 2.96 |
| Registered shaft torque, N m | 1.423201 | 2.902768 |

The corrected result remains slightly below the declared torque because the
driver drops voltage. These are single-actuator steady operating points, not
continuous torque ratings. Battery sag, thermal evolution and separate CAD
transmissions are excluded. The remaining independently estimated torque and
back-EMF constants, losses, inertia, stiffness and firmware still require
consistency and hardware-response validation. This fix does not establish
energy-consistent or calibrated actuator behavior across the operating range.

## Reproduce the experiment

`catalog-stall-correction-experiment.json` records exact before/after values for
all twelve motors, the original CAD hash and both export hashes. The CAD baseline
remains revision 1357. Reconstruct the experimental export from the existing
regularized-floor baseline, failing on a different source or output artifact:

```sh
python3 examples/full-robot/replay_catalog_stall_override.py
cargo run --locked --release -p sim-runtime --example audit_actuators -- runs/full-robot/learning/servo-regularized-floor.scene.json
cargo run --locked --release -p sim-runtime --example audit_actuators -- runs/full-robot/learning/catalog-stall-consistent.scene.json
```

Motor-driven playback uses the unchanged
`mechanical-servo-single-foot-marker-motion-shift-retracted.json` reference and
the exact earlier `auxiliary-rates-source/bin/integrate_embedding` binary.
The `-refined.json` recipe halves the timestep from 0.25 to 0.125 ms while keeping
the 1.6 s duration, 1 kHz firmware clocks and 10 ms reporting interval fixed.
The override changes only exported resistance and inductance; geometry, mass,
contact, initial state, command trajectory and controller gains are unchanged.

```sh
runs/full-robot/learning/auxiliary-rates-source/bin/integrate_embedding runs/full-robot/learning/catalog-stall-consistent.scene.json examples/full-robot/mechanical-servo-single-foot-marker-motion-shift-retracted.json
runs/full-robot/learning/auxiliary-rates-source/bin/integrate_embedding runs/full-robot/learning/catalog-stall-consistent.scene.json examples/full-robot/mechanical-servo-single-foot-marker-motion-shift-retracted-refined.json
```

This remains an explicitly recorded export experiment. Promotion requires a new
CAD revision with reviewed provenance and uncertainty; the original baseline is
not overwritten. Completed runs and measured outcomes are recorded in
`actuator-consistency-status.json`. A successful solve or visible foot lift alone
does not pass the stepping or learning gates.

## Loaded-motion results

Both corrected runs complete 1.6 s. At the planned peak (0.8 s):

| Measurement | Original, 0.25 ms | Corrected, 0.25 ms | Corrected, 0.125 ms |
| --- | ---: | ---: | ---: |
| Selected foot surface gap, mm | -0.00355 | 1.05416 | 1.05635 |
| Opposite foot surface gap, mm | -0.00336 | 0.00123 | 0.00151 |
| Worst body-relative marker tracking error over the run, mm | 2.90549 | 2.48285 | 2.48022 |
| Assumed three-support static COM margin, mm | 2.75961 | 2.83665 | 2.85414 |

The selected foot now lifts, but the opposite foot also loses contact briefly.
Its maximum gap during the lift window is about 0.154 mm in the coarse run,
larger than its near-zero gap exactly at the peak. The static margin stays an
assumed-support diagnostic: a positive value does not make an airborne foot
support the robot.

Both corrected runs also report contact between the -Y rigid curved link and
sliding crosshead. Coarse contact starts at 0.8105 s and ends at the last sampled
contact start time 0.8985 s. Maximum penetration is 4.506 micrometres coarse and
4.547 micrometres refined. The accepted contact sample counts (357 and 712)
reflect different timestep sizes, not twice as many independent collisions.
Whether this is true CAD interference or a contact-proxy artifact remains open.
No collision filter was added to remove it.

Refinement changes sampled world-foot positions by at most 0.04005 mm, motor
current by 0.08159 A and shaft torque by 0.03717 N m. One reporting sample has
different contact pairs, and backlash guard 18 fires seven versus nine times.
The main tracking/support behavior persists, but exact event equivalence and
task accuracy are not established. Position agreement alone is not a promotion
gate. Stepping takes 121.76/163.39 wall seconds for 1.6 simulated seconds on this
Intel i9-9980HK machine; builds/checks overlap part of the runs, so these are
diagnostic timings rather than a controlled performance comparison.

Four focused CAD checks and two analytic Rust audit tests pass; the same two
Rust tests pass without default features, and the runtime library compiles for
WASM. CI now runs the audit tests and checks the CLI. Full source/binary manifests
and artifact hashes accompany the status report. The next task is inspecting
loaded knee contact and improving support-foot tracking, not declaring a
successful step or beginning unvalidated learning.
