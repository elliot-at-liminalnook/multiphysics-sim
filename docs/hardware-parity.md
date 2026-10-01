# Hardware front end: parity ledger

**Purpose.** One row per feature of the browser's hardware pages, with the
native Leg calibration panel's owner for it, so the epic's claim of exact
feature parity (docs/architecture/native-viewer.md §8) can be checked row by
row. The browser pages are the reference:

- `web/viewer/calibration-ui.mjs` (327 lines): rows `CAL-…`
- `web/viewer/actuator-motion-view.mjs` (38): rows `AMV-…`
- `web/viewer/calibration-mirror.mjs` (138): rows `MIR-…`
- `web/viewer/hardware-sync.mjs` (33, installed by `web/viewer/viewer.js:428`): rows `SYNC-…`
- server endpoints the pages call: rows `EP-…`
- the native panel's own surface, which the pages do not have: rows `NAT-…`

Browser references are `file:line` in the file named by the table's heading.
Native owners are `file` + function (each checked with `grep 'fn <name>'`
against the final code); `hw/` is `crates/sim-spatial/src/robot/hardware/`,
`hc/` is `crates/sim-runtime/src/hardware_client/`. Rows added after review
are numbered after the original ones (CAL-144 on, SYNC-31 on, NAT-10 on) and
placed in the table they belong to.

**Status legend.**

- `done`: built, tested and traced in code.
- `done-by-reading`: written and traced by reading; not compiled or run yet.
- `needs-hardware-checklist (HW-nn)`: only the leg can show it; the step in
  [hardware-checklist.md](hardware-checklist.md) that does.
- `deliberately-different (reason)`: the native panel differs on purpose; the
  reason is given, and the decision is recorded in native-viewer.md
  ("Hardware front end (2026-09-30)", Decisions and Deviations).

The browser pages stay available at the servers' URLs until the user signs
off the hardware checklist.

## calibration-ui.mjs

