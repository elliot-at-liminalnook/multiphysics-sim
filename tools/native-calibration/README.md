# Future native calibration acceptance

`acceptance.py` is written verification tooling, **not an executed receipt**.
Normal writing turns must not run it, build binaries, launch a viewer or capture
screenshots. The accepted 2026-10-02 verification
([verification-20261002.md](../../docs/verification-20261002.md)) only observed
virtual HW-01 connection and idle STOP; HW-02–HW-09 and LC1–LC3 acceptance remain
pending. Portable work at f541b05f remains set aside/unaccepted.

## Launch path

The driver launches exactly three owned executable kinds: `hx_virtual_bench`,
`serve_actuator_calibration`, and `sim-spatial` (a second `sim-spatial` only in
the labelled fixture phase). The calibration server is a **separate Rust
process**; the native Leg panel is its client and never opens serial. The server
still serves the browser calibration page, which remains the reference and is not
retired. Python here is reproducible orchestration only. Physics, acquisition,
validation and records remain shared Rust owners. No result promotes CAD or
registry properties. The bench uses identified-like **simulated, uncalibrated**
responses, not measured hardware evidence or independent FPGA safety qualification.

## Prerequisites for a future authorized verification pass

Freshly build the three binaries under the separately authorized verification
workflow and retain full build logs. Then write `fresh-build-receipt.json` with
the helper, which builds nothing:

```sh
python3 tools/native-calibration/acceptance.py receipt \
  --bench target/debug/examples/hx_virtual_bench \
  --server target/debug/examples/serve_actuator_calibration \
  --viewer target/debug/sim-spatial \
  --build-log runs/NEW-VERIFICATION/build.log \
  --out runs/NEW-VERIFICATION/fresh-build-receipt.json
```

Repeat `--build-log` once per retained log. The helper refuses, writing
nothing, when no build log is given, when any binary or build log is missing or
empty (or a binary is not executable), or when `--out` already exists (the file
is created exclusively). The receipt it writes:

```json
{
  "source_sha256": "<acceptance.source_hash()['sha256'] of the tree now>",
  "source_commit": "<full git rev-parse HEAD>",
  "source_dirty": "<true if git status --porcelain lists anything, untracked included>",
  "source_provenance": "<the whole source_hash() record>",
  "build_logs": [{"path": "<absolute path>", "bytes": 0, "sha256": "<SHA256>"}],
  "binaries": {
    "bench": {"path": "<absolute>", "bytes": 0, "sha256": "<SHA256 of hx_virtual_bench>"},
    "server": {"path": "<absolute>", "bytes": 0, "sha256": "<SHA256 of serve_actuator_calibration>"},
    "viewer": {"path": "<absolute>", "bytes": 0, "sha256": "<SHA256 of sim-spatial>"}
  },
  "written_by": "tools/native-calibration/acceptance.py receipt (builds nothing)",
  "written_at": "<Unix time>"
}
```

The run checks `source_sha256` against `source_hash()` at run time and each
`binaries.<label>.sha256` against the binary it is given; the other fields are
retained for the verifier. Write the receipt right after the build, from the
same tree: the helper hashes the tree as it is when the helper runs.

The hash algorithm is in `source_hash`. It covers:

- every file `git ls-files` lists under `crates/` and `web/`
- the root `Cargo.toml` and `Cargo.lock`, and rust-toolchain files if tracked
- untracked (not ignored) files under `crates/` and `web/`
- every literal `include_str!`/`include_bytes!` target of a hashed `.rs` file,
  wherever it lives (for example `examples/` JSON and fonts)

Each file is encoded as `name + NUL + bytes + NUL` into SHA256, in sorted name
order. A tracked file deleted from the working tree is recorded in
`deleted_tracked` and hashed as `name + NUL + "DELETED" + NUL`. The provenance
also records `untracked_included` and `included_assets_outside_roots`. An
include built with `concat!`/`env!` is not resolved; it is listed in
`unresolved_includes`, so the claim stays honest. Everything is retained in
`source-provenance.json` and `results.json`. Matching hashes bind the build
receipt to the source tree and the binaries; they are not proof that
compilation occurred, whether the receipt was written by the helper or by hand.
The verifier must inspect the build logs the receipt names. The driver never
builds and never accepts an arbitrary endpoint or serial override.

