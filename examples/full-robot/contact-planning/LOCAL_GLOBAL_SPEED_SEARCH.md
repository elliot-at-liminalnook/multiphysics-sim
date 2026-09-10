# Adaptive local and global speed search

The global prediction-guided search completed twelve new proposals: nine
20-second prefixes completed and three fell. Its strongest new estimate was
0.081825 m/s, below the baseline's 0.512784 m/s estimate. The first proposal's
full 300-second test completed without falling at 0.000403 m/s, versus its
0.001625 m/s estimate. These are separate seed 2301 prefix and seed 1901 full
measurements; they show no speed improvement or calibrated uncertainty bound.

The search needs to investigate the neighborhood of a known working gait while
continuing broader exploration. [TuRBO](https://arxiv.org/abs/1910.01739) uses
local models and trust regions to address excessive global exploration.
[TREGO](https://arxiv.org/abs/2101.06808) combines local and global EGO steps.
The [TuRBO reference implementation](https://github.com/uber-research/TuRBO/blob/master/turbo/turbo_1.py)
adapts region size using consecutive successes/failures. These motivate the
new shared Rust adapter; it is not a reproduction of either algorithm, and
neither paper's convergence claims are transferred to this experiment.

`bayesian::suggest_in_region` retains all completed observations for the global
GP but restricts acquisition optimization to a caller-declared normalized region.
The physical parameter domain, measurement definition and observed feasibility
remain unchanged. The initial implementation revealed a backend issue: midpoint
starts derived from global data could lie outside a local region. The adapter
now selects in-region acquisition starts for local calls; global calls retain
their prior behavior. Returned points are checked against the requested region.
The original failing tests and source are preserved.

`local_global::suggest` serializes its region state and complete observation
prefix. Continuation requires exactly the pending point's new observation and
unchanged context/history. Any improvement may move the incumbent center.
Three sufficient improvements double the radius; eight unsuccessful outcomes
halve it. A sufficient improvement for radius adaptation is 0.1% of incumbent
objective magnitude; this does not change candidate ranking. Every fourth
proposal uses the full search domain. Falling below the minimum radius resets
the radius and requests a global proposal rather than stopping the speed goal.
Failures remain failed observations without invented speeds.

This is one isotropic acquisition region with a global GP and constrained LogEI.
It omits TuRBO's local-model allocation, lengthscale-shaped regions and Thompson
sampling, and TREGO's specific sufficient-decrease framework. The current recipe
starts at normalized half-width 0.05, can expand to 0.8, and resets below 1/256.
Those are optimizer settings, not robot limits or an exhaustion certificate.

Four focused tests cover region bounds with outside-region training data,
quadratic improvement/expansion/global steps, failed-outcome contraction/reset,
and exact continuation/replay rejection. Four existing Bayesian tests also pass.
The CLI builds in the development profile; simulator executables remain the
same pinned release binaries. A missing serialization derive in the first CLI
build was corrected; the failed build is preserved.

`run_local_global_planar_speed.mjs` reuses the shared materialization, runtime and
heading-trend predictor. It starts from all 29 prior observations: 20 completed
prefixes and 9 failures. The first local proposal retains all 20 training rows and
lies inside its requested region. Its completed 20-second prefix averages
0.460898 m/s, but predicts only 0.023499 m/s over 300 seconds as it turns. It is not
a speed improvement. Later proposals consume the resulting observations and
persist the region state for continuation.

The batch `runs/local-global-planar20-v1` completed all twelve initial update
slots: eleven completed prefixes and one sampled fall, with periodic global
proposals. None improved the initial best predicted 300-second speed. The recipe is
`runs/local-global-planar-inputs-v1/experiment.json`. The older full-horizon
amplitude search received STOP and finished its current evaluations, preserving
all eleven outcomes. All four independent phase trials also finished without a
speed improvement. These finite experiments do not exhaust gait space.

The stronger measured controller for subsequent optimization is now the neural
actor: 0.520433 m/s over 300 seconds at 0.15625 ms, versus 0.520155 m/s at 0.3125 ms.
The 0.0534% net-speed difference is encouraging, but its endpoint positions differ
by 34.31 m. This is scalar metric agreement across two runs, not full trajectory
convergence or hardware validation. See `LONG_HORIZON_FIDELITY.md`.

`local-global-speed-search-v1.json` records the experiment transition;
`local-global-speed-evidence-v1.json` preserves terminal evidence and immutable
live inputs. The physical speed maximum and a new model-guided speed gain remain
unproven.

The integration check confirms exact state persistence, consumption of the
new physical prefix outcome by the next proposal, and a fourth-step return to
the full parameter domain. This verifies the allocation loop; it does not
qualify any new gait's full-duration speed.

Candidate 006 reached 0.5299044574 m/s over 20 seconds but predicted substantial
long-run circling. Separate measured steering-response fits proposed a correction
that raised its 20-second net speed to 0.5370313245 m/s and nearly removed mean
turning. The alternative feedforward arm failed its extrapolation. Full-duration
and finer-step validations remain necessary; see `STEERING_RESPONSE.md` and
`steering-response-evidence-v1.json` for the completed batch and follow-up evidence.
