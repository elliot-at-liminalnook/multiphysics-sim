# Device-clock recording milestone

The transaction core now runs with a recorder that preserves START, motor and
terminal records through host backpressure. Run identity remains fixed until the
terminal packet drains. The shared Rust reviewer checks source identity, ordering,
fixed frame windows, sample sequences, shared controller arithmetic and PWM
readbacks. Incomplete captures stay unscored; malformed and false-success captures
are rejected.

**This archive is software verification, not a new motor experiment.** The 59
packet fixture was encoded by real RTL simulation from synthetic test inputs.
Protocol completion proves neither physical stopping nor model accuracy. No image
containing this recorder has been programmed or commissioned.

Verification retained here:

- Three serializer groups and three stream-lifecycle groups pass.
- Nine transaction-plus-recorder groups pass; the original nine transaction
  groups also pass without the recorder.
- Serializer and stream Gowin synthesis pass. A single packet bank preserves
  byte-identical fixture output while reducing duplicated storage and logic.
- Six Rust event groups and seven existing upload groups pass.
- The batch command reviewed all 59 fixture records, preserved an existing output
  and rejected corrupt input without creating a report.
- Tests reproduced from this archive. CI now runs the archived RTL checks;
  the existing Rust workspace job covers the shared-library tests. Remote CI has
  not been run in this local session.

See the [protocol and bridge contract](experiment-events.md), the
[synthetic review](verification/synthetic-review.json) and SHA-256 `manifest.json`.
`source/rust` retains the matching implementation and batch command; these use
the simulator workspace dependencies, not a standalone Cargo project.

From this directory with the hardware toolchain on PATH:

```
make sim-experiment-events sim-experiment-event-stream sim-experiment-recording
make check-experiment-events check-experiment-event-stream
```

From the simulator workspace:

```
cargo test -p sim-runtime --test fpga_events --test fpga_upload
cargo run -p sim-runtime --example review_controller -- review-device-capture CAPTURE_JSON NEW_REPORT
```

The synthetic input is `verification/synthetic-capture.json`; choose a new output
path when reproducing the command. The existing recorded report is not overwritten.

Remaining: wire the recorder and transaction core into bridge UART arbitration,
validate START against the sealed plan and starting pose, preserve raw transport
alongside events, connect serial acquisition and the existing viewer/simulation
format with physical post-stop evidence, then route/commission the complete image
and collect fresh bounded motor trials. Electrical measurements remain servo
telemetry only, with uncalibrated current.
