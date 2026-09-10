# Physics-guided gait exploration

Started 2026-09-08 11:45:36 UTC. Decision checkpoint by 12:15:36 UTC;
hard stop at 12:45:36 UTC for user review. Target sustained speed >=12.5 mm/s;
explore 25–100 mm/s where supported. Preserve failures; no unattended large jobs.

CAD r1357 and the existing Rust runtime remain authoritative. All angle grids,
controller intervals, gait schedules, and contact variants are experimental
hypotheses, not new hardware properties. Original heading WIP is preserved in
`/Users/elliot/physics-simulator`; this work is on `physics-gait-exploration`.

1. Batch coordinated kinematic screening through shared rigid closure and
   independent sampled inter-link geometry. Retain solver errors and authored
   limit violations separately. Inspect marker Jacobians for useful foot travel.
2. Derive three coordination families: long-stride wave crawl, continuous
   overlap crawl, and dynamic paired support. Use reach, force/Jacobian estimates,
   support geometry, and momentum to estimate feasible stride and timing.
3. Before short physical tests, freeze recipes and acceptance criteria. Initial
   screening: >=12.5 mm/s actual translation; no inter-link sampled penetration;
   heading error <=0.02 rad, tilt <=0.1 rad, no persistent unintended body ground
   contact; report loaded material slip, stopping, torque/speed/work and contact.
   These are preliminary screens, not minute-long robustness qualification.
4. Benchmark actual rollout wall time before learning. Reuse existing Rust
   learning/distillation and observation contracts; identify privileged inputs
   and expand authority only within tested envelopes. A CLF reward is no proof.
5. Full success still requires sustained steering/stop/slip acceptance, timestep
   sensitivity, learned-controller work, exact replay/presets and >=1x browser
   realtime at 50 Hz with p95 transition <=20 ms. A kinematic screen is incomplete.

Reference: https://arxiv.org/abs/2601.06286. Adapt physics-guided references and
teacher/student structure to this quadruped; do not assume humanoid LIP formulas
or point-support stability transfer unchanged.

Implementation diagnostics retained: first workspace invocation rejected its
input before any samples because the new wrapper confused reduced velocity
count (including six base velocities) with independent joint position count.
Corrected to `independent_joint_indices().len()`; the closure solver was unchanged.
The existing offline marker planner also inspected collision forces, which could
omit inter-link geometry under a reduced force profile. Its inspection now uses
the same independent geometric audit as online stepping; the midpoint collision
regression covers both force settings.
