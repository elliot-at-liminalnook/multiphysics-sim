# Screened gait search — 2026-09-23

Follow-up to `../gait-search-comparison-2026-09-19`, whose audit found that
111 of 198 attempts (56%) failed during preparation. They cost only about 12% of
wall time, but each consumed a trial slot. Bayesian optimization's 27 full-domain
initial-design points were all infeasible, and it re-proposed next to known
failures because it learns only from completed runs.

## What changed (shared library, not study scripts)

1. **Schedule screen first.** `contact_reference::pause_windows` is the
   all-stance check the pause-based stopping controller needs. It now runs
   before any kinematics in `contact_reference::compile`, and
   `contact_exploration::Recipe::schedule_screen` exposes it for proposals
   (microseconds instead of about 21 s). On the recorded study it rejects all
   20 stop-controller failures, plus 9 reach failures, and none of the 87 gaits
   that completed (`tests/gait_search_screening.rs`).
2. **Screened proposals.** `search_comparison::propose_until_prepared` proposes
   candidates, schedule-screens them, and prepares up to `parallel` of them at
   once. It accepts the first to pass in proposal order, so results do not
   depend on thread timing (tested equal to one at a time). Rejected candidates
   join the optimizer history (both optimizers learn from them), are written to
   `screened.json` and do **not** consume a trial slot. Only physics trials are
   charged.
3. **Feasibility-aware Bayesian proposals.** `search_comparison::feasibility`
   estimates the probability that a point is feasible from every observation
   (kernel average, uniform prior). Bayesian bootstrap and acquisition draw
   `feasibility_candidates` proposals and use the most likely feasible one.
4. **Tighter space.** From the earlier study's evidence: body height
   ≤ +5 mm (every attempt above +3 mm failed), cadence scale ≤ 1.0 (none above
   0.98 completed), stance fraction ≤ 0.75 (none above 0.71 completed). Phases
   stay free but are schedule-screened.
5. **Coarse speed pre-screen.** Before the full compile, a compile at 1/8 of the
   samples rejects the candidate if a joint's reference speed already exceeds
   its limit by 10%. A coarse compile that fails is ignored; only the full
   compile can reject on kinematics.
6. **Governor limits are searched.** `governor_speed_rad_s` (1.40–5.5) and
   `governor_acceleration_rad_s2` (6.98–60) are bound into the controller's
   reference governor through `policy_bindings`. The earlier study fixed them at
   80°/s and 400°/s², about a quarter of the servo's no-load speed. Tracking and
   stability gates are unchanged, so faster governors are only accepted if they
   still track.

`make-config.py` builds `comparison-config.json` from the earlier config; the
physics, controller code, gates, qualification and baseline are unchanged
(attempt 0 is the qualified baseline). Seed 3301, 33 trials per algorithm, up to
48 screened candidates per trial, 8 in parallel.

The reduced-model fidelity was qualified at the baseline governor. Faster
governors are outside that qualification, so any finalist needs the detailed
model and longer episodes before it counts.

## Run

```sh
cargo build --release -p sim-runtime --features evolution --example compare_gait_search
target/release/examples/compare_gait_search \
  examples/full-robot/measured-actuator-integration/gait-search-screened-2026-09-23/comparison-config.json \
  examples/full-robot/measured-actuator-integration/gait-search-screened-2026-09-23/comparison 66
```

Audit the screens against recorded attempts (no physics trials):

```sh
target/release/examples/audit_gait_screens <comparison-config.json> <study-dir> 8 1.1 8 > audit.json
```

## Results (completed 2026-09-23, reduced model, seed 3301)

| | CMA-ES | Bayesian |
|---|---|---|
| attempts | 33 | 33 |
| failed attempts | **0** | 12 (no candidate passed preparation within 48 screened proposals) |
| screened candidates (never charged a trial) | 43 | 634 |
| wall hours | 0.95 | 0.84 (0.86 h preparing screened candidates, in parallel) |
| best speed | 0.121 m/s (+0.021 over baseline) | **0.142 m/s** (+0.042) |

Earlier study (`../gait-search-comparison-2026-09-19`): 111 of 198 attempts
(56%) failed during preparation. Here CMA-ES lost no trial slots at all.
Bayesian's 12 failures are exhausted screening budgets: the proposals were all
rejected by the schedule screen, and the failures show where the feasible
region ends. Its best objective (attempt 23) used the lowest governor speed
(1.40 rad/s) with 20 rad/s² acceleration. So the faster governor range did not
pay off under the unchanged tracking gates.

Caveats:

- Speeds are from the reduced model. It was qualified at the baseline governor
  only, so finalists still need detailed-model validation before they count.
- The simulated characterization campaign's gait patch
  (`../../../actuators/hx30hm/hardware/characterization-campaign/`) would cap
  the governor at 2.98 rad/s. That cap is simulation-derived and not applied
  here.
