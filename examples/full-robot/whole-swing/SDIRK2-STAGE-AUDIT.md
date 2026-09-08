# Locate the first-return stage jump

The 20 ms teacher fails at 1.32 s under both 1e-5 and 1e-8 Newton tolerances.
Its published state reaches 247.622 rad/s sampled gear speed. Tighter solves
change accepted joint positions by at most 4.1e-9 rad, so tolerance reduction
alone is not sufficient. The external force is zero during this transition.

Expose the existing optional Newton audit window in accepted implicit-step
diagnostics, including the equation seed and endpoint states, reduced velocities,
accelerations and Newton corrections. Do not change equations, guesses, forces,
or acceptance. In SDIRK2 the second equation seed is an affine anchor, not an
accepted physical state; its equation timestamp is not an observation timestamp.
Test fixed-slider and floating-body analytic stage values, bitwise unchanged
endpoints with auditing enabled, and absence outside the selected window.

Run the original teacher 20 ms and 5 ms recipes with audits covering equation
start times 1.25 through 1.34 s. Preserve the full 24-second captures and profiles.
Require exact agreement with their previous captures after removing only host
timing and the declared audit-window configuration. The 20 ms run is expected
to retain its failure; the 5 ms run provides the bounded stage comparison.
Identify whether the large velocity first appears in a physical stage or the
affine anchor, and inspect the associated changes in mechanical coordinates
and contact memory. Do not infer the cause from convergence alone.

This is a diagnostic experiment. Instrumented timing is not performance
evidence; coarse trajectories remain failed and no browser promotion follows.
