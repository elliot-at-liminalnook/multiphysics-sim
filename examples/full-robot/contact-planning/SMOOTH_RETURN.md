# Completed swing-return and feedback diagnostics

These manually selected experiments preceded the user's TOWR correction. They
are preserved as measurements and warm starts; further single-parameter variants
are superseded by [joint force/motion/timing optimization](TOWR_GAP_AUDIT.md).

| Candidate | Forward / reverse m/s | Loaded slip | Lifts passed |
| --- | --- | --- | --- |
| Contact velocity feedback, D/K | .232566 / .222122 | 12.99% | 42/42 |
| Contact velocity feedback, 2 D/K | .238417 / .222334 | 15.95% | 39/42 |
| -X return ramp .25, command .23 | .233944 / .225110 | 10.58% | 42/42 |
| -X return ramp .25, command .25 | .261052 / .249364 | 16.40% | 44/46 |
| All return ramps .25, command .23 | .240289 / .230317 | 12.67% | 42/42 |
| -X return ramp .15, command .23 | .234492 / .224328 | 12.85% | 42/42 |
| -X return ramp .25, +X extra lift, command .25 | .260947 / .249229 | 16.38% | 46/46 |

All seven eight-second episodes pass short heading/speed/tilt/stop checks and
have zero sampled inter-link overlap across 401 recorded poses. All fail the
unchanged 5% slip gate. No sustained, refined-timestep, steering, dropout or
browser qualification was performed on these variants. They are not qualified
fast gaits or evidence of the physical speed maximum.

## Shared component and physical interpretation

`control.smooth_return` is a reusable C2 return profile with cubic velocity ramps
and a constant-speed middle. For normalized ramp fraction r, its peak normalized
rate is 1/(1-r), and peak normalized acceleration is 1.5/[r(1-r)]. At r=.25,
peak rate is 4/3 instead of the original quintic's 1.875; peak acceleration rises
to 8 instead of approximately 5.774. This trades acceleration for reduced peak
return rate; it does not guarantee lower actuator demand on a nonlinear linkage.
The optional foot `return_ramp_fraction` defaults to the existing quintic path.
Stance motion and the separate lift/lateral bump retain their original laws.

The -X r=.25 reference reduces its worm peak from 7.218 to 6.219 rad/s. The
conditional exact-reference no-load rate budget rises from .161670 to .187640
m/s. These are bounds for that prescribed trajectory, not bounds on every gait
or on achieved motion with tracking error. At r=.15 the worm peak instead rises
to 6.362 rad/s and the conditional budget falls to .183424 m/s. All of the new
nominal inverse-dynamics references retain failed balance/actuation audits;
their diagnostic compiler flag is explicit and existing gates are unchanged.

The r=.15 compiler fails the 2 rad/s² interpolation gate at both 4,000 and 8,000
samples. Tightening only placement tolerance from 1e-9 to 1e-11 m at 8,000 samples
reduces the error to 1.081 rad/s² and passes. Failed outputs and logs are retained.
This is evidence of numerical sensitivity in derived accelerations, not a
relaxed physical constraint. The extra-lift case changes only +X swing lift by
1 mm, after the earlier .25 run missed two lift-duration checks.

## Evidence

The [summary](smooth-return-summary.json) identifies seven completed captures
and exact replay/geometry reports. [The archive index](smooth-return-evidence-index.json)
records 75 raw files in three parts, CAD identity, native binary identities and
the shared-control source overlays against commit 9999b0c. The joined archive
and all extracted file hashes were verified. Restore the previous archive chain
before these parts. Recipes, capability reports and test logs accompany the data.

Shared-control tests (66), Rhai integration tests (35), and phase-planner tests
(3 at that build) passed. Recompiling the original reference is byte-identical
to the committed baseline. The focused smooth-return example reports analytical
peak rate and acceleration. `diagnose_slip_phase.mjs` assigns measured loaded-foot
path to held controller-phase bins; those bins do not prove the actual physical
contact mode or establish a causal explanation for slip.
