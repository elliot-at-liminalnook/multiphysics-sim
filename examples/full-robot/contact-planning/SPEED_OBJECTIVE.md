# Speed discovery objective

Latest user instruction: maximize sustained net distance traveled per second
without falling. Search broadly without arbitrary gait, slip, lift, tracking,
steering, stopping or speed restrictions. This supersedes the earlier development
acceptance criteria when ranking speed-discovery experiments.

The `speed20` profile runs 20 seconds of continuous motion. Its objective is
negative horizontal chassis displacement magnitude divided by the complete
requested duration, including startup. It measures endpoint displacement rather
than total path length, so oscillation or walking in circles earns no extra speed.
The only acceptance residual is a detected fall. No rewards penalize effort,
lean, slip, tracking or contact patterns. Numerical failures remain missing
measurements, not invented zero speed.

The previous runtime task itself terminated at about 5.7 degrees tilt and within
a narrow body-height interval. The new task removes both development bounds and
uses the shared Rust environment's chassis ground-contact and overturning
observations. The measurement reducer additionally checks transformed CAD chassis
hull clearance at recorded poses. A chassis ground strike or overturning counts
as falling. Recorded poses occur every 20 ms; this is sampled fall detection,
not a guarantee about every physics substep. CAD masses, transmissions, geometry,
friction and actuator physics are unchanged.

`bayesian-speed-only-diagonal21.spec.json` starts from the existing fast diagonal
reference, with requested speed 0.05–0.6 m/s, tracking gain 0–3 and velocity-lead
factor 0–3 as initial numerical search windows. These are not physical limits or
acceptance caps; promising boundary solutions require expanding the search.
The pilot uses a seeded LHS initial design followed by constrained LogEI and an
independent LHS comparison. Other contact families still need exploration; this
fixed-reference pilot cannot establish a global maximum.

The first measured baseline covered 4.1404033963 m in 20 s, averaging
0.2070201698 m/s without a sampled fall. Minimum chassis ground clearance was
0.2920416895 m. Longer-distance and finer-timestep measurements are still needed.

The same seeded initial design then reached 5.4409952067 m (0.2720497603 m/s)
and 8.4789849707 m (0.4239492485 m/s), also without a sampled fall. A second
reference, recovered from the previously rejected short-trial 003, covered
6.7343140718 m (0.3367157036 m/s). It uses the same robot/world and actuator
parameters; three initial motor targets differ with the reference pose.
The generalized preparation script accepts an existing source prefix and a
fresh experiment name, preserving the original source artifacts.

The recovered candidate's finer 0.3125 ms run covered 6.7293111568 m
(0.3364655578 m/s), a 0.0743% speed change. The maximum chassis position
difference was 0.06878 m: speed agrees closely while the path differs.
Its 60-second run covered 20.0004781368 m (0.3333413023 m/s). The 0.424 candidate
covered 25.8815260889 m in 60 seconds (0.4313587681 m/s), and 8.4452278867 m
at the finer timestep (0.4222613943 m/s). Both completed without a sampled fall.
The finer 0.424 path differs by up to 0.37754 m despite close average speed.
These discrepancies are reported directly rather than reinstating the removed
heading/tracking criteria.

One higher-command trial stopped because its policy requested a foot-servo
target outside the configured command range. That is a controller-output
construction failure, not evidence that the robot cannot move faster. Explicit
command saturation within modeled actuator bounds is now being searched in an
explicit separate context. Its two-second replay passed the former failure at
1.28 s, retained all 64 earlier physical frames exactly, and replayed 100 policy
samples with zero command error. This is a controller adapter, not a change to
torque, joint or contact physics.
The old prescribed-stance planner searches were stopped and their checkpoints
retained for possible seed reuse, freeing compute for measured speed discovery.

The obsolete human20 slip-constrained pilot was intentionally stopped, with 14
complete trials and partial trial 014 preserved. Its driver is retained as
`run_bayesian_controller_screen-slip5-v2.mjs`. Completed 70/90 command validation
captures remain useful evidence; their historical pass/fail fields do not define
the current objective. The evidence archive is recorded separately by
`record_speed_objective_transition.mjs`.

Three analytic reducer tests cover net displacement, substantial permitted lean,
chassis contact/overturning, and early-fall scoring. They are included in the
Bayesian workflow. The native runtime and observation/action contract remain the
same path used by interactive execution.

Later 20-second trials reached 0.4495772362 m/s with the original reference and
0.4483370316 m/s with the recovered reference. The latter's finer-timestep trial
measured 0.4488880620 m/s; its minute-long validation covered 26.5874132596 m
(0.4431235543 m/s), without a sampled fall. Neither result establishes a physical
ceiling. Initial lead-factor search boundaries
still need expansion, and three-parameter searches do not exhaust gait space.

