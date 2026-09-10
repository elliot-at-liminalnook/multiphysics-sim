# Contact-order neighbors with coupled refinement

The shared Rust planner can now exchange adjacent cyclic touchdown/liftoff
events and prepare each resulting order for a full motion/force optimization.
This crosses the fixed event-order restriction of an individual Ipopt solve.
It is one neighborhood experiment, not MCTS, INSAT, or exhaustive gait discovery.

The generator preserves the foot-0 phase anchor, body and foot-path initializer,
physical model and explicit search bounds. It regenerates independent force
variables after aligning their knots with the new events. Missing, duplicate,
or nonuniform force-node bounds fail explicitly rather than inventing bounds.
Invalid stances and starts outside the numerical boxes remain recorded.

The frozen source is mesh-search checkpoint 8179, with sampled planning speed
0.026107009380794514 m/s. This is a planning seed, not a newly qualified gait.
Six alternative orders were prepared; two swaps exceed existing initializer
boxes. All six receive coupled local refinement, including those whose
fixed-motion conic solution violates planning gates. Conic residuals only order
the queue. They do not establish that a contact family is impossible.

Each trial receives up to 1,200 CAD evaluations and five native iterations using
the recorded solver and common configuration. The queue starts with edge-002,
then the original control, then the other five prepared orders. Observations
retain termination, cost, missing measurements, and sampled residuals. The best
sampled feasible snapshot can be a derivative probe; it still needs dense CAD
and controller validation. These small allocations compare initial progress;
failure within them is not convergence or physical exhaustion.

Force refinement changes search dimension: the original control has 366 force
variables, while neighbors have 630–690. A companion same-order control applies
the same alignment/refinement pipeline and has 522 force variables. This checks
whether refinement alone helps, but does not equalize every order's knot count.
Event-dependent layouts and different per-evaluation costs remain comparison
limitations. Report wall time as well as evaluations.

Validation so far: all 14 contact-planner tests pass, including cyclic swaps,
anchor preservation, bounds validation, stale timing metadata rejection, and
multi-step invalid-neighbor recording. The revised preparer builds successfully;
all seven original recipes and full conic results replay identically after the
companion control was added. The refined control is sampled feasible before
local optimization. No faster executed gait is established by preparation.

`contact-order-evidence.json` records code, binaries, complete preparation data,
tests and launch inputs. Original generator source and binary hash are retained
in `contact-order-prepare-v1.*`. Active search outputs are intentionally recorded
as live locations rather than complete evidence. The research rationale and
next adaptive methods are in [the research review](SEARCH_STRATEGY_RESEARCH.md).
