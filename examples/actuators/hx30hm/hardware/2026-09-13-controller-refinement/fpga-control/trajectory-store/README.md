# Sealed FPGA trajectory storage and upload compiler

September 13, 2026. **Prototype verified in software/RTL; not loaded on hardware.**
This increment makes a complete trajectory available as immutable FPGA memory.
It does not change the prior physical accuracy results or commissioned image.

- Rust compiles existing bounded plans plus explicit encoder homes into
  CONFIG/ROW/SEAL packets, retaining experiment, firmware and source identity.
- FPGA memory validates canonical configuration, all target/delta bounds and
  CRC32 before exposing synchronous rows. Incomplete uploads cannot reuse stale
  RAM. Active, scanning and sealed writes invalidate without altering RAM.
- The packet decoder and real host input queue consume frozen Rust-generated
  packets with exact agreement on every selected motor's target/displacement.
- No ARM, START, command lease renewal or PWM computation is added here. The
  shared Rust-to-Verilog integer controller remains the only control law.

[Protocol, limits, interfaces and reproduction](source/servo/trajectory-store.md).
[Compiled synthetic fixture](vectors/upload.json). This fixture is not measured
motor evidence. Hardware capability must be checked separately from its firmware
reference before any future upload.

## Verification

- **19 storage test groups**: all-nine/sparse rows, incorrect CRC/deltas/limits,
  missing/duplicate/out-of-order rows, synchronous read identity, 256-row capacity,
  reset/collision and active/busy/sealed mutation rejection.
- **11 upload test groups**: Rust packets through host byte queue and decoder;
  malformed length/version/high bits, unsupported op, bad checksum/truncation,
  writes while active/verifying and ordinary status preservation.
- **13 Rust tests**: five compiler/provenance/fixture cases, seven retained FPGA
  acquisition/design/simulation tests and one standard CRC check.
- **Gowin synthesis passes** for decoder plus RAM, using eight SDPX9B blocks.
  Integrated place-and-route timing has not been tested for these new components.
- Rust and independently evaluated zlib CRC agree at `98c3f9fc`. Final logs are in
  `verification/`. The initial testbench used Verilog's reserved word `config` as
  a task name; the compiler failure is retained alongside corrected passing tests.

For the frozen RTL subset, from `source/servo`:

```sh
make sim-trajectory-store sim-trajectory-upload
make check-trajectory-upload
```

Use an installed Icarus/Yosys Gowin toolchain on PATH. Rust sources are frozen under
`source/rust`; run their tests in the simulator workspace using the commands in the
protocol document. `manifest.json` hashes this archive, excluding itself.

## Remaining work

Connect upload responses/status and scheduler transactions to the buffered bridge;
stream device-clock telemetry, audits and transmitted commands; consume that
stream in Rust; run full integrated UART/watchdog and routed timing checks; then
commission hardware cadence and collect fresh single/all-nine-motor comparisons.
The existing 50 ms Rust Plan minimum remains. The RAM's 40 ms lower bound is a
provisional throughput target. Accurate motor-model, calibrated amps/watts and
loaded-joint/battery acceptance remain unfinished.