The 0.424 capture's offline compiled-geometry audit found a maximum sampled
overlap of 24.73 micrometres between a hip pulley/shaft and chassis. The runtime
profile currently omits inter-link contact. This is an approximation requiring
contact-enabled comparison, not an added zero-overlap score gate or an exact
continuous CAD collision certificate.

`NEURAL_SPEED_TRAINING.md` describes the first direct neural initialization from
the fast teachers, using the existing Rust imitation fitter and runtime.

The expanded recovered-reference search tests lead factors through 6 and
requested speeds through 1.2 m/s. Its baseline exactly reproduces 0.4483370316
m/s; lead 4.5 gives 0.4361469696 m/s, while lead 6 at the same speed/gain reaches
an invalid foot-servo request at 0.88 s. This remains a controller construction
failure, not a physical speed limit.

The explicitly saturated controller search has reached 0.4618210842 m/s in
20 seconds, with requested speed 0.3748256765 m/s, tracking gain effectively zero
and lead factor 2.1479788974 relative to its recorded source. The finer trial
measures 0.4617632315 m/s, but the minute averages 0.4259347586 m/s. Because zero
gain is an initial search boundary, a new
context explores gains down to -1 as well as larger leads and requested speeds.
Two declared seeds isolate negative gain at the measured best speed/lead.
All of these are search windows; none is an acceptance restriction or ceiling.

A later saturated trial reaches 0.4848248423 m/s over 20 seconds and
0.4843240717 m/s at the finer timestep, but only 0.4171781019 m/s over a minute.
It travels a longer path while curving more: path length/time is 0.5163972514 m/s
and body yaw changes by 2.2204 rad. Path length is diagnostic, not the objective.
Among those three candidates, the strongest minute is 0.4431235543 m/s. This ranking
change motivates optimizing the full minute directly rather than adding a
heading gate to short trials.

`run_bayesian_speed_search.mjs` accepts the source's explicit episode duration.
The new `bayesian-sustained60-fast.spec.json` starts a 60-second search from the
0.448 candidate with actuator-command saturation. It uses eight stratified
initial trials and 24 adaptive LogEI proposals, with no reused short-trial
scores. Its first baseline reproduces all 3,001 physical frames of the previous
minute exactly, at 0.4431235543 m/s. The existing 20-second studies remain separate
exploration evidence.

Enabling inter-link contact on the 0.462 candidate changes its two-second prefix
speed from 0.37385548 to 0.37382219 m/s, with maximum chassis position difference
0.13823 mm. The complete 20-second comparison measures 0.4618159136 m/s, with
maximum chassis position difference 3.9130 mm and no sampled fall. The
0.448 candidate reproduced its full-minute chassis trajectory and speed exactly
with inter-link contact enabled (0.4431235543 m/s).
These comparisons do not establish continuous CAD collision accuracy or hardware
calibration.

Both original 20-second reference studies are complete. Each adaptive arm and
random comparison has the same nine initial observations plus sixteen proposals.
Adaptive versus comparison best speeds are 0.449577/0.423949 m/s for the original
reference and 0.448337/0.419035 m/s for the recovered reference. This is evidence
from one seeded matched-count experiment per family, not statistical significance
or proof of exhaustion.

The expanded saturated search reached 0.5066601375 m/s over 20 seconds and
0.5088478470 m/s over 60 seconds, without a sampled fall: the new sustained
record is 30.53087082 m net displacement in one minute. The earlier 0.4964211531
short screen falls to 0.4073132590 m/s over 60 seconds. The 0.507 candidate's
half-timestep 20-second check gives 0.5065371751 m/s (0.0243% lower speed;
maximum body trajectory difference 0.25844 m). Its full inter-link-contact minute
gives 0.5088478055 m/s, with maximum chassis position difference 0.92077 mm.
These comparisons support the speed result without proving exact trajectories
or hardware transfer. `run_speed_validation.mjs` accepts `long-first` to
prioritize sustained performance while retaining the finer-step check.

`bayesian-sustained60-507.spec.json` continues from this candidate over full
minutes, expands the outer gain/lead windows and searches signed requested
speed. Search windows are not physical limits. No physical maximum is proven.
`speed-learning-task.json` exposes the same net-displacement objective and
sampled body-fall conditions directly in Rust for subsequent neural training;
its undiscounted reward is in metres and complete return/time is in m/s.

The completed sustained studies contain 33 and 34 observations. Their respective
best full-minute speeds are 0.4521967988 and **0.5168687755 m/s**. The latter is
the final evaluation (`evaluation-033`) of the expanded signed-speed search:
31.0121265317 m net displacement with no sampled fall. Its half-timestep minute
reaches 0.5160008921 m/s (0.168% lower), and its full inter-link-contact minute
reaches 0.5168688425 m/s. These are separate validation conditions, not a combined
finer-step/contact trial. No physical model limits were changed.

