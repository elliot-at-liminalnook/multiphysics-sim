# Exact block factorization of mechanism closure

The swing-foot controller profile spends 85.4 of 173.6 stepping seconds in
closure mapping, including 27.9 seconds factoring the dependent matrix. The
matrix contains all dependent coordinates, although several mechanisms can have
independent equations. This experiment asks whether smaller exact factorizations
reduce that cost while preserving full physical trajectories.

`EmbeddingConfig.block_dependent_factorization` defaults to false. When enabled,
the shared Rust embedding partitions each freshly assembled dependent matrix by
its exact nonzero entries. Each row connects every column with a nonzero entry;
connected components define independent blocks. No magnitude threshold discards
small couplings, and no pose's partition is reused at another pose.

Both SVD and pivoted-QR solve paths are supported. The minimum and maximum
singular values across all blocks determine the original global rank threshold.
A block that looks well conditioned by itself can therefore still fail the
whole-matrix threshold. Zero coefficient rows are absent from the factorization
but every original position, velocity, acceleration and tangent closure check
remains. This changes numerical linear algebra, not mechanism physics or the
operating envelope. It does not authorize stepping across a singularity.

Focused tests cover exact partition changes, tiny nonzero couplings, multi-RHS
least-squares equivalence including a zero row, global rank rejection, moving
linkage poses, floating-base velocities, and toggle rejection. The original
embedding and integration tests remain active. The trajectory comparison tool
records this numerical option explicitly while retaining its physical-source,
policy, world and tolerance checks.

The first complete 2.8 s robot run preserves the sampled physical trajectory:
maximum marker disagreement is 1.44e-12 m, maximum joint-angle disagreement
1.44e-11 rad, and maximum current disagreement 1.51e-9 A. Sampled contact pairs
match, and supported lift still qualifies for 200 ms with 3.211 mm peak clearance.
The matrices are algebraically equivalent; serialized floating-point frames are
not bit-identical. These comparisons do not improve the model's hardware accuracy.

Closure factorization costs 12.07 s versus 27.89 s in the previous profile;
closure SVD costs 5.58 s versus 18.03 s. The complete candidate run costs 152.18 s.
The fresh same-binary reference costs 159.05 s, giving only **1.045×** observed
speedup for the pair. Its closure factorization costs 25.89 s. The older 173.6 s
reference overstates the apparent gain; these single development runs do not
establish an isolated repeatable performance multiplier. The default path still
reproduces every earlier sampled physical/controller frame exactly.

At 0.125 ms, the candidate completes in 232.46 s and retains the 210 ms supported
lift result. Its maximum foot-position difference from the old factorization is
4.36e-8 m (43.6 nm), contact-impulse difference 1.033e-4 N·s, and ordered event-time
difference 2.545e-6 s. Event counts and sampled contact pairs match. These exceed
the strict diagnostic gates of 1e-9 m, 1e-7 N·s and 1e-8 s respectively; the
status report records that failure instead of relaxing the gates after the run.
One motor current differs by 0.041 A at the final 2.8 s sample
(0.395 versus 0.436 A); maximum sampled heating-power disagreement is 0.126 W.
Those internal-state differences are another reason not to infer full-system
equivalence from nearly identical feet. The largest foot-path timestep difference
remains 2.1774 mm. No improved physical
accuracy, timestep convergence, or default promotion is claimed.

All original sampled closure rows remain within the configured tolerances.
The mathematical block decomposition remains exact for each current matrix;
rounding and event-location differences can still accumulate in full dynamics.
The option stays experimental and defaults to false.

Browser packaging disables only native process-global profiling, which browser
sessions explicitly reject. The robot, controller, solver and factorization
settings otherwise match the native benchmark. All 18 UI checks pass, including
the new solver preset and existing controllers; browser execution completes with exact replay/reset, but the strict native/WASM
entry comparison fails one force reading: 1.01283e-7 N difference versus the
1e-7 absolute limit at 0.03 s. All other sampled fields are within that limit.
The threshold is unchanged. Browser execution takes 152.77 s; the largest worker
chunk takes 1.98 s. These are separate development timings, not a native/browser
speed comparison.

This experiment is **not promoted**. The previously passing live viewer and
shareable point-feedback bundle remain installed. The isolated experimental
bundle is retained under `runs/interactive/block-factor/viewer` for investigation;
it is not substituted for the working browser deliverable. The status file
records both the refined-state and browser-parity gate failures. A mathematically
exact decomposition does not by itself prove equivalent integrated dynamics.

The next performance experiment should target avoided Jacobian rebuilds and
residual evaluations, rather than refining this modest factorization speedup.
In particular, `MotorStepper::jump` currently clears the numerical workspace at
every event, including controller samples. Investigate whether a guarded refresh
policy can retain useful matrices through selected controller events while still
refreshing on changed dynamics or failed convergence. This is a research lead,
not an implemented or validated relaxation.

## Reproduce

Prepare the point-feedback experiment first, then:

```sh
cargo test --locked -p sim-domain-robot --lib --test embedding --test embedded_step
cargo test --locked -p sim-runtime --example compare_embedding
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
node examples/full-robot/prepare_block_factor.mjs
```

Run the same release `integrate_embedding` binary with
`point-feedback/scene.json` and each of `block-factor/config.json` and
`block-factor/reference.config.json`. Save both complete captures. Run
`compare_embedding` on candidate versus reference with `foot-markers.json`;
run `evaluate_lift` with the same scene and `forward-slow/lift-requirements.json`
using `--simulation-time`. The reference config differs only in the opt-in
factorization flag. Both runs enable profiling; overlapping timer buckets must
not be summed. Retain individual run timings and source identities.