Example **future** invocation (do not run during writing):

```sh
python3 tools/native-calibration/acceptance.py \
  --fresh-build-receipt runs/NEW-VERIFICATION/fresh-build-receipt.json \
  --bench target/debug/examples/hx_virtual_bench \
  --server target/debug/examples/serve_actuator_calibration \
  --viewer target/debug/sim-spatial \
  --out runs/NEW-NATIVE-CALIBRATION \
  --screenshots
```

`run` is the default subcommand (`acceptance.py run …` is the same). Omit
`--screenshots` to capture nothing; it is off by default.

Use a new, short absolute output path (Unix socket path limits apply); an existing
output directory is refused. Do not use Python `-O`. The default total deadline is
1800 seconds, which is also the maximum (`--total-timeout` must be between 60 and
1800). The per-step budgets (90 s connects, 120 s tunes, 150 s learning and
sweep-all, 180 s + 300 s campaign) leave little slack under a shorter total. Every command job and
state assertion also has a local deadline. A port-bind/startup race fails and
retains evidence; it never attaches to an existing calibration session on purpose.

The only supplied config accepted has `serial: "virtual-capability-only"`.
Physical paths, PTY pathnames, missing serial fields and unknown values are
refused **before any subprocess starts**, including before reading git provenance.
`server.virtual.json` carries `output` and `viewer` placeholders under
`/nonexistent-…`: used directly, the server's `create_dir_all` fails before it
opens the bench (fail closed). The driver replaces output, token page and campaign
plan in an isolated copy; input and derived configurations are both retained. The
bounded plan copies the source gates/limits, selects A+B only, and records that
reduction.

Each viewer gets its own config directory under the output: the driver sets
`SIM_SPATIAL_CONFIG_DIR` (unified preferences and recent documents,
`app/recent.rs`), `SIM_SPATIAL_PREFERENCES` (legacy hardware preferences,
`robot/hardware/settings.rs`) and `SIM_LESSON_SETTINGS`, and removes an inherited
`PHENOMENA_EXHIBIT`. `HOME` is left alone. The operator's real preferences are
never read or written. The driver still sends `hold_others {on:true}` and
`drive_mode {mode:"pwm"}` explicitly and asserts them in `hardware_status.form`
(after connect and again after Robot re-entry).

The bench starts with `--capability-socket ABS_PATH --identity-file ABS_JSON`.
The server starts `CONFIG PORT --virtual-bench ABS_PATH`. It takes an already-owned
virtual transport capability and cannot open serial or fall back to it. A fixture
label or loopback URL confers no permission. The strict execution identity includes
schema, kind, bench UUID and server UUID. Native authorization also binds the
current connection generation. Server execution refuses missing, old or mismatched
identity headers with HTTP 409 and an error starting
`Calibration execution binding refused`. Ordinary refusals stay HTTP 400. STOP
bypasses identity. Readiness must match the bench-owned UUID, a still-live owned
server process whose log shows it bound the recorded port, and (for viewers)
`GET /` answering `service: sim-spatial` with `pid` equal to the owned child's pid.

## Two phases

**Positive HW-01–HW-09 path: direct.** The main viewer starts with
`--hardware <owned server URL>` and HW-01 asserts that `url` is exactly that. No
proxy sits between the viewer and the server.

The HW-01 "simulated disconnection" works like this:

1. Pause the owned server process with SIGSTOP. The paused flag is set before
   the signal is sent.
2. Keep it paused until `hardware_status` shows `stale` and then
   `authorization_revoked: true` (25 s deadline). Stale alone is not enough. An
   in-flight status request keeps the link "awaiting" for up to its request
   timeout plus a margin, and nothing is revoked in that window. Resuming early
   would let the link turn fresh, and a later `select` would really enable
   motor 2.
