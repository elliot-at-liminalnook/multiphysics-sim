# Browser compiler profile experiment

The 20 ms faster student misses active realtime and has 37.25 ms browser p95.
Test an isolated `simd-lto` Rust/WASM build with WebAssembly SIMD, fat LTO and
one codegen unit. Keep the identical controller, world, physics, solver
tolerances, native reference and rendered keyboard scenario. The scalar build
and delivered viewer stay available. Record Rust/Cargo versions, compiler
settings, all Rust source hashes and the artifact hash with `build-wasm.mjs`.

Before accepting the build, require the existing mixed numeric native/WASM
tolerance, exact same-host replay/reset and real UI loading tests. Run browser
timing with no simultaneous builds or simulations. Keep the active >=1× and
p95 <=20 ms requirements unchanged; report render scheduling and drawn-reference
latency separately. This changes compilation only, not the physical fidelity
profile, calibration or controller validation status.
