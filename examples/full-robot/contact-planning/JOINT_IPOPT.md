# Joint robot planning with Ipopt

The shared Rust joint planner is now connected to the opt-in
[native Ipopt interface](NATIVE_IPOPT.md). It uses the same `JointContactMotion`,
CAD model, initial candidate, 443 variable definitions and bounds, target-speed
cost and all **6,776 physical inequalities** as the contact-relative timing
experiment. No force, moment, motor, floor, interlink or friction acceptance
threshold is relaxed. No robot properties are inferred or changed.

The original fast-start speed search completed its **8,000 model-evaluation-attempt**
budget (100 native iterations and 1,000 callbacks were additional limits). These are
experiment limits, not the user goal's stopping condition. BLAS/OpenMP threading
is explicitly limited to one thread for this experiment. The target remains the
existing experimental 0.30 m/s objective, not a demonstrated physical ceiling.
No feasible gait or measured runtime/browser speed gain is established yet.

## Shared problem and derivative contract

Both the existing augmented-Lagrangian optimizer and Ipopt now reuse the same
decision encoding/decoding helpers. Ipopt uses variables normalized to [0,1],
with their original physical bounds restored before the shared CAD evaluation.
Fixed variables remain fixed. Its ordinary initialization can move variables
inside their bounds; the unmodified input report is retained separately.

The constraint Jacobian declares **1,599,232 structural entries**, compared with
3,001,768 entries in a dense matrix. This includes coefficients that are zero at
the starting gait but may become nonzero later. Force coefficients affect their
clock's balance and motor rows plus their own node's cone rows; they cannot
affect geometry or other clocks. Motion affects sampled physics but cannot
change force-node cone values. No numerical threshold is used to discard entries.

The existing CAD-derived force columns are scaled by each physical variable's
width. Every analytic force column is checked for entries outside the declared
structure. Motion columns, and force columns requiring a signed-capacity
fallback, use bounded central/one-sided differences with the existing 1e-4
normalized step and domain-aware step shortening.

This initial backend requires explicit contact-relative force timing, preserving
one strict contact-event ordering during the solve. All physical rows remain,
including zero endpoint-cone and interlink rows. The model has nonsmooth terms
and potentially degenerate constraints; the integration does not claim Ipopt's
smooth-NLP assumptions hold everywhere or that local convergence is guaranteed.
Broader contact families and richer motion templates remain open work.

Model attempts are counted independently of Ipopt callback requests: an analytic
force linearization counts once, and every numerical motion probe counts,
including rejected probes. The returned candidate receives a separate uncached
CAD audit. Failed decoding/audits are retained as errors alongside the native
result, and feasible candidates from valid probes are retained independently.

## Completed verification

- Five shared contact-planning tests pass with the native feature enabled.
- The prior 112-evaluation analytic augmented-Lagrangian one-step experiment
  replays **byte-identically** after the shared decision-helper refactor.
- The one-iteration pilot completes with native status -1 (the requested
  iteration limit), **316 model attempts**, six native callback evaluations and
  no rejected callbacks.
- Its original full CAD report matches the previously recorded native initial
  report after signed-zero canonicalization. All other values are identical.
- Its returned objective and all **6,776** inequalities match the independent
  uncached final evaluation exactly.
- The pilot ends at **0.2116282431 m/s**, maximum normalized inequality
  **248.2974353**, still infeasible. This only verifies the solver/planner path;
  one iteration is not a speed or convergence result.
- A separate budget case terminates at exactly **12 model attempts**, native
  status -13, with the model-budget flag set and a final full-physics report.
  Budget exhaustion is not reported as physical infeasibility or optimality.

`optimize_joint_ipopt` is a focused native CLI using the shared planner. Its
inputs are the existing scene, markers and joint recipe, a verified native
library path and a separate `JointIpoptConfig` JSON file. It does not introduce
an alternative simulation or controller runtime.

## Completed comparisons

Both original fixed-normalized-knot searches now have terminal outputs:

| Search | Evaluations | Speed (m/s) | Force error (N) | Moment error (Nm) | Torque margin (Nm) | Maximum inequality |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Event-aligned initial knots, original height bounds | 8,000 | 0.025 | 0.10216537 | 0.05025274 | +0.04752272 | 1.51263712 |
| Same knots, broader CAD-derived height bounds | 8,000 | 0.025 | 0.09044991 | 0.03525936 | +0.37300014 | 0.80899830 |

Both are infeasible; neither produced a candidate to promote. The timing-aware
augmented-Lagrangian comparison also completed its 8,000 evaluations: 0.025 m/s,
0.07916036 N force error, 0.02406937 Nm moment error, +0.68521597 Nm torque
margin, and maximum normalized inequality 0.58320728. It remains infeasible.
Its phase and duty decisions moved more than in the fixed-knot comparisons,
but it produced no feasible candidate or speed gain.
The original shared optimizer executable was copied before rebuilding only
after both processes using it had exited. The timing-aware
optimizer and original Ipopt executable remain separate, unchanged binaries.

## Constraint representation follow-up

The opt-in [constraint projection experiment](IPOPT_CONSTRAINT_PROJECTION.md)
removes only validated fixed endpoint rows and moves sampled collision checks
into an explicit trial-evaluation domain. Default behavior retains all original
rows. Its pilot and exact legacy replay are recorded separately.

## Starting from the completed AL motion

The [warm-initialization experiment](JOINT_WARM_INITIALIZATION.md) transfers the
completed timing-aware AL candidate into the same native joint speed objective.
It compares ordinary initialization with a smaller initial interior distance,
without changing any physical properties, decision bounds or acceptance gates.
This directly tests whether the near-balanced starting motion can support
feasibility restoration followed by increasing speed.

## Original fast-start search completed

The final independently audited candidate has speed 0.02540433 m/s, force error
0.26422263 N, moment error 0.07139271 Nm, torque margin +0.01972720 Nm and cone
violation 2.13622957 N. Maximum normalized inequality is 42.72459136; there is
no sampled interlink overlap, and maximum floor penetration is 0.10514 mm.
No sampled feasible candidate was found. Native status -13 records failure of
a callback after the model budget exhausted, not a physical impossibility
certificate. The native final callback could not consume another model attempt;
the reserved independent uncached final CAD audit is present. The warm-start
comparison remains live. See `joint-ipopt-speed.summary.json` for counts and
the native iteration history.

## Grouped body derivatives

The opt-in [grouped body derivative path](GROUPED_BODY_DERIVATIVES.md) reproduces
the native warm-start pilot and full CAD result with 220 model attempts instead
of 316. It retains the full structural Jacobian and physical checks, rebuilding
local groups at each derivative request and falling back on invalid probes.
It does not establish a new gait or overall wall-time speedup.

## Sixteen-control body search

The [sixteen-control experiment](BODY16_NATIVE_SEARCH.md) preserves the starting
curve through exact refinement, doubles body decisions and retains all old
frames while adding sixteen checks. Its grouped and ordinary native pilots
agree with 220 versus 508 model attempts. The longer 491-variable speed search
is running alongside the original eight-control warm comparison; neither pilot
is feasible or ready for runtime promotion.
