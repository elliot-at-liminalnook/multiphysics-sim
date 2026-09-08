# Faster student with geometric teacher feedback

The selected 2.5 mm/s student completes supported swings but drifts beyond the
0.005 rad sustained heading limit. Its inputs contain angular velocity and
gravity direction, but no absolute heading error. This development experiment
adds the existing Rust world-foot Jacobian feedback to its motor commands.
Planted-foot errors can then oppose accumulated body rotation as well as
translation. This is a privileged hybrid teacher; it is not a deployable sensor
policy or a newly distilled network.

Before evaluation, the declared sweep is point gain 0.1, 0.25 and 0.5, at 20 and
5 ms physics, over 24 and 60 seconds. Control stays at 50 Hz. The same selected
neural weights, gait, CAD physical properties, effective motor limits, geometry
checks, lateral disturbance and ordinary command validation apply. A zero-gain
minute at each timestep rechecks the current-runtime baseline. The old student
remains runnable. All outcomes, including incomplete physics runs, are retained.

Acceptance is the existing independent executed-step audit: at least four
transfers, each with at least 200 ms of simultaneous 1 mm clearance, swing force
at most 0.1 N and other-foot support at least 1 N; no sampled internal overlap;
body tilt at most 0.01 rad; final body error at most 1 mm and heading error at
most 0.005 rad; final phase idle. The numerical comparison retains the 1 mm foot
and 0.5 mm body screen. A refined comparison is not proof of convergence.
These conservative commissioning limits do not establish a dynamic-gait limit.

```sh
node examples/full-robot/hybrid-speed/prepare.mjs
node examples/full-robot/hybrid-speed/run.mjs
```

Run after building `run_environment` and `evaluate_lift` in release mode. The
runner refuses to overwrite captures. Use a fresh directory argument to both
commands for a new execution. Compact outcomes and exact input hashes are
versioned; ignored output is reproducible from the versioned scene, network,
generator and plan. Hardware calibration, terrain, held-out robustness, browser
timing and student distillation are separate gates, even if a native trial passes.
