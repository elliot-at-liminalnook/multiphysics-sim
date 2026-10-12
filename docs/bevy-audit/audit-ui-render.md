# Bevy audit — UI, text, rendering, cameras, meshes/materials, picking

Scope: `crates/sim-spatial` (the only Bevy window in the workspace),
`crates/sim-render`, `crates/sim-viewer`, `crates/sim-web`.
Bevy pin: `bevy = "0.19.1"` (workspace `Cargo.toml:62`; sim-spatial feature
list at `crates/sim-spatial/Cargo.toml:28`). Method: every cited source
body was opened and read; Bevy alternatives were verified against the
0.19.1 docs.rs pages cited inline. No code was built or run (per brief).

**Verdict up front.** This is an unusually modern Bevy codebase: headless
`bevy_ui_widgets` Button/Slider, `bevy_input_focus` + tab navigation,
`bevy_picking` observers and `MeshRayCast`, retained `GizmoAsset`s,
`DespawnOnExit` state scopes, and AccessKit labels are already adopted.
The remaining gaps are concentrated in four places: (1) the hand-rolled
text editor (no caret motion, selection, clipboard, or IME — a recorded,
deliberate trade-off), (2) widget coverage left on the table
(Checkbox/Radio/ListBox/Scrollbar/Menu/popover reimplemented as chips,
segments and custom wheel code), (3) whole-panel despawn+respawn in the
builder and lesson modes, which forces the elaborate focus-rebinding
machinery in `ui_kit::activation`, and (4) CPU-rasterized chart textures
and a fully CPU offline renderer where Bevy/native equivalents exist
only partially. Cameras, picking and mesh/material handling need
polish, not migration.

## Findings index

| # | Finding | Impact | Cost |
|---|---------|--------|------|
| F1 | Text editing is append/backspace only; no caret, selection, clipboard, IME | High (UX/a11y ceiling) | L |
| F2 | Checkbox/choice/pick reimplemented as chips/segments; no `Checked` semantics | Medium | M |
| F3 | No visible scrollbars; per-mode wheel routing; 0.16 scroll-clamp shim | Medium | M |
| F4 | List rows are individual Buttons, not `ListBox`/`ActiveDescendant` | Medium (keyboard UX, a11y roles) | M |
| F5 | Menus, palette, pie, popups hand-placed; `ui_widgets::popover`/Menu unused | Low–Med | M |
| F6 | Whole-panel despawn+respawn rebuild (builder, lesson); focus rebind machinery exists to survive it | High (perf + complexity) | L |
| F7 | Activation layer reimplements eligibility/modal-focus Bevy partly provides | Medium (maintenance) | M |
| F8 | CAD picking bypasses picking events for a custom press state machine (justified) + hand-rolled screen-space edge/vertex search | Low | — keep |
| F9 | `CadView` duplicates `Camera::world_to_viewport`/`viewport_to_world` math (justified: same-frame, job-safe, model frame) | Low | S (shrink only) |
| F10 | Per-part `materials.add` + shared-asset `get_mut` mutation in inspect view | Medium (correctness/perf risk) | S–M |
| F11 | Charts CPU-rasterized into `Image` textures re-uploaded per update | Medium | M–L |
| F12 | Section view clips by rebuilding meshes; matcap approximated — no custom material/shader anywhere | Medium | M |
| F13 | Accessibility: labels good, roles mostly absent (no `accesskit` dep) | Medium | S–M |
| F14 | No MSAA/AA, `Tonemapping::None` everywhere (feature-gated LUTs absent) | Low–Med (visual) | S |
| F15 | `bevy_feathers` evaluated and rejected; kit duplicates its token layer | Low | — keep |
| F16 | sim-render is a second, CPU 2D/3D renderer; sim-viewer is egui; sim-web renders nothing | Info | — |
| F17 | Icons rasterized per-name into individual 48×48 `Image`s, no atlas; fonts via `include_bytes!` + `Assets::add` (fine) | Low | S |

Good usage worth keeping is collected in §12.

---

## 1. The custom `ui_kit` vs `bevy_ui` / `bevy_ui_widgets` / `bevy_feathers`

### Where it lives and what it is

`crates/sim-spatial/src/ui_kit/` (~3,400 lines incl. tests): `mod.rs`
(contract doc, `ui_kit/mod.rs:1-97`), `widgets.rs`, `theme.rs`,
`activation.rs`, `text/`, `scroll.rs`, `slider.rs`, `form.rs`,
`palette.rs`, `picker.rs`, `path_field.rs`, `pie.rs`, `threads.rs`.
Installed once by `UiKitPlugin` (`ui_kit/mod.rs:124-134`), itself added
by `CorePlugin` next to `DefaultPlugins` and `MeshPickingPlugin`
(`app/mod.rs:387-399`).

Crucially, the kit is a *skin and token layer over Bevy*, not a parallel
UI stack:

- **Layout** is 100% `bevy_ui`: widgets return `(Node, BackgroundColor,
  BorderColor, children![…])` bundles with flex properties
  (`widgets.rs:108-130`); docks are absolutely-positioned `Node`s
  (`widgets.rs:278-299`); clipping/scrolling use `Overflow::clip()` /
  `Overflow::scroll_y()` + `ScrollPosition` (`scroll.rs:15-17`). Nothing
  reimplements flexbox — correct, since Bevy has no higher-level layout
  API to adopt.
