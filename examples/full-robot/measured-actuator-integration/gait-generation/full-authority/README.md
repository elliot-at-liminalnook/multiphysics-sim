# Controller authority comparison

This separate CAD revision changes the provisional controller command limit from
350 to 1000 permille. It retains motor/driver physical parameters, feedback origin,
polarity, cadence, latency estimate and other gains. The CAD archive round trip
and native profile validation pass; all 114 geometry entries are byte-identical.

Physical export is complete: 29 links, 105 joints and twelve motors.
`../prepare_authority_search.mjs` verified matching export hashes and exact
equality of the other raw exported physical definitions. It preserves the twelve
explicit historical external-backlash overrides and rebinds CAD identity checks.
Native experiment initialization passes. The first four-trial comparison and two
Bayesian follow-ups are complete, all without falls over their two-second windows.
`first-four-comparison.json` preserves the matched authority comparison; `results.json`
includes all six trials. Best short-window speed is 0.429551 m/s, versus the
original gait's 0.387144 m/s. `selection.json` records the candidate for longer tests.

The first preparation check rejected raw-export versus runtime-representation
differences, retained in `preparation.log`. The runtime import normalizes signed
zeros and supplies defaults; its `World` also lacks the legacy exported
`floor_friction_static` field. After proving raw parent/new CAD physics identical,
the final recipe reuses the exact baseline runtime physical definition and
replaces only profile/source declarations. `preparation-final.log` records the
successful check. The unsupported static-friction field remains a known limitation
in both experiments; this comparison does not introduce or fix it.

The initial two-second comparison uses the same four pace/tracking candidates,
seed, commands and world as the 35% pilot. This remains an uncalibrated simulation
scenario, with imposed 11.1 V supplies and unloaded-bench motor estimates.

The original gait also completes ten seconds at 0.450412 m/s. Its optimized-build
replay matches the saved state and input recording exactly; sampled foot clearance
peaks are 12-42 mm. See `../full-authority-10s/`. The original-gait ten-second
half-timestep check passes all predeclared bounds. The selected candidate finishes
ten seconds at only 0.160519 m/s, despite its faster first two seconds. Exact replay
passes and its recorded path speed confirms slowing after startup. It is retained
as a rejected longer-horizon improvement. The original gait proceeds to a separate
30-second test; no 300-second qualification has been established.

Authoring is reproducible with `cad/scripts/prepare_controller_authority_revision.py`;
`robot.receipt.json` records the parent/new CAD hashes and script identity.
