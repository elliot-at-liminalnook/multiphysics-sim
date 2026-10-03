# Leg in process — LIP1–LIP3

This batch has five required outcomes: (1) calibration, teaching, sweep, tune,
campaign, captures and exports in shared sessions; (2) Sim/Leg/Both gait,
mirror and Sync motors on shared acquisition; (3) no native hardware HTTP,
socket server or acquisition subprocess; (4) reference safety and truthful
physical/virtual records; (5) thin browser compatibility adapters and honest
CAD boundaries. Evidence below is **source review only, unexecuted**. No build,
test, viewer, server, screenshot or physical operation was performed.
Historical HTTP-era receipts are retained as historical evidence only.

## Ownership and migration decisions

The shared application owners are `sim_runtime::hardware::{calibration, bench}`.
`hardware::protocol` owns request builders, statuses, execution identity,
strict virtual scope and refusal categories. `hardware_client` remains a browser
compatibility client, re-exporting those types. Application libraries never
import it. `hardware::ownership::DeviceLease` is the one process-wide authority:
canonical physical device paths contend for one lease; virtual devices use an
explicit stable identity. The lease spans acquisition and final release handling.
The serial opener also retains OS exclusion; the process map does not replace it.

Native `Hardware` is global mode-owned UI state, written by the existing typed
HardwareAction apply and its JobResults consumer. Durable calibration, captures,
measured values and campaigns are runtime/file owned. UI entities retain existing
kit and mode lifetime rules. Input produces action occurrences; pending work is
held in durable run queues and jobs, and STOP is independently latched state.
The public pipeline remains Input → Actions → JobResults → SimSync → Present.
No simulation moves into Bevy frame systems. Runtime workers are caller owned;
the viewer runs them through jobs::RunThread, with discrete work through Job.

Local JSON configuration replaces URL/token discovery. Older inactive preferences
remain readable. Obsolete hardware URL/token arguments are refused by name with
local-configuration guidance, never contacted. Physical motion still needs an
operator at the window; virtual automation needs an explicitly simulated session
and freshly pinned generation/identity. A path, fixture label, URL or boolean in
a remote command cannot authorize physical motion.

CAD is outside LIP1–LIP3. The existing native CAD mode still reaches
`sim_runtime::cad_client` and RoboCAD's Python/OCCT service. This is a remaining
server dependency, not a completed §9 migration. Browser compatibility adapters
remain available; they are not part of the native launch path.

## Source map and workflow inventory

Reference citations below identify the pre-extraction source at `a4037f4e`.
Use `git show a4037f4e:<path>` for that version. Replacement citations describe
the current source. This is a reading trace, not executed parity.

| Workflow / task | Reference owner at a4037f4e | Current native entry → shared owner → effect |
|---|---|---|
| LIP1/LIP2 calibration inspect, select, enable, clear, teaching | `examples/serve_actuator_calibration.rs:686`, `:736`, `:780`, `:805`, `:1251` | `robot/hardware/handlers.rs:544` and existing typed apply → `hardware/calibration/session.rs:276` → serialized `worker.rs:215`, `:282`, `:335`, `:360`, `:900`; CalibrationBus acquisition, saved taught captures. |
| Tune, sweep, learning, campaigns | reference `:844`, `:971`, `:995` | typed handlers → session queue → `worker.rs:401`, `:588`, `:628`; `campaign.rs:86` owns resumable receipts/publication, shared sweep/characterization retains recorded bounds and models. |
| Gait Sim / Leg / Both and mirror | reference `:911`, `:1574`, `:1831` | `robot/hardware/session/sequences.rs` and `mirror.rs` → local Client → `worker.rs:497`, `gait.rs:256`; simulation remains shared runtime, hardware targets retain qualified timing and taught bounds. Sim mode does not acquire motion. |
| Captures, export, gait catalog | reference `:1251`, `:1543` and catalog/export handlers | `session.rs:221`, `:224`, `:227`, `:319` and `worker.rs:900`; completion occurs after capture save; calibration files and gait records retain their existing schema/provenance. |
| Sync motors and zero-drive inspection | `serve_motor_bench.rs:364`, `:376`, `:382`; acquisition executable launch `:168` | `HardwareAction::SyncInspect` (`actions.rs:218`) → `sync/apply.rs:12` → `sync.rs:296` dedicated Job → `hardware/local.rs:175` → bench `start_at_epoch`/Work. Live open/sample use the same bench App and device lease. Physical acquisition is direct `bench/acquisition.rs:511`; virtual acquisition uses existing Bench in process. |
| Lesson hardware exercises | old `lesson_lab.rs` TCP request/start/status implementation | `lesson/actions.rs:202` operator refusal → `lesson/lab.rs:80` dedicated Job → epoch-stamped select/lab command `:138`, `:160` → calibration `worker.rs:551`, `gait.rs:633`. `SIM_BENCH_CONFIG` replaces and explicitly refuses `SIM_BENCH_URL` (`lesson_lab.rs:98`). |
| Browser compatibility (outcome 5) | authoritative rules formerly in two server examples | examples now adapt HTTP/static/token startup to `hardware::{calibration,bench}`; `characterize_hx_bridge.rs` delegates callable acquisition CLI. No application behavior is owned only by an example. |

