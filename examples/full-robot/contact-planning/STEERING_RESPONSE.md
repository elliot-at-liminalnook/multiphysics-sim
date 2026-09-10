# Steering response for sustained speed

The 20-second local/global candidate `evaluation-006` moves at 0.5299044574 m/s,
but its fitted mean turn rate is 0.0181907890 rad/s. The constant-turn predictor
estimates only 0.0798731634 m/s net displacement over 300 seconds. That forecast
is a proposal aid, not a measured long-run result. This study tests whether
steering can preserve the candidate's translational speed over distance.

Inspection found a controller discontinuity: any nonzero requested yaw disables
the existing rate-squared dynamic feedforward correction. Two experimental arms
therefore calibrate separately. The original arm retains that switch. The
continuous arm retains the authored reference correction during steering; it
does not derive new steering inverse dynamics. All robot, world and actuator
properties remain identical to the source capture.

Removing the switch at zero yaw reproduces all 1,001 physical/controller frames
and all 1,001 environment transitions exactly, excluding wall-clock timing.
Each probe is a completed 20-second prefix of a declared 300-second episode,
with seed 2301, 0.625 ms physics steps and 20 ms actions. All five initial probes
finish without a sampled fall.

| Feedforward | Requested yaw (rad/s) | Measured 20 s speed (m/s) | Fitted yaw, 5–20 s (rad/s) |
| --- | ---: | ---: | ---: |
| Continuous | 0 | 0.5299044574 | 0.0181907890 |
| Original | -0.02 | 0.5319518880 | 0.0181668867 |
| Original | +0.02 | 0.5349015124 | 0.0105368377 |
| Continuous | -0.02 | 0.5292366221 | 0.0200929741 |
| Continuous | +0.02 | 0.5304515223 | 0.0163456292 |

The shared Rust `sim_solve::affine_response::AffineResponse` fits a scalar
input/output response with centered covariance. It exposes gain, residual RMS,
observed input range and an unclamped input proposal for a target output. The
`calibrate_scalar_response` example records units and flags proposals outside
the observed and available input domains. Analytic tests check the identified
gain/root, unit conversion, nonlinear residuals and invalid or flat inputs.

Separate fits propose +0.0752386367 rad/s for the original arm and
+0.1943754623 rad/s for the continuous arm to reduce mean turning to zero.
Fits over 10–20 seconds propose +0.0768943071 and +0.1987754082 respectively.
The original fit deliberately excludes the zero-yaw point because its
feedforward mode differs from its nonzero probes. Two points imply zero fit
residual, not established model accuracy.

Both roots extrapolate beyond the observed ±0.02 rad/s probes and the old
±0.06 rad/s teleoperation range. Root validations explicitly widen only the
controller yaw input range to ±0.25 rad/s. This is an experimental policy
setting, not a change to CAD actuator limits. The existing 0.05 rad yaw-offset
policy cap is retained to isolate this intervention. Its saturation can make
the extrapolated response nonlinear. Accepted policy changes belong in the
controller configuration; no robot property needs promotion into CAD.

Predicted straightness is not a ranking constraint or an added reward. The
objective remains full-episode net endpoint distance divided by duration,
without falling. Curved or slipping gaits remain eligible if they travel faster
over distance. This empirical calibration is neither a PLANC implementation nor
a learned dynamics model, and supplies no proof of a physical speed ceiling.
The best previously measured finer-timestep neural result remains 0.5204327404
m/s over 300 seconds; see `LONG_HORIZON_FIDELITY.md` for its limitations.

## Root validation

Both proposed roots completed 20 seconds without a sampled fall. The original
arm improved measured net speed to **0.5370313245 m/s**. Its fitted mean yaw rate
fell to 0.0002497959 rad/s over 5–20 seconds (0.0006054895 over 10–20 seconds).
The corresponding 300-second forecasts are 0.5438752305 and 0.5437930941 m/s.
These are still forecasts, not qualified sustained results.

The continuous arm failed the linear extrapolation: at +0.1943754623 rad/s its
mean turn rate increased to 0.0225509559 rad/s, and its measured prefix speed
fell to 0.5264418410 m/s. This does not locate the optimum of that arm. It shows
why a small fitted residual within the initial probe range cannot certify an
extrapolation; both the failed forecast and its physical capture are retained.

`runs/steering-validation-v1` launches the original root for a full 300 seconds
at 0.625 ms and for 20 seconds at 0.3125 ms. The full run extends the command
heartbeat through all 15,000 actions and exactly preserves the original first
1,000 inputs. The fine prefix preserves physical times and doubles step counts.
Both retain seed 2301. The fine prefix completed without a sampled fall at
**0.5366739837 m/s**, versus 0.5370313245 m/s at the nominal timestep. However,
its fitted mean yaw rate was 0.0036474979 rad/s (0.0040351748 over 10–20 seconds),
giving lower 300-second forecasts of 0.5171081766 and 0.5117468216 m/s. Short-run
speed agreement therefore does not establish sustained-speed convergence.

The full 300-second run remains active. `runs/steering-fine-response-v1` measures
two additional fine-timestep responses at ±0.02 rad/s around the coarse root,
with all other inputs unchanged. These will support a separate calibration;
changing the controller between resolutions cannot establish numerical
convergence of one controller. Their immutable inputs are archived, excluding
growing outputs. The browser stays on the previously verified WASD preset.

