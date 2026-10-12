# Bevy usage inventory — physics-simulator workspace

Scope: every `Cargo.toml` in the workspace, `Cargo.lock`, and all Bevy
source under `crates/sim-spatial/src` (541 `.rs` files, ~156k lines).
Counts below are line counts from `grep -rn` over that tree (a line may
contain several occurrences; test code is included unless noted).
No cargo/test run was performed for this inventory.

## 1. Version pin and feature set

| Fact | Evidence |
|---|---|
| Workspace declares `bevy = "0.19.1"` (bare, default features, unused directly) | `Cargo.toml:60` |
| Locked `bevy` crate is exactly 0.19.1 | `Cargo.lock` (`name = "bevy"`, `version = "0.19.1"`) |
| The only real consumer pins `bevy = "=0.19.1"` with `default-features = false` | `crates/sim-spatial/Cargo.toml:27` |
| Companion crate `bevy-settings = "=0.19.1"` (third-party, version-matched to Bevy) | `crates/sim-spatial/Cargo.toml:10` |
| Rust edition 2024 workspace-wide | `Cargo.toml:52-54` |

Enabled Bevy features (single line, `crates/sim-spatial/Cargo.toml:27`):
`std`, `async_executor`, `multi_threaded`, `bevy_asset`, `bevy_log`,
`bevy_state`, `bevy_window`, `bevy_winit`, `x11`, `keyboard`, `mouse`,
`bevy_gilrs`, `bevy_render`, `bevy_core_pipeline`, `bevy_camera`,
`bevy_light`, `bevy_mesh`, `bevy_image`, `bevy_color`, `bevy_shader`,
`bevy_material`, `bevy_pbr`, `bevy_gizmos`, `bevy_gizmos_render`,
`bevy_text`, `bevy_ui`, `bevy_ui_render`, `bevy_ui_widgets`,
`bevy_picking`, `mesh_picking`, `ui_picking`, `default_font`, `png`,
`jpeg`.

The manifest comment explains the rebuild from the 0.16.1 set after the
0.17 crate split (camera/light/mesh/shader/ui_render/gizmos_render)
and explicitly lists what is *not* enabled: `tonemapping_luts`,
`smaa_luts`, `hdr`, `scene`, `gltf`, `audio`, `gamepad` as a Bevy
feature (gilrs is enabled instead for drive input), animation
(`crates/sim-spatial/Cargo.toml:19-26`). `bevy_state` exists only for
the one app's `ViewerMode` states, and `bevy_ui_widgets` only for the
UI kit's headless slider; `bevy_feathers` is explicitly rejected
("its palette and controls do not match the kit's builder look",
`crates/sim-spatial/Cargo.toml:25-26`).

Locked transitive Bevy crates in `Cargo.lock` include the full split
set (`bevy_app`, `bevy_ecs`, `bevy_render`, `bevy_ui`, `bevy_ui_widgets`,
`bevy_picking`, `bevy_state`, `bevy_text`, `bevy_gizmos`,
`bevy_input_focus`, `bevy_a11y`, `bevy_gilrs`, `bevy_winit`, …) — i.e.
`DefaultPlugins` is usable despite `default-features = false` because
the feature list reassembles almost all of it.

### Bevy-consuming crates: exactly one

`grep bevy crates/*/Cargo.toml` matches only
`crates/sim-spatial/Cargo.toml`. No `.rs` file outside
`crates/sim-spatial` contains `use bevy` / `bevy::` (checked
`crates/`, `examples/`, `tools/`, `web/`).

