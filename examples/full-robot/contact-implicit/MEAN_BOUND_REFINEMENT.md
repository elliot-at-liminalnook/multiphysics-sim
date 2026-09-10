# Motion resolution with the continuous slip bound

The 64-control trial reduces dense force error to 0.080737 N and moment error
to 0.033634 Nm, while actual slip passes at 4.014% and sampled overlap remains
zero. Retain `mean64-inner60` as the next numerical seed. Force and moment
still exceed their 0.05 N / 0.02 Nm gates; no new gait or speed is qualified.

The previous `mean68-warm` seed passes dense slip but still fails force and
moment balance. This experiment compares a longer inner solve at 32 controls
with an exactly refined 64-control representation of the same starting curve.
It retains the continuous mean-slip barrier, CAD physics and acceptance gates.

## Shared components and checkpoint inspection

The existing shared cubic refinement doubles controls without changing the
analytic position, velocity or acceleration curve, up to roundoff. No new
trajectory implementation is added. The refiner is rebuilt from current sources.
The control interval and samples per interval are both halved, keeping the
same period and 128 physical check times. Constant non-XY bounds and the XY
window relative to the interpolated reference are retained. The first XY
control is anchored at its exact refined value as the representation gauge;
the fixed cycle displacement needs no roundoff adjustment in this trial.

`audit_contact_implicit --inequalities` optionally exports native optimizer
coordinates, flattened bounds and signed residuals through the existing shared
parameterization and inequality APIs. It requires the supplied motion to round
trip exactly through those coordinates. The ordinary audit remains unchanged.
`--pairs` and `--inequalities` work in either order; unknown and duplicate flags
fail before reading model inputs. Four command-line rejection cases are recorded.

`prepare_refined_periodic_trial.mjs` now handles nested inequality-solver
settings and explicitly removes inherited checkpoints whose coordinate layout
is invalid after refinement. A separate
`prepare_refined_inequality_checkpoint.mjs` validates native inspections and
creates the new checkpoint: coordinates and residuals come from the refined
native audit, while every physical-row multiplier, next penalty, preceding
shifted norm and completed outer count is preserved. This is an explicit
representation migration, not a claim that parameter vectors are identical.

The parent native inspection exactly reproduces the previous optimizer's
coordinates and residuals. Removing its optional inspection field leaves the
entire prior audit exactly unchanged; a new ordinary audit also matches it.
The refined seed changes position by at most 4.44e-16, velocity 1.58e-14 and
acceleration 3.92e-12 in their respective coordinate units. Base-wrench changes
are at most 6.23e-11 on the 128-time grid and 1.25e-10 on the 512-time grid.
The largest normalized inequality change is 6.04e-9, below the recorded 1e-7
migration allowance. No physical tolerance is relaxed by that numerical check.

## Matched solve

Both trials start from the retained `mean68-warm` curve and its multiplier
state. Each runs one outer iteration at penalty 100 with up to 60 inner
iterations, scaling exponent 0.25 and the same mean-bound barrier weight.
The 32-control problem has 578 parameters, 574 free; the 64-control problem
has 1,154 parameters, 1,150 free. Both retain 15,360 physical inequality rows.
Equal iteration caps therefore do not imply equal evaluations or wall time.

The projected +45 degree displacement rate stays fixed at 0.0670626816 m/s.
The independent dense comparison uses 512 physical times and 513 full CAD
geometry poses: 16 subdivisions for the old basis and 8 for the new one.
The initial dense audit also confirms the same curve and force history.

This comparison can show whether more motion freedom helps this local search;
it cannot prove the 32-control basis infeasible or identify a physical speed
maximum. Changing the representation also changes numerical conditioning.
Physical balance, dense slip, collision checks, runtime tracking and responsive
command handling remain separate requirements.

## Results

| Dense audit | Force error N | Moment error Nm | Torque margin Nm | Actual slip | Mean bound | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Earlier `mean68-warm` seed | 0.118593 | 0.074403 | +0.310268 | 3.999% | 4.970% | 0 |
| 32 controls, 60 inner iterations | 0.118916 | 0.055722 | +0.342165 | 3.760% | 4.985% | 0 |
| 64 controls, 60 inner iterations | 0.080737 | 0.033634 | +0.310275 | 4.014% | 4.943% | 0 |

The 64-control optimization grid reports 0.070131 N force error and 0.028565 Nm
moment error. Dense errors are larger but remain substantially better than
both the matched 32-control trial and the prior seed. Maximum point penetration
is 0.107882 mm. Body path remains about 1.06901 times net displacement. Its
small actual-slip increase relative to the old seed remains below the 5% gate;
the continuous sufficient bound also passes on the dense grid.

The trials use 68,961 and 138,078 evaluations respectively. Both hit their
60-step inner cap and outer cap; neither is stationary. The 32-control solve
rejects four evaluations and returns a next penalty of 1,000. The 64-control
solve rejects none and returns a next penalty of 100, because its normalized
physical violation falls from 2.58655 to 0.428264. Their different next penalties
follow the same adaptive rule applied to different residual histories.

Neither trial has an active free box bound at a normalized distance threshold
of 1e-8. Their nearest free-bound distances are 0.3411 and 0.3386 of the allowed
range. Fixed XY gauge and displacement coordinates are excluded from this
diagnostic. These results provide no evidence that joint travel is the limiting
physical speed constraint. Large final projected gradients and failed balance
also preclude an infeasibility or local-optimality certificate.

The retained refined trajectory is about 1.61 times the force-error limit and
1.68 times the moment-error limit on the dense grid. The next work should add
physical checks between the current optimization samples and continue from its
preserved state, retaining the continuous slip bound. Dense error increases
must not be hidden by accepting only the coarse result. Runtime tracking,
stable repetition, collision robustness, command handling and speed continuation
remain outstanding; this experiment does not establish the physical maximum.

## Reproduction and verification

The optimizer binary is unchanged from commit `901dfae`. Build the existing
`refine_contact_periodic` example and the updated `audit_contact_implicit`
example in release mode. Use the recorded constrained scene and
`surface-markers.json`. `mean-refinement-control-identities.json` preserves the
pre-change audit/optimizer identities; `mean-refinement-build-identities.json`
identifies the actual refiner, new optional inspector and unchanged optimizer.

Prepare the 32-control continuation with the existing checkpoint adapter, then
set its inner cap explicitly to 60. Run `prepare_refined_periodic_trial.mjs`
with no sample-count override to construct the refined inspection draft. Audit
the parent and refined initial motion with `--inequalities --pairs`, then use
`prepare_refined_inequality_checkpoint.mjs` to migrate the physical-row duals.
The optimizer must exactly re-evaluate each supplied checkpoint before taking
a step. No constraint row or multiplier is fabricated in a script.

Audit final results with 4/2 geometry subdivisions for the 32/64-control coarse
recipes, and 16/8 subdivisions for their 512-time dense recipes, using `--pairs`.
Run `summarize_mean_refinement.mjs` from the repository root. It checks ordinary
audit parity, native checkpoint identities, initial coarse/dense curve
preservation, matched solver/physics settings, initial augmented costs,
multiplier updates, free-bound activity and independent mean-slip reductions.
Inputs, CLI validation results, builds, all completed trials and prior evidence
are retained. CAD and the existing browser gait bundles are unchanged.
