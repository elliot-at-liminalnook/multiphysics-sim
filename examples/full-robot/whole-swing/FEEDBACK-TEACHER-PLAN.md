# Restore the teacher's declared force observations

The two zero-network teacher cases were rejected on their first controller
call: the student profile had explicitly disabled floor-force observations,
which the teacher uses to gate its standing correction. `teacher-status.json`
preserves both failures; no gait acceptance is inferred from them.

Repeat those two cases with `task_observations.floor_forces = true`, the
existing teacher's declared observation setting. This exposes actual simulated
contact forces to the teacher; it changes no force law or physical property.
Keep all other recipes and physical/numerical gates from TEACHER-PLAN.md.
The comparison diagnoses privileged feedback and is not hardware deployment.
