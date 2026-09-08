# Resolve the faster gait's numerical margin

The 20/5 ms teacher pair passes the task but fails the 1 mm foot / 0.5 mm body
trajectory screens. Evaluate the same teacher at 10 and 2.5 ms physics steps,
and the same 85% finish student at 10 ms. Preserve 50 Hz controller sampling,
24-second inputs, seed 0, CAD physics, solver tolerances, motor bounds and task.

Compare student 10/5 ms, teacher 10/5 ms, and teacher 5/2.5 ms at the existing
20 ms report times. Every compared case must independently pass the unchanged
task. Require maximum foot/body differences <=1/0.5 mm; retain failures.
Finer timesteps may reduce numerical error while costing more computation.
Native throughput is diagnostic only; no browser performance is inferred.

This is a convergence investigation, not a change to acceptance budgets or a
claim that either timestep is a calibrated physical reference. The detailed
mechanism model remains retained independently of these effective-servo trials.
