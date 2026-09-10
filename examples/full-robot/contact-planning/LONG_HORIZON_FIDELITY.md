# Five-minute numerical-fidelity checks

The original teacher's five-minute net speed is sensitive to timestep. Completed cases
all run from the same startup and finish 300 seconds without a sampled fall:

| Case | Physics timestep | Net distance | Net speed |
| --- | ---: | ---: | ---: |
| Nominal teacher | 0.625 ms | 154.301269 m | 0.514337563 m/s |
| Finer teacher | 0.3125 ms | 145.620286 m | 0.485400953 m/s |
| Teacher with inter-link contact | 0.625 ms | 154.301276 m | 0.514337588 m/s |
| Finer neural controller | 0.3125 ms | 156.046432 m | 0.520154773 m/s |
| Third-resolution teacher | 0.15625 ms | 136.791709 m | 0.455972365 m/s |
| Third-resolution neural controller | 0.15625 ms | 156.129822 m | 0.520432740 m/s |
| Coarse-calibrated steering root | 0.625 ms | 164.012492 m | 0.546708306 m/s |
| Fine-calibrated steering root | 0.3125 ms | 164.067482 m | 0.546891607 m/s |

The last two rows use different calibrated yaw commands. Their similar net
speeds are separate successful candidates, not a matched convergence pair. The
fine-calibrated controller is now being held fixed for a 0.15625 ms full run.

For the original teacher, halving the timestep lowers net speed by 5.626%. The final horizontal positions
differ by 59.098 m, so the trajectories have also diverged substantially over
five minutes. The recorded endpoints alone do not identify how much is due to
changes in instantaneous speed versus changes in travel direction. This result
does not establish which resolution is converged. No heading or tracking
penalty is added: net displacement over the full horizon remains the objective.

At the nominal timestep, enabling inter-link contact changes net speed by only
2.4132e-8 m/s and the final horizontal position by 5.813 mm. This particular
teacher comparison gives little evidence of a speed effect from omitting those
contacts. It does not certify omission for different amplitudes, phases or gaits.

The earlier nominal five-minute comparison favored the teacher over the neural
actor, 0.514337563 versus 0.497322885 m/s. The matching 0.3125 ms neural evaluation
now completes at 0.520154773 m/s, 7.160% above the fine teacher. The ranking
reverses, so convergence remains unresolved. Existing
nominal amplitude/phase searches remain controlled discovery experiments;
their results require matching finer checks before a physical speed gain is
claimed.

A third teacher resolution, 0.15625 ms, needs 1,920,000 physics steps. Its first
attempt failed before advancing physics because `EmbeddedSession` applied the
one-million-step host-call limit to the entire episode. This is a setup/resource
restriction, not a physical result or a speed ceiling. The original zero-step
failure remains preserved.

The shared runtime now separates those two concerns. Declared episodes may
extend beyond one million steps, with finite total time and integer step counts
representable exactly in the f64 clock (at most 2^53; platform integer capacity
also applies). An individual `advance` call still accepts at most one million
steps. The sampled environment already advances a controller interval at a
time; the headless `integrate_embedding` host now chunks a longer requested
episode through the same runtime. No physics equations, actuator parameters,
integration settings or observation/action semantics change.

Eleven runtime tests pass. The new case declares a 1.92-million-step episode,
compares its first 80 steps across chunks with the ordinary fixture, verifies
prefix replay, rejects an oversized host advance without changing state, and
rejects invalid horizons. Ten existing session tests cover firmware, contact,
controller inputs, timeout and replay behavior. A full-robot two-second check
also matches all 101 physical/controller frames and task transitions exactly,
excluding only elapsed wall-compute timing.

The corrected third-resolution run uses a newly pinned executable and the
same scene/config/task/actions as the failed attempt. It now completes all
1,920,000 steps and 300 seconds without a sampled fall, at 0.455972365 m/s.
This is another 6.06% reduction from the fine teacher. Timestep convergence
remains unresolved; no resolution is assumed exact.
A matched third-resolution neural run now uses the same pinned executable,
seed and 300-second horizon, with only timestep/count changed from the completed
fine neural case. The prior live search executables and specifications
remain unchanged. No resolution is assumed exact, and the physical maximum
remains unproven. The completed finer neural results are compared below.

`long-horizon-fidelity-v1.json` records the measurements and follow-up inputs;
`long-horizon-fidelity-evidence-v1.json` preserves completed outputs, code,
tests and immutable pending inputs. Growing logs/results are excluded.
`long-horizon-fidelity-v2.json` adds the completed fine neural comparison;
its evidence is included in `predictive-screen-study-evidence-v1.json`.
`long-horizon-fidelity-v3.json` adds the third-resolution teacher. At that snapshot
the neural run was pending; its evidence is preserved in
`planar-heading-trend-evidence-v1.json`.

The matched third-resolution neural run has now completed all 1,920,000 steps
without a sampled fall. Its 0.520432740 m/s differs by only 0.0534% from the
0.3125 ms neural result and exceeds the matched teacher by 14.1369%. This makes
the neural actor the stronger measured baseline at both finer resolutions.
Its endpoints still differ by 34.310 m, so agreement of net speed does not
establish trajectory convergence. No physical maximum or sim-to-real accuracy
is established. `long-horizon-fidelity-v4.json` records this comparison; the
completed outputs are preserved in `local-global-speed-evidence-v1.json`.

A new controller candidate, the measured-response steering root, completes
300 seconds at **0.5467083058 m/s** at 0.625 ms (164.0124917518 m, no sampled fall).
Its 0.0752386367 rad/s yaw command was selected from prior physical response
measurements. The same command reaches 0.5366739837 m/s over a 20-second 0.3125 ms
prefix but retains more mean turning. A separately calibrated fine command and
an ideal-heading feedback experiment are being evaluated. This new nominal
result does not replace the matched neural finer-pair convergence evidence;
full-duration fidelity of the new candidate remains open. See
`STEERING_RESPONSE.md` and `DYNAMICS_OBSERVATION_SAMPLING.md`.