- **Buttons** are real `bevy::ui::Button` entities whose `Ordinary`
  marker requires the headless widget contract: `#[require(
  bevy::ui_widgets::Button, bevy::ui_widgets::ActivateOnPress, Outline…)]`
  (`activation.rs:9-11`), with `ButtonPlugin` installed at
  `activation.rs:47-50`. Presses arrive as `On<bevy::ui_widgets::Activate>`
  and are captured into a transient `Activated` component
  (`activation.rs:120-126`) that the owning mode's typed input system
  consumes. This is exactly the modern Bevy pattern: headless widgets
  "have no inherent styling; it's the responsibility of the user"
  (https://docs.rs/bevy/0.19.1/bevy/ui_widgets/index.html).
- **Slider** is the real `bevy::ui_widgets::Slider` with
  `TrackClick::Snap`, `SliderRange`, `SliderStep(0.01)` and the official
  `slider_self_update` observer (`slider.rs:38-58`, `ui_kit/mod.rs:129`).
  Callers poll `SliderValue` (e.g. `lesson/controls.rs:37`,
  `robot/timeline.rs:39`) — the docs' external-state model.
- **Focus** is Bevy's: `InputFocus`, `InputFocusVisible`,
  `TabNavigationPlugin`, modal `TabGroup` (`activation.rs:3-4,22-25,51-54`).
- **Tokens/theme** are plain constants (`theme.rs:10-72`) + two style
  components. This is the one place the kit parallels another Bevy
  crate: `bevy_feathers` (in 0.19, "a collection of styled and themed
  widgets for building editors and inspectors",
  https://docs.rs/bevy/0.19.1/bevy/). The rejection is recorded and
  reasoned: "bevy_feathers is not used: its palette and controls do not
  match the kit's builder look" (`crates/sim-spatial/Cargo.toml:26-28`),
  expanded at `docs/architecture/native-viewer.md:461`. **F15 verdict:
  keep.** Feathers would add theme-prop plumbing for a look the project
  does not want; the kit's `Look`/`Tint` repaint systems
  (`widgets.rs:333-382`, driven by `Changed<Interaction>` — properly
  reactive, not per-frame) are small and audited. The one Feathers idea
  worth borrowing is token *indirection* (props resources) if a light
  theme or high-contrast UI theme is ever needed — CAD already has a
  high-contrast *viewport* mode (`cad/display/draw.rs:61-81`) but the UI
  tokens are compile-time constants, so a UI theme variant is currently
  impossible without recompilation. Cost to add theme resource later: M.

### What the kit reimplements that `bevy_ui_widgets` 0.19.1 already has

The 0.19.1 headless widget set (verified at
https://docs.rs/bevy/0.19.1/bevy/ui_widgets/index.html) includes
`Button`, `Checkbox`, `RadioButton`/`RadioGroup`, `Slider`, `ListBox`/
`ListItem` + `ActiveDescendant`, `MenuButton`/menus, `Scrollbar`,
`EditableTextInputPlugin`, and a `popover` positioning module.
sim-spatial uses only Button + Slider. The gaps:

**F2 — Checkbox and radio semantics (M).** Form checkboxes are toggle
chips: `FieldKind::Check` renders `self.chip(label, …, on)` where `on`
is parsed back from the draft *text* `"true"/"false"`
(`form.rs:274-279`, evaluate at `form.rs:101-105`); choices and picks
are `segment`/`chip` strips (`form.rs:248-269`). Impact: (a) assistive
tech sees buttons, not checkboxes/radios — Bevy's `Checkbox` pairs with
the `Checked` component and emits `ValueChange<bool>` on click or
Enter/Space (https://docs.rs/bevy/0.19.1/bevy/ui_widgets/struct.Checkbox.html);
(b) state round-trips through string parsing; (c) checked/indeterminate
states have to be repainted by hand via `Look::Chip(on)`. Migration:
add `Kit::checkbox`/`Kit::radio_group` wrapping `Checkbox` +
`checkbox_self_update` (the `slider_self_update` precedent is already
in the kit), keep the chip paint. Form `Check` rows are the first
caller. Cost M (kit widget S, form + CAD file-choice callers M).

**F4 — Lists (M).** Every list row is a full focusable `Button`
(`widgets.rs:170-188` `list_item`), so Tab walks every row of the CAD
tree, palette and pickers. Bevy's answer is `ListBox` + `ListItem` with
`ActiveDescendant`: "Individual rows should not be focusable in the
normal way… we track the current 'active' row separately"
(ui_widgets index, `ActiveDescendant`). Impact: keyboard traversal
cost grows with list size; arrow-key list navigation is currently
hand-wired per feature (e.g. `FieldEvent::Arrow` exists mainly "for a
list under the field (the command palette)", `text/mod.rs:143-145`).
Migration: a `Kit::list_box` for palette/picker/tree rows; rows keep
their look. Cost M, mostly in re-plumbing selection-as-action
components to `ValueChange`/active-descendant state.

**F5 — Menus, popovers, tooltips (M, low priority).** Context menus,
the command palette (`palette.rs`), the radial pie (`pie.rs:48-52` in
contract terms) and CAD popups are absolutely-positioned node trees the
caller despawns/rebuilds; Bevy 0.19 ships `ui_widgets::popover`
("Framework for positioning of popups, tooltips, and other popover UI
elements", ui_widgets index) and headless `MenuButton`/menu widgets.
Gain would be viewport-edge flipping and focus/escape behaviour for
free; today each surface re-derives placement and dismissal. This is
real but diffuse; adopt opportunistically per surface, not as a sweep.
Cost M per surface family.

**F3 — Scrolling (M).** Three layers of custom code:
1. `scroll_area` is native (`Overflow::scroll_y` + `ScrollPosition`,
   `scroll.rs:15-17`) — good.
2. But *which* area a wheel event scrolls is re-decided per mode with
   pointer-position geometry: `builder/ui.rs:356` `scroll_panels`,
   `cad/panel.rs:353`, `cad/surfaces/mod.rs:426`, `lesson/ui/mod.rs:377`,
   `phenomena/panel.rs:293`, `project/panel.rs:429`, etc., all consuming
   `wheel_delta` (`scroll.rs:23-30`). Modern `bevy_ui` already routes
   wheel events through picking to the hovered scrollable node and
   updates `ScrollPosition` (the kit's own clamp shim notes the layout
   computes the clamped copy since 0.17, `scroll.rs:32-35`); the modes'
   manual "column under cursor" logic largely duplicates hover
   targeting. Verifying per-mode whether native wheel scrolling matches
   (nested scrollers are the risk) is the bulk of the work. Cost M.
3. `clamp_scroll_positions` (`scroll.rs:36-45`) restores Bevy 0.16's
   write-back of the clamped `ScrollPosition`. It exists because the
   modes *accumulate into* the stored value. If (2) moves to native
   scrolling, this shim can likely go too.
4. **There are no visible scrollbars anywhere** (no `Scrollbar` usage;
   `scrollbar_size` appears only inside the clamp, `scroll.rs:39`).
   `bevy_ui_widgets` has a headless `Scrollbar`. A CAD tree/inspector
   without a scrollbar thumb gives no scroll affordance or drag target.
   Cost S–M to add a kit scrollbar to `scroll_area`.

### What the kit adds that Bevy does not have (justified custom code)

- Token/paint layer (`theme.rs`, `repaint_buttons`/`repaint_tints`) —
  Bevy headless widgets deliberately ship no styling.
- `form`, `palette`, `path_field`, `document_picker`, `pie` — composite
  editor widgets with RoboCAD-parity behaviour (unit-expression
  evaluation at `form.rs:73-135`); no Bevy equivalent exists.
- `dock` layout reservation for the mode-switcher strip
  (`widgets.rs:42-54`) — app-specific chrome math, correctly centralised.

---

## 2. Text: `Text`/`TextSpan` usage and the hand-rolled editor

### Display text — good, modern usage

All UI text goes through `Kit::text`/`mono` returning the 0.19 bundle
`(Text, TextFont, TextColor, TextLayout)` with `FontSize::Px` and
`LineBreak::WordOrCharacter` (`widgets.rs:9-10,70-77`). Fonts are
embedded TTFs added to `Assets<Font>` once at app build
(`theme.rs:88-115`, called from `app/mod.rs:412`); faces are selected
per weight by swapping `FontSource` in `repaint_buttons`
(`widgets.rs:361-366`). Rich text uses real `TextSpan` children
(`markdown.rs:131,222`; subtitle karaoke split at
`lesson/narrate/bar.rs:86-89,141-146`). Live values are updated in
place by mutating `Text.0` (66 sites, e.g. `robot/controls.rs:140`,
`lesson/controls.rs:79`, `phenomena/panel.rs:420-425` with a
change-guard) rather than respawning — correct and cheap. There is no
`Text2d`/world-space text and no hand-rolled glyph layout in the Bevy
app (all world labels are UI nodes projected by `world_to_viewport`,
`builder/markers.rs:110`). `TextUiWriter` is not used; with this kit
structure it would add indirection, not value — direct `&mut Text`
queries are the idiomatic 0.19 route for single-span labels, and the
few multi-span cases already query `&mut TextSpan` directly.

One loss: weight is chosen from three separate font *files*
(regular/medium/semibold) rather than variable-font `FontWeight`.
That matches the shipped IBM Plex statics; no action.

### F1 — Text editing is a from-scratch, append-only editor (L)

The editor is `ui_kit::text`: a durable, node-less field entity holding
a `TextDraft { text: String, select_all: bool }` (`text/draft.rs:17-21`),
edited by one PreUpdate system reading raw `KeyboardInput`
(`text/input.rs:133-255`). The editing model, in full
(`draft.rs:33-63`): type a character (append, or replace if
select-all), Backspace pops one char from the end, Enter/Escape/Tab are
control outcomes. There is **no caret position, no cursor keys
(↑/↓ are repurposed as list navigation, `input.rs:94-95`; ←/→ fall to
`Ignored`), no Home/End/word motion, no selection range, no
Delete-forward, no mouse caret placement, no clipboard, no IME
composition**. The rendered "caret" is a literal `|` appended to the
string (`widgets.rs:248-254`).

The modern alternative exists in 0.19.1 and was *evaluated in depth and
rejected with reasons recorded* (`docs/architecture/native-viewer.md:5251-5311`):
`bevy::text::EditableText` wraps a parley `PlainEditor` with
`pending_edits`, `pending_paste: Option<ClipboardRead>`, cursor blink,
`max_characters`, `is_composing()` (IME)
(https://docs.rs/bevy/0.19.1/bevy/text/struct.EditableText.html;
widget driver `EditableTextInputPlugin` in the ui_widgets index).
The recorded blockers are real: no placeholder/submit/AccessKit in
0.19.1's editor, editor state must live on a persistent node while
kit panels despawn+respawn (see F6), windowless tests would need
font/layout/clipboard resources, and the `system_clipboard` feature is
off so copy/paste would be in-app only
(`native-viewer.md:5292-5305`).

Audit assessment: **the decision is defensible today, but it is the
single largest UX ceiling in the app**, and the blocker list is
coupled to F6 (panel rebuild). Two cheaper intermediate steps before
any `EditableText` migration:

1. Grow `TextDraft` a caret + selection range and the standard motion
   keys (Left/Right/Home/End/Shift-select, Ctrl+word). The draft is a
   pure struct with pure tests (`text/draft.rs`, `text/tests.rs`), so
   this is contained. Cost M. Impact: every numeric/name/path field in
   CAD/builder/robot stops being append-only — the most-touched
   papercut for keyboard users.
2. Add clipboard via Bevy's clipboard resource once the feature
   question is settled (`system_clipboard`); `EditableText`'s
   `pending_paste` shows the intended shape. Cost S–M.

Full `EditableText` adoption stays L and should be re-evaluated only
after F6 (persistent nodes) or a Bevy release adding placeholder +
AccessKit to the editor, exactly as `native-viewer.md:5309-5311` says.

Note the genuinely good parts that any replacement must preserve:
one focus (`InputFocus`) and one `typing` run condition gating every
mode keymap/camera/pick (`text/mod.rs:159-184`); `release_held`
semantics that stop hardware jogs when a field takes the keyboard
(`text/input.rs:22-32` — safety-critical, well documented); field
entities survive mode scopes because they have no `Node`
(`text/mod.rs:5-8`).

---

## 3. Picking: `bevy_picking` vs manual raycast

### Adopted, current API

- `MeshPickingPlugin` + `MeshPickingSettings { require_markers: true }`
  (`app/mod.rs:395-396`); cameras opt in with `MeshPickingCamera`
  (`cad/scene.rs:78`, `inspect_view/scene.rs:32`); parts carry
  `Pickable::default()` and entity observers `On<Pointer<Click>>`,
  `On<Pointer<Over/Out>>`, drag observers
  (`inspect_view/scene.rs:135,160,171-219`; `view.rs:186`;
  `linked.rs:116`; `robot/actions/keys.rs:154`). This is the pattern
  the picking docs recommend ("using observers is often more
  expressive… events bubble up the entity hierarchy",
  https://docs.rs/bevy/0.19.1/bevy/picking/index.html).
- Non-targets are correctly excluded with `Pickable::IGNORE`
  (markers, quads, pins, split labels — e.g. `cad/mesh.rs` bodies are
  `Pickable::default()` while plates are `IGNORE`,
  `cad/display/draw.rs:597,606`).
- CAD's tree drag-and-drop reads the full `Pointer<Drag…>` family as
  messages (`cad/tree/input.rs:80-88`) — also a supported consumption
  style.
- Where picking events are the wrong tool (CAD sub-body selection),
  the code uses Bevy's own immediate raycast: `MeshRayCast` system
  param with `MeshRayCastSettings::with_filter/with_visibility/
  never_early_exit` and `RayCastBackfaces` on bodies
  (`cad/pick.rs:74-75,183-200`; `cad/mesh.rs:7,389`). `hit.triangle_index`
  maps to B-rep faces via the drawn mesh's own triangle order
  (`cad/pick.rs:14-18,208-215`) — the right way to get face picking
  out of 0.19.

### F8 — Custom CAD pick pipeline on top (keep, with one caveat)

Around that core, `cad/pick.rs` (≈800 lines with `cad/measure.rs`,
`cad/snap.rs`, `cad/transform/gizmo.rs`) hand-rolls what picking
cannot express: click-vs-box Manhattan slop, a 9-ray 7×7px candidate
ring (`pick.rs:217-235`, RoboCAD parity), edge/vertex picking as a
screen-space nearest-polyline search with ray-depth occlusion
(`pick.rs:19-21,59-63,246-260`; `segment_distance` at
`cad/measure.rs:178` and again in f64 at `cad/selection/mod.rs:354-355`),
33 ms hover coalescing with an off-thread `Pool::Compute` search
(`pick.rs:45-52`), and `HoverMap`-based "is the pointer over UI" gates
(`cad/scene.rs:162-164`, duplicated at `camera/input.rs:30`).

Assessment: **justified.** No Bevy API picks *sampled B-rep edges*
within 6 px on screen or enumerates Alt-click candidates; a custom
picking *backend* (~100 lines per the docs) could feed these hits into
the standard pipeline, but CAD needs the candidate *list*, locked-node
transparency, and section-clip filtering, which fit the current
action-writing design better. Two small cleanups only:

- The `HoverMap` over-UI test is copy-pasted (`cad/scene.rs:162-164`
  vs `camera/input.rs:30`, plus ~15 `Option<Res<HoverMap>>` parameters
  threaded through CAD tools). One shared `SystemParam` would remove
  the plumbing. Cost S.
- `segment_distance` exists twice (f32 screen, f64 model). Trivial;
  leave or unify in passing. Cost S.

### F9 — `CadView`: hand-rolled projection/ray math (keep core, S trim)

`cad/view.rs:48-107` reimplements `Camera::world_to_viewport` /
`viewport_to_world` in the model (mm, Z-up) frame, plus `ray_plane`.
The module doc gives the two reasons, and both hold up: jobs and
apply-systems need a *copyable* snapshot without a camera query, and
Bevy's `GlobalTransform`/`Camera::clip_from_view` are PostUpdate
values that would lag `Orbit::place` by a frame while orbiting
(`cad/view.rs:122-132`). Other modes do use the native calls
(`builder/placement.rs:484,535`; `camera/input.rs:218`). No migration
recommended; at most, `ray()` could be built from
`Camera::viewport_to_world` on the snapshot's matrices — cosmetic. S.

There is **no hand-rolled ray/AABB or ray/triangle intersection code
anywhere** — triangle tests are all `MeshRayCast`, AABBs are used only
as screen-rectangle early-outs (`pick.rs:53-56,246-260`), which is the
right division of labour.

---

## 4. Interaction & activation (F7)

Hover/press painting uses `bevy::ui::Interaction` + `Changed<…>`
filters (`widgets.rs:333-382`) — textbook. On top sits the kit's most
complex module, `ui_kit/activation.rs` (309 lines, plus 257 of tests):
an `Activated` occurrence component, `PointerGate`, `HeldControl`/
`KeyboardOnly` opt-outs, five ordered `ActivationSet`s, modal focus
stack with priority + ancestry ordering, eligibility sweeps that
insert/remove `TabIndex`/`InteractionDisabled`, focus rebinding by
`InputIdentity` + document `RenderSource` across panel rebuilds, and
stale-source activation invalidation (`activation.rs:44-309`).

Why it exists: three real gaps in stock Bevy behaviour as used here —
(1) tab navigation filters only on `TabIndex`, not on hidden/disabled
ancestors (`activation.rs:85-86`); (2) `Button` activates for any
pointer cause, and secondary-button presses must not become ordinary
actions (`activation.rs:294-306`); (3) modal focus must survive the
wholesale panel rebuilds of F6 and route Enter/Space to buttons but
never while a text field types (`activation.rs:128-135,153-270`).

Assessment: this is the *right shape* (it wraps `ui_widgets::Activate`
rather than replacing `Interaction`), but it is also the module most
exposed to upstream API churn — `bevy_ui_widgets` warns its API "is
likely to change substantially" (ui_widgets index). Two reductions
available without a redesign:

- If F6 lands (persistent panels), the `InputIdentity`/`RenderSource`
  rebinding and modal-rebuild retention logic
  (`activation.rs:168-256`) mostly evaporates — focus would simply
  stay on the surviving entity. That is roughly a third of the module.
- `eligibility` runs every PreUpdate over every `Ordinary` control
  and walks ancestors per control (`activation.rs:75-118`). With
  whole-panel rebuilds this doubles as the TabIndex bootstrapper;
  with persistent nodes it could be change-driven
  (`Changed<Node/Visibility/Enabled>` + hierarchy hooks). Cost M,
  do it as part of F6, not before.

---

## 5. Cameras

### Current shape — one shared, custom orbit controller (justified)

Every 3D mode uses one camera module (`crates/sim-spatial/src/camera/`,
~1,800 lines incl. tests): `Orbit` + `OrbitRules` + `ViewArea`
components, intents as typed `CameraAction`s applied by one system,
pointer gestures sharing the same `Orbit` methods
(`camera/mod.rs:1-24,78-231,474-491`). Modes spawn plain
`Camera3d::default()` + `Projection::Perspective(…)` +
`MeshPickingCamera` and a separate `Camera2d` UI camera at `order: 3`
with `IsDefaultUiCamera` (`cad/scene.rs:70-92`;
`inspect_view/scene.rs:26-89`, incl. split-view camera at order 1 and
picture-in-picture inset at order 2; same pattern in
`phenomena/scene.rs:49-76`, `robot/ui.rs:141,164`,
`place_view.rs:379-382`). Viewports between docks and lesson-card
sub-views use native `Camera::viewport` + `SubCameraView`, written
only on change (`camera/viewport.rs:30-87`) — a 0.19 feature used
exactly as intended. Ortho/perspective switching swaps `Projection`
with same-height preservation (`camera/orbit.rs:338-356`).

Bevy ships **no orbit controller** in-tree (only examples such as
`pan_orbit_camera`), and this one must reproduce RoboCAD's turntable/
trackball, named views, Z↔Y-up conversion, zoom-to-cursor, glides,
spin, and REST-addressable actions (`camera/mod.rs:26-54,329-427`) —
far beyond any example controller. **Verdict: keep; nothing to
migrate to.** The same holds for Place mode's WASD fly camera
(`camera/fly.rs:28,69`).

Minor notes:

- **No `RenderLayers` anywhere** (0 hits). Layering is done with
  camera `order` + viewports, and 3D gizmo lines therefore also draw
  on the UI `Camera2d` — a recorded known wart
  (`cad/overlay.rs:33`). `RenderLayers` on gizmo config + cameras
  would fix the double-draw cleanly. Cost S.
- Depth precision is handled manually per mode (near 0.001/0.0005 +
  explicit `near_clip_plane`, `cad/scene.rs:77`,
  `inspect_view/scene.rs:28-31,45,66`). Bevy 0.19 has no logarithmic
  depth; the manual near planes are the available tool. Keep.
- **F14 (part):** every camera sets `Tonemapping::None`
  (`cad/scene.rs:73-75` explains: built without `tonemapping_luts`,
  the default would sample a placeholder LUT) and there is no MSAA
  anywhere (0 hits for `Msaa::`). Enabling `Msaa::Sample4` on the 3D
  cameras is a one-line-per-camera visual upgrade for CAD edge
  quality; tonemapping would require adding the LUT features and
  re-tuning the colour pipeline that currently matches RoboCAD's
  flat look — leave that alone. Cost S (MSAA only).

---

## 6. Meshes and materials

### CAD path — exemplary

- Meshes are built off-thread from B-rep tessellation with
  per-face split normals (`cad/mesh.rs:274-320`, on `Pool::Compute`
  jobs, `cad/mesh.rs:413-417`), moved into `Mesh` without copies, and
  applied by **swapping `Mesh3d(handle)` on the existing entity**
  (`cad/mesh.rs:381-393`). Bodies despawn only when nodes are
  hidden/deleted or the document changes (`cad/mesh.rs:336-367`).
  There is no per-frame `meshes.add`.
- Materials are cached: one `StandardMaterial` per quantized colour
  plus a selection material (`cad/mesh.rs:234-252`); display modes
  (xray/wireframe/matcap) derive copies cached by
  `(AssetId, MaterialLook)` and swap handles in `MeshMaterial3d`
  (`cad/display/draw.rs:101-160`). Highlighting swaps handles, it
  does not mutate shared assets (`cad/mesh.rs:503-504`).

### F10 — inspect/builder path: per-part materials + shared mutation (S–M)

`inspect_view/scene.rs:126-161` calls `materials.add(…)` for **every
model piece and every primitive part** at scene spawn, and
`update_parts` then does `materials.get_mut(&handle.0)` per part per
scene change to recolour/alpha/glow (`scene.rs:243-281`; same pattern
`linked.rs:143`, `phenomena/scene.rs:157`). Consequences: identical
materials are not deduplicated (draw-state churn), and the pattern is
one refactor away from the classic "mutating a shared asset recolors
everything" bug — it is only safe today because each part owns its
handle. Options, in order of increasing payoff: (a) cache by
(colour, metallic) like `CadMaterials` and swap handles for
selection/ghost states — S; (b) for the per-part tint/alpha cases,
keep per-part materials but create them lazily on first tint — S.
Cost S–M. Note `builder/rebuild.rs:194-195` similarly allocates a
fresh mesh+material per reference quad rebuild; acceptable
(rebuild-on-change, not per frame).

### F12 — No custom materials/shaders at all; two workarounds show it (M)

There is not one `Material`, `AsBindGroup`, `ExtendedMaterial`, or
WGSL shader in the crate (0 hits) — everything is `StandardMaterial`.
Mostly a virtue (less surface to break on upgrades), with two costs:

1. **Section view** cannot use a clip plane ("Bevy's
   `StandardMaterial` has no clip plane", `cad/display/section.rs:9-13`),
   so every body gets a *CPU-clipped copy* of its tessellation built
   on a job and swapped into `Mesh3d`, with careful triangle-order
   preservation so face picking still works
   (`section.rs:13-22,372-465`). An `ExtendedMaterial<StandardMaterial>`
   with a fragment discard on `dot(n, world_pos) > d` would delete
   the copy-build/cache machinery *but* would break the triangle↔face
   picking invariant (the raycast would hit clipped-away triangles),
   needing a shader-aware pick filter. Net: M, only worth it if
   section-drag latency becomes a complaint.
2. **Matcap** is approximated by tinting (`draw.rs:11-17,112-118`);
   a true matcap needs a custom `Material` sampling a sphere texture
   by view-space normal. Cosmetic; M if RoboCAD visual parity is
   ever audited.

Wireframe-via-transparent-material + gizmo edges (`draw.rs:8-11`) is
likewise a workaround for the absence of a polygon-mode/material
wireframe option in the shipped build; acceptable.

### Gizmo usage — good, modern

Immediate `Gizmos` for transient overlays (129 sites: snap markers,
measure lines, sketch previews, robot glyphs), custom `GizmoConfig`
groups per concern (`ToolGizmos`, `CadHighlightGizmos` with width 3 +
depth bias `cad/overlay.rs:63-79`, `RobotGizmos`
`cad/robot/glyphs.rs:46-49`, `SketchGizmos`
`cad/sketch/display.rs:30-33`), and — the part many codebases miss —
**retained `GizmoAsset`s** rebuilt only when inputs change for
persistent line sets (grid, axes, display edges:
`cad/display/draw.rs:283-471`; edge-mode outlines
`cad/overlay.rs:87-113`). No action. One nit: `inspect_view/scene.rs:285-319`
redraws a 25×2-line grid + net lines through immediate gizmos every
frame; it is small, but it is the kind of thing `GizmoAsset` exists
for. Cost S if profiling ever points there.

---

## 7. Sprites, images, charts (F11, F17)

No `Sprite`/`TextureAtlas` usage — correct for a UI+3D app; charts and
icons are `ImageNode`s (`widgets.rs:181,303-306`).

**F11 — Charts are CPU-rasterized textures (M–L).**
`crates/sim-spatial/src/chart.rs` draws traces pixel-by-pixel into a
720×200 RGBA buffer (`chart.rs:10,36-116`); callers keep a persistent
`Handle<Image>` (good — `get_or_insert_with(blank_image)`, no
per-frame `images.add`) and replace `image.data` on update
(`builder/graphs.rs:157-204`, `phenomena/panel.rs:577`,
`robot/hardware/panel.rs:604-650`). Every update therefore re-uploads
576 KB per chart and re-rasterizes on CPU. Bevy has no charting or 2D
vector API in-tree, so the alternatives are: gizmo/UI-node line
rendering (loses the shared look with sim-render exports), or keeping
this. Given chart counts are small and updates are throttled by
callers, **keep**, with one cheap improvement: several producers
still allocate a fresh `Image` per result instead of reusing the
handle+`data=` pattern (`lesson/watch.rs:233,267,297`,
`cad/experiment_review/plots.rs:64-80`) — normalize them. Cost S.
A real GPU line pipeline is L and not justified by current chart
counts.

**F17 — Icons (S, optional).** Each icon name is rasterized to its own
48×48 `Image` asset (`theme.rs:89-104` via `sim_core::icons::rgba`).
Fine at current icon counts (one texture bind per distinct icon per
draw is absorbed by UI batching by texture); a texture atlas would
matter only if icon variety per panel grows a lot. No action.

Dynamic-image handling elsewhere is correct: `Image::new_fill` blank
+ `RenderAssetUsages::default()` (`chart.rs:15-17`), measured/figure
images added once per result (`lesson/practice/mod.rs:39,64-72`).

---

## 8. Hierarchy rebuild patterns (F6) — the structural finding

Two regimes coexist:

**Targeted updates (the good exemplar).** Phenomena rebuilds its
inspector only when the shown exhibit/generation changes
(`phenomena/panel.rs:315-359`, `despawn_related::<Children>()` on a
persistent root) and *refreshes values in place* — `&mut Text`,
`&mut Tint`, `&mut Enabled`, node widths — every frame/4 Hz
(`panel.rs:428-460`). CAD middle-ground: several surfaces despawn
only their children (`cad/display/ui.rs:181`, `cad/numeric.rs:321`,
`cad/overlay.rs:302,372`, `cad/panel.rs:438`,
`cad/surfaces/toolbar.rs:176,187`), and mode teardown uses stock
`DespawnOnExit<ModeScope>` state scoping (`app/mod.rs:307-310`) —
exactly current Bevy practice. The switcher updates `Look`s in place
(`app/switcher.rs:74`).

**Whole-panel despawn+respawn (builder, lesson).**
`builder::ui::rebuild_panel` despawns *every* `BuilderPanel` root and
respawns toolbar+sidebar+inspector+graph dock+schematic+status bar
whenever `panel_dirty` is set (`builder/ui.rs:116-148`); `panel_dirty`
is set from 100+ action sites. `lesson::ui::rebuild` does the same
for the whole lesson page on `learn.dirty` (`lesson/ui/mod.rs:63-112`).
Both manually rescue state across the rebuild: scroll offsets are
read out before despawn and fed back in (`builder/ui.rs:126-128`;
`lesson/ui/mod.rs:91-97`), rebuilds are deferred while the left mouse
button is down so the pressed entity survives its click
(`builder/ui.rs:122`; `lesson/ui/mod.rs:85-87`), and — the expensive
part — keyboard focus, modal return targets, text-field anchors and
in-flight activations are rebound/invalidated by identity after every
rebuild (`ui_kit/activation.rs:168-256,272-292`; `text/mod.rs:71-81`).

Impact: (a) each dirty flag costs a full subtree spawn + layout +
accessibility-tree rebuild (the builder even times the schematic
respawn, `builder/ui.rs:141-147`); (b) it is the root cause of F7's
complexity and F1's `EditableText` blocker #1
(`native-viewer.md:5294-5298`); (c) any future per-entity ephemeral
state (hover animations, `EditableText`, list active-descendants)
must fight it.

Modern Bevy direction is the phenomena pattern: persistent roots,
`Changed<T>`-driven in-place updates, observers for one-shot
reactions, and child-list replacement only where the *list itself*
changed. Migration is incremental, not big-bang: start with the
builder's inspector/status bar (mostly text values — `phenomena`'s
`Mark` component pattern at `phenomena/panel.rs:367-396` is the
in-house template), then sidebar lists, leaving the schematic (a
genuinely structural view) as a child-replacement. Cost L overall,
decomposable into S–M steps; each step deletes activation-rebind
cases. This is the highest-leverage item in this audit because it
unlocks F1 and shrinks F7.

---

## 9. Accessibility (AccessKit) (F13)

Foundation is better than most Bevy apps:

- `DefaultPlugins` ships `AccessibilityPlugin`; every kit button/list
  row/property/input/slider carries `AccessibleLabel`
  (`widgets.rs:116,176,217,262`; `slider.rs:51`), labels follow text
  rewrites (`widgets.rs:387-394`), and `keep_labels` re-applies them
  after `bevy_ui`'s own labelling pass, ordered before
  `AccessibilitySystems::Update` (`widgets.rs:396-406`,
  `ui_kit/mod.rs:132`) — someone has clearly read Bevy's a11y
  pipeline closely.
- Text fields get a hand-built `AccessibilityNode` with label,
  placeholder and live draft value so Bevy doesn't skip AccessKit
  updates for a node-less focus (`text/mod.rs:315-323,353-360`).
- Focus is visible (`Outline` driven by `InputFocusVisible`,
  `activation.rs:139-145`), tab navigation is real (§1), sliders set
  a sane AT step (`slider.rs:48-49`), `reduced_motion` is honoured
  by the camera (`camera/mod.rs:182-183`).

Gaps:

- **Roles are almost entirely absent.** The only explicit
  `AccessibilityNode` construction sets label/placeholder/value but no
  role — the comment admits why: "no role: the `accesskit` crate is
  not a dependency" (`text/mod.rs:315-317`). So text fields don't
  announce as text inputs, chips/toggles announce as buttons
  (see F2 — Bevy's `Checkbox` docs even cover overriding the role to
  `Switch`), list rows aren't list items, sections aren't headings.
  Adding `accesskit` (already in the tree transitively via Bevy/
  winit, and a direct dep of sim-viewer through eframe) + setting
  roles in the kit's widget constructors is S for the kit, M to cover
  ad-hoc mode UI. **Highest value-per-cost a11y item.**
- Chart images have no text alternative at the Bevy layer (axis
  corner labels exist visually, `widgets.rs:308-316`); an
  `AccessibleLabel` summarizing range/last values on `chart_image`
  would be S.
- The `|` caret is rendered *into* the text string
  (`widgets.rs:250-251`), so a screen reader reading the node hears
  the pipe. Minor; disappears if F1 step 1 introduces a real caret
  drawn as a separate node.

---

## 10. sim-render, sim-viewer, sim-web (F16)

- **sim-render does not use Bevy at all** — and shouldn't. It is a
  deterministic CPU PNG/video renderer over immutable snapshots
  (`crates/sim-render/src/lib.rs:1-2`), with its own tiny canvas:
  per-pixel blending, `ab_glyph` text with manual kerning/caret
  layout (`canvas.rs:17-99`), software 3D in `physical.rs`/`cad.rs`,
  H.264 via `openh264` (`video.rs`). Bevy's renderer is the wrong
  tool for headless, byte-deterministic artifacts; a Bevy
  "screenshot" path exists in sim-spatial for the live window
  (`app/switch/mod.rs:478`, `Screenshot::primary_window()` + observe)
  and the two are complementary. One duplication note: sim-render's
  `graphs.rs` charts and sim-spatial's `chart.rs` are two CPU
  rasterizers for the same visual job (different inputs:
  publication vs live). Unifying the trace rasterizer behind
  sim-render would be S–M and mostly a hygiene win.
- **sim-viewer renders with egui/eframe, not Bevy**
  (`crates/sim-viewer/src/main.rs:9`, `eframe … features=["accesskit",
  …]` in its `Cargo.toml`); its `render.rs` only delegates to
  sim-render off-thread (`render.rs:18-80`). Out of Bevy scope; noted
  because the workspace therefore maintains *three* UI/render stacks
  (Bevy, egui, CPU canvas) — a product decision, not a Bevy finding.
- **sim-web renders nothing**: it is `wasm-bindgen` physics/session
  bindings returning JSON (`crates/sim-web/src/lib.rs:1-17`). No
  Bevy, no canvas. Nothing to audit.

---

## 11. Prioritized roadmap

1. **F13 roles (S–M)** — add `accesskit` dep; set roles in kit
   constructors (`TextInput` for fields first).
2. **F1 step 1 (M)** — caret/selection/motion keys in `TextDraft`.
3. **F3 scrollbar + native wheel audit (M)** — kit scrollbar;
   per-mode check whether manual wheel routing can retire, taking
   `clamp_scroll_positions` with it where possible.
4. **F10 (S–M)** — material cache in inspect view (mirror
   `CadMaterials`).
5. **F2/F4 (M each)** — `Kit::checkbox`/`radio` on headless widgets;
   `ListBox` pilot on the command palette.
6. **F6 (L, incremental)** — convert builder inspector/status → 
   sidebar to persistent-node targeted updates (phenomena pattern);
   each step simplifies F7 and unblocks F1's endgame.
7. **F14 MSAA (S)**, **§5 RenderLayers nit (S)**, **F11 image-handle
   normalization (S)** — opportunistic.
8. Re-evaluate **F1 `EditableText`** and **F12 clip-plane material**
   only on the stated triggers (F6 done / Bevy editor gains
   placeholder+AccessKit; section latency complaints).

## 12. Good usage to keep (explicitly)

- Headless-widget architecture: `ui_widgets::Button/ActivateOnPress/
  Slider` + own paint layer; `slider_self_update` observer
  (`activation.rs:9-11`, `slider.rs:38-58`).
- `InputFocus` as the single focus, `TabNavigationPlugin`, modal
  `TabGroup`, `InputFocusVisible` outline (`activation.rs:47-72`).
- Picking: `require_markers`, `MeshPickingCamera`, entity observers,
  `Pickable::IGNORE` discipline, `MeshRayCast` with triangle→face
  mapping (`app/mod.rs:395-396`, `cad/pick.rs:183-215`).
- Cameras: native `Viewport`/`SubCameraView`, `Camera2d` UI camera
  ordering, `Projection` swapping (`camera/viewport.rs:30-87`).
- Assets: handle-swap over asset mutation in CAD; colour/derived
  material caches; off-thread mesh building; persistent chart image
  handles (`cad/mesh.rs:381-393`, `cad/display/draw.rs:123-160`,
  `builder/graphs.rs:169-171`).
- Retained `GizmoAsset` for static line sets + grouped gizmo configs
  (`cad/display/draw.rs:283-471`).
- `DespawnOnExit<ModeScope>` state scoping; `Changed`-filtered
  repaint/refresh systems; in-place `&mut Text` updates
  (`app/mod.rs:307-310`, `widgets.rs:333-382`,
  `phenomena/panel.rs:428-460`).
- Accessibility labels maintained through Bevy's own labelling pass,
  node-less field `AccessibilityNode`s (`widgets.rs:396-406`,
  `text/mod.rs:315-323`).
- Feature hygiene: explicit minimal Bevy feature list with rationale
  comments (`crates/sim-spatial/Cargo.toml:18-28`).

## Evidence & limits

Read in full or relevant part: all of `ui_kit/` (mod, widgets,
theme, activation, scroll, slider, form, text/{mod,input,draft}),
`camera/{mod,viewport}`, `cad/{pick,scene,view,mesh,display/draw,
display/section}`, `app/mod.rs` (CorePlugin), `builder/ui.rs`
(rebuild), `lesson/ui/mod.rs` (rebuild), `phenomena/panel.rs`
(rebuild/refresh), `inspect_view/scene.rs`, `chart.rs`,
`sim-render/src/{lib,canvas}.rs`, `sim-viewer/src/{main,render}.rs`
(heads + manifests), `sim-web/src/lib.rs` (head + manifest),
`docs/architecture/native-viewer.md:5238-5327`. Workspace-wide
pattern counts (picking/gizmos/mesh/material/text/rebuild/shader/
MSAA/RenderLayers) were taken by scripted grep over
`crates/sim-spatial/src` and spot-verified by reading the bodies
above. Bevy 0.19.1 API claims verified at
https://docs.rs/bevy/0.19.1/bevy/ui_widgets/index.html,
https://docs.rs/bevy/0.19.1/bevy/text/struct.EditableText.html,
https://docs.rs/bevy/0.19.1/bevy/ui_widgets/struct.Checkbox.html,
https://docs.rs/bevy/0.19.1/bevy/picking/index.html, and the 0.19.1
crate index (feathers description).

Not inspected line-by-line: the ~40 mode panel files that *call* the
kit (sampled via grep + the rebuild sites above), CAD tool internals
beyond pick/snap/measure headers, sim-viewer's egui panels (out of
Bevy scope), and runtime behaviour (no build/run per brief) — impact
claims about rebuild cost are structural, not profiled.


