# Shared unpacked finite dense products

The browser CPU sample in ../browser-cpu-profile identifies generic masked GEMM
as a major leaf, especially in reduced inertia projection and closure-direction
checks. sim-solve::dense_products provides checked A B and A-transpose B products
for small dense matrices containing many exact zeros. They avoid packing buffers
and transpose materialization. Every input is checked for finiteness before any
zero skip; tiny nonzero coefficients are retained. Nonfinite products are rejected.
The kernels are portable Rust and expose a focused documented example.

EmbeddingConfig.direct_dense_products enables the shared kernels for the original
J T closure-direction check and the original (T-transpose M) T inertia projection.
It defaults false. Matrix association, rank checks, closure rows, physical forces,
solver tolerance and timestep stay unchanged. Accumulation order can differ from
platform GEMM, so physical parity gates are required; no exact-bit claim is assumed
for this experimental path. Original invalid-input and dynamics checks remain.

3 kernel unit tests and the documented example pass, including rectangular/empty
matrices, comparison with independent GEMM, tiny coefficients, nonfinite operands
hidden behind zeros, and overflow. 108 robot tests pass. Extended cases exercise
both paths against analytic pendulum gravity/inertia, independent full constrained
KKT dynamics and free-base contact/force balance. Source snapshots use complete
relative paths to avoid basename collisions between source and test files.

The default, direct-products option and direct-products plus existing analytic
motion are measured separately. Original input/binary hashes and qualification
gates are retained. Optional paths remain experiments until their actual native
and browser measurements support promotion. The selected recipe is unchanged.

## Rejected measurements and source restoration

| Route | Native wall seconds | Browser worker wall seconds |
| --- | ---: | ---: |
| Current default | 22.810525 | 25.714915 |
| Direct products | 23.946855 | 25.612125 |
| Direct products + analytic motion | 21.232350 | 23.796865 |

Every run simulates three seconds. No route reaches the unchanged 1.2x promotion
gate or realtime/latency gates. Direct products are slower natively and essentially
flat in this browser pair; the combination reaches only 1.074x native and 1.081x
browser speedup. These are ordered single trials, not repeated-machine estimates.
The freshly replayed preserved browser bundle takes 26.050920 s.

All physical gates pass with roundoff-sized changes. Maximum native differences
across optional routes are 3.508e-14 rad, 1.003e-14 m, 2.760e-13 A and 2.397e-10 N.
Contact identities and controller cadence agree. Default output, complete task
transitions and retained solver diagnostics match the preserved binary exactly.
Every route's profiling replay also preserves physical frames and transitions
exactly. All 151 native/worker frame fields and full task transitions agree within
1e-7; old/current default workers agree exactly.

A second actual-worker CPU sample uses direct products. It preserves every saved
field and task transition exactly. The replacement product and transpose-product
kernels themselves consume 1.627 and 1.469 sampled seconds, respectively. The old
profile had 2.627 s in the generic masked kernel plus its supporting GEMM work.
The replacement shifts cost without providing a useful total-runtime reduction.
Sample weights are approximate; inclusive callers overlap, and profiled wall
timing is not performance acceptance.

Because this change did not qualify, all experiment-owned simulation source/test
edits were restored byte-for-byte from source-before. The new active library file
was removed after verifying its candidate hash. No unrelated edits were reverted.
Candidate sources, native binary, WASM artifact, build manifests, raw captures,
CPU samples and qualification receipts remain available for inspection/replay.
The profile-worker-cpu.mjs inspection tool is retained. See disposition.json.
Archived configs requiring direct_dense_products must use candidate-native.bin
or the preserved candidate WASM; that option is absent from restored active code.
The selected recipe and the port-4192 bundle are unchanged.

The restored source rebuilt successfully. Its complete three-second replay takes
22.556381 s and exactly matches all 151 original physical frames and all task
transitions. verify-restored.mjs reproduces that check. This is restoration
verification, not a new optimization claim. No build or benchmark is pending.
