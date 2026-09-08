# Probe radius and host portability

The 1e-6 tangent-probe profile passes the native task and reproduces solver
trajectories within 1.8 nm, but native/WASM differ by 1.45e-7 N at one force
sample (also exposed as an observation). Exact replay/reset and UI tests pass.
Keep that failed result and the existing 1e-7 absolute + 1e-8 relative host
tolerance. Do not measure/promote rendered performance until parity passes.

Test relative probe steps 1e-6, 4e-6 and 1e-5, all multiplied by 1+|unknown|,
with no change to exact-derivative fallback probes, solver acceptance, physics,
controller, inputs or task gates. A larger difference step reduces division
of nested floating-point noise by a tiny interval. It also increases local
truncation error, so check all three against the same analytic mechanism,
contact and final-state tests before the robot study. The permitted numerical
parameter range is finite (0,1e-4]; invalidate reused matrices when it changes.

Run the same 24-second 20 ms steering case in native and WASM. Require physical
acceptance, fixed host-parity tolerance, exact replay/reset and the existing
1 mm foot / 0.5 mm body solver-comparison screen. Preserve all three outcomes.
If multiple cases pass, compare their processing costs before choosing a
rendered candidate. Timestep accuracy remains a separate known failure.