The first validated learned improvement reaches 0.5116049776 m/s on the previous
0.5088478470 teacher. Applying that actor to the new teacher yields
0.5158248388 m/s, so the new teacher alone remains the measured leader. See
`PREDICTIVE_SPEED_TRAINING.md` and `predictive-speed-gains-v1.json` for optimizer,
forecast and validation comparisons. The finite parameter studies do not
exhaust broader contact patterns, gait families or learned control; no physical
maximum is established. New full-horizon PPO branches remain active.

The first current-dynamics-only PPO update on that teacher reaches
**0.5171798423 m/s**, or 31.0307905350 m in one minute. It retains its lead at
half timestep (0.5166651163 m/s) and with inter-link contact enabled
(0.5171796510 m/s), without sampled falls. The separate learned-model
receding-horizon planner completes the minute at only 0.3407768323 m/s and is
not selected. `receding-forecast-study-v1.json` records both results and the
matched training-input comparison. Training and model-guided discovery continue;
this small speed improvement does not establish a physical ceiling.

The second update improves nominal speed to **0.5177078967 m/s**. Completed
full-minute finer-step and inter-link-contact checks reach 0.5190404374 and
0.5177078626 m/s without sampled falls. The nominal profile defines the common
optimization comparison; the finer result documents timestep sensitivity.
`learned-speed5177-validation.json` and its evidence manifest preserve the new
leader. Both tested receding-horizon planners remain slower and are not promoted.

The longer 300-second comparison reverses that ranking: the original
zero-correction teacher covers 154.3012690273 m (**0.5143375634 m/s**), while
the 0.5177078967 m/s minute-long neural actor covers 149.1968653738 m
(**0.4973228846 m/s**). Both complete without a sampled fall, with the same
physics and startup. The teacher is therefore the stronger baseline for further
sustained-speed work at this horizon. The neural result remains a measured
60-second improvement, not a demonstrated longer-horizon gain.
`sustained300-speed-comparison-v1.json` and
`independent-command-forecast-study-evidence-v1.json` preserve the comparison.
No heading, slip, trajectory-tracking or other gait-quality penalty was added.
The goal remains active; these outcomes do not establish the physical maximum.

The next campaign broadens the teacher's reference search from three to eight
parameters: signed speed, tracking gain, and independent belt/worm/foot amplitude
and derivative-lead factors. `AFFINE_SPEED_SEARCH.md` records the shared Rust
transformation, exact two-second physical parity, short Bayesian workflow checks
and pending300s search/fidelity evaluations. Short setup scores do not change the
five-minute baseline or establish a speed gain.

The subsequent timestep check makes that nominal ranking provisional. The
teacher averages **0.4854009535 m/s at 0.3125 ms**, versus **0.5143375634 m/s
at 0.625 ms**, over the same300s horizon without falling. Inter-link contact at
nominal timestep gives 0.5143375876 m/s. `LONG_HORIZON_FIDELITY.md` records the
5.626% timestep sensitivity and the running fine neural/third-resolution teacher
comparisons. Nominal search results remain discovery measurements; they do not
by themselves establish improved physical speed under refinement.

The matched fine neural run has now completed: **0.5201547734 m/s** over 300
seconds without a sampled fall, versus **0.4854009535 m/s** for the teacher at
that same 0.3125 ms timestep. This reverses their nominal ranking. Both are
being compared at 0.15625 ms; convergence and the physical maximum remain
unproven. `long-horizon-fidelity-v2.json` records the matched comparison.

The third-resolution teacher now completes 300 seconds at **0.4559723646 m/s**
without a sampled fall (0.15625 ms, 1,920,000 steps). This is another 6.063%
change from the fine teacher; convergence remains unresolved. The matching
neural test is still running. See `long-horizon-fidelity-v3.json`.

The matched third-resolution neural run now completes 300 seconds without a
sampled fall at **0.5204327404 m/s**, versus the teacher's **0.4559723646 m/s**.
Neural net speed differs by **0.0534%** between the two finer resolutions;
endpoint positions still differ by 34.31 m. The neural actor is the stronger
measured baseline at those resolutions. This is metric agreement, not a
physical speed ceiling or hardware-transfer claim. See
`long-horizon-fidelity-v4.json`.

Measured-response steering has now produced **0.5467083058 m/s over 300 seconds**
at the nominal 0.625 ms timestep (164.0124917518 m, zero sampled falls). The full
run exactly reproduces its prior 20-second physical prefix and slightly exceeds
its trajectory forecast. This is a new nominal sustained-speed result. Finer
full-duration validation remains unresolved; the separately calibrated fine root
is still running. The original neural actor retains the stronger prior evidence
of scalar agreement across the two finer timesteps. See `STEERING_RESPONSE.md`.