- `crates/sim-spatial` — the one native Bevy viewer ("Reusable Bevy
  presentation of shared inspection contracts… Build mode … contains
  no physics of its own", `crates/sim-spatial/src/lib.rs:1-4`).
  It re-exports `spatial` presentation types (`SpatialViewerPlugin`,
  `SpatialScene`, …) at `crates/sim-spatial/src/lib.rs:44-52` and
  glob-imports `bevy::prelude::*` at the crate root for child modules.
- `crates/sim-render` — **not** Bevy: deterministic CPU PNG rendering
  (`crates/sim-render/src/lib.rs:1-2`), deps are `image`, `ab_glyph`,
  `openh264`, `mp4` (`crates/sim-render/Cargo.toml`). It is a custom
  renderer that replaces Bevy for headless image/video export.
- `crates/sim-viewer` — **not** Bevy: an `eframe`/egui schematic
  preview (`crates/sim-viewer/src/main.rs:11`, eframe 0.36.2 in its
  `Cargo.toml`). Launched as a sibling process by sim-spatial
  (`crates/sim-spatial/src/main.rs:377-381`).
- `crates/sim-web` — **not** Bevy: `wasm-bindgen` worker bindings over
  `sim-runtime` (`crates/sim-web/src/lib.rs:1-4`).
- All physics/domain/runtime crates (`sim-core`, `sim-runtime`,
  `sim-domain-*`, `sim-cad`, …) are Bevy-free; sim-spatial depends on
  ~20 of them (`crates/sim-spatial/Cargo.toml:8-46`) and only presents
  their data. Physics never runs inside Bevy systems (see §11).

## 2. App and plugin construction

There is exactly **one** production `App`. `sim-spatial`'s binary
(`crates/sim-spatial/src/main.rs`, clap CLI) resolves launch flags to a
`Launch` value and calls `sim_spatial::app::run(launch)`
(`crates/sim-spatial/src/main.rs:223-229`); the doc states "One `App`,
built by `run` and nowhere else" (`crates/sim-spatial/src/app/mod.rs:3-4`).

`run()` (`crates/sim-spatial/src/app/mod.rs:221-275`):

- `App::new()` at line 224, then
  `app.set_error_handler(bevy::ecs::error::warn)` (line 230) — command
  errors on torn-down entities warn instead of panicking; tests keep
  the strict default.
- `CorePlugin { initial: mode, compact }` (line 231), then launch
  documents inserted as resources (REST server, `ModelLibrary`,
  switch documents, document registry, and per-mode payloads such as
  `SpatialScene`, `Builder`, `Learn`, `RobotView`, `PlaceView`,
  `CadDocument`; lines 232-257).
- One tuple `add_plugins((ModesPlugin, CameraPlugin,
  SpatialViewerPlugin, DriveInputPlugin, BuilderPlugin, LearnPlugin,
  RobotPlugin, PlacePlugin, CadPlugin, PhenomenaPlugin,
  ProjectPlugin))` (lines 258-273), then `.run()`.

`CorePlugin` (`crates/sim-spatial/src/app/mod.rs:384-415`) is where
`DefaultPlugins` is configured — the only `DefaultPlugins` site in
production code:

```rust
app.add_plugins(DefaultPlugins.set(WindowPlugin {
    primary_window: Some(Window { title, resolution, resize_constraints, ..default() }),
    close_when_requested: false, ..default()
}))
```

(lines 387-391). It then inserts `ClearColor`, `GlobalAmbientLight`
and a per-mode `WinitSettings` (lines 392-394), adds
`MeshPickingPlugin` with `MeshPickingSettings { require_markers:
true }` (lines 395-396), the occlusion tracker in `PreUpdate`
(line 398), `UiKitPlugin`, `SettingsPlugin`, `ClosePlugin`
(lines 399-406), an `OnEnter(mode)` look-applier for every mode
(lines 407-409), and pre-loads fonts via
`UiFonts::load(app.world_mut())` because the first mode's `OnEnter`
runs before `Startup` (lines 412-413).

Winit behaviour is per-mode data (`Look`, lines 317-367): most modes
use `UpdateMode::reactive(1/60)` focused /
`reactive_low_power(40ms)` unfocused (lines 331-333); Place and
Phenomena use `UpdateMode::Continuous` while focused
(lines 349, 364). Window title/size/min-size/clear colour/ambient
light are likewise per-mode constants applied on `OnEnter`
(`apply_look`, lines 419-430).

Plugin inventory (all hand-written `impl Plugin`, no `PluginGroup`
except Bevy's own): `CorePlugin`, `ModesPlugin`
(`app/mod.rs:285,379`), `SettingsPlugin`
(`app/settings/plugin.rs:9`), `ClosePlugin` (`app/close.rs:63`),
`UiKitPlugin` (`ui_kit/mod.rs:124`), `TextEntryPlugin`
(`ui_kit/text/mod.rs:375`), `CameraPlugin` (`camera/mod.rs:474`),
`SpatialViewerPlugin` (`inspect_view/mod.rs:333`), `BuilderPlugin`
(`builder.rs:538`), `StudyPlugin`
(`builder/calibration/study/mod.rs:15`), `LearnPlugin`
(`lesson/mod.rs:634`), `RobotPlugin` (`robot/mod.rs:230`),
`PlacePlugin` (`place_view.rs:130`), `PhenomenaPlugin` +
`PhenomenaCorePlugin` (`phenomena/mod.rs:95,107`), `CadPlugin` +
`CadCorePlugin` (`cad/mod.rs:144,194`), `ProjectPlugin`
(`project/mod.rs:360`), `DriveInputPlugin`
(`drive_input/plugin.rs:16`), plus 11 CAD sub-plugins all named
`CoreParts` (e.g. `cad/robot/mod.rs:158`, `cad/tree.rs:152`,
`cad/threads/mod.rs:626`) so windowless "core" halves can be composed
without the window (`cad/mod.rs:176-181`). The repeated
`XPlugin`/`XCorePlugin` split is the testability pattern: core halves
run on `MinimalPlugins + StatesPlugin` in tests
(e.g. `app/tests.rs:73`, `app/registry_tests.rs:8`).

Tests construct ~40 additional `App::new()` instances (grep:
`App::new` in `*_tests.rs` / `#[cfg(test)]` modules), typically
`MinimalPlugins` + `StatesPlugin` + one feature plugin, and drive
`app.update()` manually. There is no second production binary using
Bevy, and no `build_app`/manual schedule runner: the REST "headless"
mode (`main.rs:582-586`) does not build a Bevy `App` at all — it calls
`sim_spatial::rest::headless(...)` directly.

## 3. Schedules, SystemSets, ordering

Everything frame-driven runs in `Update` (268 `add_systems`
occurrences total; only ~35 target other schedules). The other
schedules used: `Startup` (mode switcher spawn, REST wake —
`app/mod.rs:401`; settings load — `app/settings/plugin.rs:18`),
`PreUpdate` (occlusion tracking `app/mod.rs:398`; UI-kit activation
eligibility/containment/consume `ui_kit/activation.rs:62-64`),
`PostUpdate` (UI repaint ordered against `bevy::ui::UiSystems::{Prepare,Layout,PostLayout}`
`ui_kit/mod.rs:130-132`), `Last` (mode-scope entity sweep
`app/mod.rs:293`; CAD exit after `bevy::window::ExitSystems`
`cad/mod.rs:186`), and state-transition schedules `OnEnter`/`OnExit`
(see §4). **`FixedUpdate`/fixed timestep: zero uses.** Physics
stepping happens on background run threads, not in any Bevy schedule
(§11).

The central contract is one chained pipeline in `Update`
(`app/mod.rs:153-185`, configured at `app/mod.rs:278-281`):

```
ViewerSet: Input → Actions → JobResults → SimSync → Present
InputSet (inside Input): Rest → Window
```

Input systems only *write* action messages; one apply system per
action type runs in `Actions`; background-job results land in
`JobResults`; continuous gestures/scene sync in `SimSync`; drawing
only in `Present`. Feature-local public sets nest inside it — 11
`#[derive(SystemSet)]` types in total:

- `ViewerSet`, `InputSet` (`app/mod.rs:155,178`)
- `CameraSet::{Viewport,Navigate,Place}` chained inside `SimSync`
  (`camera/mod.rs:461-466,477-479`)
- `InspectViewSet::{Notes,Link,Camera,Parts}` in `SimSync`
  (`inspect_view/mod.rs:309-320,324-326`)
- `CadSet::{Results,Mesh,Highlight,Plane,View}` and
  `CadKeySet::{Gate,Focus,Keys,ToolKeys,NumericEntry,EscapeTool,Escape}`
  (`cad/mod.rs:76-126`), wired with six `configure_sets` calls
  including partial orders, not just chains (`cad/mod.rs:130-139`)
- `ActivationSet::{Eligibility,Containment,Consume,Validate,Clear}`
  spanning `PreUpdate`/`Update`, ordered against Bevy's own
  `InputSystems`, `InputFocusSystems::Dispatch` and
  `PickingSystems::Hover` (`ui_kit/activation.rs:44-45,55-61`)
- `SettingsSet::Publish` in `JobResults`
  (`app/settings/plugin.rs:5-8,17`), `CloseSet` (`app/close.rs:60`),
  `TextInputSet` (`ui_kit/text/mod.rs:365`), a robot set
  (`robot/mod.rs:211`)

Ordering style: `.in_set(…)` 283 occurrences, `.before/.after` 171,
`.chain()` ~100 (many are iterator chains; schedule chains are the
norm for multi-system tuples, e.g. the 6- and 7-system chains in
`inspect_view/mod.rs:358-369`). Cross-feature ordering against
*private functions* is banned by a source-scanning test,
`ordering_guard::features_order_only_against_public_sets`
(`ordering_guard.rs:380-399`, 530 lines of tokenizer/resolver), and by
schedule-graph tests (`app/ordering_tests.rs`). Ambiguity detection /
`ScheduleBuildSettings` are not customized anywhere (no matches).

Run conditions are the main gating mechanism: `run_if` 119
occurrences, `in_state` 91 — almost every mode system is
`.run_if(in_state(ViewerMode::X))` or `in_state(ModeScope::X)` /
`in_state(SpatialScreen)`, plus the shared
`not(crate::ui_kit::text::typing)` condition that silences key
systems while a text field owns the keyboard
(`place_view.rs:137`, `camera/mod.rs:485`).

## 4. States

One `States` type in the whole workspace: `ViewerMode`
(Inspect/Build/Lessons/Robot/Place/Cad/Phenomena), a serde-serializable
copy enum (`app/mod.rs:54-74`). It is inserted with
`insert_state(initial)` (`app/mod.rs:290`) — no `init_state` — and
switched only through `NextState<ViewerMode>` set by the switch
arrival handler (`app/switch/arrival.rs:35`; 4 `NextState` lines
total). Two `ComputedStates` derive from it (the only two impls):

- `ModeScope` (6 variants; Build+Lessons collapse to one `Builder`
  scope so switching between them despawns nothing)
  (`app/mod.rs:116-139`)
- `SpatialScreen` (present iff Inspect/Build/Lessons)
  (`app/mod.rs:143-151`)

Both set `ALLOW_SAME_STATE_TRANSITIONS: bool = false`. There are no
`SubStates` (0 matches). `add_computed_state::<…>()` for both at
`app/mod.rs:291-292`.

State-scoped cleanup is heavy: `OnEnter` 47 lines, `OnExit` 92,
`DespawnOnExit` 51. The distinctive pattern is automatic scoping: a
`Last`-schedule sweep tags every root `Node`/`Transform` entity
without `ChildOf`/`Persistent` with
`DespawnOnExit<ModeScope>(current)` (`scope_new_entities`,
`app/mod.rs:307-312`), so modes never mark their own spawns.
`OnEnter(ModeScope::Cad)` inserts CAD resources
(`cad/mod.rs:151-159`); `OnExit` handlers remove mode resources by
hand (`cad::clear`, `cad/mod.rs:265-277`) and abandon in-flight REST
actions (`app/actions.rs:282-288`). `State<ViewerMode>` /
`State<ModeScope>` are read in ~32 places.

`bevy_state` being a non-default feature (added solely for this,
`sim-spatial/Cargo.toml:21-22`) means `StatesPlugin` must be added
manually in tests (`app/tests.rs:73` etc.).

## 5. Resources vs Components

Counts: **76** `#[derive(Resource)]` types vs **291**
`#[derive(Component)]` types; `insert_resource` 187 occ,
`init_resource` 122 occ. The component count is inflated by UI marker
components (one per widget/panel/action-button); the *state* lives
overwhelmingly in a handful of very large Resources — one "document"
resource per mode, inserted at launch (`app/mod.rs:236-257`) or on
mode entry, removed on mode exit:

- `Builder` (`builder.rs:161-162`) — the whole system-builder state:
  document, store, registry, palette, run handle (`Option<LiveRun>`),
  compile job, study, textures (`Handle<Image>` map), dozens of UI
  fields (struct spans ~80 lines, `builder.rs:162-230+`).
- `CadDocument` (`cad/document/mod.rs:18-19`) — local archive
  snapshot, undo history, generation, tool + ops state, robot/
  materials/print sub-states; the single largest state object,
  fed by ~692 lines of `cad/document/{mod,state,types}.rs`.
- `SpatialScene` (`inspect_view/mod.rs:32-33`) — parsed
  description + spatial geometry + view state + annotations store
  (fields at `inspect_view/mod.rs:33-69`).
- `RobotView` (`robot/mod.rs:69-70`), `Learn` (`lesson/mod.rs:371-372`),
  `PlaceView` (`place_view.rs:105-106`), `SettingsOwner`
  (`app/settings/mod.rs:114-115`, jobs + persistence state),
  `PreferenceGroup` (`app/settings/mod.rs:28`), `Rest`
  (`rest.rs:9`, wraps the `sim_api::Server`), `Replies`/`InFlight<A>`
  (`app/actions.rs:172,267`), `UiFonts` (`ui_kit/theme.rs:77-85`,
  font/image handles), `ModelLibrary` (`models.rs:33`),
  `DocumentRegistry` (`document/mod.rs:152`), `DriveTarget`/
  `DriveBindings`/`DriveInput` (`drive_input/mod.rs:43`,
  `drive_input/bindings.rs:86`, `drive_input/input.rs:31`).

Components that carry real per-entity state: the shared camera trio
`Orbit` / `OrbitRules` / `ViewArea` (`camera/mod.rs:79,156,220`),
`Part { index }` (`inspect_view/mod.rs:411-414`), `CadBody`,
`LinkMesh(i)` (`robot/scene.rs:120`), kit styling `Look`/`Tint`
(`ui_kit/theme.rs:130,155`), text-field `TextField`/`FieldId`, and
hundreds of zero-sized markers. Required-component attributes are
used for kit widgets, e.g.
`#[require(bevy::ui_widgets::Button, ActivateOnPress, Outline = …)]`
on `Ordinary` (`ui_kit/activation.rs:9-11`) and
`#[require(TabGroup = TabGroup::modal())]` on `ModalFocus`
(`ui_kit/activation.rs:23-25`).

`Res<T>` for the big mode resources is almost always taken as
`Option<Res<…>>` / `Option<ResMut<…>>` in shared systems so one App
can host all modes while only one document exists; mode exit removes
the resource and `run_if(in_state(…))` keeps stale systems from
running. `NonSend` resources: none; the only `NonSend` use is
`NonSendMarker` to pin macOS occlusion reading to the main thread
(`rest.rs:36`). Direct `&mut World` systems exist for the REST poll
and mode switching (`actions::serve(world: &mut World)`,
`app/actions.rs:459`), plus `world.resource_scope` around reply
bookkeeping (`app/actions.rs:353,465`); `world_mut()` appears ~400
times but overwhelmingly in tests.

## 6. Messages, Events, observers

The workspace is fully on the post-rename API: `EventWriter` /
`EventReader` have **0** occurrences. The dominant pattern is not
one message type per intent but a generic envelope:

- `Act<A> { action: A, origin: Origin }`, `impl Message for Act<A>`
  (`app/actions.rs:121-126`). `A` is one enum per mode/feature
  implementing the local `Action` trait (`app/actions.rs:64-85`):
  16 registered features (`registry()`, `app/actions.rs:356-384`) —
  close, settings, window, project, inspect, build, measured_study,
  lessons, robot, hardware, place, cad, phenomena, camera, selection,
  switcher.
- Each plugin calls `actions::register::<A>(app)` =
  `add_message::<Act<A>>().init_resource::<InFlight<A>>()`
  (`app/actions.rs:300-303`); `add_message` total 38 occ.
- Writers: `MessageWriter` 159 occ (buttons/keys write `Act::ui(…)`,
  the REST poll writes with a `Reply` token via
  `world.write_message(act)`, `app/actions.rs:352-354`).
  Readers: `MessageReader` 74 occ; apply systems usually bypass
  `MessageReader` and drain `Messages<Act<A>>` directly
  (`actions::apply`, `app/actions.rs:308-333`) so REST "Pending"
  calls can be re-applied from `InFlight<A>` each frame with a JSON
  continuation.
- Only 2 concrete `#[derive(Message)]` types exist outside `Act`:
  the kit's `FieldMsg` (`ui_kit/text/mod.rs:153`) and drive input's
  `Disarm` (`drive_input/mod.rs:106`). Bevy's own messages are read
  too: `MouseWheel` (scroll helper `ui_kit/scroll.rs:23`),
  `WindowCloseRequested`, `RequestRedraw`, `FileDragAndDrop`
  (81 window-message lines total).
- The design comment is explicit about why messages, not observer
  triggers, carry actions: exactly one consumer per type, ordered
  once per frame in `Actions` (`app/actions.rs:10-15`).

Observers are used narrowly and only for picking/UI-widget events:
`add_observer` 7 occ — `bevy::ui_widgets::slider_self_update`
(`ui_kit/mod.rs:129`), kit activation's `pointer_intent` /
`key_intent` / `capture` on `On<Pointer<Press>>`,
`On<FocusedInput<KeyboardInput>>`, `On<bevy::ui_widgets::Activate>`
(`ui_kit/activation.rs:66-68,120,297,307`) — and entity observers
`.observe(…)` 23 occ, e.g. robot link picking
`.observe(actions::pick_link)` with `On<Pointer<Click>>`
(`robot/scene.rs:121`, `robot/actions/keys.rs:154`), text-field
focus (`ui_kit/text/mod.rs:323`), and Bevy's screenshot API
`world.spawn(Screenshot::primary_window()).observe(save_to_disk(path))`
(`app/switch/mod.rs:478`). No custom `#[derive(Event)]` types and no
`commands.trigger` usage were found. `Pointer<…>` message-readers
(drag/drop family) are used where ordering matters, as a
`SystemParam` bundle of 9 `MessageReader<Pointer<E>>` in the CAD
tree (`cad/tree/input.rs:76-88`).

## 7. Change detection

Lightly used. `Changed<T>` filters: 16 occ — all in the UI kit
(button/tint repaint `ui_kit/widgets.rs:335`, text accessibility
`ui_kit/text/mod.rs:354`, input press `ui_kit/text/input.rs:138`).
`Added<T>`: 2 occ (`Added<CadBody>` fit-on-first-mesh,
`cad/mesh.rs:505`). `RemovedComponents`: 0. The common idiom is
resource-level: `is_changed()` / `set_changed()` /
`bypass_change_detection()` / `set_if_neq` — 67 occ, e.g. the
settings publisher polls its owner through
`bypass_change_detection()` and only calls `set_changed()` when the
published status actually changes
(`app/settings/plugin.rs:50-52`), spatial sync early-outs on
`!scene.is_changed()` (`physics_view.rs:126`), and drive target
writers use `set_if_neq` (`drive_input/mod.rs:145-146`). Most
per-frame sync is unconditional (rebuild-UI-each-frame style, §9)
rather than change-detection driven.

## 8. Queries, Commands, hierarchy, Transform

`Query<…>` 437 occ, `Single<…>` 88 occ (very common for the one
camera/window: `Single<(&Camera,&GlobalTransform), With<Orbit>>`,
`physics_view.rs:515`; often `Option<Single<…>>` in mode-shared
systems, `app/mod.rs:419`). `ParamSet` 32 occ (e.g. pairing a
`MessageReader<FieldMsg>` with the `TextFocus` system-param,
`app/picker/modal.rs:65-66`). `Local<T>` 125 occ for per-system
caches (previous-frame snapshots, steady-state filters). Custom
`SystemParam`s exist (e.g. `TextFocus`, the CAD tree's pointer-message
bundle above).

`Commands`: `commands.spawn` 211 occ + `commands.entity` 207 occ,
plus heavy `world_mut().spawn` in tests. Deferred `commands.queue(|
world| …)` is used where a closure needs the `World`
(`place_view.rs:393`). EntityCommands idioms: `try_insert`
(`app/mod.rs:310`, `ui_kit/activation.rs:124` — deliberately ignoring
despawned targets), `despawn_related::<Children>()` 33 occ for the
rebuild-panels-each-frame UI style (despawn a panel's children and
re-`with_children` them, e.g. `physics_view.rs:647`).

Hierarchy is Bevy-native and current-API: `ChildOf` 29 occ,
`Children` 50 occ, **`with_children`/`with_child`/`add_child` 521
occ** (the main UI construction verb), `children![…]` macro 74 occ.
No old `Parent` component, no custom parenting scheme. Ancestor
walks use `Query<&ChildOf>` + `iter_ancestors`
(`ui_kit/activation.rs:75-82,225`). 3D hierarchy depth is shallow:
mode roots carry a `Transform` (CAD/Robot hang models from a root
rotated −90° about X to map Z-up model space to Y-up display,
`camera/mod.rs:50-54`) with mesh children; there are no animated
skeletons/armatures in ECS (robot link poses are computed off-thread
and written into per-link `Transform`s).

`Transform` 209 occ, `GlobalTransform` 41 occ — read mostly for
label projection / picking rays (`physics_view.rs:515`,
`camera/input.rs:218`); the camera system writes the camera entity's
`Transform` from the custom `Orbit` component each frame
(`camera/orbit.rs:411-422`). No use of Bevy `AnimationPlayer`,
tweens, or transform interpolation/extrapolation; glides/spins are
hand-integrated in `Orbit::step` with `time.delta_secs().min(0.1)`.

## 9. Rendering: meshes, assets, cameras, lights, text

**Assets.** `AssetServer`: **0 occurrences.** Nothing is loaded from
disk through Bevy's asset pipeline; there are no `.wgsl`/custom
shaders, no scene/gltf files (features `scene`/`gltf` off). All
assets are procedural: `Assets<T>` 71 occ, `Handle<T>` 62 occ —
`meshes.add(Cuboid/Cylinder/…)` / custom `Mesh` buffers and
`materials.add(StandardMaterial { … })` (81 `StandardMaterial` occ;
e.g. `physics_view.rs:45-57`). Fonts and icons are embedded bytes
added directly to the asset stores:
`fonts.add(Font::from_bytes(include_bytes!("../../assets/fonts/IBMPlexSans-*.ttf")))`
and 48×48 RGBA `Image`s generated by `sim_core::icons`
(`ui_kit/theme.rs:88-115`). Display-model geometry (OBJ catalog) is
parsed by workspace code into `Mesh` data, never via an asset
loader. Chart/dial images are CPU-rasterised `Assets<Image>`
updated in place. Modern render components throughout: `Mesh3d` /
`MeshMaterial3d` 60 occ, zero `PbrBundle`/`MaterialMeshBundle`.

**Cameras.** Per mode, spawned on `OnEnter`: one `Camera3d`
(14 occ) + one UI `Camera2d` (6 occ) with
`Camera { order: 3, clear_color: ClearColorConfig::None }` +
`IsDefaultUiCamera` (pattern at `place_view.rs:382`,
`phenomena/scene.rs:76`, `robot/ui.rs:164`, `cad/scene.rs:91`).
Inspect additionally spawns two inactive inset/split `Camera3d`s at
orders 1-2 (`inspect_view/scene.rs:43-65`). Separation is by camera
`order` + `Viewport` (the shared camera writes `Camera::viewport` /
sub-camera views from the custom `ViewArea` component,
`camera/mod.rs:217-231`); **`RenderLayers`: 0 uses.** Projection is
swapped at runtime between `Projection::Perspective` and
orthographic from `Orbit` state (`camera/orbit.rs:338-346`).
Tonemapping is set per camera (`Tonemapping::None` in Place,
`place_view.rs:379`); no HDR/bloom/post-processing stack (features
off). Lights: one `GlobalAmbientLight` resource per mode look
(`app/mod.rs:393,422`) plus a single `DirectionalLight` per 3D scene
(11 light lines; shadows on only in Phenomena,
`phenomena/scene.rs:78`). `Gizmos` 80 occ — a primary drawing path
for physics overlays, robot outlines, CAD sketch geometry
(`physics_view.rs:234`, `phenomena/scene.rs:104`).

**Text/UI.** All text is Bevy UI text (`Text`, `TextFont`,
`TextColor`, `TextLayout`) built through the custom kit's
`Kit::text(…) -> TextBundle` (`ui_kit/mod.rs:21`); **`Text2d`: 0
uses** (world-space labels are UI `Node`s repositioned by projecting
with the camera each frame, `physics_view.rs:515-606`). UI layout is
raw `bevy_ui` `Node` (664 occ) — no egui/bevy_egui, no feathers.
Images in UI use `ImageNode` (5 occ).

The UI itself is the workspace's largest Bevy surface: a bespoke kit
in `crates/sim-spatial/src/ui_kit/` (16 modules incl. `text/`),
documented as "The one UI kit… Every mode builds its common UI from
it" (`ui_kit/mod.rs:1-2`), built directly on `bevy_ui` +
`bevy_ui_widgets` + `bevy_input_focus`. Notably:

- Buttons are `bevy::ui_widgets::Button` + `ActivateOnPress` +
  a per-widget action *component*; the kit's activation layer turns
  Bevy's `Activate` observer event into a transient `Activated`
  component that feature input systems convert to `Act<…>` messages
  (`ui_kit/mod.rs:85-97`, `ui_kit/activation.rs:120-126`).
  `ButtonPlugin` is added manually if absent
  (`ui_kit/activation.rs:48-50`), plus `TabNavigationPlugin`
  (line 51) for keyboard focus with modal `TabGroup`s.
- The slider is Bevy's headless `bevy_ui_widgets::Slider` with
  `slider_self_update` observer (`ui_kit/mod.rs:43-45,129`); styling
  is fully custom (`Look`/`Tint`, repaint systems in `PostUpdate`
  before `UiSystems::Prepare`, `ui_kit/mod.rs:130`).
- Text entry is a wholly custom field system on top of Bevy's
  `InputFocus` (`ui_kit/text/`), because Bevy has no text-input
  widget; accessibility goes through Bevy a11y
  (`AccessibleLabel`, systems ordered `.before(bevy::a11y::AccessibilitySystems::Update)`,
  `ui_kit/mod.rs:132`).

## 10. Input and picking

Keyboard: `ButtonInput<KeyCode>` (146 occ; plus `ButtonInput<Key>`
and `ButtonInput<MouseButton>`) read in `InputSet::Window` systems
that translate presses into action messages
(`place_view.rs:311-321`); text fields instead receive
`FocusedInput<KeyboardInput>` through `bevy_input_focus`
(`ui_kit/activation.rs:307`).CAD has its own two-step chord gate and
per-tool key arbitration sets (§3). Mouse: `MouseWheel` messages
for scrolling/zoom (56 mouse-message lines; helper
`ui_kit/scroll.rs:23`), cursor position from `Window`, gesture
state machines hand-written in `camera/input.rs` (orbit/pan/zoom in
SimSync, deliberately not actions, `camera/mod.rs:21-24`).
Gamepad: real use via `bevy_gilrs` — `Gamepad`/`GamepadAxis`/
`GamepadButton` 58 occ, all inside the mode-neutral drive poller
`drive_input/` (bindings in `drive_input/bindings.rs`, poller
`drive_input/input.rs`), mapping sticks/buttons + keys to normalized
drive axes for the robot drive host (`drive_input/mod.rs:1-9`).

Picking is Bevy's (`bevy_picking` + `mesh_picking` + `ui_picking`
features): `MeshPickingPlugin` with `require_markers: true`
(`app/mod.rs:395-396`), `MeshPickingCamera` on 3D cameras
(`robot/ui.rs:142`, `cad/scene.rs:78`), `Pickable` 85 occ (mostly
`Pickable::IGNORE` on UI text/fills). Two layers on top:

1. Simple click targets use entity observers
   (`.observe(pick_link)`, `robot/scene.rs:120-121`).
2. CAD's sub-body (face/edge/vertex) picking bypasses the picking
   backend and ray-casts inline with Bevy's low-level
   `MeshRayCast`/`MeshRayCastSettings` + `HoverMap`
   (`cad/pick.rs:74-75`), including a 7×7 px multi-ray neighbourhood
   and its own filtering (locked/hidden nodes) — documented at
   `cad/pick.rs:1-58`. Hand-rolled ray math beyond that is minimal:
   `Camera::viewport_to_world` 3 occ (zoom-to-cursor anchor
   `camera/input.rs:218`, drag placement
   `builder/placement.rs:535`).

## 11. Time and tasks

`Res<Time>` is used for display pacing only: camera glides/spin and
fly movement (`camera/orbit.rs:422`, `camera/fly.rs:137`), label/
chart refresh via `elapsed_secs_f64` (`physics_view.rs:445,518`),
animation easing. No `Time<Fixed>`/virtual/real specialization, no
`Timer` component anywhere (0 occ) — intervals are `f64` fields in
resources or `std::time::Instant`s. Simulation time is owned by
`sim-runtime` sessions on background threads.

Background work goes through one module, `jobs/`
(`jobs/mod.rs:1-40`), guarded by source tests
(`threads_are_started_only_in_jobs`). It *does* use Bevy's pools —
`IoTaskPool` for file I/O, `AsyncComputeTaskPool` for CPU work —
wrapped in a `Job<T,U>` handle with generation stamps, cancel
tokens, progress and streaming updates, polled (never awaited) from
apply systems in `JobResults` (`jobs/mod.rs:46,147-170,212-220`);
pool selection at 197 `Pool::` call sites. Long or unbounded work
explicitly avoids the pools: `Pool::Dedicated` = a named
`std::thread` (studies, network, recordings), and `RunThread<Cmd,S>`
= one OS thread per simulation run with an mpsc command channel and
a generation-stamped snapshot out (`jobs/run_thread.rs:37`).
`std::thread::spawn` appears 0 times *outside* `jobs/` by design
(inside, via `thread::Builder`, incl. child-process reapers
`jobs/mod.rs:330-344`). Pools are also force-initialized headlessly
via `TaskPoolOptions::default().create_default_pools()` so jobs work
in tests/`--validate-only` (`jobs/mod.rs:156-164`). No async runtime,
no `futures` usage in sim-spatial.

## 12. Reflection and diagnostics

Reflection is almost unused. The single reflected type is the
settings contract:

```rust
#[derive(Resource, Reflect, SettingsGroup, Default, Clone)]
#[reflect(Resource, Default, SettingsGroup)]
#[settings_group(group = "preferences", file = "viewer-preferences")]
pub struct PreferenceGroup { recents: String, hardware: String, … }
```

(`app/settings/mod.rs:28-37`), using the external `bevy-settings`
crate (=0.19.1). It is registered with `app.register_type::<PreferenceGroup>()`
(2 occ total: `app/settings/plugin.rs:14` and its test) and the plugin
asserts the `ReflectSettingsGroup` type-data exists in
`AppTypeRegistry` at build (`plugin.rs:22-26`). The reflected payload
is opaque JSON strings; all real parsing/validation is hand-written
serde below it ("Registration is an actual contract, not a marker
derive", `plugin.rs:20-21`). No other `Reflect` derives, no
`TypeRegistry` reads, no scene (de)serialization, no
`bevy_inspector`-style tooling. Bevy's own derives still pull
`bevy_reflect` into the tree transitively.

Diagnostics: `bevy_diagnostic` comes in via DefaultPlugins but no
`FrameTimeDiagnosticsPlugin`/`DiagnosticsStore` usage exists in
workspace code ("Diagnostic" matches are domain words). Logging is
`bevy::log::warn!` (e.g. `app/settings/plugin.rs:56`,
`jobs/mod.rs:335`) with default `LogPlugin` config. Performance
visibility is homegrown: REST state replies carry timings (job
progress, load seconds), and window occlusion is tracked manually
via `RawHandleWrapper` + platform APIs for REST screenshots
(`rest.rs:36`, macOS `objc2` deps in `sim-spatial/Cargo.toml:48-53`).

## 13. What is disabled, replaced, or deliberately not used

| Bevy capability | Status in this workspace |
|---|---|
| Default features | Off; ~35 features re-enabled by hand (`sim-spatial/Cargo.toml:27`) |
| Audio (`bevy_audio`) | Feature off; no audio anywhere |
| Animation (`bevy_animation`, scene/gltf) | Features off; motion comes from simulation snapshots written into `Transform`s |
| Asset loading (`AssetServer`, loaders, hot reload) | Replaced: procedural assets + `include_bytes!` fonts (§9) |
| Scenes / prefabs / reflection-driven data | Not used (§12) |
| UI framework (feathers, egui) | Replaced by custom `ui_kit` over `bevy_ui`/`bevy_ui_widgets` (§9); egui exists only in separate `sim-viewer`/`sim-diagram` crates |
| CPU/headless rendering | Replaced by `sim-render` (own rasteriser) for PNG/MP4 export |
| Fixed timestep physics in ECS | Replaced by `sim-runtime` run threads + `Job`/`RunThread` polling (§11) |
| `RenderLayers` | Not used; camera order + viewports instead |
| `Text2d` / sprites (`Sprite` only via `ImageNode` UI) | Not used; UI-projected labels instead |
| HDR/bloom/SMAA/tonemapping LUTs | Features off; `Tonemapping::None`/default PBR only |
| Bevy tasks for long work | Partially replaced: pools for short jobs, dedicated threads for long ones (§11) |
| Bevy diagnostics overlay | Not added; REST JSON carries timings instead |
| WASM/web build of the viewer | None: `sim-web` is wasm-bindgen physics only; browser UI is separate JS (`web/`, `index.html`) |
| Window close default | Disabled: `close_when_requested: false` (`app/mod.rs:389`); a custom `ClosePlugin` negotiates unsaved-work drain before exit |
| Command-error panics | Relaxed in production only: `set_error_handler(warn)` (`app/mod.rs:230`) |

## 14. Structural summary

Bevy in this workspace is a **single-application shell**: one
0.19.1 `App`, in one crate (`sim-spatial`, ~156k lines), hosting
seven document modes as `States` over one 5-stage `Update`
pipeline, with all intents funnelled through generic
`Act<A>` messages to one apply system each (so window, keys,
`system_ui` automation and REST share one code path). State is
resource-centric (one giant document resource per mode) with
components mostly as UI markers; rendering is plain PBR + gizmos +
a from-scratch `bevy_ui` kit; and everything Bevy would normally
own beyond that — physics, asset loading, headless rendering,
fixed-step time — lives in sibling workspace crates and on
background threads, entering the ECS only as polled job results in
`ViewerSet::JobResults`.

### Count reference (grep over `crates/sim-spatial/src`, lines)

Resource 76 · Component 291 · SystemSet 11 · States 1 +
ComputedStates 2 · `add_systems` 268 occ · `add_message` 38 ·
MessageWriter 159 · MessageReader 74 · add_observer 7 ·
`.observe(` 23 · Changed 16 · Added 2 · Query 437 · Single 88 ·
ParamSet 32 · Local 125 · commands.spawn 211 · commands.entity 207 ·
with_children 521 · children! 74 · Transform 209 · GlobalTransform
41 · Assets 71 · Handle 62 · StandardMaterial 81 · Mesh3d family 60 ·
Camera3d 14 · Camera2d 6 · Gizmos 80 · ButtonInput 146 · Gamepad 58 ·
Pickable/Pointer 85 · Res<Time>-adjacent 165 (word `Time`) ·
insert_resource 187 · init_resource 122 · run_if 119 · in_state 91 ·
OnEnter 47 · OnExit 92 · AssetServer 0 · FixedUpdate 0 · Timer 0 ·
Text2d 0 · RenderLayers 0 · EventWriter 0.



