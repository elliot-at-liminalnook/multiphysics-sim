# Force evaluation cost and fixed-motion balance limits

The warm-start 173-variable search ends after 1,400 evaluations and only three
accepted steps. Its .054309 m/s final reference is infeasible: force error is
5.963 N, moment error 2.696 Nm, torque margin -.311 Nm and cone violation 3.587 N.
No feasible candidate is retained. The result is rejected, and does not replace
the experimental runtime gaits or establish a physical speed ceiling.

Follow-up: the [instantaneous support audit](INSTANTANEOUS_SUPPORT.md) finds
incompatible point-support balance even after temporal force-curve restrictions
are removed. The wider joint search is running; finite-foot support is the next
model check before interpreting this as a physical motion restriction.

## Exact reuse of CAD inverse-load calculations

The shared `PointForceLoadMap` stores the affine dependence of wrench residuals
and joint loads on point forces for fixed configuration, velocity and acceleration.
It exposes force-to-wrench and force-to-motor Jacobians. The phase planner caches
these maps only while its complete recipe, motion and force-template layout
remain identical. Changing placements, timing, knots or interpolation invalidates
the cache. The model and seed belong to the planner instance. Force cones and
signed actuator envelopes are still evaluated for every force candidate.

The native audit compares every serialized report field with an independent
full CAD evaluation. All 18 cases match byte-for-byte: 13 force-only/unchanged
checks and five invalidations. Force-only evaluation averages .000536 s versus
.228133 s, a measured 426x reduction for that portion of the search. This is not
an overall solver speedup or browser performance claim. Repeating the entire
1,400-evaluation warm search produces a byte-identical final result and solver
history. The ten robot motion-capability tests and four phase-planner tests pass.

## Why increasing the old search budget is not the only issue

For the fixed .211710 m/s motion, normalize each of the 960 sampled force/moment
balance equations by its unchanged tolerance. The 120 force-node components
enter these equations affinely: `r = A f + b`. A shared SVD calculation finds an
unconstrained least-squares force assignment and uses its residual as a dual
weight. For any weight w and the declared force-variable box:

`||r||∞ >= [wᵀb + min_box(wᵀAf)] / ||w||₁`.

The box term retains any residual lack of orthogonality from floating-point SVD.
The resulting numerical lower bound is **35.6671**, while passing all balance
rows requires **at most 1**. An independent full CAD evaluation of the fitted
forces agrees with the affine prediction to 5.40e-12 in normalized residuals.
Tests cover an analytically incompatible system, rank deficiency, and a tiny
singular direction whose large variable box would invalidate a naive certificate.

Thus the current motion and force basis cannot satisfy these sampled balance
requirements within the declared force bounds, even with friction and motor
restrictions omitted from this relaxation. The fitted forces themselves violate
cones by 126.4 N and motor capacity by 8.009 Nm; they are diagnostic only. Large
dual contributions occur in world-X balance near cycle phases .71875 and .90625,
in both clock directions.

This conclusion is conditional on the fixed motion, force basis, point-support
model and sampled mesh. It is floating-point linear algebra, not an interval
arithmetic proof about hardware. It does not establish a gait-family or robot
speed limit, or yet isolate interpolation freedom from support geometry.

## Next discriminating work

Compare the trajectory-basis bound with independent per-frame force bounds at
the same motion. That separates force-curve restrictions from support-wrench
restrictions, before increasing basis freedom or spending a larger joint-search
budget. Motion, placement and timing remain free in the eventual speed search.

Six XY foot-center decisions also reach the original +/-20 mm search-box edges.
`prepare_joint_workspace.mjs` prepares wider XY intervals from the recorded
r1357 CAD workspace samples, without moving the initial gait or changing any
physical gate. These are sampled exploration boxes, not certified reachable sets
or hardware bounds; the prepared search has not run. It also reserves enough
evaluations for multiple full inner iterations of the 173-variable problem.

## Reproduction

- `audit_joint_force_cache scene markers recipe` checks cache agreement and cost.
- `audit_joint_force_basis scene markers recipe` computes the affine relaxation,
  dual bound and independent reconstruction check.
- `summarize_joint_contact.mjs examples/full-robot/contact-planning/joint-x25-warm`
  reproduces the completed failed-search summary.

Raw audit outputs, recipes, source archives, binary identities and test logs
accompany this report. The CAD artifact and prior runtime captures retain the
existing evidence chain. No new gait has been promoted to the browser.
