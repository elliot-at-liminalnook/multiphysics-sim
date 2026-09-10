# Fast human-controlled gait, second bounded session

Objective: as fast as physically supported, with eventual human WASD control on
the real robot. Started 2026-09-08 13:50:45 UTC. Checkpoint by 14:20:45;
hard review stop by 14:50:45. Preserve the previous gait and original user WIP.

Prior evidence: 25–27 mm/s paired gait, 20 s forward/turn/reverse/stop; excessive
slip and 73 ms rendered p95. Four rejected longer-stride points are outside exact
CAD solids by about 0.261 mm. Full-grid refinement was too costly for the loop.

1. Reusable local CAD distance refinements with smooth boundaries, bounded export
   cost, analytic sampling checks, exact CAD probes and original geometry outside
   each patch. No collision exclusion waiver or changes to robot mass/actuation.
2. Compare longer strides and faster cadence at 50–100 mm/s and beyond only where
   measured actuation/contact permits. Retain rejected trials and geometry audits.
3. Human command quality is part of speed: W/S translate, A/D steer; release,
   reversal and lost commands must have explicit controlled behavior. Distinguish
   normal stopping from emergency hardware disable. No hardware commands here.
4. Prospectively screen directed speed, turn sign, stopping, loaded material slip,
   clearance/contact, tilt, torque/speed/work and timestep sensitivity. Retain the
   5% slip and 20 ms p95/1x browser gates. Bigger requested speed or falling motion
   cannot count as faster walking. Record simulation and wall command response.
5. Deliver the fastest validated development preset and replay with remaining
   limits. No unbounded learning job or sim-to-real qualification claim.
