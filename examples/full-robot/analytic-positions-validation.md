# Direct positions for certified mechanisms (experimental)

`EmbeddingConfig.analytic_mechanism_positions` replaces iterative position
reconstruction with explicit slider-crank and ideal-transmission coordinates.
It is default-off. It preserves the existing numeric velocity map, acceleration
curvature solve, singular-value rank checks, physical parameters and every
original closure equation. It does not remove any moving link or its inertia.

The shared Rust component recognizes compiled topology and local joint frames,
not robot or component names. A supported loop has two serial revolute joints
and a prismatic joint on a common rigid carrier, with parallel rotation axes
and a perpendicular slider axis. The exported zero-coordinate configuration
must close within 1e-12 m, and axis errors must be at most 1e-12. Offset slider
guides and opposite coupler axes are supported. Modal flexibility and unsupported
loops are rejected explicitly. Intervening fixed joints are not traversed by
the recognizer; the current robot export has already merged rigid parts.

Every crank must be independent; its two passive coordinates must be dependent
and disjoint from every other mechanism. Every ideal transmission must map an
independent driver to a disjoint dependent output. These mechanisms must cover
all dependent coordinates. Explicit opt-in with incomplete coverage fails.

The slider coordinate comes from intersecting its guide line with the circle
defined by the rigid rod length. The seed's rod projection chooses the assembly
branch; angle unwrapping retains the nearby coupler winding. Unreachable targets
and toggles fail instead of being clamped. Both branches are tested. As with the
existing local chart, callers must bound increments and check collision and
actuator feasibility: this is not path planning through singularities.

The standalone component also computes first and second derivatives with
respect to crank angle. They are audited against the existing numerical chart,
but this experiment does **not** substitute them for the runtime's tangent or
curvature solves. Failed original position closure does not silently fall back
to iteration. Every final motion still undergoes the original rank, position,
velocity and acceleration checks.

## Geometry evidence

All four actual robot knee loops satisfy the structural checks. Their projected
crank radius is 75 mm and rod length is 100 mm. The four loops and eight ideal
transmissions cover all 16 dependent coordinates. An 81-pose sweep of crank
angles from -1.2 to +1.2 rad gives maximum differences of 2.45e-12 in mixed
joint coordinates, 6.93e-13 in joint velocities, and 4.20e-13 in joint
acceleration bias against the iterative chart. Maximum original position,
velocity and acceleration closure values are below 1.67e-16 in their declared
units. This geometric sweep is not a motor travel or collision-clearance limit.

The mechanism suite covers independent scalar formulae, both assembly branches,
an offset guide, opposite coupler axes, floating-base motion, applied-load
accelerations, incorrect axes/topology, unreachable positions, and rank/toggle
rejection. Runtime comparisons retain numeric singular values rather than
substituting a fabricated rank diagnostic.

## Full dynamic comparison

Both complete 2.8 s runs pass the existing numerical comparison limits used for
earlier solver experiments; the thresholds were not changed for this method.

| Maximum difference from matching iterative run | 0.25 ms | 0.125 ms |
|---|---:|---:|
| Foot marker position | 1.79e-12 m | 1.36e-12 m |
| Motor current | 2.25e-9 A | 3.08e-9 A |
| Joint velocity (mixed coordinates) | 4.15e-8 | 9.40e-9 |
| Integrated contact impulse | 7.54e-10 N s | 5.39e-10 N s |
| Ordered event time | 6.88e-10 s | 5.33e-10 s |

Event counts and sampled contact identities match. All sampled original closure
rows pass their unchanged scaled tolerances. Both runs retain the supported-lift
gate, with qualifying spans of 200/210 ms and peak clearances of 3.211/3.203 mm.
These are comparisons with the previous model, not measured hardware accuracy.
They preserve its unresolved landing, calibration and training-model limitations.

The base run requires 147,084 closure Jacobian/factorization builds instead of
379,736. Residual evaluations increase slightly (1,192,971 to 1,196,379), so
removed assembly calls do not alone establish a speedup. Development runs
overlapped compilation and other simulations. The separate sequential ABBA
benchmark checks exact repeatability against each method's own captured result;
the cross-method numerical comparison above establishes their agreement.

