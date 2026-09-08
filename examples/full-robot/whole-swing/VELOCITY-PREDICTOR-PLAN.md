# Guarded temporal Newton seed

The same-bundle frame transport experiment keeps pace but still misses active
p95: 20.29 ms object steering, 20.19 ms JSON steering, 22.07 ms JSON forward.
Rendering and frame encoding changes have not supplied enough headroom.
The shared implicit solver currently starts every mechanical solve at the old
reduced velocity. Test whether a temporal guess reduces Newton work, especially
the costly return phase, before changing physical integration.

Add an optional, default-off velocity extrapolation seed to the existing Rust
implicit workspace. Require the existing cached-step path. Save the previous
accepted reduced-velocity increment transactionally; reuse it only across
matching dimensions, consecutive equal steps, matching accepted velocity and
the existing configuration/cache invalidation rules. Clear predictor history
after contact-pair changes, failed trajectory reuse, explicit workspace reset
or step/configuration changes. Callers must still clear the workspace after
external edits and force-law definition changes, as its public contract requires.

Keep auxiliary initial values unchanged. Evaluate the original and proposed
initial guesses with the exact current physical residual. Use the prediction
only if finite and its infinity norm is at least 10% smaller. Otherwise retain
the original guess. This screen selects a guess; it does not replace any Newton
residual/correction acceptance. If the predicted solve fails, clear reused
derivatives and retry exact numerical derivatives from the original guess,
before existing subdivision/recovery logic. Record predictor use/fallback;
default-off physical results must remain exact.

Before robot evaluation, preserve Newton's per-row initial residual bounds:
a smaller infinity norm can still increase an individual row. Pass the original
residual as a reference to the shared solver and use the smaller original/new
magnitude when forming each absolute-plus-relative bound. This can tighten a
row but cannot loosen it. Ordinary calls without a reference keep the original
solver path. Validate reference dimensions/finiteness before state/cache work.

Check constant-force and stiff linear analytic endpoints, nonlinear linkage
closure, state continuity/invalidation, rejected predictor geometry, complete
rollback, and an accepted prediction whose Newton solve fails but whose original
guess succeeds. Preserve the existing tolerances and analytic accuracy bounds.

Then compare the same 24-second 20 ms steering recipe with predictor off/on in
native Rust. Require all 15 supported swings, stop/heading/overlap gates and
the existing 1 mm foot / 0.5 mm body solver difference screen. Profile the
complete task without concurrent heavy work. Require full fixed-tolerance
native/WASM comparison and exact replay/reset before rendered timing. If it
reduces work, compare the same-binary off/on steering and candidate forward
cases with the original >=1 pace and <=20 ms p95 targets. Keep failed results.
This does not establish coarse timestep accuracy or hardware calibration, nor
complete the outstanding faster teacher/student, robustness and terrain work.
