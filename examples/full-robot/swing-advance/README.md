# Advance the body while a foot swings

The measured 2.5 mm/s student's minute spends 31.16 seconds in raise/lower
with a stationary planned body. Its horizontal body path is 1.631 m but net
advance is 0.141 m. Shift/return account for 1.432 m of that path; there are no
support waits in the measured baseline. The planner, rather than support
waiting alone, therefore imposes substantial stop/start motion.

This experiment uses the shared Rust sequence's optional
`swing_body_advance_fraction` in [0,1]. It moves that fraction of the commanded
planar advance along a smooth profile spanning raise and lower. It retains the
support shift until landing, then removes it during return. Feet, cycle
endpoints, support qualification and ordinary CAD inverse kinematics and
geometry checks remain authoritative. Zero is the original sequence. Translation
and heading use the same fraction; this does not directly move physical poses.

Before evaluation the development targets are 2.5, 3.75 and 5 mm/s, with fractions
0, 0.5 and 1 at 20 and 5 ms physics and 50 Hz control. These are feasibility
targets, not measured capabilities. The 1.38-second transfer cycle commands
3.45, 5.175 and 6.9 mm of body advance respectively. For the four-foot cycle,
the leading foot's nominal forward travel is 13.8, 20.7 and 27.6 mm before the
configured stance offset. Prior sequential trials already exposed geometry
interference at larger strides; overlapping body motion tests whether that
relative workspace limit can be reduced without changing the CAD mechanism.

The same selected student weights and 18/9 mm posture are used. This is outside
the previous network's command training range. CAD motor capabilities are
unchanged: the effective-servo profile derives 2.942 Nm stall torque and
5.512 rad/s no-load speed, with uncalibrated stiffness/damping. The recorded
baseline peaks at 0.972 Nm and 2.347 rad/s; these separate maxima do not imply
that both limits are simultaneously available or that hardware is calibrated.

The 24-second physical gates and 1 mm foot / 0.5 mm body timestep screens remain
those declared in `../hybrid-speed/README.md`. Every outcome is retained, including
geometry rejection. Only a candidate passing these short development checks
should proceed to sustained motion, command changes, held-out disturbances and
browser performance. This study does not waive the requirement for those gates.

```sh
node examples/full-robot/swing-advance/prepare.mjs
node examples/full-robot/hybrid-speed/run.mjs runs/full-robot/learning/swing-advance examples/full-robot/swing-advance/status.json
```

Build the current `run_environment` and `evaluate_lift` release examples first.
The runner refuses to overwrite captures. All generated recipes come from
versioned CAD-derived scenes, controller weights and this declared generator.