A structural integrity check reverses only the documented yaw/feedforward/range
interventions and verifies exact recipe equality with the source for all seven
probes. It likewise checks every extended heartbeat and reverses only duration
and timestep changes for the two validations. No hidden CAD, environment, task,
actuator or controller parameter differences were found. The two scalar-response
unit tests and the example build pass. `steering-response-evidence-v1.json`
preserves these checks, completed captures, model requests/results and immutable
validation inputs in the shared content-addressed evidence store.

## Finer-timestep response

Both follow-up probes completed 20 seconds without falling. At requested yaw
0.0552386367 rad/s, measured speed was 0.5359651062 m/s and the fitted 5–20-second
turn rate was 0.0071975744 rad/s. At 0.0952386367 rad/s, speed was
**0.5371092625 m/s** and mean turning fell to 0.0002192059 rad/s. Its two
300-second forecasts were 0.5440455383 and 0.5438705364 m/s.

Fitting these points with the intervening fine-prefix observation gives gain
-0.1744592146, residual RMS 0.0000287049 rad/s and a proposed root of
**0.0963787796 rad/s**. The 10–20-second fit proposes 0.0987453403 rad/s. The chosen
5–20-second proposal is just outside the observed interval, and therefore still
requires physics validation. Small fit residuals do not certify future behavior.

`runs/steering-fine-root-v1` evaluates that same proposal for 300 seconds at
0.3125 ms and for 20 seconds at 0.15625 ms, both with seed 2301 and identical
physical properties. The original 0.625 ms full run continues at its earlier
0.0752386367 rad/s root; these differently tuned commands are separate candidates,
not a matched convergence pair. The first quarter-step launcher started before
preparation finished and failed before simulation with a missing script. Its
error is preserved; the retry uses the completed immutable recipe and driver.

`steering-fine-response-evidence-v1.json` preserves the two completed probes,
new response fits and immutable validation inputs. No new 300-second result is
available at this checkpoint. Code and initial steering evidence were committed
as `baa5dd8c`.

## Third-timestep prefix

The fine-response root completed 20 seconds at 0.15625 ms without a sampled fall,
covering 10.7365951649 m (**0.5368297582 m/s**). Fitted mean turning was
0.0023498649 rad/s over 5–20 seconds, with predicted 300-second speeds
0.5327443294 and 0.5292865121 m/s across the two fit windows. These remain
predictions. Both full-duration steering runs continue separately.

Continuous correction of that residual drift needs an appropriate observation.
A diagnostic found that integrating the existing 20 ms gyro samples substantially
misestimates the actual heading change, even after exact timestamp/frame checks.
See `DYNAMICS_OBSERVATION_SAMPLING.md` for the dense-sampling investigation before
using that signal for feedback.

## Completed nominal 300-second result

The original steering root at +0.0752386367 rad/s completed 300 seconds at the
0.625 ms physics timestep, covering **164.0124917518 m** at **0.5467083058 m/s**,
with zero sampled falls across all 15,000 actions. Its first 1,001 physical
frames and transitions exactly match the prior 20-second prefix, excluding wall
timing. The two prior trajectory forecasts were 0.5438752305 and 0.5437930941 m/s.
This is a measured full-duration improvement at the nominal timestep, rather
than just a prediction. The same command has only a 20-second finer-prefix
check; the active fine full-duration test uses its separately calibrated root.
No matched-controller full-duration convergence or physical maximum is proven.

## Completed fine 300-second result

The fine-calibrated root at **+0.0963787796 rad/s** completed 300 seconds at
0.3125 ms, covering **164.0674820674 m** at **0.5468916069 m/s**, with zero sampled
falls. Checks confirm all 960,000 physics steps, 15,000 actions, the authored
scene and the exact full input-event schedule. The two first-20-second forecasts
were 0.5441609828 and 0.5443029374 m/s. This exceeds the prior neural fine result
of 0.5201547734 m/s at that timestep, but is not a learned neural improvement.

`runs/steering-quarter300-v1/sustained300` now holds this same controller fixed
at 0.15625 ms for 300 seconds, extending only the already completed quarter
prefix's duration/heartbeat. The coarse and fine winners use different steering
commands; their similar speeds cannot establish numerical convergence.
`steering-sustained-result-v2.json` and `steering-fine-sustained-evidence-v1.json`
preserve the completed fine result and the next validation inputs.

## Completed timestep and inter-link contact checks

The unchanged fine winner completes 300 seconds at 0.15625 ms at
**0.5361838427 m/s**, with no sampled fall and exact parity to its previous
20-second prefix. The 1.9579% speed change from 0.3125 ms leaves timestep
convergence unresolved. `composite-speed-search-evidence-v1.json` preserves
the full result and checks.

At 0.3125 ms, enabling inter-link contact for that same controller completes
300 seconds at **0.5461336476 m/s** (163.8400942689 m), with no sampled fall.
The net-speed change is about -0.1386%. Only the explicit contact-omission flag
changes; the controller, seed, world, actuator definition and all 15,000 actions
are identical. The result retains all 960,000 steps and 15,001 sampled frames.
This measures the contact approximation for the current winner, without assuming
an earlier gait's result transfers or claiming full-trajectory equivalence.

The first scene-equality diagnostic expected the authored false flag to remain
serialized. Rust intentionally omits that false default. The corrected check
verifies exactly that canonical omission and equality of every other scene
field, retaining the initial diagnostic. `steering-contact-evidence-v1.json`
archives the full capture, input recipe, canonicalization definition and checks.
