# Live joint candidates and a working controller seed

The shared native solver now exposes an immutable candidate/report observer,
while retaining the original report-only API. The optional sixth CLI argument
is a fresh checkpoint directory. `optimize_joint_ipopt_checkpoint` is a separate
executable so existing searches need not be interrupted or their binaries replaced.

Snapshots retain the fastest sampled-feasible point and the point with the least
maximum positive normalized inequality (speed breaks ties). These rankings only
select output; no value returns to the numerical solver. The full recipe, search
settings and physical report accompany each point, including derivative probes
or rejected NLP trials. Files are replaced atomically after syncing; first output
is immediate, subsequent dirty output is throttled to five seconds, and normal
termination flushes remaining points. The two files are independently atomic.
An existing directory is rejected. A write failure disables further checkpoint
output, permits the solve to finish, and is reported with a nonzero CLI exit after
the numerical result is printed. Sampled feasibility is not runtime qualification.

## Verification

`check_joint_checkpoints.mjs` executes a four-model fixture using the prior binary,
the new binary without snapshots, and the new binary with snapshots. Terminal
JSON is byte-identical in all three cases and to the previously recorded baseline.
The saved feasible candidate matches the library's retained best candidate;
model/bounds are preserved. An infeasible fixture writes no feasible snapshot.
Reusing a directory fails without changing any existing file. The atomic-file
test also retains an open reader across replacement and protects an existing
temporary file. Nine shared planner tests and the separate checkpoint test pass.
The four-model check is not a full-iteration numerical regression.

CI now builds the separate executable and runs its file-ownership test. The CAD
replay is local evidence in the archived verification directory; remote CI has
not run here. Source overlays, binary/input hashes and completed experiment data
are recorded by `record_joint_checkpoints.mjs`.

## Completed older search and force repair

The eight-control mesh search completed at 8,000 model attempts with native -13
(model budget). Its final candidate reaches 0.0883106085 m/s but violates the
friction cone by 1.219316674 N. Its independent force/moment errors are
0.0496404051 N / 0.0180631308 Nm, with torque margin -0.0060929450 Nm.
This is a failed physical candidate, not a speed result or an optimum.

Both its final motion and its retained sampled-feasible motion were reevaluated
using the existing conic force solver with the current nominal servo-command
bounds. Motion, existing mesh and decision bounds were preserved. The final
motion's hard-command conic subproblem is `PrimalInfeasible`; this floating-point
fixed-motion result does not eliminate alternative motions in that gait family.
The retained motion's force solve is `Solved`, sampled feasible at
0.0257957737 m/s, with independently checked command affine error
5.6843418861e-14. Both have 6,384 hard command rows.

The repaired motion passes reference compilation with 1,000 uniform samples plus
all previous uniform/additional phases, unchanged interpolation and pause gates,
and required reverse/static audits. Nominal maximum force/moment errors are
0.0499135663 N / 0.0195134018 Nm. Maximum sampled reference interpolation errors
are 5.6105731e-6 rad, 0.0005048313 rad/s and 0.83164044 rad/s².

## Detailed runtime screen

The unchanged shared Rhai controller completes the standard eight-second
forward/stop/reverse/stop schedule at 0.000625 s physics steps, seed zero:

| Measurement | Result |
|---|---:|
| Forward speed | 0.0257187261 m/s |
| Reverse speed | 0.0260142758 m/s |
| Maximum loaded-foot slip ratio | 4.9102366% |
| Stop settling times | 0.12 / 0.18 s |
| Maximum tilt | 0.07165682 rad |
| Planned lift checks | 20 / 20 pass |
| Interlink overlap | zero at 401 sampled poses |

Exact policy replay has zero command mismatch. The short control and contact
gates pass. Geometry uses the shared sampled surface/SDF representation and its
authored exclusions; it does not prove continuous-time collision freedom.
Timestep sensitivity, sustained operation, steering and browser qualification
remain outstanding. Slip is close to the 5% threshold. This is a working seed
for speed optimization, not a faster gait than earlier experimental candidates.

The new eight-control, 443-variable search starts from the repaired candidate
with the same current command bounds and 0.30 m/s experimental target. It saves
live snapshots under `runs/joint-checkpoints/mesh-command-speed/`. The existing
sixteen-control search continues independently. Neither proves the physical
maximum, and the adaptive outer search from the research note remains to implement.