Paths in the native column are relative to `crates/sim-spatial/src/` for
`robot/` and `lesson/`, and `crates/sim-runtime/src/` for `hardware/` and
`lesson_lab.rs`. Reference example paths are under `crates/sim-runtime/`.
The reusable source owners are the shared hardware modules; source inputs are
local configurations, calibration files, measured actuator models and gait files.
Dependencies still requiring operator evidence are physical transport, watchdog
readback and motion/release timing. CAD migration is not a hardware dependency.

| Safety / LIP1–LIP3 parity | Reference | Replacement and observed reading result |
|---|---|---|
| Generation binding, capture completion, gait ownership/heartbeat, sweep update | calibration handler `:1521`, `:1543`, `:1574`, `:1588` | `calibration/session.rs:276` retains validation before dispatch; shared worker owns captures, gait and sweep state. Native pinned execution rejects stale generations and sequences. |
| STOP epoch versus queued select/clear/motion | handler `:1597`, `:1600`, halt `:1613`; worker `:622` queue expiry | `session.rs:267` stamps/checks epochs; select/clear compare and advance under the safety mutex (`:384`). `mod.rs:493`, `:568` latch/observe independently of acquisition. Halt/STOP survive stale stamps. |
| Ownership and expiry | calibration gait ownership and bench live heartbeat/reference validation | `hardware/ownership.rs` canonical device lease spans bus cleanup; bench Signals permanently cancel expired leases. Calibration, bench and lesson acquisition contend for the same authority. |
| Watchdog proofs, bounded targets, taught travel and recorded limits | reference select/enable/tune/motion branches above and acquisition example | moved worker branches and bench acquisition helpers retain supervisor checks and limits. Physical bus is direct `CalibrationBus::open_baud` (`acquisition/calibration_serial.rs:184`); virtual `in_process` (`:173`) has explicit simulated identity. |
| STOP during acquisition, publication and shutdown | reference observe_stop `:495`, worker `:564`, campaign `:2218` | independent latch interrupts ordinary work; worker BusGuard releases on unwind. `calibration/session.rs:53` marks closure and publishes uncertainty on failed startup; bench Work Drop cancels/releases. Final release is current-epoch evidence, never inferred from latch acceptance. Abrupt termination cannot publish new readback. |
| Lifecycle and authorization | existing native front-end refusal/loss rules | native Link/Client Drop latch STOP; focus/panel/mode/disconnect, sync inspection and replaced jobs retain ownership until cleanup. Physical automation remains refused; virtual authorization is freshly generation scoped. Lessons window loss (`lesson/lab.rs:188`) immediately latches and queues its existing typed stop. |