The browser recipe disables native process-global profiling, which WASM rejects.
Its physics and controller settings otherwise match the native base recipe.
The maintained previous controller and the new experiment both require full
281-frame native/browser comparison plus exact replay/reset. The new preset also
participates in the viewer's interaction suite.

Both full browser comparisons pass: the previous controller's maximum entry
difference is 7.34e-8 and the new experiment's is 1.08e-8 (both worst entries are
foot-force components in N), below the unchanged 1e-7 diagnostic threshold.
Replay/reset are exact; invalid requests and incompatible replay attempts
preserve state. All 22 UI checks pass on the 20-preset bundle, including the new
experiment's gain controls, target/actual display and replay. The rendered robot
view was inspected. The new browser run takes 59.20 s for 2.8 simulated seconds
in overlapping development conditions; its longest chunk is 0.863 s. Responsive
rendering is not realtime simulation or low-latency control.

Selected Rust suites pass 69 test executions, including the 17-test mechanism
suite with and without default features, the original-constraint audit, coupled
motor and integration tests, and the trajectory-comparison example tests. The
existing CI jobs run those suites; the structural-audit CLI now has a CI build
check as well.

## Reproduction and acceptance

```sh
cargo test --locked -p sim-domain-robot --test embedding
cargo build --locked --release -p sim-runtime --example audit_analytic_mechanisms --example integrate_embedding --example compare_embedding
node examples/full-robot/prepare_analytic_positions.mjs
target/release/examples/audit_analytic_mechanisms runs/full-robot/learning/point-feedback/scene.json > runs/full-robot/learning/analytic-positions/geometry-audit.json
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/analytic-positions/base.config.json > runs/full-robot/learning/analytic-positions/base.execution.json
```

Repeat the full motion for `refined`, then compare each result with its matching
`closure-preparation` capture using `compare_embedding` and
`examples/full-robot/foot-markers.json`. Inputs and hashes are recorded by the
preparation script. Full trajectory, contact, event, lift, performance and
browser results must be assessed before promotion. The parent training-model
acceptance, hardware calibration and learning requirements remain open.

After freezing the runner and waiting for other heavy assistant jobs to finish:

```sh
node examples/interactive/benchmark_exact_runtime.mjs runs/full-robot/learning/analytic-positions/benchmark runs/interactive/closure-preparation/native-runner runs/interactive/analytic-positions/native-runner runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/closure-preparation/base.config.json runs/full-robot/learning/closure-preparation/base.execution.json runs/full-robot/learning/analytic-positions/base.config.json runs/full-robot/learning/analytic-positions/base.execution.json
node examples/full-robot/summarize_analytic_positions.mjs
```

The optional final benchmark arguments supply the candidate configuration and
its independent expected capture. Every repeat must exactly match its method's
physical frames, events, solve records, contact stages and impulses. This allows
the two algorithms' harmless roundoff differences without accepting uncontrolled
run-to-run variation. It does not replace the cross-method accuracy gates.

The isolated ABBA timings are **59.817, 46.518, 46.396, 59.947 s** on the
recorded Intel i9-9980HK host. Means are **59.882 → 46.457 s** for 2.8 simulated
seconds: **1.289× faster, 22.4% less stepping time**, still about **16.6× slower
than realtime**. All four repeats match their respective expected physical and
numerical records exactly. Two repeats per method on one workload do not
establish broader learning throughput or hardware accuracy.

The shareable browser archive is
`runs/interactive/robot-lab-analytic-positions-2026-09-07.zip`. Its default-off
experiment is labeled **Quadruped · direct linkage positions**. The independent
native runner, 255-file source snapshot, scene/configurations, geometry audit,
full trajectory comparisons and validation results are preserved under
`runs/interactive/analytic-positions`; `analytic-positions-status.json` records
the numerical gates and artifact hashes. The separately packaged experiment
archive preserves evidence beyond ignored run-directory references.
