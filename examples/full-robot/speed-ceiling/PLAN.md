# Speed ceiling and gait optimization

The user removed the earlier one-hour boundary. This work continues from
`df669b0`; the older fast-wasd review stop is superseded. The validated baseline
is approximately 0.063 m/s with responsive forward/reverse/walking turns.

1. Calculate conditional CAD/actuator speed bounds before new gait tuning.
   Distinguish rate budgets, loaded dynamic feasibility, and unknown hardware
   limits. No-load motor speed is not a hard backdrive limit. Static traction
   bounds acceleration, not steady drag; motor power alone does not give a
   terminal walking speed.
2. Use the resulting limiting joints and phases to explore foot-return timing,
   posture, belt/worm sharing, stance allocation, and dynamic feedback. Preserve
   the accepted controller's command lease, braking and reversal behavior.
3. Validate sustained speed, sampled CAD collision clearance, contact slip,
   tilt, actuator demand, timestep sensitivity, WASD and browser responsiveness.
   Keep rejected candidates and exact reproduction inputs.
4. Continue until physical constraints explain remaining speed limits. A local
   search plateau or passing a narrow gait test does not prove the global limit.

Physical definition remains the versioned CAD model. Experimental policy and
collision-derivation overrides remain explicit. Shared Rust computes mechanics;
JavaScript scripts only assemble inputs and summarize recorded results.