**Dependency audit (outcome 3).** Reading all native hardware consumers,
Lessons hardware acquisition, CLI/configuration and the shared hardware modules
finds no hardware HTTP client, acquisition child process or virtual socket-server
call. Legacy browser HTTP and standalone socket-reference code remain separately
available. `hardware_client` types moved to `hardware::protocol`; its remaining
transport is not imported by native production hardware. `--hardware`,
`--hardware-token-file`, `--motor-bench`, `--motor-bench-token-file` are explicitly
refused (`main.rs:447`); use `--hardware-config FILE` and
`--motor-bench-config FILE`. Old bench acquisition-executable fields are readable
but ignored: execution always calls the extracted library. Native Inspect bench
creates/refreshed inspection records without a CLI or server detour; disconnect
calibration first when its device lease is still held.

Virtual bench identity is **virtual_host_bench**: it exercises the existing Bench
host behavior, not installed FPGA timing or physical watchdog proof. Preserve
this label on all records and do not promote virtual results to physical evidence.
The design choice avoids inventing a fake FPGA simulator or socket fallback;
revisit only when a qualified shared supervisor model is available.

All five `leg-in-process:outcome-1` through `outcome-5`, and tasks
`leg-in-process:task-LIP1`, `task-LIP2`, `task-LIP3`, are implemented and reviewed
by reading. Compilation, executed parity and physical acceptance remain unverified.

## Acquisition source identity repair (LIP1 / LIP3)

The extraction in `b6b907e1` narrowed `source_blake3` to `acquisition.rs`,
although that file previously contained motion behavior now in `motion.rs.inc`.
That commit's identities and HTTP-era identities are historical, with their
original scope; no stored data is rewritten or promoted to comprehensive proof.

New runs use `sim-runtime-acquisition-source-v1`. The common provenance owner
builds a deterministic, name-sorted list of compile-time production source bytes.
Its composite BLAKE3 is length-framed over the scheme, constituent names and exact
contents. `source_identity` records the schema, composite hash and each named
constituent's hash, byte length and relative artifact path. Tests and example
wrappers are not substitutes for the production implementation. The conservative
shared set covers motion and specialized helpers, serial/safety/virtual transport,
application cancellation/ownership, controller plans, the shared Rhai controller engine, its registered control
primitives and acquisition dependencies.
Changing any included production constituent changes all acquisition identities,
even if a particular run does not execute that constituent.

Before effects, exact bytes are published under `sources/<repository-path>` and
`source-manifest.json` using shared immutable publication and file/directory sync.
An existing artifact is accepted only if its bytes match and durability can be
confirmed; mismatches or publication uncertainty refuse acquisition. Companion
artifacts extend the retained historical `transport-source.rs.txt` filename;
that compatibility file alone no longer represents the split implementation.
The source manifest describes implementation identity, not build flags, dependencies
outside its declared set, physical timing qualification or stationary readback.

Specialized controller/device/FPGA recordings retain their existing schema and
individual host/RTL/bitstream identities, add the shared composite source identity,
and carry its manifest with the initial evidence. Sweep checkpoints identify the
new scheme and runner composite. Old checkpoints remain readable; continuing one
under a different identity is refused by the existing runner-mismatch gate.
Recording consumers retain flexible source hash maps and JSON initial evidence;
bench application results preserve the acquisition record, and sweep review reads
JSON without converting historical identities to the new scheme.

Reference/replacement evidence and publication/consumer traces are recorded below
as source review only. No retained artifact was produced by executing acquisition
in this repair, and no physical acceptance or parity is claimed.