3. Send SIGCONT. Assert that revocation persists for the same generation.
4. Assert that REST `select` is refused with "authorization expired" and that
   the server still has no enabled motor.
5. Press `hardware:connect`. Require a new generation, no revocation and no
   ready session. Require `hardware:select_2` to be listed enabled again, which
   shows authorization is restored only for the new generation.

Cleanup always resumes a paused server first. **A `kill -9` of the driver
during HW-01 leaves the server frozen.** To recover, find the pid in
`processes.json` and run `kill -CONT <pid>`. Then STOP it (POST
`{"action":"stop"}` with the page token) or kill that server.

**LC1 refusal on the real server.** Out-of-scope `raw_step`, `gait_play` and
`sync_start` are refused natively with the remote-refusal text. Direct requests
with no identity headers and with a foreign server UUID get 409 plus the binding
text, and STOP with the same headers returns `stop_latched: true`, `enabled_id:
null`, `busy: false`. The owned server is then terminated and restarted on the
same port and output: the native viewer refuses with "authorization expired"
while stale and again before reconnect. The old identity headers get 409. STOP
with them still latches. Reconnect gets a new generation and no ready session.

**FIXTURE phase (labelled, separate).** Only after that, a second owned viewer
starts with `--hardware <proxy URL>`. The proxy forwards to the owned server
(timeout 15 s, above the native 12 s STOP timeout) and presents the execution as
`physical`, then (after reconnect) removes it (`unknown`). It never presents a
virtual identity. Non-object JSON, undecodable bodies and `execution: null` are
forwarded without crashing and without upgrading identity. Native REST and
`system_ui` `select` must fail with the virtual-requirement text, the listing
must show the same disabled reason, and the proxy's count of non-STOP command
POSTs must not change. The fixture viewer is stopped and terminated at the end
of the phase.

## Assertions and retained evidence

| Step | Native control ids / REST actions | Required observation (not HTTP success) |
|---|---|---|
| HW-01 | `hardware_status`, `hardware:connect`; REST `select` | Direct URL, `fidelity: virtual_simulated`, fresh age, no enabled motor. The server stays paused until `authorization_revoked` (not just stale). After resume, revocation persists, `select` is refused with "authorization expired", and the server has no enabled motor. Reconnect has a new generation, no revocation, no ready session, and `select_2` is enabled again. |
| HW-02 | `hold_others`, `drive_mode`; `hardware:select_1/2/3`, `hardware:set_disabled` | Form shows `hold_others: true`, `drive_mode: "pwm"`. Session ready per motor. A disabled motor leaves no ready session when selected. Re-enabled and selected again. |
| HW-03 | `hardware:jog_upper/lower`; REST `speed`, `jog_release` | Encoder movement, held feedback, opposing-input hold within 32 counts for 1 s. |
| HW-04 | `hardware:section_*`, `hardware:stop` | Top STOP listed and enabled while sections toggle; each STOP ends the jog (server `enabled_id: null`, not busy). Visual non-scrolling placement is judged from the `HW-04-stop-*` screenshots by the operator, not by the driver. |
| HW-05 | REST `loss {reason: focus_lost}`; `hardware:close`, `hardware:toggle_panel`; `mode:phenomena` then `mode:robot` | Drive ends with no ready motion. Closed panel reports `open: false`. Mode exit: direct server status shows `enabled_id: null` and not busy, and `hardware_status` is refused with "the active mode is phenomena". Re-entry reconnects idle with a new generation. `mode:build` is not used because Build refuses to open without a document (`switch/prepare.rs` `needs`). REST loss exercises the interruption consumer, not an OS focus event. |
| HW-06 | `hardware:capture_lower/upper/reference`, `hardware:reset_poses`; REST `speed`, `target`, `target_commit` | After poses are taught, `speed`/`target` of −1 and 101 are refused with "must be a number from". The encoder reaches each target, and the commanded target lies inside the four-count inset of the taught lower/upper. The reference pose is captured (non-null). Reset clears lower/upper. Motor 3 is taught too. |
| HW-08 | `hardware:tune_confirm`, `hardware:tune`, `hardware:stop` | The tune is still running when STOP is pressed, and the interrupted tune stops. Fresh tune shows at least 2 observed stages, finite gains, a retained record file inside the output, and the confirmation cleared. Repeated for motor 3. |
| HW-07 | `hardware:sweep`, `hardware:learn`, `hardware:sweep_all`, `hardware:stop`; REST `loss` | Saved-range travel stays within poses. Pause holds. Interruption keeps poses. `session.learning_terminal` shows `learning_complete` with at least three stops each way, `learning: false` and intent hold (no second Learn press, which would start a new run). Two taught motors complete two half cycles each. Another sweep-all is interrupted by STOP. |
| HW-09 | `hardware:campaign_confirm`, `hardware:campaign`, `hardware:campaign_resume`, `hardware:stop` | At least one saved receipt per completed stage (`*.execution.json` provenance files are not counted). Completed count retained across STOP. Receipt hashes unchanged after resume. Terminal report inside the output. Confirmation cleared. |
| LC1 real server | see "Two phases" | 409 plus binding text, latched STOP replies, expiry refusals and the new server UUID. |
| LC1 FIXTURE | second viewer via proxy | Virtual-requirement refusals with no crossings. |

