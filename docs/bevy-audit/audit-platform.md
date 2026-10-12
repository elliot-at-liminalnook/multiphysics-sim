# Platform-adjacent Bevy audit — sim-spatial (+ sibling crates)

Date: 2026-10-12 · Bevy **0.19.1** (`crates/sim-spatial/Cargo.toml:27`, workspace
`Cargo.toml:60`). Scope: windowing/input, time, tasks/jobs, assets,
reflection/serialization, diagnostics, app exit/lifecycle.

Cost key: **S** ≤1 day, **M** days, **L** weeks/structural. "Modern alternative"
claims verified against docs.rs bevy 0.19.1 where load-bearing (URLs inline).

## 0. Scope correction: only sim-spatial is a Bevy crate

| Crate | Framework | Evidence |
|---|---|---|
| `sim-spatial` | Bevy 0.19.1 (curated feature set) | `crates/sim-spatial/Cargo.toml:27` |
| `sim-render` | none — CPU raster (`image`, `ab_glyph`, `epaint_default_fonts`) | `crates/sim-render/Cargo.toml` deps; no `bevy` in `src/` |
| `sim-viewer` | `eframe` 0.36.2 (egui), not Bevy | `crates/sim-viewer/Cargo.toml`; no `bevy` in `src/` |
| `sim-web` | `wasm-bindgen` cdylib only | `crates/sim-web/Cargo.toml`; no `bevy` in `src/` |

All findings below are therefore `crates/sim-spatial/src/` unless stated. The
feature list is deliberately minimal: no `scene`, `gltf`, `audio`, `hdr`, tonemapping
LUTs (`Cargo.toml:18-27`). Note the comment says "no … gamepad" but the list
*does* include `bevy_gilrs` — see F6.

**Headline:** this is a *good*, deeply idiomatic 0.19 integration (States/ComputedStates,
Messages, `ButtonInput`, `InputFocus`, `FileDragAndDrop`, task pools, `WinitSettings`).
The real gaps are narrow: no `FrameTimeDiagnosticsPlugin`/`LogPlugin` tuning, no
`AssetServer` at all (everything `Assets::add` + manual `fs`), no IME handling, no
cursor-icon use, physics/pacing on hand accumulators + `RunThread` instead of
`FixedUpdate`/`Time<Virtual>`, and reflection used only as a settings-contract seam.

## 1. Windowing

### F1 [GOOD] One window, per-mode look, reactive winit — keep
- **Current:** `DefaultPlugins.set(WindowPlugin{ primary_window: Some(Window{title,
  resolution, resize_constraints, ..default()}), close_when_requested:false, ..default() })`
  in `app/mod.rs:387-391`. Per-mode `Look` (title/clear/ambient/size/min/winit) applied
  on `OnEnter(mode)` via `apply_look` (`app/mod.rs:317-430`): mutates `Window.title`/
  `resize_constraints`, re-inserts `ClearColor`, `GlobalAmbientLight`, `WinitSettings`.
  `WinitSettings`: reactive 60 Hz focused / 40 ms low-power unfocused for most modes
  (`app/mod.rs:331-333`); `Continuous` focused for Place (fly) and Phenomena (run-thread
  frames) (`:349,:364`). REST wake writes `RequestRedraw`
  (`app/actions.rs:508`, `app/switch/mod.rs:479`, `app/settings/plugin.rs:45-70`) so a
  reactive window still answers headless-driven requests promptly — exactly the pattern
  Bevy intends for reactive mode.
