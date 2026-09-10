# Grouped numerical body derivatives in joint Ipopt planning

The joint native planner now has an opt-in `group_body_derivatives` setting,
default false. It uses the [verified compact body support](BODY_LOCALITY.md)
to combine fixed-time body-value probes. The speed objective, physical model,
variables, bounds, acceptance checks and permanent native Jacobian pattern are
unchanged. No numeric threshold is used to infer structural zero derivatives.

A shared `group_disjoint_columns` helper greedily groups caller-declared column
supports, ensuring at most one member can affect each residual row. It rejects
out-of-range and duplicate support rows. A nonlinear analytic test compares
simultaneous and individual probes. The caller remains responsible for deriving
support from equations throughout the probe domain.

At each native derivative request, the planner rebuilds body supports from the
current cached frame times using the shared periodic spline support API. Body
values do not affect contact/force knots or the speed-only objective. Their
nonzero rows can include every physical field of an influenced frame, while
force-node cone rows remain independent. This local grouping is distinct from
Ipopt's permanent sparsity pattern: timing decisions can move support to other
rows, so the full declared pattern is retained.

Each grouped probe applies the same normalized step and bound clipping as the
ordinary per-column path. Columns are recovered using their own actual step
sizes. If a group fails, its members retain the ordinary individual probing
path, including bounded step shortening. Budget exhaustion remains an error;
it cannot trigger uncounted fallback work. Singleton groups and nonperiodic
body curves use ordinary probes. New counters report grouped probe requests
and group/column fallbacks. Every physical audit and final uncached evaluation
remains in place.

Verification compares a one-iteration native pilot against the archived
ungrouped warm-initialization pilot at the same original .30 m/s target. Exact
native and physical output agreement is expected except evaluation counts and
new counters. A 12-model budget case checks termination and final audit. A
separate failure-path fixture retains the original CAD, world and initial
motion but deliberately expands only body-translation *test search bounds* to
[-1,1] m and uses a 0.1 normalized probe step, forcing unreachable trial poses.
Its 180-model budget tests invalid-group fallback and accounting. These test
bounds are not proposed operating limits or used by speed searches.

The expected ordinary case uses 48 grouped body probes per Jacobian instead of
96 individual ones, reducing total ordinary motion probes from 154 to 106. A
one-iteration pilot with two Jacobian requests should therefore fall from 316
to 220 model attempts. This prediction requires measurement and does not imply
a proportional overall optimizer or browser speedup. Feasible dynamics and
runtime controller validation remain separate requirements.

## Completed verification

The native one-iteration pilot reproduces the archived ungrouped search result,
returned candidate, full CAD report, objective, permanent Jacobian dimensions
and native iteration history after signed-zero canonicalization. It uses
**220 model attempts instead of 316**, a 30.38% reduction, with 96 grouped probes
across two Jacobian requests and no fallbacks. The returned motion is still
infeasible; this is equivalent optimization with fewer physics evaluations,
not a new gait or measured wall-time acceleration.

The 12-model case stops at exactly 12 attempts. The deliberately unreachable
fixture exercises **12 group fallbacks** and stops at exactly 180 attempts.
Both retain an independent uncached final CAD report. Their native final
callbacks cannot consume another model attempt, so those native callback
values are absent; this is recorded rather than confused with missing physical
audit or a physical impossibility certificate. The ordinary pilot's native
final callback values exactly match all original full-audit constraints.

All 33 shared solver tests and seven shared planner tests pass in release mode.
The existing native CI workflow covers these tests; it has not been run remotely
here. Source overlays, binaries, inputs, results and logs are indexed in
`joint-body-grouping-build.json`. The original fast-start search completed its
8,000 attempts before its executable was copied and rebuilt. The separate
warm-start optimizer remains running with its unchanged executable.
