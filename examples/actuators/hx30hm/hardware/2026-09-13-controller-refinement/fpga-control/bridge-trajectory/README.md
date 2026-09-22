# Upload-enabled controller bridge — validation checkpoint

The optional bridge now accepts sealed trajectory uploads and reports each
CONFIG/ROW/SEAL/STATUS result. The [protocol](source/servo/trajectory-bridge.md)
and shared Rust transfer preserve plan identity, raw receipts, cancellation and
rejections. Uploads cannot arm motors or execute trajectories. **No START exists
in this image and no new image has been loaded onto hardware.**

Eight UART upload groups and the retained A1 controller/supervisor tests pass.
Seven Rust upload/transfer tests parse real RTL reply bytes and check ordered
progress, cancellation, errors and provenance. Two separate sets of 4,096 vectors
verify the unchanged generated controller and its narrower, already-bounded
bridge bindings. The compact supervisor passed formal equivalence at all 1,157
comparison points. Thresholds, leases and motor limits were not relaxed.

**Build result:** the compact image passed final routed timing at **53.71 MHz**
against **50 MHz**. The earlier 43.18 MHz placement estimate was preliminary.
This result qualifies the upload-only design's timing, not the future autonomous
image or physical behavior. No hardware commissioning has occurred. The first
larger build failed placement; two intermediate builds were deliberately stopped
to apply resource reductions. Their logs are retained separately.

Bitstream packaging completed successfully. The retained image is
`impl/bridge_trajectory.fs`; source, image and evidence hashes are in
`manifest.json`. The intermediate routing checkpoint is retained as history,
while `verification/compact-build.log` contains the final successful build.

The [bench inspection](bench-inspection/summary.json) contains 46 successful
read-only transactions on the previously loaded image: all nine motors reported
torque disabled, PWM/speed zero, 12.1–12.4 V and 48–54 C. It contains no new motion
trial. Current readings are uncalibrated servo register counts, not battery amps.

## Reproduce

With the hardware toolchain on PATH, run from `source/servo`:

```
make sim-bridge-trajectory sim-buffered-control sim-control sim-bounded-control
make bridge-trajectory
```

These targets do not program the board. From this artifact root, the formal
comparison is `yosys -s verification/equivalence.ys`. In the simulator workspace,
`cargo test -p sim-runtime --test fpga_upload` exercises the current shared API;
the corresponding source used for this checkpoint is retained in `source/rust`.

Next: finish timing validation, connect the [transaction core](../autonomous-transactions/README.md),
add bounded start/event/terminal recording and Rust acquisition, then commission
the complete image before new measured motor trials. Existing physical accuracy
failures remain unchanged.
