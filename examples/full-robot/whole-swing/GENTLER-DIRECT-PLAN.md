# Gentler direct support transfers

The 2× Shift/Return gait travels about 2.73 mm/s. At 0.03 mm/s contact smoothing,
Return and Raise still account for 76.6% of its loaded contact motion; the worst
foot moves 17.8% as far as net body advance, failing the fixed 5% screen. The
earlier 3.70 mm/s direct-transfer gait also failed sliding and heading. Test the
combination of direct transfers and gentler body motion explicitly.

Before evaluating, freeze three 60-second development minutes with smoothing
speeds **1, 0.1, 0.03 mm/s**, paired with the corresponding completed
`gentler-contact` cases. Enable the existing shared Rust `direct_support_transfer`
option. Reallocate the same 1.90 s nominal transfer to phase durations
**[1.10, 0.38, 0.38, 0.02, 0.02] s**. Assert that the duration sum matches the
old total within 1e-15 s (floating-point summation roundoff). Foot stride
remains 5.175 mm and forward request 2.7236842105263155 mm/s. This changes both
the body path and its time allocation; it does not isolate either alone.

Keep all other settings identical to each matched gentler-contact baseline:
CAD physical model and friction coefficients, controller gains, effective
servos, foot-placement geometry, inputs, seed, development push, integration
method, tolerances and 1.25 ms physics step. Only the two declared sequence
fields differ within each matched fidelity profile. The three contact laws
remain separate fidelity profiles. Reconstruct recipes from versioned inputs,
without requiring prior ignored captures to prepare a new run.

Retain all complete failures and runtime-error prefixes. Audit each profile
independently, verify the matched baseline recipe, and compare all completed
constant-forward planned foot landings to 1e-12 m. Measure full-minute task
outcomes, actual sustained speed, stop position, heading, contact motion by
phase, positive shaft work and native computation. Retain the existing task
gates and **5%** maximum per-foot loaded contact-motion / net horizontal body
advance screen. Do not change these after seeing results.

No analysis/build jobs will overlap native simulation. Existing baseline
0.1 mm/s timing had concurrent analysis and is unsuitable for an isolated
throughput ratio. Native timing cannot qualify realtime browser control.
Any candidate that passes needs timestep refinement, steering, fresh held-out,
terrain and browser tests. Sampled contact motion does not certify hardware
traction or between-sample behavior. Keep existing live bundles intact.
