# Secants near convergence

The previous bounded secants reduce 20 ms rendered steering p95 from 33.72
to 26.10 ms, still above 20 ms. There remain 1,996 fresh Jacobians in 1,200
control transitions. The solver takes small but insufficiently tight cached
corrections without updating the matrix, then refreshes after four such steps.

Test `broyden_negligible_updates` disabled/enabled with ordinary Broyden
updates enabled in both. Use the exact existing 24-second 20 ms mixed
steering scene, controller, seed, inputs and physical/convergence gates.
Only decreasing actual residuals may supply these extra secants. Keep the
eight-update cap, scale/finite/singularity guards and four-step tail refresh.
Do not relax the 100-times stricter cached correction or raw residual bounds.

Require the existing analytic and discontinuous-no-root tests, physical task
acceptance, and whole trajectory agreement within 1 mm foot / 0.5 mm body.
Compare derivative counts and wall cost before choosing further browser
tests. The existing delivered bundle and previous failed timing stay intact.
