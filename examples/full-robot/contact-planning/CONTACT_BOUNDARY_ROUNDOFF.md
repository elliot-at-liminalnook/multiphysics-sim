# Preserve valid touchdown samples during contact search

The mixed CEM pilot's `g0-trial-1` failed before trajectory optimization with
`return phase must be finite and in [0, 1]`. Its validated two-step contact
configuration was valid. At time 0.29655284396536696 s, subtraction across the
cycle boundary produced a normalized swing coordinate of 1.0000000000000002.

The shared multi-step sampler now bounds that derived interpolation coordinate
to [0, 1]. Validated nonoverlapping intervals and the phase partition establish
the mathematical range. The external `SmoothReturn` input contract still rejects
out-of-range phases; actuator, collision and contact constraints are unchanged.
The legacy single-step sampling path is unchanged.

The regression test failed before the change and passes afterward. All 20
shared control tests and all 15 contact-planning tests pass. The audit example
samples every touchdown/liftoff at the endpoint and ±1/±2 floating-point steps,
plus a uniform grid over four periods. For the actual failed search input:

| Outcome | Before | After |
|---|---:|---:|
| Successful samples | 4,482 | 4,488 |
| Sampling errors | 6 | 0 |
| Previously successful samples changed | — | 0 |

The complete original failed trial and both audit reports are retained in
[contact-boundary-evidence.json](contact-boundary-evidence.json), with verified
gzip restoration and SHA-256 identities. The regression motion and
[comparison](contact-boundary-comparison.json) are separately versioned.

`run_boundary_replay.mjs` replays the original recipe and search budget using
the independently built `optimize_joint_ipopt_events` executable. Its output
directory is `runs/contact-boundary-replay`. The first corrected evaluation
reached the physics planner: nominal speed 0.041118 m/s, maximum inequality
13.548976, infeasible. This confirms that the sampler no longer rejects the
candidate before evaluation; it does not qualify a gait. The solve is ongoing.
The older CEM run retains its pinned executable and original failure observation;
this replay must not be inserted into that older measurement context.

Separately, the full 20-second measured controller search's initial LHS trial
009 reached 0.0691668524/0.0692228645 m/s, 1.8263% loaded slip, 80/80 required
lifts and zero overlap at the 1,001 audited poses. Turn response was 0.21464 rad.
The `human20-lhs70` validation family preserves this controller and requested
speed for 1.25/0.625/0.3125 ms timestep and command-dropout checks. These checks
are ongoing; no browser promotion or physical maximum is claimed.