The order is HW-01–HW-06, HW-08, HW-07, HW-09: tuning both taught motors before
sweep-all supplies its existing measured-model stopping envelope rather than
raising the adaptive bootstrap limit or pretending an untuned slow traverse fits
a short deadline (`calibration_sweep.rs`, tuned-braking branch). This virtual
prerequisite remains simulated; physical tuning must still be supervised.

Wait conditions treat missing fields, wrong types and `None` as "not yet".
The driver issues one command at a time and waits for each job to finish before
sending the next. So a STOP never lands on a pending change, and the native
"applied, but STOP was pressed afterwards" error is not expected on the
positive path; if it appears, the step fails. A remote `jog_release` is exempt
from freshness authorization natively. The driver never relies on a refused
release. On a
command or poll timeout the driver sends `DELETE /v1/jobs/{id}`. Cancellation in
sim-api is cooperative: a pending hardware ticket observes it, requests STOP and
fails. The driver then sends a direct server STOP so nothing queues behind the
stuck job. Both are recorded in `cancelled-jobs.jsonl` and `server-stops.jsonl`.

`results.json` records each step's outcome and final observed state.
`http.jsonl` retains every request and response. Also retained, on success,
failure or timeout:

- `identity-fixture.jsonl` (proxy crossings), `execution.json` and
  `replacement-execution.json`
- `source-provenance.json`, isolated configs and the per-viewer launch
  environment and root (`viewer-<label>-launch.json`, `viewer-<label>-root.json`)
  with their config directories
- bench/server/viewer logs
- the learned terminal state, sweep ends, tune stages and every record
- with `--screenshots`, `screenshots/<checkpoint>.png` for each captured checkpoint

Each failed deadline writes a `timeout-state-<ms>.json`. Failed steps are not
relabelled passing on HTTP success. The total result is false unless every
assertion passes.

Written `fixtures.py` (unexecuted) covers the following: physical/PTY/unknown
preflight refusal before any subprocess or git call; failure-receipt retention
with no STOP sent to an unowned server; refusal of an existing evidence directory;
the fail-closed config placeholders; the "not yet" wait semantics; the
latched-STOP reply check; proxy robustness against an in-process stub; receipt
helper refusals (no, missing or empty build log, empty binary, existing output)
with nothing written and no git call; step verdicts that screenshots never
change; the screenshot summary and PNG completeness check; and that this
README's checkpoint list equals the driver's.
Runtime and native fixtures exercise their real consumers separately.

## Cleanup

At its start, cleanup ignores SIGINT, SIGTERM and SIGHUP (SIGTERM and SIGHUP
otherwise raise an interruption that retains evidence). Each stage runs on its own
and records its outcome in `results.json` `cleanup`:

