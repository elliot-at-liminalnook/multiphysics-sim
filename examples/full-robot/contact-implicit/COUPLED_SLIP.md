# Continued slip shaping and collision attribution

The continued searches reduce slip but still fail physical qualification. Adding
targeted balance samples does not resolve the tradeoff: the targeted result has
lower slip and acceptable sampled torque margin, but larger force and moment
errors. No result is promoted to the runtime or browser, and none establishes a
physical maximum speed.

Both continuations start from `slip32-shaped05.result.json`, with 32 periodic
controls, fixed horizontal displacement, the same contact model and actuator
bounds, slip ratio scale 0.05, and 30 optimization iterations. The control retains
144 collocation times. The targeted trial retains all of them and adds the 32
worst missed violating times selected from the parent's dense audit. Both stop
at the iteration limit (34,486 and 34,483 evaluations respectively).

Independent validation uses 512 physical times and 513 geometry poses:

| Motion | Maximum loaded slip | Force error N | Moment error Nm | Minimum torque margin Nm | Interlink overlap µm |
| --- | ---: | ---: | ---: | ---: | ---: |
| Parent | 72.87% | 0.215390 | 0.069810 | -0.002322 | 16.65 |
| Continued, 144 times | 66.59% | 0.223523 | 0.053710 | -0.001291 | 16.33 |
| Targeted, 176 times | 60.99% | 0.394645 | 0.130031 | -0.000044 | 16.64 |

Acceptance requires at most 5% loaded slip, 0.05 N force error, 0.02 Nm moment
error, -0.001 Nm minimum torque margin, 1 mm point penetration, and no reported
interlink overlap. All motions retain a planned +45° projected displacement
rate of 0.2009265099 m/s; this is not a measured walking speed. The finite
sampling does not establish continuous-time feasibility.

Adding nodes changes more than resolution: restoration includes unweighted
physical residuals at each time, whereas slip contributes four aggregate
residuals. Thus the targeted search also changes their relative influence.
These results do not isolate a pure grid-resolution effect. Continuing to vary
a weighted sum is not evidence that balance and slip can be satisfied together.
A constrained formulation respecting the physical tolerances is a justified
next optimizer experiment, not an implemented solution or convergence claim.

## Collision evidence

The shared Rust geometry audit now optionally exposes the existing surface/SDF
contact pairs, their link indices, world points and normals. `--pairs` also
exports the authoritative embedded link poses, using the existing `LinkPose`
contract for read-only CAD inspection. Compact audit output is unchanged.
Geometry forces, exclusions, model properties and optimizer behavior are unchanged.

All three dense audits attribute the overlap to `-X | Hip motor pulley and shaft
3090` against `Robot | Chassis and hip mounts`. The parent's worst point occurs
at 0.1516272424446593 s with 16.649625151 µm reported penetration. This confirms
the same region identified in earlier geometry experiments; it does not by
itself prove physical interference. The chassis SDF has 5.023 mm cells and its
exporter uses 0.15 mm tessellation tolerance, both much larger than that result.
No collision exemption or CAD alteration is made.

`prepare_planned_cad_probe.mjs` copies the worst existing Rust geometry frame,
without recomputing kinematics, into the existing CAD inspection input contract.
The export explicitly identifies itself as a planned geometry probe, not a
runtime rollout. `slip-coupled-cad-probe.input.json` records source hashes and
the exact pose and witness used by the CAD-side B-rep inspection.

The B-rep classifier places that witness inside one solid of `-X | Hip servo
internal geometry`; its nearest target-member boundary is 0.002139284752 mm
away (2.139 µm). On the source side it is outside all pulley solids, only
0.000068026857 mm (0.068 µm) from the nearest pulley surface. Thus the 16.65 µm
grid penetration is not an accurate exact-CAD depth, but the two-sided evidence
supports a small CAD interference rather than dismissing it as a pure grid
false positive. This is an inference from one point and its boundary distances,
not a whole-part Boolean intersection or a hardware clearance measurement.
Whole-part distances remain explicitly unmeasured. All eight grid-node weights
reconstruct the original Rust interpolated penetration within 1e-14 m.

The CAD library now offers `probe_both_sides=True` (CLI `--probe-both-sides`),
which adds exact source-link classification without changing target results.
The extended analytic box test verifies source classification and distances;
all three CAD inspection tests pass. `summarize_cad_witness.mjs` checks unchanged
target results, grid interpolation parity, and produces the two-sided summary.
Neither contact exemptions nor widened motion limits are justified by this check.

## Reproduction and provenance

Run `node examples/full-robot/contact-implicit/summarize_coupled_slip.mjs` to
reproduce `slip-coupled-summary.json`. It checks matched continuation inputs,
retention of the prior sample grid, fixed displacement, exact independent
physical/slip reports and optimization residual vectors, equality of physical
frames at shared dense times, and every geometry maximum against its raw pairs.

The optimizer remains the recorded executable built from commit `441e036`.
The pair-only audit and the subsequent pose-export audit have separate build
identities. Exact pair-only source snapshots are preserved and checked against
their build hashes; the final pose sources are in the worktree. The new pose
audit reproduces every prior parent audit field exactly across 513 frames.

The contact-planning tests pass all 16 cases, including an analytic rotated-body
pose check and equality of detailed/compact geometry output. The existing CAD
inspection tests cover rigid transforms and units, SDF interpolation probes,
solid point classification, and source preservation. Logs and native recipe,
result, and audit artifacts accompany this report.

The exact CAD command uses `audit_captured_geometry.py`, the baseline
`robot.rcad`, the recorded constrained scene, and
`slip-coupled-cad-probe.input.json`, with the two link names above and time
`0.1516272424446593 --probe-sdf-cells --points-only --probe-both-sides`.
`PYTHONPATH=cad` and the existing CAD virtual environment supply the CAD kernel.
This remains a read-only CAD-side operation; all simulation and embedding stay
in the shared Rust path.