- **Modern alternative:** none needed. `WinitSettings`/`UpdateMode::reactive` is the
  documented knob (https://docs.rs/bevy/0.19.1/bevy/winit/struct.WinitSettings.html).
- **Impact/cost:** — / —.

### F2 Present mode / vsync never set (default `AutoVsync`)
- **Current:** no `present_mode`, `WindowMode`, fullscreen, `resizable`, or
  `enabled_buttons` anywhere (grep over `sim-spatial/src`); the `Window{..}` literal
  at `app/mod.rs:388` takes `..default()`. On 120 Hz+ displays the Continuous modes
  (Place/Phenomena) render at refresh rate with vsync; in reactive modes vsync can
  add a frame of latency to a click → the code already works around a one-frame lag
  narratively (`app/mod.rs:328-330` "one frame behind a click").
- **Modern alternative:** `Window.present_mode = PresentMode::AutoNoVsync` /
  `Mailbox` (https://docs.rs/bevy/0.19.1/bevy/window/enum.PresentMode.html) per mode,
  e.g. `Mailbox` for Place/Phenomena only. Also consider `Window::mode`/fullscreen
  toggle for Place walkthrough — currently impossible.
- **Impact:** low-mid (input latency, GPU power on high-refresh). **Cost: S** (add a
  field to `Look`, set in `apply_look`; needs per-OS sanity check).

### F3 No cursor icon / grab management
- **Current:** no `CursorIcon`, `CursorOptions`, `CursorGrabMode` use at all (grep:
  zero hits). Orbit/fly drags (`camera/input.rs:124-175`, `camera/fly.rs:72-78`) never
  change the cursor to Grab/Grabbing, and fly-mode never grabs/confines the pointer,
  so look-drag ends at the window edge.
- **Modern alternative:** insert `CursorIcon` on the primary window entity during a
  drag, and `CursorOptions{grab_mode: Locked|Confined, visible:false}` for fly
  (https://docs.rs/bevy/0.19.1/bevy/window/struct.CursorOptions.html).
- **Impact:** mid for Place fly UX; cosmetic elsewhere. **Cost: S.**

### F4 macOS occlusion via raw `objc2` — justified, keep
- **Current:** `rest.rs:36-70`: on macOS reads `NSWindow.occlusionState` through
  `RawHandleWrapper`+`objc2` (deps `Cargo.toml:48-53`) because "winit sends no event
  for a window created hidden" and wgpu 29 draws nothing into a hidden window, so
  `screenshot` refuses (`rest.rs:66-71`); elsewhere uses `WindowOccluded` messages
  properly (`rest.rs:62-70` non-macOS arm).
- **Alternative:** none in Bevy (no occlusion query API). Correctly scoped unsafe with
  `NonSendMarker`. **Impact/cost:** — (track upstream; drop when winit emits it).

## 2. Input: keyboard / mouse / gamepad / text

### F5 [GOOD] Bevy input used correctly throughout
- `ButtonInput<KeyCode>`/`ButtonInput<MouseButton>`/`ButtonInput<Key>` polled in
  systems, never raw winit: `place_view.rs:311`, `camera/input.rs:124-128,245`,
  `ui_kit/text/input.rs:134-141`, `drive_input/input.rs:193-206`. Mouse motion/wheel
  via `Messages<MouseMotion>`+`MessageCursor` (correct for reactive mode where
  multiple updates worth of deltas can batch): `camera/input.rs:45,126-128`,
  `camera/fly.rs:75-78`, `app/picker/modal.rs:68-92`. Focus loss via
  `WindowFocused` messages (`drive_input/input.rs:197,208`; hardware STOP on loss in
  `robot/hardware/actions/input.rs:77`). Text entry is the *sole* `KeyboardInput`
  reader and goes through `bevy_input_focus` (`FocusedInput<KeyboardInput>` observer
  `ui_kit/text/mod.rs:332`, `InputFocus`/`dispatch_focused_input` ordering pinned at
  `ui_kit/text/mod.rs:370`) — exemplary 0.17+ style. No `winit` event types leak
  outside `bevy::winit::WinitSettings` (`app/mod.rs:49`).
- **Minor:** 0.19 also offers `AccumulatedMouseMotion`/`AccumulatedMouseScroll`
  resources; the `MessageCursor` pattern is equivalent and more precise about
  modal consumption — no change. **Cost: —.**

### F6 Gamepad: works, but feature comment lies and deadzone is hand-rolled
- **Current:** `bevy_gilrs` enabled (`Cargo.toml:27`) despite the comment at
  `:18-21` claiming "without audio/gamepad … no … gamepad". Poller queries
  `&Gamepad` components (`drive_input/input.rs:195,217,322-339`) — correct modern
  API (no `Gamepads` resource). However deadzone/axis shaping is hand-coded in the
  shared bindings layer (`drive_input/bindings.rs`, tests at `tests.rs:81-116`
  "deadzone zeroes small deflections and rescales the rest") and duplicated
  conceptually for the browser in `sim-web`.
- **Modern alternative:** `Gamepad`'s built-in `AxisSettings`/`ButtonSettings`
  (deadzone via `gamepad.axis_settings_mut()` /
  https://docs.rs/bevy/0.19.1/bevy/input/gamepad/struct.AxisSettings.html) would
  remove the rescaling math natively — but the shared-with-browser
  `sim.drive-bindings/1` JSON (`crates/sim-runtime/src/drive_bindings.rs:35`) is the
  real contract, so native-only simplification would fork semantics. Verdict: fix the
  Cargo comment (**S**), keep the shared deadzone (**M**, only if web/native parity
  is dropped).
- **Impact:** doc drift now; low runtime risk.

### F7 Custom keymap JSON — justified (cross-surface), not a Bevy gap
- **Current:** drive bindings are a versioned JSON schema `sim.drive-bindings/1`
  parsed/validated in `sim-runtime` (`drive_bindings.rs:1,35`), mapped to
  `KeyCode`/`GamepadAxis`/`GamepadButton` by name in
  `drive_input/bindings.rs:35-62`, persisted via settings
  (`app/settings/actions.rs:39,68`), and shared with the browser
  (`crates/sim-web/src/lib.rs:213`). Mode keys elsewhere are hard-coded `ButtonInput`
  checks (e.g. `phenomena/keys.rs`, `cad/keys.rs`, `camera/input.rs:245`).
- **Modern alternative:** Bevy has no built-in input-mapping/keymap crate (that is
  `leafwing-input-manager` territory, third-party). The hand JSON is the right call
  for a REST-editable, browser-shared binding set. Only nit: name tables in
  `bindings.rs` must be kept in sync with Bevy enum additions manually. **Cost: —.**

### F8 No IME handling — text fields will mishandle CJK/dead-key composition
- **Current:** zero `Ime` references; text comes only from `KeyboardInput.text`
  (`ui_kit/text/input.rs:47-141`). Bevy forwards `bevy::window::Ime`
  (Preedit/Commit) when the window has IME enabled; without enabling it, composition
  input is lost or arrives as raw keys.
- **Modern alternative:** set `Window.ime_enabled = true` while a kit field is
  focused and handle `Ime::Preedit`/`Commit` (+ `ime_position`) in `ui_kit/text`
  (https://docs.rs/bevy/0.19.1/bevy/window/enum.Ime.html).
- **Impact:** mid (international text entry in lessons/notes/search broken).
  **Cost: M** (preedit rendering in the kit's text model is the bulk).

### F9 No touch / trackpad gestures
- **Current:** no `TouchInput`, `PinchGesture`, `PanGesture` (grep zero). macOS
  trackpad pinch-to-zoom and two-finger pan therefore do nothing; wheel zoom only
  (`camera/input.rs:45` handles `MouseScrollUnit::Line|Pixel`, good for wheel/trackpad
  scroll but not pinch).
- **Modern alternative:** read `PinchGesture`/`PanGesture` messages in
  `camera/input.rs` (winit forwards them on macOS;
  https://docs.rs/bevy/0.19.1/bevy/input/gestures/index.html — verify module name at
  implementation time).
- **Impact:** low-mid (laptop UX). **Cost: S.**

### F10 [GOOD] File drag-and-drop via Bevy; file dialogs are hand-rolled (necessarily)
- **Current:** drops use `FileDragAndDrop::DroppedFile` correctly and mode-gated,
  with the two-update message-lifetime edge handled explicitly
  (`cad/references/drop.rs:7-60`, `builder/drafts.rs:222-228`). File *picking* is a
  custom in-window document picker (directory walk on `Pool::Io`, path field,
  recents): `app/picker/mod.rs:1-20`, `app/picker/discover.rs`. No `rfd`/native
  dialog dependency anywhere.
- **Modern alternative:** Bevy has no file-dialog API; native dialogs would need
  `rfd` (third-party) and would break the REST/`system_ui` parity the picker gives
  for free (picker is drivable via `picker:*`, `app/mod.rs:28-31`). Keep. **Cost: —**
  (L if native dialogs ever required, and not recommended).

## 3. Time

### F11 `Res<Time>` (virtual) used for presentation only — fine; `FixedUpdate` unused
- **Current:** 15+ systems take `Res<Time>` (= `Time<Virtual>`) for camera easing,
  label throttling, chart refresh (`physics_view.rs:234,392,515`,
  `camera/orbit.rs:411`, `camera/fly.rs:72`, `lesson/frames.rs:315`,
  `builder/graphs.rs:127`, `view.rs:40` …). `delta_secs().min(0.1)` clamps appear
  ad hoc (`physics_view.rs:392`). Exactly one `Time<Real>` use — video capture
  pacing (`robot/video.rs:143`) — and **zero `FixedUpdate`, `Time<Fixed>`,
  `Time<Virtual>` speed control (`set_relative_speed`), or `Timer`** anywhere
  (grep). The virtual clock is thus never paused/scaled; it is effectively an
  unclamped-beyond-default delta source.
- **Modern alternative:** Bevy's model is `Time<Real>` → `Time<Virtual>`
  (pause/`relative_speed`/`max_delta`) → `FixedUpdate` accumulation
  (https://docs.rs/bevy/0.19.1/bevy/time/index.html). Setting
  `Time<Virtual>::set_max_delta` once would replace scattered `.min(0.1)` clamps.
  Using `Time::pause()` is *not* applicable: simulation time is owned by run
  threads / `sim-runtime`, and pausing Bevy time would freeze camera/UI easing too.
- **Impact:** low. **Cost: S** for max-delta consolidation.

### F12 Physics/exhibit pacing = manual accumulators on run threads, by design
- **Current:** phenomena pacing is a hand accumulator: `real` clamped to 0.05,
  `dt = real × time_scale × speed`, gridded exhibits take whole grid steps carrying
  the remainder (`phenomena/run.rs:132-228`, `accumulator` at `:139,186-192`), ticked
  from `Instant::now()` deltas on the run thread (`:389-416`). Robot/builder runs
  similarly advance inside `RunThread` workers (see F15) with their own
  chunk/step clocks, not in `Update`/`FixedUpdate`.
- **Modern alternative:** `FixedUpdate` + `Time<Fixed>::set_timestep` per exhibit
  grid — but exhibits/runs are *shared non-Bevy code* (`sim-phenomena`,
  `sim-runtime`, also driven headless/browser), and each exhibit has a *different*
  grid, so one fixed timestep cannot serve them; multiple `FixedUpdate` rates are
  not a Bevy feature. Moving stepping into ECS would also put multi-second solver
  chunks on the UI thread. **Verdict: correct architecture; do not migrate.**
  The only real cost is that Bevy's fixed-loop interpolation machinery is unused —
  motion trails/smoothing are hand-built on `Time` (`physics_view.rs:444-452`).
- **Impact:** none (accepted). **Cost: L / not recommended.**

### F13 Wall-clock `Instant` deadlines inside ECS-adjacent state — mostly justified
- **Current:** hardware/session/robot code carries dozens of `Instant::now()`
  deadlines (`robot/hardware/session.rs:100-499`, `sync.rs:479-632`,
  `session/periodic.rs:51-101`, `beat.rs:125-249`) and settings uses an
  `Instant` retry clock (`app/settings/mod.rs:174,274`, checked against
  `Instant::now()` in a system at `app/settings/plugin.rs:102,118`). These are
  real-time lease/heartbeat semantics (motors, network) — `Time<Virtual>` (pausable)
  would be *wrong* there; `Time<Real>` would be equivalent inside systems.
- **Nit:** the settings retry (a UI concern) could use `Time<Real>::elapsed` or a
  Bevy `Timer` so tests can advance it deterministically; today tests must mutate
  `retry_at` directly (`app/settings/tests.rs:131-135`). **Impact:** low (testability).
  **Cost: S.** Hardware deadlines: keep `Instant` (they also run off-thread where no
  `Time` exists).

## 4. Tasks / jobs (`crate::jobs`)

### F14 [GOOD] Custom `Job` is a thin, correct layer *over* Bevy pools — not a rival
- **Current:** `jobs/mod.rs:28-46,156-222`. `Pool::Io` → `IoTaskPool`,
  `Pool::Compute` → `AsyncComputeTaskPool` via `pool().spawn(async move{ run() })`,
  storing the returned `Task<()>`; drop cancels (dropping `Task`, `:259-273`),
  `complete_on_drop` calls `task.detach()` (`:227-230`). `Pool::Dedicated` is a
  named `std::thread` for minute-long/network work so it cannot starve the ≤4-thread
  pools (`:36-40,214-219`). Pools are created headlessly via
  `TaskPoolOptions::default().create_default_pools()` under a `Once` (`:161-164`) —
  correct interop with `TaskPoolPlugin`'s `get_or_init`. On top it adds what Bevy
  `Task` lacks: cooperative cancel token (`AtomicBool`, `Ctx::cancelled` `:108-116`),
  progress (`Progress` `:65-72`), streamed partial updates (`Ctx::emit` `:128-134`),
  panic capture naming the job (`catch_unwind` `:209`), generation stamps and a
  `Latest<T>` supersede wrapper (`:276-319`). An architecture test enforces "no
  thread/process outside `jobs`" (`:1-4`).
- **Modern alternative:** raw `AsyncComputeTaskPool::spawn` + `futures_lite::
  future::poll_once(&mut task)` (the canonical Bevy poll-back,
  https://docs.rs/bevy/0.19.1/bevy/tasks/struct.Task.html) would *lose* progress,
  streaming, cancel tokens and naming. The wrapper's `Mutex<Option<Result>>` poll
  (`Job::poll` `:238-240`) is one uncontended lock per frame per job — negligible.
  **Verdict: keep.** Only nit: `next_update` is O(n) `Vec::remove(0)` (`:246-249`) —
  fine at current rates, `VecDeque` if a stream ever gets deep. **Cost: —/S.**

### F15 `RunThread` (named OS thread + mpsc + stamped snapshot) — no Bevy equivalent
- **Current:** `jobs/run_thread.rs:37-126`: one named thread, `mpsc::Sender<Cmd>` in,
  `Arc<Mutex<S>>` snapshot out with generation stamping (`latest(gen)` `:93-99`),
  drop closes the channel and joins within `JOIN_BOUND = 200 ms`, else detaches with
  a `bevy::log::warn!` (`:109-126`). Child processes likewise (`jobs/child.rs`,
  reaper threads `:330-337` in `mod.rs`).
- **Modern alternative:** none — Bevy pools are for finite tasks, and a permanent
  simulation/hardware thread with a command channel is exactly what
  `AsyncComputeTaskPool` docs tell you *not* to park on a pool thread. Keep.
- **Impact/cost:** —.

### F16 [GOOD] Results are polled back in one ordered ECS set
- **Current:** the frame pipeline reserves `ViewerSet::JobResults` between Actions
  and SimSync (`app/mod.rs:153-171,279-281`); feature apply-systems poll their
  `Job`/`Latest` there (settings `app/settings/plugin.rs:17`, picker/switch
  `app/switch/mod.rs:348-353` + `app/switch/arrival.rs:46`, phenomena/robot/cad/
  project receivers — 30+ `job.poll()` sites, all in `Update` systems, none blocking:
  e.g. `robot/hardware/actions/jobs.rs:12,42,54`, `cad/mesh.rs:373`). No
  `future::block_on`, no `.await` in systems. This is the Bevy-idiomatic shape, just
  with `Job::poll` in place of `poll_once`.
- **Impact/cost:** —.

## 5. Assets

### F17 No `AssetServer` at all — everything is `Assets::add` + manual `fs`
- **Current:** zero `AssetServer`/`asset_server.load` references (grep). Fonts and
  icons are `include_bytes!` + `Assets<Font>::add(Font::from_bytes(..))` /
  procedurally rasterized `Assets<Image>::add` at app build (`ui_kit/theme.rs:88-113`,
  called from `app/mod.rs:412` because first `OnEnter` precedes `Startup`). Display
  models are read with `std::fs::read(catalog.json)` + OBJ text parse behind a custom
  `ModelLibrary` (`models.rs:46,61,120,141`), then `Assets<Mesh>::add`. Reference/
  lesson images go file-bytes → `Image::from_buffer(..)` (`cad/references/planes.rs:98`,
  `lesson/practice/mod.rs:42`). Dynamic charts are `Image::new_fill/new` mutated in
  place (`chart.rs:16`, `builder/calibration/study/*.rs` `images.get_mut` writes).
  The asset root (`assets/`) is unused as an `AssetPlugin` source; `bevy_asset` is
  enabled only as storage/dependency plumbing (`Cargo.toml:27`).
- **Modern alternative:** `AssetServer::load("fonts/…ttf")` etc.
  (https://docs.rs/bevy/0.19.1/bevy/asset/struct.AssetServer.html) would give async
  loading + `LoadState` + hot reload (F18) — but at real costs here: assets live at
  *arbitrary user/workspace paths* (library, models dir, dropped files, lesson
  dirs), not under one asset root, so it would need custom `AssetSource`s; fonts are
  needed synchronously before first `OnEnter`; and most "assets" are generated, not
  files. **Verdict: justified for this app's I/O shape.** The genuine losses are
  (a) no load-state/failure events — errors are ad-hoc strings, (b) no dedup by path
  — the same image dropped twice decodes twice, (c) OBJ parsing rather than glTF
  (deliberate: `gltf` feature off, `Cargo.toml:20-21`).
- **Impact:** low-mid (dev ergonomics, duplicate decodes). **Cost: L** to adopt
  `AssetServer` properly; **S** to add a path-keyed cache in `ModelLibrary`/
  references if duplicate decode ever shows up in profiling.

### F18 No hot reload — but documents already live-reload by hand
- **Current:** no `watch_for_changes`, `AssetEvent::Modified`, or `file_watcher`
  feature (grep zero; feature absent from `Cargo.toml:27`). Shader/font/icon edits
  need a restart. However *user documents* are watched manually: builder polls file
  stamps every 0.4 s of `Time` and reloads on change
  (`builder/background.rs:42-66`), library/parts rescanned on a 2 s cadence.
- **Modern alternative:** `AssetPlugin{ watch_for_changes_override: Some(true) }`
  only pays off after F17. A `Timer`/notify-based watcher would beat 0.4 s polling
  latency, but polling is robust across editors/network FS and already shipped.
- **Impact:** low (developer iteration on shaders/fonts only). **Cost: S** to enable
  the flag (no effect without AssetServer loads) — effectively **L**, tied to F17;
  not recommended independently.

## 6. Reflection / serialization

### F19 Reflect: used narrowly and correctly (settings contract + gizmo groups)
- **Current:** the *only* `register_type` is `PreferenceGroup`
  (`app/settings/plugin.rs:14`), a `#[derive(Reflect, SettingsGroup)]` resource from
  the `bevy-settings` crate (`Cargo.toml:10`, `app/settings/mod.rs:16,28-37`). Its
  four fields are *JSON strings*, re-serialized from the validated owner every change
  (`app/settings/plugin.rs:60-72`); the plugin even asserts the `ReflectSettingsGroup`
  type-data exists in `AppTypeRegistry` (`:22-26`). Other `Reflect` derives are just
  `GizmoConfigGroup`s (`robot/overlay_view.rs:249`, `cad/overlay.rs:62`,
  `cad/transform/mod.rs:147`, `cad/sketch/display.rs:29`, `cad/robot/glyphs.rs:45`).
  No inspector, no `DynamicScene`, no scene serialization (feature `scene` off).
- **Assessment:** the settings reflect layer is currently a *contract seam with no
  consumer* — the code says so itself ("Registration is an actual contract, not a
  marker derive. The seam uses its group/source mapping…", `plugin.rs:20-21`) yet
  nothing reads `PreferenceGroup` reflectively; persistence is hand JSON in
  `app/settings/jobs.rs` via `Pool::Io`. Either wire `bevy-settings`' own load/save
  (dropping the parallel JSON-of-JSON-strings encoding) or drop the derive/registry
  assert as dead weight.
- **Modern alternative:** typed `Reflect` fields (instead of `String` JSON blobs) +
  `bevy-settings` persistence; or plain `serde` (status quo minus the seam).
- **Impact:** low (complexity/double-serialization only). **Cost: M** either way.

### F20 Documents are hand JSON schemas, not Bevy scenes — correct boundary
- **Current:** every document is a versioned serde schema owned outside Bevy
  (`sim.system/2`, `sim.drive-bindings/1`, `.simrobot.json`, `.rcad`) parsed into
  non-ECS domain values, with ECS entities treated as a *view* rebuilt from them.
  `ViewerMode` itself derives `Serialize/Deserialize` for REST (`app/mod.rs:54-55`).
- **Modern alternative:** `DynamicScene`/BSN would couple file formats to ECS
  component layouts and Bevy versions — wrong for files shared with the browser,
  REST clients and headless tools. Keep. **Cost: — (L, not recommended).**

## 7. Diagnostics & logging

### F21 No `FrameTimeDiagnosticsPlugin`; frame timing is hand-rolled (×2)
- **Current:** `DefaultPlugins` is used un-customized for diagnostics
  (`app/mod.rs:387` — only `WindowPlugin` is `.set()`); no
  `FrameTimeDiagnosticsPlugin`, `DiagnosticsStore`, or `LogDiagnosticsPlugin`
  anywhere. Instead: builder measures "worst frame + count >50 ms per 10 s" from
  `Time::delta` by hand (`builder/background.rs:67-79`, surfaced via `system_state`),
  and phenomena keeps its own (wall, sim, real) stats tuple printed with
  `eprintln!` under `SIM_VIEWER_STATS` (`phenomena/run.rs:144-215`).
- **Modern alternative:** `app.add_plugins(FrameTimeDiagnosticsPlugin::default())`
  and read `DiagnosticsStore` for FPS/frame-time (smoothed + raw history)
  (https://docs.rs/bevy/0.19.1/bevy/diagnostic/struct.FrameTimeDiagnosticsPlugin.html),
  feeding the same REST `system_state` fields; optionally `LogDiagnosticsPlugin`
  behind `SIM_VIEWER_STATS` instead of `eprintln!`.
- **Impact:** low-mid (better percentiles, one implementation, works in every mode
  not just Build). **Cost: S.**

### F22 `LogPlugin` left at defaults; most output bypasses it
- **Current:** `bevy_log` feature on, `LogPlugin` comes with `DefaultPlugins` but is
  never `.set()` — so filter/level are Bevy defaults and `RUST_LOG`-style control is
  whatever `LogPlugin::default()` gives. In-app logging does use `bevy::log::warn!`
  correctly where it exists (`jobs/child.rs:79,113`, `jobs/run_thread.rs:123`,
  `app/settings/plugin.rs:55`), and the app explicitly downgrades ECS command errors
  to warnings (`app.set_error_handler(bevy::ecs::error::warn)`, `app/mod.rs:230` —
  good, deliberate). But all CLI/headless/validation output is raw
  `println!`/`eprintln!` (`main.rs:187,227,258-585` — 20+ sites), and the phenomena
  stats line likewise (F21). Those run *before* any `App` exists, so `LogPlugin`
  cannot own them without restructuring `main`.
- **Modern alternative:** `.set(LogPlugin{ filter: "wgpu=error,sim_spatial=info".into(),
  ..default() })` (https://docs.rs/bevy/0.19.1/bevy/log/struct.LogPlugin.html) to
  silence wgpu/naga noise for end users; leave `main`'s pre-app prints (they are CLI
  stdout contracts — `--validate-only` output is presumably scraped).
- **Impact:** low (log noise/supportability). **Cost: S.**

## 8. App exit & lifecycle

### F23 [GOOD] Close is a first-class guarded flow on Bevy primitives
- **Current:** `WindowPlugin.close_when_requested = false` (`app/mod.rs:389`);
  `WindowCloseRequested` → `Act<CloseAction>` action pipeline
  (`app/close.rs:75-77`); a `CloseOwner` state machine refuses while studies/
  preferences are unpreserved, STOPs hardware *before* any refusal
  (`app/close.rs:84-127`); final authorization re-reads facts in `Last` before
  `bevy::window::ExitSystems`, inserting/removing `ClosingWindow` and despawning the
  window only on the second armed frame (`app/close.rs:132-153`) — a precise use of
  Bevy's two-stage window-close contract. `AppExit` itself is *consumed* where
  needed, never written by hand: hardware synchronous STOP in `Last` after
  `ExitSystems` (`robot/hardware/actions.rs:478-510`, `stop_on_exit`), CAD load
  cancellation (`cad/sync/mod.rs:245-247`).
- **Modern alternative:** none — this *is* the modern pattern (vs. writing `AppExit`
  directly and losing the preservation window). Documented residual gap is honest:
  SIGKILL/crash bypasses all of it, covered server-side by leases/watchdogs
  (`actions.rs:491-498`). **Cost: —.**

### F24 Headless mode bypasses Bevy entirely (acceptable, note the fork)
- **Current:** `--headless` never builds an `App`: `rest::headless(scene, link, api, …)`
  polls the REST server in a plain loop (`main.rs:580-587`, `rest.rs` headless fn),
  serving Inspect only.
- **Modern alternative:** `App` with `WinitPlugin` disabled /
  `ScheduleRunnerPlugin` would unify the pipeline, but would drag window/render
  plugin surgery into `DefaultPlugins` for one mode. Current split is contained
  (same action handlers). **Impact:** low (two loops to keep behaviorally equal).
  **Cost: M** if ever unified; not recommended now.

## Priority summary

| # | Finding | Impact | Cost |
|---|---|---|---|
| F21 | Adopt `FrameTimeDiagnosticsPlugin`, retire hand frame-timing | low-mid | S |
| F22 | Tune `LogPlugin` filter (wgpu noise) | low | S |
| F2 | Per-mode `present_mode` (latency/power on Continuous modes) | low-mid | S |
| F3 | `CursorIcon` on drags; grab for fly | mid (Place) | S |
| F9 | Trackpad pinch/pan gestures | low-mid | S |
| F6 | Fix stale "no gamepad" Cargo comment (gilrs *is* on) | doc | S |
| F11/F13 | `Time<Virtual>` max-delta; `Timer` for settings retry | low | S |
| F8 | IME enable + preedit in kit text fields | mid | M |
| F19 | Resolve the consumer-less `bevy-settings`/Reflect seam | low | M |
| F17/F18 | `AssetServer` + hot reload | low-mid | L (not recommended as-is) |
| F12/F20 | `FixedUpdate` physics / `DynamicScene` documents | — | L (not recommended) |

Keep-as-is (verified good): F1 reactive `WinitSettings`+`RequestRedraw`, F5
`ButtonInput`/`MessageCursor`/`InputFocus` input stack, F10 `FileDragAndDrop` +
custom picker, F14–F16 `jobs` over Bevy pools with `ViewerSet::JobResults`
poll-back, F23 guarded close on `ClosingWindow`/`ExitSystems`.


