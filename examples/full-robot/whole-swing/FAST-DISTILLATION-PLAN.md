# Distill the faster validated teacher

The latest exact-work optimization still misses rendered latency, and the
20 ms student trajectory still misses the existing refinement screen. Stop
treating small solver changes as a sufficient route to the requested walking
outcome. Continue the learning work from the retained 1.25 ms teacher, whose
minute-long travel is approximately 3.75 mm/s and whose 0.625 ms comparison
passes the existing foot/body trajectory budgets.

Use the existing shared Rust neural policy and distillation components.
Generalize dataset preparation so it consumes pinned teacher recipes and
captures, rather than silently selecting the older slow robot/controller.
Keep the robot's physical definition, actuator bounds and policy sampling
contract identical. The student replaces the teacher's privileged motor
corrections with learned residuals beyond reference plus local joint tracking.
Compute labels with the recorded tracking-gain channel, not a hardcoded gain.
Explicitly distinguish commanded targets from clipped/applied targets.

Use the existing fine minute and mixed-steering demonstrations for development
training. First create and independently evaluate a separate fine teacher
episode with a changed forward/turn/reverse/stop sequence; preserve its exact
recipe, seed and input events. Hold that complete episode out of training and
checkpoint selection. Fit with the existing deterministic Rust optimizer,
select by training loss only, and retain the initial and fitted networks,
typed feature/output definitions and physical-unit imitation errors.

Start with the existing encoder, joint-reference, gravity-direction, angular
velocity and motion-command features. These are ideal simulated signals,
not calibrated hardware sensors. The planner still uses privileged kinematics
and support information. Record this boundary explicitly; neither a student
network nor a lower imitation loss establishes deployability. If missing
history or unobserved body translation prevents successful feedback, measure
that limitation before extending shared observation components.

Evaluate the fitted student closed-loop on the original development minute,
mixed steering and separate held-out episode. Apply the unchanged supported
swing, collision, tilt, heading, stopping and trajectory budgets. Report actual
speed, energy, slip and failures. Keep the detailed teacher as the accuracy
reference; test browser fidelity separately with exact native/WASM replay and
the original >=1 pace / <=20 ms p95 requirements. Preserve failed students.

Only after the first fit and closed-loop results, predeclare a distinct
robustness-training set and an untouched evaluation set with explicit world
pushes and progressively harder terrain. A seed without any randomized input
is not a different robustness case. Do not alter CAD properties or invent
uncertainty ranges to make the controller pass. Keep all thresholds fixed and
retain both reliable slower and faster experimental policies on the leaderboard.