| Producer / consumer | Reference before repair (`b6b907e1`) | Replacement reading evidence |
|---|---|---|
| Generic acquisition | `hardware/bench/acquisition.rs:204` hashed only acquisition.rs; original `a4037f4e` characterize example contained the moved motion implementation | `acquisition.rs:204` retains sources, `:205` attaches composite/schema manifest, `:206` publishes initial record before `open()`; `provenance.rs:466` hashes all 112 unique named constituents, including motion, `:484` verifies/publishes exact bytes, `:500` publishes manifest last. |
| Specialized device / FPGA | `hx_device.rs.inc:45`, `hx_fpga.rs.inc:31` retained only acquisition.rs transport snapshot | `hx_device.rs.inc:40`, `hx_fpga.rs.inc:26` retain full companion set before supervisor effects. Device `:348`, `:358` and FPGA `:57`, `:60`, `:109`, `:298` preserve manifest/composite through initial metadata and terminal capture/results. |
| Controller and sweep | controller used only its own host source; sweep runner concatenated three partial sources | `hx_controller.rs.inc:57`, `:75`, `:115`, `:273` preserve source manifest/composite through preflight and final result. `hx_sweep_support.rs.inc:142` uses composite runner identity; `:182` retains before STOP; `:467` records scheme/manifest in checkpoint and `:564` in terminal result. Resume mismatch gate remains `:154`. |
| Safety-probe / sweep run identities | generic branch hashed individual extracted helpers | `acquisition.rs:367`, `:391` explicitly record the new scheme with composite hashes; outer source_identity stays attached through terminal updates. |
| Virtual bench | no comprehensive source companion manifest | `virtual_run.rs:31` retains full sources, `:34` attaches composite, `:36` durably publishes initial record before Bench construction; virtual fidelity remains host Bench, not physical proof. |
| Application and readers | flexible historical recording structures | `bench/mod.rs:295` reads run.json into Value and preserves it in application-result/status; `controller_refinement/fpga.rs:132` uses sources map and initial Value; `recording.rs:27` uses source_hashes map and initial_registers Value. `electrical_measurements.rs:70` accepts hex hashes, including the composite. `sweep_review.rs:63` reads historical run/checkpoint Values without requiring the new source scheme. |

All runtime paths above are relative to `crates/sim-runtime/src/`. Retention
publication uses `publication.rs:87` (ImmutableNew) and `:129` (confirm-existing),
whose failures preserve visible artifacts and propagate uncertainty. Subsequent
legacy run/record updates still use their existing filesystem writers: a crash
can lose terminal metadata. The independently synchronized source manifest and
companions retain the exact implementation even then; this repair does not claim
new terminal-publication or hardware-release guarantees.

## Operator run sheets (not executed)

Do these only after a separately authorized fresh-binary verification pass.
The agent did not drive the leg. Keep the fixture supported and the operator
present, with physical motor power cutoff available. Use the existing recorded
limits and measured models; do not raise limits to make a check pass.

1. **Connection/ownership (HW-01, HW-02).** Open Robot mode with a local
   calibration configuration. Confirm PHYSICAL or VIRTUAL matches the configured
   transport, records and identity. Inspect with torque off. While calibration
   owns its bus, try Sync motors: it must refuse exclusive ownership, not open
   a second transport. Disconnect calibration, then inspect the bench before
   authorizing sync. Confirm a replacement connection requires fresh authorization.
2. **Teaching and STOP (HW-03–HW-05).** Select only after watchdog proofs.
   Within the taught window, hold-to-move then release to hold. Capture a pose;
   completion must appear only after save. Press STOP during select, a queued
   move, an in-flight capture, sweep, tune and campaign. Watch eventual
   all-axis torque-off/readback status. A latch acknowledgement is not stationary
   proof; if readback is unverified, cut motor power and retain records. Repeat
   release on focus loss, panel close, mode exit, disconnect and ordinary close.
3. **Records and playback (HW-06–HW-11).** Use an isolated output destination,
   keep accepted measurements unchanged, and run sweep/tune/campaign within
   existing bounds. Check cancelled and rejected records remain. Requalify gaits
   as required by gait-lab before physical Leg/Both playback. Compare Sim, Leg
   and Both clocks, stale-frame refusal and mirror provenance; STOP must remain
   reachable in every section. Export and reopen records, checking identity,
   timestamps, limits and simulated labels.
4. **Sync and leases (HW-12–HW-16).** Inspect first, authorize at the window,
   stream targets with the recorded FPGA timing and limits. Lose focus and stop
   samples/heartbeats; verify timeout release, final recorded result and independent
   FPGA watchdog. Remote physical start/select/sync commands must refuse by name;
   STOP remains available. Repeat while a long acquisition is in progress.

Abrupt process termination cannot run Rust destructors or publish new readback.
The independent FPGA supervisor/watchdogs remain the boundary in that case.
Ordinary close requests STOP, but neither host latch acceptance nor a detached
worker proves stationary motors. Actual release/readback, timing, platform
serial behavior, publication durability and physical parity remain unverified.
