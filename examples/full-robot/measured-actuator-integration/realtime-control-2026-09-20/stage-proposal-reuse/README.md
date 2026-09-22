# Guarded correction-matrix reuse for coupled SDIRK2

The two stages and later accepted macrosteps may propose a previous Newton
correction matrix when `reuse_sdirk_jacobian` is explicitly enabled together
with `sdirk2` and `reuse_step_jacobian`. Every ordinary residual and accepted
endpoint still uses the original physical equations. Failed proposals retry
the same stage from the same seed with a fresh matrix. Affine stage anchors
do not carry endpoint or velocity-history caches. Discontinuous physical time,
step/config changes, events and contact changes retain their existing checks.

`source-before/` and `source-candidate/` preserve the change; `protocol.json`
and binary receipts retain identities. Recipes differ only by this reuse flag
from their matching `coupled-sdirk2` references.

| Method | Cold wall seconds | Reused wall seconds | Ratio | Sim/wall |
| --- | ---: | ---: | ---: | ---: |
| SDIRK2 800 Hz | 17.907633 | 11.244452 | 1.593 | 0.2668 |
| SDIRK2 6400 Hz | 97.943629 | 43.831631 | 2.235 | 0.06844 |

Each row simulates three seconds. Coarse cold and candidate were freshly
sequential; fine cold comes from the prior checkpoint. These are screening
runs, not repeated host statistics. Both qualify as same-method numerical
optimizations at the unchanged gates; both fail realtime. Contact identities
and sample/application counters match. Peak motor angle changes are below
7e-9 rad and peak contact force changes below 5e-5 N. The SDIRK integration
recipes remain unqualified against the original physical comparison gates.

61 focused tests pass; wasm32 without default features compiles.
`profile-parity.json` verifies exact physical agreement between timed and
profiled captures. Profiling time is 11.603 s, with nested Jacobian assembly
3.452 s, dynamics preparation 3.994 s and closure mapping 2.790 s.
All runs have finished; do not restart a completed benchmark as recovery.
