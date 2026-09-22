# Actual browser worker CPU sampling

The shared profile-worker-cpu.mjs harness attaches Chrome CPU sampling to the
actual dedicated Rust/WASM worker after model construction. It uses the same
three-second recipe/actions and the preserved force-output bundle on port 4192.
Sampling requests 1000 us intervals; actual intervals vary. Raw CDP profile and
Chrome version/target metadata are retained. No physics source changes were
needed to gather this evidence.

19,501 samples cover 28.102 weighted seconds, including 2.145 s idle. The replay
itself reports 26.005 s with profiling active; this is not throughput acceptance.
All 151 saved physical frames and full task transitions are exactly equal to the
unprofiled reference. Input hashes and the actual packaged WASM identity are
recorded. analyze.mjs verifies parity and aggregates self/inclusive sample weights.

The generic masked matrix-multiply fallback is the largest non-idle leaf at
2.627 s (9.35% of all weighted samples). Most sampled stacks lead to reduced
inertia projection; closure-direction checks are another caller. Allocation
malloc/free and related allocator routines are also prominent. Kinematics has
2.074 s self weight and full rigid inertia 1.474 s. Inclusive times overlap;
sampled weights are estimates, not allocation counts or exact wall breakdowns.

This motivates shared unpacked finite matrix kernels in direct-dense-products/,
with full physical qualification required. The sample does not prove an upper
speed limit or imply realtime is close. Initialization is excluded, but protocol
round trips and post-replay capture transfer before Profiler.stop can contribute
idle time. Keep the raw profile with the build manifest and copied harness.
