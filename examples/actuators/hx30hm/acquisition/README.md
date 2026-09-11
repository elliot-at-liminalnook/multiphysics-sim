# Time-series acquisition for HX-30HM identification

The acquisition profile lives in `~/Projects/sipeed-tang-primer-25k/servo`.
Its `capture.md` specifies the wire layout and controller behavior. Build with
`make -C servo capture`; the result is `servo/impl/capture.fs`. This work has
not programmed hardware or acquired new hardware measurements.

The shared Rust `acquisition` module decodes framed records and preserves
FPGA device-clock windows. `capture_hx` converts recognized HX registers into
typed SI readings while retaining raw counts, raw packets, command requests,
reply status, device errors, losses and clock discontinuities.

## Record hardware

After the acquisition profile is deliberately loaded, use an explicit USB
UART port (1 Mbaud). This logger receives only and sends no motion commands.
The existing physical buttons can provide excitation; the image retains its
existing controller and stop/boot-cleanup behavior.

```sh
cargo run --locked --release -p sim-runtime --example capture_hx -- \
  --port /dev/cu.usbserial-YOUR_CHANNEL_B \
  --metadata /path/to/experiment.json --seconds 30 /path/to/new-output
```

The metadata JSON should identify the actual loaded bitstream/hash, servo
identity and firmware, supply setting, mounting, load/inertia, controller mode,
gains/protections, ambient conditions and excitation procedure. Unknown values
must stay explicit. A template is provided in `hardware-metadata-template.json`;
it is not a measured fixture definition.

Outputs: `wire.bin`, `transactions.jsonl`, `observations.csv`, `commands.csv`,
and `run.json`. The directory must be new. The run manifest is created before
capture and marked complete only after normal input completion. Files flush
as data arrives; raw bytes and failures are preserved. Completion does not mean
a lossless stream: inspect its separate quality counters.

Each observation has request/completion ticks, clock Hz and host arrival time.
Separate reads are irregular, with overlapping or distinct acquisition windows.
**The FPGA timestamps do not reveal the age of data cached inside the servo.**
Raw positions are unsigned 16-bit; no calibration zero or unwrap is guessed.
Internal current is not assumed to equal total supply current. Temperature is
converted from Celsius to kelvin in the typed CSV. Writes remain attempted
commands with acknowledgement status, not claims about exact application time.

These logs can constrain the prior HX-30HM simulation, but are not silently
converted to uniform-time samples for the existing fitter. Before fitting:
identify fresh-data cadence and delay, segment by device/clock epoch, reject
invalid transactions and explicitly choose how to align time windows. Fast
motion reads are prioritized; electrical sub-millisecond transients need a
separate current/voltage instrument. The existing jog controller still commands
speed, not arbitrary position trajectories.

## Reproduce the cross-language check

`fixture.bin` is **synthetic** UART output from the FPGA Verilog test. It
contains 35 attempted transactions: 10 recorded, 25 deliberately dropped by
FIFO overflow. Of the recorded events, seven are valid position observations,
one is a reverse-speed write request with an empty ACK, and two are failures.
The test supplies unset upper reply lanes; those must not leak into the stream.

```sh
cargo run --locked --release -p sim-runtime --example capture_hx -- \
  --input examples/actuators/hx30hm/acquisition/fixture.bin \
  --metadata examples/actuators/hx30hm/acquisition/fixture-metadata.json \
  /path/to/new-replay
cargo test --locked --release -p sim-runtime --lib acquisition::tests
cargo test --locked --release -p sim-runtime --example capture_hx
cargo test --locked --release -p sim-runtime --test acquisition_wire
```

`replay/` contains the executed Rust replay, not hardware results. The shared
transport tests exercise fragmented reads, garbage, CRC failures, incomplete
frames, timestamps and invalid records. The independent UART fixture tests
byte ordering, commands, error records and loss accounting. Board-side tests
also check foreign-ID reply rejection and both legacy and capture controller
stop behavior. Build/validation identities are in `verification.json`.
