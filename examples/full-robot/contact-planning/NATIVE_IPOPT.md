# Native constrained nonlinear solver

The shared `sim-solve` library now has an opt-in `native-ipopt` feature. It exposes
Ipopt's constrained nonlinear program interface to Rust callbacks, including a
fixed sparse Jacobian structure and a limited-memory Hessian approximation.
This addresses the solver capability missing from the TOWR comparison, but
the interface checkpoint did not yet include a gait-planner connection.
The subsequent [joint robot integration](JOINT_IPOPT.md) now records that
connection and its pilot. No robot speed gain, feasible gait or physical maximum
is claimed by the interface checks below.

Ipopt's [official interface documentation](https://coin-or.github.io/Ipopt/INTERFACES.html)
describes explicit variable/constraint bounds, objective and constraint
derivatives, and a sparsity pattern that must contain all potentially nonzero
entries throughout a solve. The new driver preserves that contract; it does not
infer sparsity from zeros at a single candidate. Solver status and independently
re-evaluated final constraint/bound violations are recorded separately.

## Native dependency and architecture

`prepare_native_ipopt.mjs` fetches a pinned artifact from the
[official CasADi distribution](https://web.casadi.org/get/), verifies its SHA-256,
and extracts native Ipopt plus its bundled dependencies. No Python interpreter,
CasADi modeling API, separate robot simulation, system package installation or
browser dependency is used. Native numerical optimization calls Rust evaluation
code; the existing Rust physics/controller runtime remains the execution path.

- Distribution: CasADi 3.8.0 macOS x86_64 wheel, 53,391,344 bytes.
- Archive SHA-256: `456eb3b43ca868ac0b46f38526ba4f09201943dc7553e23c212945fce79aa909`.
- Ipopt: **3.14.19**, double-precision `ipnumber`, 32-bit `ipindex`, C `bool`.
- Six native libraries, three ABI headers and six relevant license files have
  their identities recorded in `joint-ipopt-native-library.json`.
- The audit also passes in a fresh directory containing exactly those six
  libraries. System libraries/frameworks remain platform dependencies.

The dynamic loader is explicitly unsafe: its caller must verify the selected
library's trust and ABI. A version check alone cannot distinguish alternate
integer/precision builds. Only this pinned ABI/platform is verified. Loading and
optimization are explicit operations, and concurrent/reentrant native solves
are rejected. Callbacks catch Rust panics before they can unwind across C.

The driver disables option-file loading, automatic NLP scaling, relaxed variable
bounds and acceptable-iteration early success. Callers supply search tolerances
and limits. Its callback budget counts unique value/derivative requests plus the
initial and final evaluation; it does **not** count any numerical probes hidden
inside a caller's derivative function. Gait integration must also enforce the
existing CAD-evaluation budget.

## Executed acceptance cases

`audit_ipopt` evaluates known optima and failures through the native interface:

| Case | Observed result |
| --- | --- |
| Official HS071 nonlinear reference | Native status 0; objective 17.0140172891763; maximum constraint/bound violation 7.10542736e-15; 22 callback evaluations |
| Synthetic support-force/speed problem | Native status 0; speed 1.333333333313333; force shares 0.6666666666641666 and 0.3333333333358334; zero measured violation |
| Infeasible bounded problem | Native status 2; retained violation 3.0000000000005 |
| Nonfinite derivative | Native status -13; callback rejected |
| Deliberate derivative panic | Native status -13; panic caught; no final feasibility claim |
| Caller cancellation | Native status 5; cancellation recorded |
| Three-callback budget | Native status 5; budget exhausted; exactly three counted evaluations |

The support/speed oracle is derived independently: with normalized load
`fA + fB = 1`, capacities `fA <= 1 - v/4` and `fB <= 1 - v/2` imply
`v <= 4/3`. Equality is attainable at `fA=2/3`, `fB=1/3`. These synthetic
capacities test joint load redistribution and speed optimization; **4/3 is not
the real robot's speed bound**.

Duplicate Jacobian entries are rejected before native execution. A second HS071
solve passes after the deliberate failures, checking cleanup and recovery. The
deliberate panic appears in stderr because Rust's ordinary panic hook runs even
when unwinding is caught; the audit process exits successfully.

All **32 existing shared-solver tests pass**, including in release mode with the
new feature enabled. `.github/workflows/native-ipopt.yml` adds these checks and
the native audit on the documented macOS Intel runner. The workflow is configured
but has not been run on GitHub in this session; the local equivalents passed.

The first native attempt returned status -11: Ipopt's C adapter requires a
non-null Hessian callback even in limited-memory mode. The driver now supplies
a callback that rejects any unexpected exact-Hessian request; it does not invent
a zero Hessian. This correction and the earlier missing-example-path build
failure are retained in the logs. The dependency recorder also distinguishes a
Mach-O image's own install name from its dependencies; no library was patched.

## Reproduction and next use

```sh
mkdir -p runs/ipopt-audit
node examples/full-robot/contact-planning/prepare_native_ipopt.mjs runs/ipopt-audit/native-libraries.json
cargo test --locked --release -p sim-solve --features native-ipopt --lib
cargo build --locked --release -p sim-solve --features native-ipopt --example audit_ipopt
target/release/examples/audit_ipopt "$PWD/runs/native-ipopt-casadi-3.8.0/extracted/casadi/libipopt.dylib"
```

The [joint planner integration](JOINT_IPOPT.md) now connects the existing
`JointContactMotion` variables, physical constraints, analytic force columns and
numerical motion derivatives, retaining independent final audits.
The contact model contains piecewise/nonsmooth terms, so these small smooth
reference successes do not establish convergence on the robot problem.

## Initial-point distance options

The shared configuration now exposes `initial_bound_push` and
`initial_bound_fraction`, with backward-compatible defaults of 0.01. They map
only to Ipopt's initial-point interior adjustment, leaving physical variable
bounds and bound relaxation unchanged. The native audit adds two scalar
initialization cases (0.01 and 1e-8) and rejects invalid distances. All seven
previous native cases reproduce exactly. See
[the joint warm-initialization experiment](JOINT_WARM_INITIALIZATION.md) for the
CAD motivation, pilot results and source/build identities.
