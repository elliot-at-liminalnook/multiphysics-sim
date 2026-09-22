# Mechanical-only secant updates

The earlier solver-secant experiment enabled Broyden updates in both the outer
mechanical and inner auxiliary Newton solves through inherited configuration.
This configuration-only screen explicitly sets auxiliary_broyden_updates=false
and compares outer updates with and without negligible-correction updates, plus
the existing analytic-motion option. It uses the current preserved executable;
no simulation source changes or new physical approximations are introduced.

The original matrix-update safeguards, fresh derivatives, backtracking, residual
and correction acceptance, event/contact invalidation and timestep remain intact.
All input, relevant solver-source and executable hashes are retained. The selected
physical recipe remains warm-probes/config.json until qualification demonstrates
an improvement. Raw unsuccessful runs remain evidence and are not overwritten.

The initial controller-allocation investigation found only about 0.263 weighted
seconds of allocator leaves under servo commands in the existing browser sample,
versus 3.743 seconds across all allocator leaves. That small isolated opportunity
motivated testing the broader solver-policy separation first. Sample weights are
approximate and are not direct allocation counts or throughput measurements.

## Native screen completed

| Route | Wall seconds for 3 s | Speedup | Outer Newton iterations |
| --- | ---: | ---: | ---: |
| default | 22.763944 | 1.00000 | 91123 |
| outer | 21.014733 | 1.08324 | 66375 |
| outer-tail | 24.147557 | 0.94270 | 66350 |
| outer-analytic | 19.126857 | 1.19016 | 66375 |

The mechanical-only route reduces Newton iterations 27.2% and takes 7.7% less
wall time. Its analytic combination reaches 1.19016x, narrowly below the unchanged
1.2x promotion gate. No candidate is selected. The negligible-correction variant
adds many derivative probes and is slower. All physical gates pass; exact saved
FPGA states/commands, contact identities and sample/application counts are retained.
All route profiling replays preserve every saved physical field and full task
transition exactly. Default output/diagnostics match the preserved earlier build.

The combination records 29,595 Broyden attempts and 3,194 rejected/capped updates.
The subsequent outer-secant-lifetime experiment separates cap hits from unsafe
updates and measures different bounded caps while retaining default eight and all
physical checks. No browser speed claim is made for this native-only screen.