1. Resume a paused server.
2. Send a **direct server STOP**, decided by ownership rather than identity: our
   server child is alive and its log shows it bound the recorded port. The STOP
   carries no identity headers. A sent STOP whose reply lacks `stop_latched:
   true`, `enabled_id: null` and `busy: false` makes the run fail.
3. Send native `hardware_stop` to each still-running owned viewer whose `GET /`
   pid matches, polling its job for up to 5 s.
4. Terminate only the unreaped `Popen` children this driver started, newest
   first.
5. Close the proxy.
6. Close the logs.

No `pgrep`, process-group kill, port-based kill or retained-file deletion is
used. An OS `kill -9` of the driver cannot run cleanup. The server's leases and
the independent hardware safety stay authoritative. Failure receipts do not
promise a successful STOP if the server is unreachable.

## Screenshot checkpoints

Off by default: only `--screenshots` captures, so a writing turn never does.
With it, the driver asks the main viewer for its own REST `screenshot`
(`{"path": "<out>/screenshots/<checkpoint>.png"}`) at each checkpoint below.
The command answers once the capture is queued, and Bevy writes the PNG after
the next frame, so the driver polls the job and then the file, within 20 s per
checkpoint (and the total deadline). A screenshot counts as captured only when
the file exists, is non-empty, starts with the PNG signature and ends with the
IEND chunk. The command is refused, naming the cause, while the window is not
visible (minimized, covered, locked screen or another Space): keep the window
visible. An existing file is never overwritten. A stuck screenshot job is
cancelled with `DELETE /v1/jobs/{id}` only. It holds no hardware ticket, so no
server STOP is sent for it; a real viewer stall fails the next semantic command
under its own deadline.

Each attempt is recorded in its step's `screenshots` list as `{checkpoint,
path, ok, error}` (plus `bytes` and `sha256` when captured, `cancel` when a job
was cancelled). `results.json` `screenshots` reports `off`, or `captured` of
`planned` with the `failed` list and the checkpoints `not_reached` because an
earlier step failed. A failed or timed-out screenshot never fails a step and
never stops its remaining assertions. A captured one never passes a step:
step verdicts come only from semantic assertions. Captures happen before
cleanup; cleanup is unchanged. Worst case, each capture takes about 25 s (3 s
request, 20 s job and file polling, a final read), so 19 checkpoints add about
475 s under the same 1800 s total deadline. Because a capture can delay the
next action, HW-08 and HW-09 re-check that the tune or campaign is still running
immediately before STOP; otherwise STOP could land on finished work and test
nothing.

The checkpoints, in run order (`<step>-<checkpoint>`):

- `HW-01-identity`: first direct connection, virtual identity shown
- `HW-01-fresh`: the same link fresh, before the simulated disconnection
- `HW-01-stale`: the server stalled and the link stale
- `HW-01-reconnect`: after reconnect, the new generation authorized
- `HW-02-disabled`: motor 2 disabled with no ready session
- `HW-03-held`: held after opposing inputs, encoder and target shown
- `HW-04-stop-tune`: top STOP visible in the Tune section
- `HW-04-stop-campaign`: top STOP visible in the Campaign section
- `HW-04-stop-advanced`: top STOP visible in the Advanced section
- `HW-06-taught`: three taught poses
- `HW-06-target`: the encoder at the 75 % target
- `HW-06-reset`: poses reset
- `HW-08-stages`: the tune running, before it is interrupted
- `HW-08-terminal`: terminal gains, record and cleared confirmation
- `HW-07-learned`: learned stops, holding
- `HW-07-sweep-all`: sweep-all terminal
- `HW-09-saved`: the campaign with a saved stage
- `HW-09-stopped`: the campaign stopped, completed count kept
- `HW-09-terminal`: the terminal report

Screenshots supplement semantic receipts; animation alone never passes a step.
The actual Q/A keyboard, pointer release outside a jog button, OS focus/window
close and visible STOP placement still need native fixtures or an operator
reading the captures. Physical operator run sheets are in
[hardware-checklist.md](../../docs/hardware-checklist.md); this driver never runs
them.
