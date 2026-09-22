# Shared rigid kinematics

Inertia and force preparation now borrow the same immutable kinematic evaluation.
The full mass assembly order, projected solve, exact endpoint checks, robot
recipe, controller and physical parameters are unchanged. The projected-inertia
option remains separate and is disabled in this experiment.

`source-before/`, `source-candidate/`, `before-native.bin` and `protocol.json`
retain the local change and reproducible numerical gates. The copied scene,
config, task and actions are the unchanged selected BE6400 recipe.
64 release tests pass (inertia, embedding, implicit stepping and embedded motors).
Native before/after: 28.238166 s and 28.251849 s for three seconds, ratio
0.999516. All 151 physical frames match exactly. **No measured speed gain** and
no newly qualified recipe; the implementation simply removes duplicate work.
Profiled/unprofiled physical frames match exactly as well.

The SIMD/LTO browser worker takes 28.606820 s, 0.10487x realtime, p95 transition
225.95 ms. Native/WASM held servo states and commands match exactly; maximum
contact-field difference is 1.74e-10. `candidate.comparison.json` records full
metrics. Rendered W/A/S/D and release-to-stop checks pass, with 10 submitted
actions and 11 received frames; `rendered/wasd.png` was visually inspected.
This short functional check is not a sustained gait or realtime qualification.

An isolated browser compiler experiment uses SIMD128, fat LTO and one codegen
unit, built by `web/build-wasm.mjs`; no fast-math flags or physics overrides.
Its complete compiler/source manifest and artifact are preserved here.
Compiler effects must not be attributed to the source optimization.

The temporary viewer is served at http://127.0.0.1:4190/?preset=robot-measured-400hz-reuse.
The preset describes its prior 0.08x measurement; this new measurement is about
0.105x. Neither is realtime. No same-source scalar browser comparison has been
run, so the effect of SIMD/LTO alone is unmeasured.
