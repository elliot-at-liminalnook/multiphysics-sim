# Separate coarse integration failure from solver tolerance

The first SDIRK2 study fails at 1.32 s, in the first return phase. The recorded
external force is zero in the preceding samples. Teacher 20/10 ms and student
20 ms show gear speeds above 200 rad/s, unlike the retained 5 ms teacher and
20 ms backward-Euler student. These observations locate the failure, not its
cause. Neither successful Newton convergence nor scalar L-stability guarantees
an acceptable nonlinear contact trajectory.

Before changing the method or contact model, run two teacher 20 ms cases with
the original 24-second recipe and identical inputs. One repeats the authored
absolute/relative Newton tolerances of 1e-5; the other tightens both to 1e-8.
Keep contact history, closure, subdivision, geometry, forces and commands fixed.
Retain solver profiles for both. Profile timings include instrumentation and
must not be used for performance acceptance. Compare accepted state growth,
residual checks, and any accepted subdivision. Preserve either failure and
require all original task/trajectory gates before considering promotion.

This tests whether reducing the declared nonlinear residual tolerance removes
the early failure. It cannot by itself identify contact-memory overshoot,
root selection, or a mechanical-chart issue. A stable outcome would still need
independent accuracy and throughput evaluation; an unstable outcome rules out
this simple tolerance change as a sufficient remedy.