The separately calibrated finer-step steering controller now completes
300 seconds at **0.5468916069 m/s** (164.0674820674 m, 0.3125 ms, zero sampled
falls). A fixed-controller 0.15625 ms full run is active. The similarly fast
coarse result uses a different steering command, so numerical convergence is
not inferred from that pair. A slower ideal-heading feedback controller also
passes two 20-second prefixes and now has a fine full-duration trial. No speed
ceiling or new learned-neural speed improvement is claimed.

The heading-feedback controller now completes 300 seconds at **0.5468435312 m/s**
without a sampled fall at 0.3125 ms, essentially matching the constant-steering
winner. Joint nine-dimensional command/trajectory search is active; its first
promising candidate predicts 0.5490613651 m/s and is undergoing an unchanged
300-second physical test. Typed command bindings, legacy/physical parity checks,
closed prefix outcomes and the selection feedback loop are documented in
`JOINT_COMMAND_SEARCH.md` and archived in `joint-command-search-evidence-v1.json`.
The predicted improvement remains unqualified until that full test completes.

The fixed-controller third-resolution run now completes 300 seconds at
**0.5361838427 m/s** without a sampled fall. That is 1.9579% below the same
controller at 0.3125 ms; timestep convergence remains unresolved. An unchanged
heading-feedback third-resolution full run is active. A new shared composite
optimizer models translation/turning responses and propagates uncertainty
through the existing displacement prediction. All 46 solver library tests
pass; its first physical candidate test exposed underestimated turning and
did not improve speed. The scalar search's candidate 010 reaches 0.5584056911 m/s
over 20 seconds and is undergoing full300s validation. See
`COMPOSITE_SPEED_SEARCH.md` for measured results versus predictions.

The fine winner's matched inter-link-contact test now completes 300 seconds at
**0.5461336476 m/s** without a sampled fall, about 0.1386% below the same run
with inter-link contact omitted. `steering-contact-evidence-v1.json` preserves
the complete input/output comparison. This strengthens the candidate's contact
validation; timestep convergence and the physical maximum remain unresolved.

Joint-search candidate 002 now completes 300 seconds at **0.5512450674 m/s**
(165.3735202142 m, 0.3125 ms, zero sampled falls), with exact first20s replay
parity and full input/scene verification. Its unchanged finer-timestep test is
running. `joint-command-sustained-result-v1.json` records this new completed
best; `COMPOSITE_SPEED_SEARCH.md` separates it from the ongoing composite-model
prediction experiments. The physical maximum remains unproven.

Candidate 010 now completes 300 seconds at **0.5523818095 m/s** without a
sampled fall at 0.3125 ms, with verified scene/actions and exact prefix replay.
Its third-resolution test is active. The separate heading-feedback controller
reaches **0.5468228713 m/s** at 0.15625 ms, within 0.003778% of its fine speed;
its endpoint still differs by 3.7133 m. See
`joint-command-sustained-result-v2.json` for the updated completed results.

The composite-derived gait with measured steering adjustment now completes
300 seconds at **0.5621811105 m/s** (168.6543331506 m, 0.3125 ms, no sampled fall),
1.7740% above candidate010. Exact full input/scene and prefix replay checks pass.
Matched third-resolution and inter-link-contact trials are active. Candidate002's
third-resolution result is **0.5361298664 m/s**, 2.7420% below its fine result;
fidelity sensitivity remains material. A reusable automatic composite loop now
updates its trajectory-response model after every physical prefix and has started
from 20 frozen observations. See `COMPOSITE_SPEED_SEARCH.md` and
`composite-sustained-result-v3.json`; the physical maximum remains unproven.

Automatic composite trial000 completes300 seconds at **0.5671121795 m/s**
(170.1336538489 m, 0.3125 ms, no sampled fall), passing full scene/command and
exact prefix-replay checks. The previous composite gait reaches
**0.5735639103 m/s** (172.0691730779 m) in its completed0.15625 ms test. That is
2.0248% above its own fine speed, with substantial endpoint divergence, so this
is not a convergence claim. Matched contact/finer tests continue. The faster
0.57606 m/s prefix also survives a timestep check, but transferred heading
feedback does not improve its turning. See `composite-sustained-result-v4.json`;
the physical speed maximum remains unproven.

The previous composite gait also completes its matched inter-link-contact test
at **0.5685365125 m/s over300 seconds** (170.5609537531 m,0.3125 ms, no sampled
fall), with exact scene/command/seed verification. Its contact-enabled finer
test is still active. `composite-contact-result-v3.json` records this additional
physical-model qualification; numerical convergence and the maximum remain open.
