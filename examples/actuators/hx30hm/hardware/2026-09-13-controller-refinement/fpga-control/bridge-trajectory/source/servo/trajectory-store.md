# Sealed trajectory upload — prototype, not commissioned

`src/trajectory_store.v` stores up to 256 nine-axis rows in synchronous RAM.
`src/trajectory_upload.v` decodes complete packets into that store. Both are
component-tested and connected in the optional [upload bridge](trajectory-bridge.md).
The new profile has not been loaded on the FPGA. Existing A1/host-scheduled
hardware remains unchanged.

The Rust library `sim-runtime::controller_refinement::fpga_upload::Upload` compiles
an existing bounded Plan and explicit home positions into this protocol. Its saved
artifact includes the full source experiment, source hash, homes, firmware
reference, clock/version, CRC and exact packets. Loading validation regenerates the
packets from that source. The compiler and decoder cannot arm or start motors.
Firmware hashes record provenance; they do not establish that an image implements
this new protocol. Hardware acquisition must check image capability before upload.

## Version 1 encoding

Packets use `FF FF FE LEN A2 payload CHECKSUM`; the existing packet checksum covers
ID through payload. The payload forms are:

| Operation | Payload | Total packet bytes |
| --- | --- | --- |
| CONFIG | `00 01` followed by 34-byte header | 42 |
| ROW | `01 index_u16` followed by 36-byte row | 45 |
| SEAL | `02 crc32_u32` | 11 |
| STATUS | `04` | 7 |

Every multibyte field is little endian. Header fields, in order: `mask:u16,
frames:u16, period_ticks:u32, kp_q8:u16, kd_q8:u16, kv_q8:u16, limit:u16,
homes:[u16;9]`. A row holds nine interleaved `(target:u16, delta:i16)` pairs.
Mask bit 0 is ID4; bit 8 is ID12. Unused mask/index bits must be zero. Inactive
axis homes, targets and deltas must be zero on the wire. Rust retains the original
input homes and plan while canonicalizing these inactive fields.

Plan CRC is CRC-32/ISO-HDLC (IEEE/zlib): reflected polynomial `edb88320`, initial
`ffffffff`, final complement. It covers the 34 header bytes followed by all row
bytes in order, excluding packet metadata and checksums. The independently checked
three-frame Rust fixture CRC is `98c3f9fc`.

CONFIG invalidates old validity and resets the write count. Rows must arrive once
in exact order, starting at zero. SEAL only begins after every row was written;
it checks every axis, target/delta relationship and CRC before asserting `valid`.
Writes after sealing or during execution/verification invalidate the plan without
changing RAM. Incomplete, malformed, duplicate or out-of-order uploads cannot use
old RAM contents. Reconfigure and upload again to recover.

The store accepts 2–256 frames, 40–150 ms at 50 MHz, at most 12 seconds, gains up to
4096 Q8, duty at most 100/1000, selected homes 600–3495, targets within ±80 counts
of home and deltas within ±32. First targets equal home with zero deltas; every
later delta must exactly equal target minus preceding target. The Rust Plan still
requires at least 50 ms, so its maximum duration admits at most 240 frames. It
rejects periods that do not map exactly to clock ticks. The 40 ms RTL floor is a
provisional target, not measured nine-motor throughput.

## Integration contract

- The decoder takes one `check` pulse for a stable, complete packet, its exact
  length and framing/checksum verdict. `handled` is combinational selection of A2;
  it is **not completion, a successful seal or permission to move**.
- Feed host-parser overflow/framing faults into `transport_fault` to revoke plan
  validity. Ordinary status/heartbeat packets do not change trajectory validity.
- `fault` pulses on a rejected operation; `valid` stays low until another complete
  successful upload. CONFIG/ROW outcomes are available after their sampling edge.
  SEAL must wait for `busy` to clear, then inspect validity and returned CRC.
  The optional bridge now returns these outcomes; see its reply format.
- During execution connect scheduler `active` to the lock input and `frame_index`
  to `read_index`; gate execution with `valid` and `row_ready`. Row data is only
  usable when ready, after the synchronous read tag matches the requested index.
- Route uploads locally; never forward A2 to the motor bus. Pass scheduler control
  requests through the existing compiled A1 controller and independent supervisor.
  This memory adds no control arithmetic, lease renewal or direct PWM output.
- CRC verifies an upload, not continuous ECC against later physical RAM bit flips.

## Verification and reproduction

```sh
make sim-trajectory-store sim-trajectory-upload
make check-trajectory-upload
```

Nineteen store groups cover CRC, timing/bounds, delta consistency, sparse masks,
all-nine targets, synchronous reads, stale/missing RAM, active/busy/sealed mutation,
reset collisions and all 256 rows. Eleven upload groups pass the frozen Rust
packets through the real host byte queue and decoder, including malformed lengths,
wrong version/high bits, bad checksum, truncated input, unknown operations,
active/verification writes and unrelated status preservation. This is decoded-byte
transport testing, not a new UART-on-hardware measurement. Gowin synthesis uses
block RAM; integrated routing/timing remains unverified.

From physics-simulator, compile a new offline artifact/vector directory:

```sh
cargo run -p sim-runtime --example compile_fpga_trajectory -- plan.json homes.json new-directory
cargo test -p sim-runtime --test fpga_upload --test fpga_controller
cargo test -p sim-runtime --lib controller_refinement::fpga_upload
```

`homes.json` is an explicit nine-element encoder array. The example creates a new
directory and never opens a serial port. `tb/trajectory-v1/` contains synthetic
vectors generated by this compiler, not bench measurements.

Remaining: scheduler transaction adapter and execution integration,
device-clock evidence transport and Rust acquisition, full integrated UART and
watchdog checks, routing/timing, then physical commissioning and new accuracy runs.