### Panel, placement and connection

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-01 | The panel installs only when the page carries a `calibration-token` meta (:4-5); it is part of the page the calibration server serves. | `hw/actions.rs` `connect`, `enter` (with `--hardware`); `hw/handlers.rs` `handle` (`Connect`); `hc/token.rs` `connect`, `discover`, `from_page` (reads the same meta from `GET /`), `read_file` (`--hardware-token-file`) | deliberately-different (the native viewer is not served by the calibration server: it connects to one, from `--hardware URL` or the panel's Connect with the default `http://127.0.0.1:4194`, `hw/mod.rs` `DEFAULT_CALIBRATION_URL`) |
| CAL-02 | Header toggle "Leg calibration" (:9) shows and hides the panel (:323). | `robot.rs` `setup` (header button, `HardwareAction::TogglePanel`); `hw/handlers.rs` `handle` (`TogglePanel`) | done-by-reading |
| CAL-03 | Panel titled "Leg calibration", labelled "Physical leg calibration" (:10-11). | `hw/panel_sections.rs` `top_bar` (title); `hw/panel.rs` `spawn` (`AccessibleLabel::new("Physical leg calibration")` on the dock) | done-by-reading |
| CAL-04 | The top bar (title, Stop, ×) stays on screen while the panel scrolls (:7 `.top{position:sticky}`, :11). | `hw/panel.rs` `spawn` (the top bar is a sibling above the scroll area); `hw/panel_sections.rs` `top_bar`, and `section` (each section header also carries a Stop) | needs-hardware-checklist (HW-04) |
| CAL-05 | Intro "Select a motor. Hold to move. Release to hold its pose." (:12). | `hw/panel_sections.rs` `body` (`HINT`) | done-by-reading |
| CAL-06 | Static explanations: tune (:27), campaign (:32), gait (:37, :42), angle note (:49), colour key and "Changing tabs stops drive" (:51), control modes (:55), PWM (:57). | `hw/panel_sections.rs` `body` (`TUNE_TEXT`, `CAMPAIGN_TEXT`, `GAIT_TEXT`, `GOVERNOR_TEXT`, `ANGLE_NOTE`, `TARGET_NOTE`, `SERVO_NOTE`, `PWM_NOTE`), same words | done-by-reading |
| CAL-07 | The "Changing tabs stops drive" wording (:51). | `hw/panel_sections.rs` `TARGET_NOTE` (the page's sentence verbatim; the window losing focus is the native "tab change", CAL-31) | done-by-reading |
| CAL-08 | Collapsible sections with initial state: Tune open (:26), Campaign closed (:31), Gait closed (:36), Recent leg runs closed (:47), Mirror open (:52), Advanced closed (:53). | `hw/mod.rs` `Section::open_initially`; `hw/handlers.rs` `handle` (`ToggleSection`); `hw/panel.rs` `refresh` | done-by-reading |
| CAL-09 | Opening Gait playback loads the gait list once, if empty (:272). | `hw/handlers.rs` `handle` (`ToggleSection{Gait}` → `LinkCommand::LoadGaits`); `hw/session/sequences.rs` `load_gaits` | done-by-reading |
| CAL-10 | The toggle reopening the panel calls `mirror.begin()` (:323). | `hw/mirror_panel.rs` `mirror_sync` (`want` turning true → `Mirror::begin`, then `Mirror::prepare`) | done-by-reading |
| CAL-11 | Stale status is not marked (the page shows the last answer). | `hw/link.rs` `LinkSnapshot::stale` (older than `STALE_AFTER`, 2.4 s); `hw/view.rs` `render`, `PanelView::block` | deliberately-different (native addition: a status older than four idle polls is shown as stale, never as live, and motion controls are blocked) |

### Requests, identity and loops

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-12 | One client id per page, `crypto.randomUUID()` (:64). | `hc/http.rs` `process_client_id` (one 36-character UUID per viewer process, made on first use), reused by every connect and reconnect (`hc/token.rs` `connect`) | done-by-reading |
| CAL-13 | Headers `X-Control-Token`, `X-Client-Id`, `Content-Type: application/json`; GET without a body, POST with `JSON.stringify` (:66). | `hc/http.rs` `Client::get`, `Client::post`, `send`; `hc/mod.rs` `Body::text` (members in the page's order), `js_number`, `js_number_text` (ECMAScript `Number::toString`: `0.0000032`, `1e-7`, `1e+21`, -0 as `0`) | done-by-reading |
| CAL-14 | A non-2xx answer throws its `error` field (:66). | `hc/http.rs` `exchange` → `ClientError::Server` (the `error` field verbatim; `hc/mod.rs` `fmt` prints it) | done-by-reading |
| CAL-15 | An error answer without an `error` field reads "Request failed" (:66). | `hc/http.rs` `exchange` ("Request failed (HTTP {status})") | deliberately-different (the HTTP status is added: it is the only fact such an answer carries) |
| CAL-16 | `send(action, extra)` = `{action, id, sequence: ++sequence, ...extra}`; `id` is null before a motor is chosen (:67). | `hc/calibration.rs` `command`; `hw/link.rs` `Link::sequence` (shared with `stop_now` and `hw/handlers.rs` `post_stop_on_leave`, so sequences only grow) | done-by-reading |
| CAL-17 | STOP is sent with `keepalive` so it survives the page going away (:66 `keepalive: body?.action==='stop'`). | `hw/link.rs` `stop_now` (fresh connection on a `jobs::Pool::Dedicated` job, `complete_on_drop`, `STOP_TIMEOUT` 12 s; never queued behind the link thread); on window close also `hw/handlers.rs` `post_stop_on_leave` (`hc/http.rs` `Client::send_only`: written, answer never read) | needs-hardware-checklist (HW-05, HW-14) |
| CAL-18 | `epoch` drops answers to requests in flight when STOP or a new select happened (:125, :128-130, :150-153, :326). | `hw/link.rs` `Link::epoch`, `stop_now` (bumps it and returns the new epoch); `hw/session.rs` `bump_epoch`, `stop_applied`, `interrupted`, `select_motor`, `begin`, `poll` | done-by-reading |
| CAL-19 | Status poll every 600 ms, 150 ms while a motion session or a leg gait runs, after each answer (:326). | `hw/link.rs` `POLL_IDLE`, `POLL_ACTIVE`; `hw/session.rs` `poll`, `next_deadline` | done-by-reading |
| CAL-20 | The poll adopts status only if the epoch is unchanged and nothing is busy or starting; `enabled_id` other than the motor without a session → not ready; a target session now holding → intent hold + update; a session the server ended → clear it (:326). | `hw/session.rs` `poll` | done-by-reading |
| CAL-21 | A failed poll stops if ready and shows the error (:326). | `hw/session.rs` `poll` | done-by-reading |
| CAL-22 | Heartbeat every 100 ms: `motion_update {run_id, ...input()}` while a session is open (:325, :133-135). | `hw/link.rs` `HEARTBEAT`; `hw/session.rs` `run_due`, `update` (skipped while a STOP is pending) | needs-hardware-checklist (HW-03) |
| CAL-23 | A failed heartbeat whose session the server already ended adopts that final status (:136-141). | `hw/session.rs` `update` → `beat_now`, `motion_failed`; periodic failures from the beat via `beat_failure` (acted on only when run and epoch still match) | done-by-reading |
| CAL-24 | Any other failed heartbeat stops and shows the error (:142). | `hw/session.rs` `motion_failed` | done-by-reading |
| CAL-25 | `input()`: `speed_counts_s`, `drive_pwm = round(PWM % × 10)`, `motion`, `target_raw`, `hold_others`, `drive_mode`, in that order (:70). | `hc/calibration.rs` `Input::members`; `hw/session.rs` `input`, `speed`, `drive_pwm`; `hw/link.rs` `Inputs` | done-by-reading |

### STOP and loss of control

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-26 | Stop button "Z Stop" (:11, :297). | `hw/panel_sections.rs` `top_bar`, `section` (a Stop in every section header); `hw/actions.rs` `buttons` → `apply` → `hw/handlers.rs` `handle` (`Stop`) → `hw/actions.rs` `stop_immediate` → `hw/link.rs` `stop_now` | needs-hardware-checklist (HW-04) |
| CAL-27 | Z or Escape stops, whenever the panel is shown, even in a field (:292). | `hw/actions.rs` `keys` (the panel has no text fields) | needs-hardware-checklist (HW-04) |
| CAL-28 | `stop()`: ends a leg gait and sweep-all, bumps epoch, clears keys and intent, run, starting, sweeping, learning, ready; sends `stop` and adopts its answer, or shows its error (:125). | `hw/actions.rs` `stop_immediate` (`LinkCommand::Stopped { epoch }`, the epoch `stop_now` bumped to, so a second STOP pressed before the first is applied stays pending); `hw/session.rs` `handle` (`Stopped` → `stopped_locally`, `stop_applied`; `StopAnswered` adopts or shows), `stop` (the link's own STOP) | needs-hardware-checklist (HW-04) |
| CAL-29 | `stop()` with no motor chosen sends no request (:125 `if(id==null)return`). | `hw/link.rs` `stop_now` (no job for `None`); `hc/calibration.rs` `stop` (takes `Option<u8>`) | done-by-reading |
| CAL-30 | `loss()` stops only if a motor is ready, starting or in a session (:321). | `hw/handlers.rs` `loss`; `hw/link.rs` `drive_active` (ready, starting, run, busy, sweep-all, tuning, campaigning, leg gait); `hw/session.rs` `handle` (`LinkCommand::Loss` re-checks on the link's newer state) | deliberately-different (safety: the page leaves a leg gait, tune, campaign or sweep-all driving after the tab is hidden; here any drive stops on the immediate path) |
| CAL-31 | `visibilitychange` to hidden stops drive (:322). | `hw/actions.rs` `window_loss` (Bevy `WindowFocused { focused: false }` → `Loss { FocusLost }`) → `hw/handlers.rs` `loss` | needs-hardware-checklist (HW-05) |
| CAL-32 | `pagehide` stops drive (:322). | Window close: `hw/actions.rs` `window_loss` (`WindowCloseRequested` → `Loss { Leaving }`) → `hw/handlers.rs` `loss` (always `stop_immediate`, plus `post_stop_on_leave`). Leaving Robot mode: `hw/actions.rs` `leave` (OnExit: `stop_immediate`, `LiveSync::stop_ours`, then `jobs::drop_off_thread`). Link drop: `hw/session.rs` `shutdown` (STOP when `drive_active`; `hw/link.rs` `Link::spawn` sets join bound zero) | needs-hardware-checklist (HW-05, HW-14) |
| CAL-33 | × and the toggle closing the panel stop drive and end the mirror display (:323). | `hw/handlers.rs` `close` (`loss(PanelClosed)`, `open = false`); `hw/mirror_panel.rs` `mirror_sync` (`want` false → `Mirror::end`) | needs-hardware-checklist (HW-05) |

### Motor selection

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-34 | Motor chips Knee 1, Worm 2, Belt 3 (:13). | `hw/view.rs` `chips`: always the page's three fixed chips (`DEFAULT_MOTORS`), labels never from calibration roles or ids; disabled from `calibration.axes[id].disabled` (:84); `hw/panel_sections.rs` `chips`; `HardwareAction::Select` | done-by-reading |
| CAL-35 | The chosen chip is pressed; chips are disabled while busy (:84). | `hw/view.rs` `chips`; `hw/panel.rs` `control_list`, `refresh` | done-by-reading |
| CAL-36 | A disabled motor's chip is faded, struck through, titled "Disabled" (:7 `.motor.off`, :84). | `hw/view.rs` `chips` (label prefixed "⊘ ") | deliberately-different (the kit has no strike-through style or tooltips: the "⊘ " prefix marks it) |
| CAL-37 | Choosing a chip during sweep-all sets "Sweep-all stopped: another motor was chosen." (:157). | `hw/session.rs` `handle` (`Select`) | done-by-reading |
| CAL-38 | Choosing a disabled motor stops and chooses it without a select request (:127). | `hw/session.rs` `select_motor` | done-by-reading |
| CAL-39 | Select: epoch bump, clear input, busy; `select {hold_others: !solo and (holdAll or the checkbox)}`; ready on success, message on error (:128-131). | `hw/session.rs` `select_motor`; `hc/calibration.rs` `select` | needs-hardware-checklist (HW-02) |
| CAL-40 | "Hold the other enabled motors in place while one moves", checked by default (:15). | `hw/link.rs` `Inputs::default`; `hw/handlers.rs` `handle` (`HoldOthers`, saved) | done-by-reading |
| CAL-41 | Disable button text "Disable this motor" / "Enable this motor"; disabled without a motor, while busy or during sweep-all (:85). | `hw/view.rs` `render` | done-by-reading |
| CAL-42 | Disabling stops first, then `set_disabled {disabled}`; enabling sends it directly; errors show (:158-162). | `hw/session.rs` `set_disabled`; `hc/calibration.rs` `set_disabled` | needs-hardware-checklist (HW-02) |

### Status, sequence and warnings

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-43 | Status while busy: "Connecting and checking this motor at zero drive…" (:87). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-44 | Status without a motor: "Choose the motor you want to calibrate." (:18, :87). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-45 | Otherwise the server's `message`, else "Ready" (:87). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-46 | A disabled motor, not busy: "<role> is disabled. Enable it to move it." (role, else "This motor") (:89). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-47 | Ready, not busy, poses from another encoder session: "Saved poses are from an earlier session and are ignored until re-taught; manual moves still work." (:90). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-48 | Ready, not busy, beyond a taught pose: "Beyond the saved {upper or lower} pose. Move back inward freely; driving further out is blocked." (:90). | `hw/view.rs` `status_line` | done-by-reading |
| CAL-49 | `outsidePose()`: none without a reading; `reference` when the axis's `coordinate_session` differs from the server's; direction reversed when upper < lower (or `reverse` with fewer than two poses); `upper`/`lower` when past that pose (:75-81). | `hw/view.rs` `outside_pose`; `hw/session.rs` `outside_pose` (the same rule on the link thread, for Reset poses) | done-by-reading |
| CAL-50 | Sequence line (`sequenceText`), hidden when empty (:16, :86). | `hw/view.rs` `render`; `LinkSnapshot::sequence_text` (field); `hw/panel.rs` `refresh` (empty texts hidden) | done-by-reading |
| CAL-51 | Warnings: from the latest sample and every sweep axis, each added when it differs from the newest, newest first, at most 6, "time · text" per line (:92-95). | `hw/session.rs` `merge_warnings` (from `render`); `hw/view.rs` `render`; `LinkSnapshot::warnings` (field) | done-by-reading |
| CAL-52 | Warning time stamps in the browser's local time (:93 `toLocaleTimeString`). | `hw/session.rs` `time_of_day` | deliberately-different (UTC time stamps, labelled "UTC": the viewer carries no timezone database) |
| CAL-53 | Telemetry line "V · °C · encoder · effort %" (:119). | `hw/view.rs` `render` | done-by-reading |
| CAL-54 | Capture message line (`state.capture_message`) (:23, :118). | `hw/view.rs` `render` | done-by-reading |

### Hold-to-move and speed

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-55 | "Q Upper ↑" and "A Lower ↓" enabled when ready and not busy (:19, :91). | `hw/view.rs` `render` (`jog_enabled`); `hw/panel.rs` `control_list` | done-by-reading |
| CAL-56 | A held jog button is highlighted while its direction is in a session (:96). | `hw/view.rs` `render` (`held_upper`, `held_lower`); `hw/panel.rs` `refresh` | done-by-reading |
| CAL-57 | Pointer: left button only, when ready; press moves, release, cancel or lost capture releases (:284-285). | `hw/actions.rs` `jog_buttons` (`JogPress` on press, `JogRelease` when the press ends) | needs-hardware-checklist (HW-03) |
| CAL-58 | A focused jog button: Space or Enter press moves, release releases (:286-287). | none | deliberately-different (kit buttons take no keyboard focus; Q/A are the keyboard path, CAL-59) |
| CAL-59 | Q/A anywhere while the panel is shown, not with Ctrl/Meta/Alt, not while typing in a field, repeats ignored, only when ready (:290-294). | `hw/actions.rs` `keys`; `hw/handlers.rs` `handle` (`JogPress`: nothing unless ready and not busy) | needs-hardware-checklist (HW-03) |
| CAL-60 | Q and A held together → hold (:294). | `hw/handlers.rs` `handle` (`JogPress` with the other held → `LinkCommand::BothKeys`); `hw/session.rs` `handle` (`BothKeys`) | needs-hardware-checklist (HW-03) |
| CAL-61 | Releasing a held key releases (:296). | `hw/actions.rs` `keys`; `hw/handlers.rs` `handle` (`JogRelease`); `hw/session.rs` `release` | needs-hardware-checklist (HW-03) |
| CAL-62 | The page takes A from WASD steering while the panel is shown (capture-phase handler, :290-294). | `robot/actions.rs` `motion_keys` (A dropped from WASD while `Hardware::open`) | done-by-reading |
| CAL-63 | `move(direction)`: intent, sweeping and learning off, `begin()` (:155). | `hw/session.rs` `move_` | done-by-reading |
| CAL-64 | `release()`: an upper/lower intent becomes hold, then one heartbeat (:156). | `hw/session.rs` `release` | done-by-reading |
| CAL-65 | `begin()`: only when ready, not busy, not starting; an open session gets a heartbeat instead; invalid PWM refuses; `motion_start` then its `run_id` then a heartbeat; an error makes it not ready (:146-154). | `hw/session.rs` `begin`; `hc/calibration.rs` `motion_start` | done-by-reading |
| CAL-66 | Movement speed slider 0–100, starting at 0 (:20). | `hw/panel_sections.rs` `body` (kit slider); `hw/actions.rs` `sliders` → `HardwareAction::Speed` | done-by-reading |
| CAL-144 | End labels "Slow crawl" and "Faster" under the speed slider (:20). | `hw/panel_sections.rs` `body` | done-by-reading |
| CAL-67 | Speed curve `5 × (max / 5)^(slider / 100)` counts/s, `max` from `maximum_speed_counts_s`, else 500 (:69). | `hw/view.rs` `speed`; `hw/session.rs` `speed` | done-by-reading |
| CAL-68 | Speed label "x.xx°/s motor", plus " · limited to x.xx°/s here" while moving and the permitted speed is under 95 % of it (:97). | `hw/view.rs` `render` | done-by-reading |
| CAL-69 | Moving the speed slider sends a heartbeat (:298). | `hw/handlers.rs` `handle` (`Speed` → `LinkCommand::SpeedChanged`); `hw/session.rs` `handle` → `update` | needs-hardware-checklist (HW-03) |

### Dial, readout and target

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-70 | Position readout: angle + " motor", or "—" without a reading (:98). | `hw/view.rs` `render` | done-by-reading |
| CAL-71 | Angle formatting `counts × 360 / 4096` to one decimal, "°" (:72). | `hw/view.rs` `angle`, `fixed` | done-by-reading |
| CAL-147 | Every `toFixed` rounds as JavaScript does (an exact binary tie rounds away from zero). | `hw/view.rs` `fixed` (exact ties only, checked with a fused multiply-add residual; everything else as `format!`); -0 prints as "0.0", a negative value rounding to zero keeps "-" as in JS; also used for the sync and mirror numbers (`hw/sync_panel.rs` `stats_text`, `hw/sync.rs` `reading_lines`, `hw/mirror.rs` `degrees_text`) | done-by-reading |
| CAL-72 | `fraction(raw, axis)`: between taught poses when both exist, else the encoder turn fraction (reversed with `reverse`) (:73). | `hw/view.rs` `fraction` | done-by-reading |
| CAL-73 | Needle maths: angle `π(1 − clamp(f, 0, 1))`, end `(150 + 85 cos, 85 − 65 sin)` in a 300×95 box (:74). | `hw/dial.rs` `needle_end` | done-by-reading |
| CAL-74 | Measured needle (green) at the position, requested needle (blue, dashed) at the latest target or the position; drawn only with a reading (:99). | `hw/dial.rs` `rasterize`; `hw/view.rs` `render` (`needles` None keeps the last drawing); `hw/panel.rs` `refresh`; caps as the page's SVG: track butt ends with round inner joins, measured needle round, requested needle butt with dash 5 4 (`hw/dial.rs` `stroke`) | done-by-reading |
| CAL-75 | Dial arc with "Lower" and "Upper" labels (:48). | `hw/dial.rs` `rasterize` (arc); `hw/panel_sections.rs` `body` (the two captions) | done-by-reading |
| CAL-76 | Target slider enabled when active, both poses taught and poses are not from another session (:101). | `hw/view.rs` `render`; `hw/panel.rs` `refresh` (`InteractionDisabled`) | done-by-reading |
| CAL-77 | Target slider follows the target (or position) fraction unless it is being dragged (:102). | `hw/view.rs` `render` (`target_value`); `hw/panel.rs` `refresh`, `slider_fraction` (not while held) | done-by-reading |
| CAL-78 | Target label "lower → upper" or "teach both poses first" (:103). | `hw/view.rs` `render` | done-by-reading |
| CAL-79 | Dragging: `target_raw = lower + s·4 + (upper − lower − s·8) × v/100`, intent target, sweeping and learning off, `begin()` (:299). | `hw/actions.rs` `sliders` → `hw/handlers.rs` `handle` (`Target` → `LinkCommand::Target`) → `hw/session/buttons.rs` `target` | needs-hardware-checklist (HW-06) |
| CAL-80 | Releasing the target slider sends one heartbeat (:300). | `hw/actions.rs` `sliders` (`TargetCommit`); `hw/session.rs` `handle` → `update` | done-by-reading |

### Poses, sweep and learn

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-81 | Pose captions: angle + " motor", else "Not taught" (lower, upper) or "Not aligned" (reference) (:21-22, :104). | `hw/view.rs` `render` | done-by-reading |
| CAL-82 | Save buttons enabled when active, not sweeping, intent hold, and no session or the session is holding (:104). | `hw/view.rs` `render` | done-by-reading |
| CAL-83 | Save lower/upper: `capture_hold {boundary, run_id, ...input(), motion: 'hold'}` in a session, else `capture {boundary}`; an error goes to the capture message (:301-303). | `hw/session/buttons.rs` `capture`; `hc/calibration.rs` `capture`, `capture_hold` | needs-hardware-checklist (HW-06) |
| CAL-84 | Save sim alignment adds `reference_joint_rad` from the mirror's alignment angle (:302). | `hw/handlers.rs` `handle` (`Capture{Reference}` with `hw/mirror.rs` `Mirror::alignment_angle`); `hw/session/buttons.rs` `capture` | needs-hardware-checklist (HW-06) |
| CAL-85 | Try saved range: enabled when active, both taught, not another session's poses; text "Pause & hold" while sweeping or targeting (:105). | `hw/view.rs` `render` | done-by-reading |
| CAL-86 | Try saved range: pause → hold + heartbeat; else begin a hold session if none, then intent sweep, speed slider to 0, heartbeat (:308-314). | `hw/session/buttons.rs` `sweep` (`speed_reset` counter); `hw/actions.rs` `poll_jobs` (slider to 0, `Inputs::speed_reset` stamped); `hw/session.rs` `handle` (`Inputs` older than the reset keep the reset speed) | needs-hardware-checklist (HW-07) |
| CAL-87 | Reset button enabled when active; text "Re-teach both poses" / "Reset {pose} pose" / "Reset poses" (:105). | `hw/view.rs` `render` | done-by-reading |
| CAL-88 | Reset: boundary both for another session, else the pose beyond, else both; stop, `clear {boundary}`, re-select (:305-306). | `hw/session.rs` `handle` (`ResetPoses` picks the boundary); `hw/session/buttons.rs` `reset`; `hc/calibration.rs` `clear` | needs-hardware-checklist (HW-06) |
| CAL-89 | Learn button enabled like Try saved range; text "Pause learning & hold" while learning (:106). | `hw/view.rs` `render` | done-by-reading |
| CAL-90 | Learn: pause → hold; else begin a hold session if none, then intent learn (:315-319). | `hw/session/buttons.rs` `learn` | needs-hardware-checklist (HW-07) |
| CAL-91 | Learning line: "{status}. Stops learned: d / i. Allowed now: x.xx°/s motor.", else the teach-first text (:116). | `hw/view.rs` `render` | done-by-reading |
| CAL-92 | Learning complete while learning → hold + heartbeat (:117). | `hw/session.rs` `render`, `learning_complete` | needs-hardware-checklist (HW-07) |

### Sweep all

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-93 | Button "Sweep all enabled motors" / "Stop sweeping all"; disabled while busy unless sweeping all (:86). | `hw/view.rs` `render` | done-by-reading |
| CAL-94 | Pressed while running: "Sweep-all stopped." and stop (:169). | `hw/session/sequences.rs` `sweep_all` | done-by-reading |
| CAL-95 | Eligible: enabled with both poses, sorted; none → "No enabled motor has both poses taught." (:170-172). | `hw/session/sequences.rs` `sweep_all` (sorted as text, as the page's `.sort()`) | done-by-reading |
| CAL-96 | Start: "Sweep-all: checking every enabled motor at zero drive…", select the first with all held; failure "Sweep-all stopped: {message or 'could not connect'}" (:175-178). | `hw/session/sequences.rs` `sweep_all`, `sweep_all_fail` | done-by-reading |
| CAL-97 | `sweep_all {...input()}` with intent sweep, then a heartbeat; an error ends it with its reason (:179-182). | `hw/session/sequences.rs` `sweep_all`; `hc/calibration.rs` `sweep_all` | needs-hardware-checklist (HW-07) |
| CAL-98 | Every 250 ms: "Sweep-all: {role} starting / {role} n/2 ends · … · skipped …" (:184-190). | `hw/link.rs` `SWEEP_ALL_TICK`; `hw/session/sequences.rs` `sweep_all_tick` | done-by-reading |
| CAL-99 | End: "Sweep-all stopped: {motion_error}" or "Swept {roles} through their saved ranges. Skipped: …." (:191-194). | `hw/session/sequences.rs` `sweep_all_tick` | done-by-reading |

### Tune and campaign

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-100 | Tune confirmation "The motor is mid-travel with room to move both ways" (:28, :197). | `hw/panel.rs` `TUNE_OK`; `hw/panel_sections.rs` `body`; `HardwareAction::TuneConfirm` | done-by-reading |
| CAL-101 | Tune button: disabled unless ready, not busy, not tuning, no sweep-all and confirmed; "Tuning…" while running (:108). | `hw/view.rs` `render` | done-by-reading |
| CAL-102 | Tune status: "Tuning: {stage}" / "Last tuning stopped: {error}" / "Tuned gains in use: kp, ki, kd, friction % · record" / "Using the shared provisional gains." (:109). | `hw/view.rs` `render` | done-by-reading |
| CAL-103 | Tune: stop, select the motor alone, `tune {supported: true, drive_pwm}`, wait in 300 ms steps while running, then not ready and the confirmation unchecked (:203-210). | `hw/handlers.rs` `handle` (`Tune` needs the confirmation); `hw/session/sequences.rs` `tune`, `tune_tick` (`TUNE_TICK`; `tune_done`); `hw/actions.rs` `poll_jobs` (unchecks); `hc/calibration.rs` `tune` | needs-hardware-checklist (HW-08) |
| CAL-104 | Campaign confirmation "The leg is suspended with clear space around every joint" (:33, :273). | `hw/panel.rs` `CAMPAIGN_OK`; `HardwareAction::CampaignConfirm` | done-by-reading |
| CAL-105 | Run campaign / Resume disabled without a motor, busy, tuning, campaigning, sweep-all or confirmation; "Campaign running…" (:112-113). | `hw/view.rs` `render` | done-by-reading |
| CAL-106 | Campaign status: "{stage} · n stage results saved · last stopped by {gate}" / "Last campaign stopped: …" / "Finished: {headline}. {directory}" / "Tune each motor and teach both poses first." (:114). | `hw/view.rs` `render` (the gate from `last.abort`, `truthy`) | done-by-reading |
| CAL-107 | Campaign: stop, select with all held, `campaign {supported: true, resume}`, wait in 500 ms steps, then not ready and unchecked (:274-282). | `hw/session/sequences.rs` `campaign`, `campaign_tick` (`CAMPAIGN_TICK`, `campaign_done`); `hc/calibration.rs` `campaign` | needs-hardware-checklist (HW-09) |

### Gait playback

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-108 | Gait select from `/calibration/gaits`; a failure shows "Gait list unavailable: …" (:38, :240). | `hw/session/sequences.rs` `load_gaits` (`gait_notice`); `hw/view.rs` `render_gait`; `hw/panel_sections.rs` `gaits` | done-by-reading |
| CAL-109 | Option labels "Poses · study · trial" for pose sequences, else "★ " when measured + "x.xxx m/s · study without gait-search- · trial" (:240). | `hw/view.rs` `gait_option_label` | done-by-reading |
| CAL-110 | An option's tooltip is the gait's summary (:240). | `hw/handlers.rs` `gaits_json` (REST `hardware_gaits` answers each gait's `summary`) | deliberately-different (the kit has no tooltips; the summary is in REST `hardware_gaits` only) |
| CAL-111 | Radios Sim only (default), Leg only, Both (:39); disabled while a gait plays (:221). | `HardwareAction::GaitMode`; `hw/handlers.rs` `handle` (refused while a gait plays); `hw/view.rs` `render_gait` (`modes_enabled`) | done-by-reading |
| CAL-112 | Playback speed 5–100, default 100, "n% of the gait's timing" (:40, :216). | `HardwareAction::GaitSpeed`; `hw/handlers.rs` `handle`; `hw/view.rs` `render_gait` | done-by-reading |
| CAL-113 | Leg effort 10–100, default 50, "n% of measured motor capability"; disabled while a leg gait plays (:41, :217). | `HardwareAction::GaitEffort`; `hw/handlers.rs` `handle`; `hw/view.rs` `render_gait` | done-by-reading |
| CAL-114 | Leg/Both confirmation checkbox (:43, :271). | `hw/panel.rs` `GAIT_OK`; `HardwareAction::GaitConfirm` | done-by-reading |
| CAL-115 | Play text Play / Pause / Resume (:218). | `hw/view.rs` `render_gait` | done-by-reading |
| CAL-116 | Play disabled with no gaits, Leg/Both unconfirmed before start, or while campaigning or tuning (:219). | `hw/view.rs` `render_gait`; `hw/handlers.rs` `gait_play` (the same checks) | done-by-reading |
| CAL-117 | Gait Stop enabled while a gait plays; ends it and, for Leg/Both, stops drive (:220, :269). | `hw/handlers.rs` `handle` (`GaitStop`: a leg gait first goes through `stop_immediate`); `hw/session/sequences.rs` `gait_stop` | needs-hardware-checklist (HW-10) |
| CAL-118 | Status "No gaits found yet." with an empty list (:222). | `hw/view.rs` `render_gait` | done-by-reading |
| CAL-119 | Status "{Sim only / Leg only / Sim + leg} · gait time t s of P s period · n% speed" (the server's speed for a leg gait) (:224). | `hw/view.rs` `render_gait` | done-by-reading |
| CAL-120 | Leg gait: "Limits: {role} ≤ n counts/s, n counts/s² · …" (:225). | `hw/view.rs` `render_gait`, `rounded` | done-by-reading |
| CAL-121 | Leg gait: "Leg: {phase or starting} · error {role n, …} counts · n targets clamped to taught poses", then "Not driven: …" (:226). | `hw/view.rs` `render_gait`, `rounded` | done-by-reading |
| CAL-149 | `g?.limits` is truthy for an empty object, so the page prints a bare "Limits: " line; `Math.round(null)` prints 0 for a missing governor limit or tracking error (:225-226). | `hw/view.rs` `render_gait` (the line is left out when there are no limits; `rounded` prints "—" for null) | deliberately-different (honest labels, AGENTS.md: a missing value is not shown as a measured 0) |
| CAL-122 | No gait playing, server error: "Last leg gait stopped: …" (:227). | `hw/view.rs` `render_gait` | done-by-reading |
| CAL-123 | A leg gait that started and stopped on the server ends the run (:228). | `hw/session.rs` `render` (with `hw/session/sequences.rs` `leg_frame`, `end_gait`) | needs-hardware-checklist (HW-10) |
| CAL-124 | Statistics table: Motor, RMS error, Peak, Sim RMS, Lag, Effort, At ceiling, Governed, Peak accel, Min V, Max °C; "—" for missing (:230-231, :234-239). | `hw/view.rs` `stats_rows`, `STATS_HEADER`; `hw/panel_sections.rs` `stats` | done-by-reading |
| CAL-148 | `f(r.lag_s*1000,0)` and the three percentages scale before the check, so a null statistic prints "0 ms" / "0%" (:234-239). | `hw/view.rs` `stats_rows` ("— ms", "—%") | deliberately-different (honest labels: a missing measurement is not shown as a measured zero) |
| CAL-125 | Recent leg runs: "trial folder · effort · speed · s · outcome" + table each, else "No leg runs yet." (:47, :232). | `hw/view.rs` `render_gait` (`runs`), `NO_RUNS`; `hw/panel_sections.rs` `runs` | done-by-reading |
| CAL-150 | A run whose gait path has no '/' (or no gait) prints "undefined" as its folder; a missing outcome prints "undefined"/"null" (:232). | `hw/view.rs` `render_gait` (the whole path; an empty outcome) | deliberately-different (the page's text names nothing; the path is the one fact available) |
| CAL-126 | Play (Sim): fetch `gait?path=`, load it into the mirror's sampler, clock from wall time × scale (:252-255, :264). | `hw/handlers.rs` `gait_play`; `hw/session/sequences.rs` `gait_play`, `start_gait` (`LinkSnapshot::compiled_gait`, a field), `sim_frame`; `hw/mirror.rs` `Mirror::follow_gait` (shared `sim_runtime::gait_playback`) | done-by-reading |
| CAL-127 | Play (Leg/Both): the mirror's bindings, none → "No motor is aligned, taught and enabled: …"; stop, select all held, `gait_start {supported, gait, bindings, speed_scale, effort, drive_pwm, drive_mode}`, not ready (:256-263). | `hw/handlers.rs` `gait_play` (bindings from `hw/mirror.rs` `Mirror::gait_bindings`); `hw/session/sequences.rs` `start_gait`; `hc/calibration.rs` `gait_start` | needs-hardware-checklist (HW-10) |
| CAL-128 | Gait lease heartbeat `gait_update {speed_scale, playing}` every 300 ms (:261). | `hw/session/beat.rs` (the "hardware-beat" `jobs::RunThread`: `gait_update` every `GAIT_HEARTBEAT` and at once on a pause or speed change, 1 s timeout, errors ignored; not sent while a STOP is pending), planned by `hw/session.rs` `beat_plan`/`sync_beat`, so a slow request on the link thread cannot expire the server's 1.5 s lease; `hc/calibration.rs` `gait_update` | needs-hardware-checklist (HW-10) |
| CAL-129 | Pause/Resume toggles playing; a leg gait sends `gait_update` at once (:251). | `hw/session/sequences.rs` `gait_toggle` → `sync_beat` (the beat sends at once) | needs-hardware-checklist (HW-10) |
| CAL-130 | The speed slider during a gait changes its scale; a leg gait sends `gait_update` (:270). | `hw/session/sequences.rs` `gait_scale` → `sync_beat` (the beat sends at once) | needs-hardware-checklist (HW-10) |
| CAL-131 | Each frame: leg gait time from `state.gait.t`; sim time advanced while playing; sample with dt clamped to 0.001–0.2 s, reset on the first sample, governed when the gait has a governor; "Gait sample failed: …" (:242-249). | `hw/session/sequences.rs` `leg_frame`, `sim_frame`; `hw/mirror.rs` `Mirror::follow_gait` (dt clamp, reset), `Mirror::poll` ("Gait sample failed: …"), `worker` (governed); `hw/panel.rs` `panel_view` (the notice in the gait status) | done-by-reading |
| CAL-132 | Both keeps the bound leg on the real encoders; Sim only poses every joint from the gait (:247, :264). | `hw/mirror.rs` `Mirror::update`, `set_gait` | needs-hardware-checklist (HW-10) |

### Advanced, preferences and export

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-133 | Control mode select: PWM (default), Servo position, Servo speed (:54); applies when a session starts. | `hw/actions.rs` `DriveMode`; `hw/handlers.rs` `handle` (`DriveMode`, saved) | needs-hardware-checklist (HW-12) |
| CAL-134 | PWM ceiling: a number field 0–100, step 0.1, validated before `begin` and on change (:56, :149, :298). | `hw/panel_sections.rs` `body` (kit slider); `hw/actions.rs` `sliders`; `hw/handlers.rs` `handle` (`PwmCeiling` validates); `hw/session.rs` `pwm_valid` | deliberately-different (no free-text number fields in the kit: a slider that can only hold valid values) |
| CAL-135 | A valid PWM change sends a heartbeat (:298). | `hw/session.rs` `handle` (`PwmChanged` → `update` when `pwm_valid`) | needs-hardware-checklist (HW-12) |
| CAL-136 | Swap upper / lower direction: stop, re-select, `flip` (:58, :307). | `hw/session/buttons.rs` `flip`; `hc/calibration.rs` `flip` | needs-hardware-checklist (HW-12) |
| CAL-137 | Reset lower only / Reset upper only (:59, :306). | `hw/handlers.rs` `handle` (`ClearLower`, `ClearUpper`); `hw/session/buttons.rs` `reset` | needs-hardware-checklist (HW-12) |
| CAL-138 | Raw step: a number field −4095..4095, step 1 (:60). | `hw/panel_sections.rs` `body` (stepper, `hw/panel.rs` `STEPS`); `hw/handlers.rs` `handle` (`RawStepValue`) | deliberately-different (no free-text number fields in the kit: a stepper) |
| CAL-139 | Send raw step: valid, non-zero, a motor chosen; stop, re-select, `jog {delta, drive_pwm}`, ready when `enabled_id` is the motor (:320). | `hw/handlers.rs` `handle` (`RawStep`); `hw/session/buttons.rs` `raw_step`; `hc/calibration.rs` `jog` | needs-hardware-checklist (HW-12) |
| CAL-140 | Preferences `calibration-drive-mode`, `calibration-hold-others` in localStorage (:199-202). | `hw/settings.rs` `load`, `path` (free functions), `Settings::save` (an Io job); read once at app build (`hw/actions.rs` `Preferences`, `build`) | deliberately-different (a JSON preferences file, `$SIM_SPATIAL_PREFERENCES` or `~/.config/sim-spatial/hardware-preferences.json`, replaces the browser's localStorage) |
| CAL-141 | Download calibration: `/calibration/export` + `display_mirror`, saved as `leg-calibration.json` by the browser (:61, :324). | `hw/handlers.rs` `export`, `start_export`, `write_export` (a Dedicated job); `hw/mirror.rs` `Mirror::record` | deliberately-different (written to `<server output>/viewer-exports/leg-calibration-<unix_ms>.json`, create_new, with the path shown: the viewer has no downloads folder) |
| CAL-142 | The mirror is created on the first status with axes, with the axes' roles, and updated on every render (:121-122). | `hw/mirror_panel.rs` `mirror_sync` (`Mirror::set_roles` on the first status with axes, `Mirror::update` on each new snapshot) | done-by-reading |
| CAL-143 | The motion view is updated on every render with the motor, axis, telemetry and sweep (:120). | `hw/motion_view.rs` `update` (called from `hw/view.rs` `render`) | done-by-reading |

### Widgets and accessibility

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| CAL-145 | `aria-label` "Movement speed" (:20) and "Target pose between saved limits" (:50) on the two range inputs. | `hw/panel_sections.rs` `body` → `slider` (the kit slider's `AccessibleLabel`, `ui_kit/slider.rs`) | done-by-reading |
| CAL-146 | `aria-label` "Close calibration" on × (:11), "Choose motor" on the chip group (:13), "Recent warnings" (:17), "Where to play" on the radios (:39), "Measured motor angle and requested angle" on the dial (:48), "Control mode" on the select (:54). | `hw/panel.rs` `control_list` ("Close calibration" is the × control's `system_ui` label; the kit button's `AccessibleLabel` is its text "×"); the visible headings "Gait" and "Control mode" (`hw/panel_sections.rs` `body`) | deliberately-different (the kit labels interactive widgets with their own text; groups, the warnings text and the dial image carry no `AccessibleLabel`) |
| CAL-151 | HTML form widgets: checkboxes, the gait `<select>`, radio buttons, the statistics `<table>`. | `hw/panel_sections.rs` `body`, `gaits`, `stats` (kit chip toggles, a list of kit chips, kit segments, one "header value · …" line per motor) | deliberately-different (the kit's widgets; the same choices and values) |

## actuator-motion-view.mjs

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| AMV-01 | Part direction sign: from the taught poses (upper < lower → −1), else `reverse` (:3). | `hw/motion_view.rs` `comparison`, `js_sign` | done-by-reading |
| AMV-02 | Agreement: "No measured step yet" / "Awaiting measured reply" / "No measured movement" / "Measured direction matches command" / "DIRECTION MISMATCH — check mapping" (:4-7). | `hw/motion_view.rs` `comparison` | done-by-reading |
| AMV-03 | Jog receipts per motor (:15); the page never passes `jog` or `preview` (calibration-ui.mjs:120), so only the continuous record is ever drawn. | `hw/motion_view.rs` `update` (no receipts, as the page) | done-by-reading |
| AMV-04 | Continuous record: this motor's sweep samples; requested and actual change from the first sample (:16-18). | `hw/motion_view.rs` `update`, `sample` | done-by-reading |
| AMV-05 | Heading "Command vs real motion"; direction line "Toward upper ↑ = encoder ± · toward lower ↓ = encoder ∓" (:12, :19). | `hw/motion_view.rs` `update`, `HEADING` | done-by-reading |
| AMV-06 | Agreement line with "requested ↑/↓ n counts · measured ↑/↓/— n counts" (:21). | `hw/motion_view.rs` `update` | done-by-reading |
| AMV-07 | Live line "Holding/Moving/Stopped · toward upper ↑/lower ↓ · measured v part counts/s · tracking error e counts" (:22). | `hw/motion_view.rs` `update` | done-by-reading |
| AMV-08 | The line is red on a direction mismatch without a continuous record, else green (:23). | `hw/motion_view.rs` `update` (`mismatch`); `hw/panel.rs` `refresh` (`DANGER`/`OK`) | done-by-reading |
| AMV-09 | Without a record: "Encoder: n counts" or "Waiting for encoder readback", and "Hold Q/A to compare command and motion." (:27). | `hw/motion_view.rs` `update` (`placeholder`) | done-by-reading |
| AMV-10 | Scale: values include 0, requested, measured and targets; 15 % margin; time from the first to the last sample (:28-31). | `hw/motion_view.rs` `update`, `MotionChart::x_range`/`y_range` (the page's lo/hi with 0 and the last request, span at least 1, 15 % margin); `hw/panel.rs` `refresh` (`chart::rasterize_fixed`, so one sample draws) | done-by-reading |
| AMV-11 | Zero line; blue target polyline (dashed requested line without samples); green measured polyline with dots (:32-35). | `hw/motion_view.rs` `update`; `hw/panel.rs` `refresh` | done-by-reading |
| AMV-12 | Labels: "Motor encoder change (counts)", high and low values, start and end ms (:36). | `hw/motion_view.rs` `MotionChart::labels` (the data's hi and lo, not the padded edges); `hw/panel.rs` `refresh`; `hw/panel_sections.rs` `chart_labels` (`motion_view::CHART_TITLE`); the chart keeps the raster's aspect ratio | done-by-reading |
| AMV-13 | Footnote "Blue: requested · green: encoder readback. …" (:12). | `hw/motion_view.rs` `NOTE`; `hw/panel_sections.rs` `body` | done-by-reading |
| AMV-14 | Chart tracks the real leg while it moves. | `hw/motion_view.rs` `update` | needs-hardware-checklist (HW-03) |

## calibration-mirror.mjs

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| MIR-01 | 4096 counts per turn; the robot held 0.25 m up (:4). | `hw/mirror.rs` `COUNTS`, `LIFT_M`; `worker` (`sim_runtime::kinematic_mirror::KinematicMirror`) | done-by-reading |
| MIR-02 | CAD joints offered: Hip swing (belt), Worm drive, Foot slide (:5). | `hw/mirror.rs` `JOINTS`; `hw/mirror_panel.rs` `fill` | done-by-reading |
| MIR-03 | Default role → joint: knee → Foot, worm → Worm, belt/hip → Hip, else Hip (:7, :21). | `hw/mirror.rs` `default_joint` | done-by-reading |
| MIR-04 | Alignment pose CAD home or Mid-travel; default Mid-travel for the foot, else CAD home (:12-13). | `hw/mirror.rs` `default_align`, `align_label`; `hw/actions.rs` `Align` | done-by-reading |
| MIR-05 | Settings (enabled true, leg +X, per motor joint, polarity 1, align) in localStorage `calibration-mirror-v1` (:8, :18-23, :46). | `hw/settings.rs` `MirrorSettings`, `load` (tolerant: a missing field takes its default; a stored sign is clamped to ±1, `sign`), `Settings::save` | deliberately-different (a JSON preferences file replaces localStorage) |
| MIR-06 | Section "Simulated leg mirror", open (:24; calibration-ui.mjs:52). | `hw/panel.rs` `section_title`; `hw/mirror_panel.rs` `fill`; `Section::Mirror` | done-by-reading |
| MIR-07 | "Show the real leg on the suspended simulated robot": saves, then begins or ends the display (:25, :32). | `HardwareAction::MirrorEnabled`; `hw/mirror.rs` `apply` | done-by-reading |
| MIR-08 | Leg select +X, −X, +Y, −Y: saves and begins again (:26, :33). | `HardwareAction::MirrorLeg`; `hw/mirror.rs` `apply`; refused from REST and `system_ui` (it shapes the Leg/Both gait bindings) | done-by-reading |
| MIR-09 | A row per motor: role, "ID n", joint, sign (+/−), alignment pose; a change saves and begins again (:34-40). | `hw/mirror_panel.rs` `fill`; `MirrorJoint`, `MirrorPolarity`, `MirrorAlign`; `hw/mirror.rs` `apply`; joint, sign and alignment refused from REST and `system_ui` (they become the `gait_start` bindings and the saved alignment reference) | done-by-reading |
| MIR-10 | Help text "Align each motor once: …" (:28). | `hw/mirror.rs` `NOTE`; `hw/mirror_panel.rs` `fill` | done-by-reading |
| MIR-11 | Status line: "Mirror unavailable: {error}", else the pose text (:29, :47). | `hw/mirror.rs` `Mirror::status_text`; `hw/mirror_panel.rs` `mirror_panel` | done-by-reading |
| MIR-12 | Export record: settings + `lift_m`, `counts_per_revolution`, the display-only note (:49). | `hw/mirror.rs` `Mirror::record` | done-by-reading |
| MIR-13 | Joint name "{leg} \| {joint}" (:50). | `hw/mirror.rs` `Mirror::joint` | done-by-reading |
| MIR-14 | Alignment angle: mid-travel (mean of the CAD limits, when both exist) or CAD home; none before the model loads (:53-56). | `hw/mirror.rs` `Mirror::alignment_angle` | done-by-reading |
| MIR-15 | Saved alignment angle: the axis's `reference_joint_rad`, else CAD home (:58). | `hw/mirror.rs` `Mirror::saved_angle` | done-by-reading |
| MIR-16 | Begin only when enabled and the panel is shown; "Waiting for the robot model to load…" retried every 500 ms; "Preparing the suspended robot…" while the mirror loads (:59-64). | `hw/mirror.rs` `Mirror::prepare` (the scene told apart by `SceneId`, `Weak::ptr_eq`; a new scene or a dead worker gets a new worker), driven every frame by `hw/mirror_panel.rs` `mirror_sync` while `Mirror::due`; a replaced, dead or dropped worker is released with `jobs::drop_off_thread` (`release_worker`, `Drop for Mirror`), never joined on the UI thread | done-by-reading |
| MIR-17 | "Bind each motor to a different CAD joint" when two motors share one (:66). | `hw/mirror.rs` `Mirror::prepare` | done-by-reading |
| MIR-18 | "CAD model has no motor joint {joint}" (:67). | `hw/mirror.rs` `Mirror::prepare` | done-by-reading |
| MIR-19 | Begin shows the chosen leg's links tinted blue, poses at once, fits the camera (:70-72). | `hw/mirror_panel.rs` `mirror_sync` (`RobotView::mirror`, `MirrorDisplay::tinted`, `RobotAction::Fit`; begun again while shown, only the tint changes and the shown poses are kept until the next solve); `robot.rs` `highlight` (`Materials::mirrored`, emissive 0x1d4a7a) | needs-hardware-checklist (HW-11) |
| MIR-20 | Nothing updates while disabled, before coordinates, or after an error (:78). | `hw/mirror.rs` `Mirror::update` | done-by-reading |
| MIR-21 | A sim-only gait poses every joint; text "Simulated gait" (:81-82). | `hw/mirror.rs` `Mirror::update` | done-by-reading |
| MIR-22 | Not aligned: shown at its alignment pose, "{role}: not aligned — shown at {pose}" (:86-87). | `hw/mirror.rs` `Mirror::update` | done-by-reading |
| MIR-23 | A multi-turn reference from another encoder session: "{role}: alignment is from an earlier session — re-align (shown at {pose})" (:88-90). | `hw/mirror.rs` `Mirror::update` | done-by-reading |
| MIR-24 | No reading: "{role}: no reading" (:91). | `hw/mirror.rs` `Mirror::update` | done-by-reading |
| MIR-25 | Joint = saved angle + sign × (counts − reference) × 2π/4096; "{role}: x.x° from its alignment pose" (:92-94). | `hw/mirror.rs` `Mirror::update`; angle text via `mirror::degrees_text` (`toFixed` ties away from zero: 128 counts read 11.3°) | needs-hardware-checklist (HW-11) |
| MIR-26 | An unchanged pose is not solved again unless forced (:96-98). | `hw/mirror.rs` `Mirror::update`, `solve` | done-by-reading |
| MIR-27 | One solve at a time; the newest waiting pose is solved next (:127-136). | `hw/mirror.rs` `worker` (drains its channel, solves only the newest pose) | done-by-reading |
| MIR-28 | " · Beyond CAD limit: {joints}" for authored limits exceeded on the chosen leg (:133-134). | `hw/mirror.rs` `Mirror::poll` | done-by-reading |
| MIR-29 | " · Pose not solved: {error}" (:135). | `hw/mirror.rs` `Mirror::poll` | done-by-reading |
| MIR-30 | Mirror worker failure: "Mirror worker failed" (:43). | `hw/mirror.rs` `worker_failed` (`WORKER_FAILED`: the status reads "Mirror unavailable: Mirror worker failed"; the display ends; the next begin replaces the worker, `Mirror::prepare`) (the dead worker released off the UI thread) | done-by-reading |
| MIR-31 | Gait load and sample through the shared Rust sampler (:102-112). | `hw/mirror.rs` `worker` (`sim_runtime::gait_playback`), `Mirror::follow_gait` | done-by-reading |
| MIR-32 | `setGait` shows or clears a gait pose, re-posing at once (:114). | `hw/mirror.rs` `set_gait` | done-by-reading |
| MIR-33 | Gait bindings: aligned, taught, enabled motors with joint, polarity and saved angle; skipped as "{role} (disabled / poses not taught / not aligned to the sim / no CAD joint)", in that order (:117-126). | `hw/mirror.rs` `Mirror::gait_bindings` | done-by-reading |
| MIR-34 | The body is held still while the leg follows the encoders (:2-3). | `hw/mirror.rs` `worker` (`KinematicMirror`, base fixed) | needs-hardware-checklist (HW-11) |
| MIR-35 | While mirroring, Play is refused (viewer.js:95 `if(value&&mirrorActive)return`). | `robot/actions.rs` `check` (Run refused with `hw/mirror.rs` `MIRRORING`) | done-by-reading |
| MIR-36 | Save sim alignment then shows the leg at its alignment pose (reference captured). | `hw/mirror.rs` `Mirror::update` | needs-hardware-checklist (HW-11) |

## hardware-sync.mjs

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| SYNC-01 | The section exists only with a `motor-bridge-token` meta that is not the placeholder `__CONTROL_TOKEN__` (:3-4). | `hw/sync_panel.rs` `controls` (`sync::NO_BENCH`) | deliberately-different (without `--motor-bench` the section is shown and explains how to start `serve_motor_bench` and pass `--motor-bench URL`) |
| SYNC-02 | The token is in the page the bench serves (`/walking/` meta). | `hc/token.rs` `discover` (`const token='…'` in `/`, or the `/walking/` meta), `read_file` (`--motor-bench-token-file`) | done-by-reading |
| SYNC-03 | Placement: a section at the top of the walking page's inspector, a banner over the workspace, a chart strip below it (:5-10). | `hw/sync_panel.rs` `sync_panel` (a section of the Leg calibration panel, `Section::Sync`), `sync_overlay` (banner strip over the 3D view) | deliberately-different (one hardware panel in Robot mode; the walking page's inspector has no native counterpart) |
| SYNC-04 | Heading "Real motor sync" and intro text; footnote on feedback, buffering and stops (:6). | `hw/panel.rs` `section_title`; `hw/sync_panel.rs` `controls` (`sync::DESCRIPTION`, `sync::NOTE`) | done-by-reading |
| SYNC-05 | Same client id and headers as the calibration page (:14-15). | `hc/token.rs` `connect` (`process_client_id`, the same id as the calibration link, `for_kind(MotorBench)`); `hc/http.rs` `send` | done-by-reading |
| SYNC-06 | Legs from `/config` coordinates (`joint.{leg} \| …`), deduplicated (:30). | `hw/sync.rs` `legs`, `connected_with`; `hc/bench.rs` `Config` | done-by-reading |
| SYNC-07 | A row per leg coordinate: joint title without " servo output", motor ID select from `config.ids` (default `ids[i]`), polarity + / − (:20). | `hw/sync.rs` `mapping`; `hw/sync_panel.rs` `row_title`, `controls` | done-by-reading |
| SYNC-08 | Bench motion scale 3 %, 5 %, 9 % (:6). | `hw/sync.rs` `SCALES`, `apply` (`SyncScale`) | done-by-reading |
| SYNC-09 | Leg, scale and bindings saved in localStorage `walking-hardware-map-v1` and restored (:19, :24, :30). | `hw/settings.rs` `SyncSettings`; `hw/sync.rs` `to_save`, `apply` | deliberately-different (the JSON preferences file replaces localStorage) |
| SYNC-10 | Changing the leg rebuilds the rows and saves (:24). | `hw/sync.rs` `apply` (`SyncLeg` → `mapping`) | done-by-reading |
| SYNC-11 | Initial text "Connecting…", then "Ready. Choose a leg, then start sync and steer with WASD." or the error (:6, :30). | `hw/sync.rs` `LiveSync::new`, `connect`, `connected_with` (`READY`), `LiveSync::poll` (the error) | done-by-reading |
| SYNC-12 | "Sync motors · 12 seconds" disabled without config, while the server is busy, active, preparing or stopping (:25). | `hw/sync.rs` `LiveSync::start_enabled`; `hw/sync_panel.rs` `controls` | done-by-reading |
| SYNC-13 | Selects disabled while the server is busy, active or preparing (:25). | `hw/sync.rs` `LiveSync::selects_enabled`, `editable` | done-by-reading |
| SYNC-14 | A live run is required: "Reset the episode, then choose a live walking controller with named motor targets." (:17). | `hw/sync.rs` `live_input`, `sample_from` (`NOT_LIVE`) | done-by-reading |
| SYNC-15 | Distinct IDs: "Assign a different motor ID to each joint." (:23). | `hw/sync.rs` `distinct` (from `LiveSync::start`) | needs-hardware-checklist (HW-13) |
| SYNC-16 | Start: save, "Starting bounded motor session…", play, `/live/open {bindings, amplitude, source, initial}`, then "Syncing live WASD targets…" and send; an error pauses and shows it (:23). | `hw/sync.rs` `apply` (`SyncStart` saves), `LiveSync::start`, `worker` (`/live/open`), `LiveSync::poll` (the open's answer); `hc/bench.rs` `open`. Native addition: before opening, the run must already be running or accept Start (`RunController::check(RunAction::Start)`); a refusal is shown through the same error path and no `/live/open` is posted (`StartInput::of`, `LiveSync::start_with`) | needs-hardware-checklist (HW-13) |
| SYNC-17 | `source` = `{preset, cad}` of the running preset (viewer.js:428). | `hw/sync.rs` `source_text` | done-by-reading |
| SYNC-18 | Samples sent every 50 ms, only a newer sequence, one at a time (:21, :31). | `hw/sync.rs` `worker` (the "hardware-sync" RunThread, `SEND_PERIOD`), `drain` (`Outbox`: the newest sample wins) | needs-hardware-checklist (HW-13) |
| SYNC-19 | A failed send stops with "Reference transport failed" (:21). | `hw/sync.rs` `worker` (`send_error`), `LiveSync::poll` | done-by-reading |
| SYNC-20 | Stop: "{reason} — verifying physical stop…", `/stop {}`; a failure reads "Stop request unavailable; FPGA watchdog remains independent. …" (:22). | `hw/sync.rs` `LiveSync::stop`, `queue_stop`, `post_stop` (its own Dedicated job, `STOP_TIMEOUT`), `LiveSync::poll` (the failure text) | needs-hardware-checklist (HW-13) |
| SYNC-21 | "Stop motors": "Operator stop" and pause (:24). | `hw/sync.rs` `apply` (`SyncStop` → `LiveSync::stop("Operator stop")`, Pause); the panel's STOP also calls it (`hw/handlers.rs` `handle`, `Stop`) | needs-hardware-checklist (HW-04, HW-13) |
| SYNC-22 | Pausing or resetting the simulation stops sync ("Simulation paused", viewer.js:95). | `hw/sync.rs` `LiveSync::watch_run` ("Simulation paused", "Simulation reset"), called from `hw/sync_panel.rs` `sync_frames`; also stops when the run fails ("Simulation failed"), when the latest frame stops being live input, and when a Start the session sent is not seen Running within `START_GRACE` (2 s, "Simulation did not start") (`LiveSync::watch`) | needs-hardware-checklist (HW-13) |
| SYNC-23 | Status poll every 150 ms (:25, :29). | `hw/sync.rs` `LiveSync::poll_status` (`/status` on its own Dedicated job, one at a time, `POLL_PERIOD` after each answer) | done-by-reading |
| SYNC-24 | Readings per bound motor: "ID n: x.xx° · V · °C · input age ms", or "waiting" (:26). | `hw/sync.rs` `reading_lines`; `hw/sync_panel.rs` `sync_texts` | done-by-reading |
| SYNC-25 | Session end: "Motors stopped and verified." or "Stop verification incomplete.", then the failure, error, "12-second live session complete." or "Motor session ended without completion.", then "Saved: {run}" (:27). | `hw/sync.rs` `LiveSync::poll`; `hc/bench.rs` `SessionResult::summary`; the sync thread is sent `Deactivate`, so no further sample is posted | needs-hardware-checklist (HW-13) |
| SYNC-26 | Poll failure: while active, stop "Bridge disconnected" and pause; else show the error (:29). | `hw/sync.rs` `LiveSync::poll` | done-by-reading |
| SYNC-27 | Banner "MOTOR SYNC · / CONNECTING MOTORS · / SIMULATION ONLY · {text}", green while active, amber otherwise; initial "Simulation only — real motors are not connected. …" (:10, :16). | `hw/sync.rs` `banner_text`, `INITIAL_BANNER`; `hw/sync_panel.rs` `sync_overlay`, `sync_texts` | done-by-reading |
| SYNC-28 | Charts per binding: "{joint} · ID n", "RMS error x.xx° · at drive limit n%" against the previous target, ±bound, 0 s and 12 s, blue target and green encoder, "Waiting for measured motion"; note with the retained sample count (:9, :12-13). | `hw/sync_panel.rs` `charts`, `rms_and_saturation`, `chart_note_of`, `sync_panel` (`chart::rasterize_fixed`: fixed axes 0–12 s and ±bound, so no point falls outside) | done-by-reading |
| SYNC-29 | `pagehide` stops with "Page closed" (:31). | `hw/handlers.rs` `loss` (`Loss::Leaving` → `LiveSync::stop_ours("Page closed")`, and from the window's own close request `LiveSync::post_stop_on_leave`: `send_only` of `/stop` with a 500 ms write timeout, once per session); `hw/actions.rs` `stop_on_exit` (the same on `AppExit`) and `leave` (`stop_ours("Robot mode was left")`); `Drop for LiveSync` (the same write, a STOP job, the sync thread released with `jobs::drop_off_thread`) | needs-hardware-checklist (HW-13) |
| SYNC-30 | Every simulation frame: sequence + 1 and the latest sample; an error while active stops and pauses (:32; viewer.js:349). | `hw/sync_panel.rs` `sync_frames` → `hw/sync.rs` `LiveSync::on_frame` (`sample_from(live_input)`; `done` is the frame's `MotorTargets.done`, `robot_run.rs`); not live = a recorded preset, a replay in progress or a run a replay replaced (until Reset), or a gait preview loaded or loading (`sync::live_run`), with the page's `NOT_LIVE` text | done-by-reading |
| SYNC-31 | A failed request without an `error` field throws `r.statusText` (:15). | `hc/http.rs` `exchange` ("Request failed (HTTP {status})") | deliberately-different (the client has no reason phrase to show; the status code is the same fact) |
| SYNC-32 | The page stops sync only on `pagehide`, pause, reset, a failed send or poll and Stop motors. | `hw/handlers.rs` `loss` (focus loss, panel close and leaving also call `LiveSync::stop_ours`); `hw/sync.rs` `stop_ours` (only a session this viewer opened, `ours`) | deliberately-different (safety: the sync's Stop button hides with the panel, so every loss of control stops it; another client's session is not ended by this viewer's loss) |
| SYNC-33 | A STOP while `/live/open` is in flight: the page's `/stop` may reach the bench before the open. | `hw/sync.rs` `LiveSync::poll` (an open that lands after a stop is deactivated and `/stop` is posted again), `LiveSync::stop` (posted whenever a session may be open, `may_be_open`, never gated on `active` alone), `drain` (a stop queued during the open is seen before any sample) | deliberately-different (safety addition: the open cannot outlive the STOP) |

## Server endpoints

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| EP-01 | Calibration `GET /` carries `<meta name="calibration-token">` (calibration-ui.mjs:4). | `hc/token.rs` `from_page`, `discover` | needs-hardware-checklist (HW-01) |
| EP-02 | `GET /calibration/status` (calibration-ui.mjs:140, :326). | `hc/calibration.rs` `Status`; `hw/session.rs` `get_status`, `poll` | done-by-reading |
| EP-03 | `GET /calibration/gaits` (:240). | `hc/calibration.rs` `Gaits`; `hw/session/sequences.rs` `load_gaits` | done-by-reading |
| EP-04 | `GET /calibration/gait?path=` with `encodeURIComponent` (:254). | `hc/calibration.rs` `gait_path`; `hc/mod.rs` `encode_uri_component` | done-by-reading |
| EP-05 | `GET /calibration/export` (:324). | `hw/handlers.rs` `start_export` | needs-hardware-checklist (HW-15) |
| EP-06 | `POST /calibration/command` `stop` (:125). | `hc/calibration.rs` `stop`; `hw/link.rs` `stop_now` | needs-hardware-checklist (HW-04) |
| EP-07 | `select {hold_others}` (:129). | `hc/calibration.rs` `select` | needs-hardware-checklist (HW-02) |
| EP-08 | `set_disabled {disabled}` (:161). | `hc/calibration.rs` `set_disabled` | needs-hardware-checklist (HW-02) |
| EP-09 | `motion_start {...input()}` (:151). | `hc/calibration.rs` `motion_start` | needs-hardware-checklist (HW-03) |
| EP-10 | `motion_update {run_id, ...input()}` (:135). | `hc/calibration.rs` `motion_update` | needs-hardware-checklist (HW-03, HW-14) |
| EP-11 | `sweep_all {...input()}` (:180). | `hc/calibration.rs` `sweep_all` | needs-hardware-checklist (HW-07) |
| EP-12 | `tune {supported, drive_pwm}` (:207). | `hc/calibration.rs` `tune` | needs-hardware-checklist (HW-08) |
| EP-13 | `campaign {supported, resume}` (:278). | `hc/calibration.rs` `campaign` | needs-hardware-checklist (HW-09) |
| EP-14 | `gait_start {supported, gait, bindings, speed_scale, effort, drive_pwm, drive_mode}` (:260). | `hc/calibration.rs` `gait_start` | needs-hardware-checklist (HW-10) |
| EP-15 | `gait_update {speed_scale, playing}`, sent through `api` without `id` or `sequence` (:251, :261, :270). | `hc/calibration.rs` `gait_update` | needs-hardware-checklist (HW-10, HW-14) |
| EP-16 | `capture {boundary, reference_joint_rad?}` (:302). | `hc/calibration.rs` `capture` | needs-hardware-checklist (HW-06) |
| EP-17 | `capture_hold {boundary, run_id, ...input(), motion: 'hold', reference_joint_rad?}` (:302). | `hc/calibration.rs` `capture_hold` | needs-hardware-checklist (HW-06) |
| EP-18 | `clear {boundary}` (:305). | `hc/calibration.rs` `clear` | needs-hardware-checklist (HW-06) |
| EP-19 | `flip` (:307). | `hc/calibration.rs` `flip` | needs-hardware-checklist (HW-12) |
| EP-20 | `jog {delta, drive_pwm}` (:320). | `hc/calibration.rs` `jog` | needs-hardware-checklist (HW-12) |
| EP-21 | Bench `GET /` (`const token='…'`) or `GET /walking/` (`motor-bridge-token` meta) (hardware-sync.mjs:3). | `hc/token.rs` `discover` | done-by-reading |
| EP-22 | Bench `GET /config` (hardware-sync.mjs:30). | `hc/bench.rs` `CONFIG`, `Config`; `hw/sync.rs` `LiveSync::connect` | done-by-reading |
| EP-23 | Bench `GET /status` (:25). | `hc/bench.rs` `STATUS`, `Status`; `hw/sync.rs` `LiveSync::poll_status` | done-by-reading |
| EP-24 | Bench `POST /live/open {bindings, amplitude, source, initial}` (:23). | `hc/bench.rs` `open` | needs-hardware-checklist (HW-13) |
| EP-25 | Bench `POST /live/sample {sequence, time_s, targets_rad}` (:17, :21). | `hc/bench.rs` `sample` | needs-hardware-checklist (HW-13) |
| EP-26 | Bench `POST /stop {}` (:22). | `hc/bench.rs` `stop`; `hw/sync.rs` `post_stop` | needs-hardware-checklist (HW-13) |

## Native surface (no browser counterpart)

| ID | Browser behaviour (file:line) | Native (file:function) | Status |
|---|---|---|---|
| NAT-01 | none (the pages have no automation surface) | REST `hardware_status`: the link (connected, stale, age), session state, form, rendered panel and last server status; `hw/handlers.rs` `handle` (`Status`) → `hw/view.rs` `status_json` | done-by-reading |
| NAT-02 | none | REST `hardware_stop` and `system_ui` `hardware:stop`: STOP on the immediate path and live sync's `LiveSync::stop("Operator stop")` (`hw/handlers.rs` `handle`, `Stop`); always allowed | needs-hardware-checklist (HW-16) |
| NAT-03 | none | REST `hardware_export` (the Download calibration file; Pending until written, then `{path}`): `hw/handlers.rs` `export` | done-by-reading |
| NAT-04 | none | REST `hardware_gaits`: `hw/handlers.rs` `load_gaits`, `gaits_json` | done-by-reading |
| NAT-05 | none | REST `hardware {action}`: any `HardwareAction` in serde form (`hw/actions.rs` `parse`, `wire::Command`) | done-by-reading |
| NAT-06 | none | Refusal rule: `HardwareAction::starts_motion` (`hw/actions.rs`) from `Origin::Rest` or `Origin::SystemUi` is refused with `remote_refusal` ("hardware `{name}` starts, changes or arms motion and needs an operator at the window: REST and system_ui may read status, list gaits, export, connect, turn the mirror on or off and STOP only"), in `hw/actions.rs` `apply` and in `robot/actions.rs` `apply`. Refused: select, set disabled, sweep all, hold others, jog press/release, speed, target and its commit, capture, reset poses, clear lower/upper, sweep, learn, the tune/campaign/gait confirmations, tune, campaign, gait select/mode/speed/effort/play, drive mode, PWM ceiling, flip, raw step value, raw step, live sync's leg/motor/polarity/scale/start, and the mirror's leg, joint, polarity and alignment bindings (they become the Leg/Both gait bindings and the saved alignment reference). Allowed: toggle/close panel, connect, sections, status, STOP, loss (`leaving` is refused from REST: only the window's close request sends it), export, load gaits, gait stop, mirror on/off, sync connect, sync stop. While live sync is engaged, remote `robot_input`, jogs, run Start/Step and speed are refused too (`robot/actions.rs` `moves_synced_motors`) | needs-hardware-checklist (HW-16) |
| NAT-07 | none | `system_ui` lists `hardware:<name>` controls (`hw/panel.rs` `controls`, `control_list`) after robot mode's own (`robot/actions.rs` `apply`, `Controls`); motion ones are listed disabled with the refusal | done-by-reading |
| NAT-08 | none | Launch flags `--hardware URL`, `--hardware-token-file`, `--motor-bench URL`, `--motor-bench-token-file` (`main.rs`, kept in `app::switch::Documents::hardware`; `hw/actions.rs` `enter` opens and connects the panel with `--hardware`) | needs-hardware-checklist (HW-01) |
| NAT-09 | none | Loopback only: `hc/http.rs` `Endpoint::parse` accepts `http://127.0.0.1:PORT` and `http://localhost:PORT` (connected as 127.0.0.1) and refuses every other host, `[::1]` and other 127.x included (both servers bind IPv4 127.0.0.1) | done-by-reading |
| NAT-10 | none | The top bar's connection line ("{url} · link n", "· status stale", "Connecting to …", "Not connected · …") and its Connect/Reconnect button, shown without a link or with a stale one: `hw/panel_sections.rs` `top_bar`; `hw/panel.rs` `connection_line`, `control_list`, `refresh` (`Shown::Connect`) | done-by-reading |
| NAT-11 | none | The dock takes the pointer: `FocusPolicy::Block` on it (`hw/panel.rs` `spawn`), so a click on its empty areas does not reach robot mode's inspector; the wheel over the open panel scrolls it (`hw/panel.rs` `scroll`) and robot mode's inspector ignores the wheel while the panel covers it (`robot.rs` `scroll`) | done-by-reading |
| NAT-12 | none | Preferences read once when the app is built (`hw/actions.rs` `Preferences`, `build`), cloned in `enter` and written back in `leave`, so entering Robot mode reads no file on the UI thread | done-by-reading |
| NAT-13 | none | A `system_ui` activation of a disabled hardware control is refused with "{id} is disabled: {why}" (`robot/actions.rs` `apply`); REST is checked the same way (`hw/handlers.rs` `remote_check`) | done-by-reading |
| NAT-14 | none | Request body limits before connecting: 4096 bytes for the calibration server, 8192 for the bench (`hc/http.rs` `Client::for_kind`, `checked_body`; `hc/mod.rs` `CALIBRATION_MAX_BODY`, `MOTOR_BENCH_MAX_BODY`); a zero timeout is clamped to 1 ms (`hc/http.rs` `Client::with_timeout`) | done-by-reading |
| NAT-15 | none | A request that succeeded but whose answer is dropped because STOP arrived while it was in flight (`select`, `motion_start`, `sweep_all`, `gait_start`, `tune`, `campaign`) is followed by a `stop` at once (`hw/session.rs` `stop_after_dropped`); while a STOP is pending no other request is sent (`Session::send`, `STOP_PENDING`) | done-by-reading |

## Counts

Recounted from the tables above (2026-09-30, after review; each row's last
cell, up to its reason, counted with a short script; 275 unique IDs):

| Status | Rows |
|---|---|
| done | 0 |
| done-by-reading | 177 |
| needs-hardware-checklist | 74 |
| deliberately-different | 24 |
| **total** | **275** |

No row is `todo`. Nothing is `done` because nothing in the batch has been
compiled or run yet; the verification pass moves rows to `done` as it builds
and tests them, and the operator's checklist settles the
`needs-hardware-checklist` rows.
