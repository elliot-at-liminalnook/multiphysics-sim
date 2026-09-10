# 30-minute checkpoint — 2026-09-08 14:20 UTC

Local CAD detail corrected the four previously rejected exact-CAD exterior
points in 5.1 seconds. The 50 and 100 mm/s paired reference paths now pass the
sampled geometry screen; the preserved wave alternatives hit a different region.

Six-second live-command trials measured 47–110 mm/s. Shorter swing periods and
100 mm/s requests produced excessive loaded material slip. An 8 mm lift with
0.32 s swing time reached 49.3 mm/s, 2.9% slip, and 1.2 mm stop drift; all 24 lift
checks and all 301 recorded-pose collision checks passed. Increasing that recipe
to a 65 mm/s command reached 62.9 mm/s, 4.0% slip, and 2.0 mm drift. Nominal D/K
velocity feedforward reached 63.8 mm/s, 4.98% slip, and 0.79 mm drift, leaving
little slip margin. These faster variants still need independent geometry and
longer command validation. Angled hips (30/45 degrees) and centered strokes
increased slip in tested cases. Failures remain recorded.

The reusable Rust command lease and Rhai binding pass packet loss, reordering,
recovery, replay, registry schedule, and invalid-input checks. Three 20-second
forward/turn/reverse/stop cases are now running with phase-gated stopping and
bounded acceleration. Native/WASM rebuilds completed; browser validation remains.

No hardware commands or calibrated sim-to-real claims. Stop for review by
14:50:45 UTC. Remaining work: command trials, independent geometry, numerical
and browser profiles, a packet-loss physical trial, durable evidence/preset.
