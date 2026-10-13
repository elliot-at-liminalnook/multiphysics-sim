# Bevy utilization audit — final (modern Bevy practice vs this codebase)

Date: 2026-10-12. Evidence: `bevy-modern.md` (recommended patterns,
Bevy 0.15→0.20 migration guides + release posts), `repo-inventory.md`,
`audit-ecs.md` (F1–F13), `audit-ui-render.md` (F1–F17), `audit-platform.md`
(F1–F24) in this directory, all read in full; load-bearing sites spot-opened
in source (`crates/sim-spatial/Cargo.toml`, `src/app/mod.rs:145-281`,
`src/ui_kit/activation.rs:1-80`, `src/ui_kit/text/draft.rs`,
`src/cad/document/mod.rs:17-120`). No cargo/tests were run; impact claims
that need profiling are marked structural.

**Scope correction (all five artifacts agree).** Only `crates/sim-spatial`
uses Bevy (`crates/sim-spatial/Cargo.toml:27`, one `App` at
`crates/sim-spatial/src/app/mod.rs:224`, 541 `.rs` files, ~156k LOC).
`crates/sim-render` is a CPU PNG/MP4 renderer (`crates/sim-render/src/lib.rs:1`),
`crates/sim-viewer` is eframe/egui (`crates/sim-viewer/src/main.rs:11`),
`crates/sim-web` is wasm-bindgen physics only (`crates/sim-web/src/lib.rs:1`).
Every finding below is therefore about `sim-spatial` unless stated.

## 1. Verdict

This is a modern, disciplined Bevy 0.19.1 integration — `States`+`ComputedStates`+`DespawnOnExit`, a test-enforced public `SystemSet` pipeline, typed `Message` actions, headless widgets, picking observers and task-pool jobs are already current practice.
The debt is concentrated: god-`Resource` documents force hand counters and string rebuild keys, whole-panel despawn/respawn forces focus-rebinding and the global `warn` handler, and over-chained schedules waste the `multi_threaded` executor.
The largest user-facing ceilings are the append-only text editor (no caret/selection/clipboard/IME) and unadopted `Checkbox`/`ListBox`/`Scrollbar` widgets with their accessibility roles.
Physics on run threads, procedural assets, the custom orbit camera, CAD sub-body picking and versioned-JSON documents are correctly *not* Bevy (§5).
Biggest win: retained panels fed by split resources + `Changed<T>`, which deletes whole classes of workaround at once.
Next: text caret + a11y roles — cheap, felt daily.
Then bump to 0.20, whose breakers (flat pointer events, `Interaction` deprecation) land exactly where this codebase is most invested.

## 2. Version position

| Fact | Value | Evidence |
|---|---|---|
| Pinned | bevy **0.19.1** exactly (`=0.19.1`, `default-features=false`, ~35 hand-picked features) + `bevy-settings = "=0.19.1"` | `Cargo.toml:60`, `Cargo.lock` (`name="bevy" version="0.19.1"`), `crates/sim-spatial/Cargo.toml:10,27` |
| Latest stable | bevy **0.20.0**, 2026-10-08 | `bevy-modern.md:15` (docs.rs + bevy.org/news/bevy-0-20 + lib.rs triangulated) |
| Distance | **exactly one minor behind** | `bevy-modern.md:18` |

What the pin already gives (0.19, per `bevy-modern.md` §3–§9) and this
codebase only partly uses: Resources-as-Components (`bevy-modern.md:73`),
headless widgets stable and in `DefaultPlugins` (`bevy-modern.md:183`),
BSN scenes (`bsn!`/`SceneComponent`, no `.bsn` loader yet,
`bevy-modern.md:189`), render-graph-as-systems (`bevy-modern.md:213` —
unused here; no custom render graph was audited, see §6), delayed commands
(`bevy-modern.md:90`), observer run conditions (`bevy-modern.md:113`),
`EditableText`/`TextInput` (`bevy-modern.md:182`), serialisable handles
(`bevy-modern.md:239`). None of these needs an upgrade to adopt.

What a 0.19.1 → 0.20 upgrade alone would force in *this* codebase
(from `bevy-modern.md` §15, cross-checked against the inventory):

1. **Interaction model (highest risk).** `ui::widgets::Button` +
   `Interaction` deprecated; flat pointer events replace `Pointer<E>`
   nesting; observer `B` generic moves into event types
   (`bevy-modern.md:184,200,305-307`). Local exposure: 23 `.observe()` +
   7 `add_observer` sites, `Changed<Interaction>` repaint at
   `crates/sim-spatial/src/ui_kit/widgets.rs:335`, the whole
   `ActivationSet` ordering against `PickingSystems::Hover`
   (`crates/sim-spatial/src/ui_kit/activation.rs:55-61`, spot-read).
   Note the kit already uses headless `bevy::ui_widgets::Button`
   (`activation.rs:10`), so the deprecated *component* may not bite —
   the `Interaction` polling and `On<Pointer<…>>` signatures will.
2. **Text.** `EditableText`/`TextInput` split, `TextScroll` gone,
   `Font::from_bytes` signature, `TextFont` default becomes `Rem(1.)`
   (`bevy-modern.md:308-310`). Local exposure: fonts via
   `Font::from_bytes(include_bytes!(…))`
   (`crates/sim-spatial/src/ui_kit/theme.rs:88-115`); `EditableText`
   itself is not used (§3, UI-F1), so the split is an opportunity,
   not a break.
3. **Systems.** Exclusive systems unified (two local `&mut World`
   systems, ECS-F9); built-in schedules weakly ordered — the local
   `(ViewerSet…).chain()` at `crates/sim-spatial/src/app/mod.rs:279`
   stays strict unless changed, but `ordering_guard.rs` expectations
   about built-ins need a re-check; `DeferredWorld::query` deprecated
   (local `DeferredWorld` use: zero). `NextState::set_if_neq` →
   `set_if_different` (`bevy-modern.md:141`): no local `NextState`
   use of it was evidenced (only `ResMut::set_if_neq`,
   `crates/sim-spatial/src/drive_input/mod.rs:145`) — verify, don't assume.
4. **Rendering/imports.** `Tonemapping`/`DebandDither` moved to
   `bevy_render::view`, `MeshTag::new`, `bevy_shape`/`bevy_curve`
   splits, retained UI rendering (`bevy-modern.md:319-329`). Local
   exposure: per-camera `Tonemapping::None`
   (`crates/sim-spatial/src/cad/scene.rs:73`); no custom UI extraction
   or shaders were found, so retained-UI/WESL breakage should be nil.
   Sprite→`Mesh2d + SpriteMaterial` (`bevy-modern.md:220`): no
   `Sprite` use locally — not applicable.
5. **Settings & windowing.** `bevy-settings` 0.20 errors on useless
   `SettingsGroup`s — directly relevant to the consumer-less
   `PreferenceGroup` seam (§3, P-F19); bump `bevy-settings` in lockstep
   (`crates/sim-spatial/Cargo.toml:10`). macOS activation follows
   `Window::focused`; render-world window data via ECS
   (`bevy-modern.md:333-334`) — re-check the objc2 occlusion path
   (`crates/sim-spatial/src/rest.rs:36`).

What 0.20 would newly enable here: `chain_weak`/`before_weak` (directly
answers ECS-F7 over-chaining, `bevy-modern.md:133`), schedule
`shuffle_seed` for tests (`bevy-modern.md:134`), the `Ready` event if BSN
is piloted (`bevy-modern.md:119`), error-context messages +
panic-catching for fallible systems (`bevy-modern.md:89,91`),
`Val::Em/Rem` + `FixedNode` for the kit (`bevy-modern.md:175-176`), and
retained UI rendering (free perf once panels stop respawning).

**Recommended target and sequencing.** Target **0.20.0** (or the then-latest
0.20.x patch), but not first. Sequence: (a) on 0.19.1, do the cheap,
upgrade-independent wins — typed keys, `Single`/fallible sweep, a11y
roles, caret, scrollbar, diagnostics (§4 items 1–5, 9); (b) pilot one
retained panel + one resource split on 0.19.1 so the upgrade diff is not
confounded with a redesign; (c) bump with `bevy-modern.md` §15 as the
checklist, compiler-guided through pointer-observer signatures, updating
the feature rationale comment at `crates/sim-spatial/Cargo.toml:18-27`;
(d) only after the bump, adopt `chain_weak` set-by-set with frame-time and
`shuffle_seed` checks. Do not attempt BSN, `AssetServer`, or `FixedUpdate`
migration as part of the bump (§5).

## 3. Findings

Verdicts: **ADOPT** (start using), **REPLACE-custom-with-Bevy** (delete
custom code in favour of Bevy), **KEEP-custom-justified**,
**ALREADY-GOOD**. Source-finding ids (ECS-Fn / UI-Fn / P-Fn) point to the
originating artifact section for the full argument. "Modern way" items are
only those catalogued in `bevy-modern.md` or verified against Bevy 0.19.1
docs in the originating artifact; anything else is marked unverified.

### 3a. ECS, schedules, states

| Area | Current pattern (file:line) | Modern Bevy way (API + since) | Verdict | Impact | Cost |
|---|---|---|---|---|---|
| App/modes (ECS-F8, inv. §2/§4) | One `App` `app/mod.rs:224`; `ViewerMode: States` `app/mod.rs:54`; `ModeScope`/`SpatialScreen: ComputedStates` `app/mod.rs:125,145`; `DespawnOnExit` sweep `app/mod.rs:307` | `States`/`ComputedStates`/`DespawnOnExit`, state-scoped always-on since 0.17 (`bevy-modern.md:141-143`) | ALREADY-GOOD | — | — |
| God-resources (ECS-F1) | `CadDocument` ~50 fields + `revision: u64` `cad/document/mod.rs:18-152,106`; `Builder` `builder.rs:161-260`; `SpatialScene` `inspect_view/mod.rs:32-77`; `RobotView` `robot/mod.rs:69-130` (76 Resources vs 291 Components, inv. §5) | Split per-concern `Resource`s so `Res::is_changed()`/`Changed<T>` are precise (pattern, no version); per-item viewer state as components; Resources-as-Components 0.19 (`bevy-modern.md:73`) | ADOPT (split) | High maint., med. perf (root cause of next 3 rows) | L (M per lifted feature state) |
| Rebuild keys (ECS-F2) | Nine-condition gate + per-part `format!` keys `cad/panel.rs:404-437`; string `render_key` though typed `SourceStamp` exists `cad/activation.rs:7-30`; same `project/panel.rs:134-140`, `robot/threads/panel.rs:131` | `Changed<T>`/`Added<T>` + `Res::is_changed()` on split resources; lifecycle `On<Add>`/`On<Insert>` (observers since 0.14, `bevy-modern.md:110`); interim: typed tuple keys | REPLACE-custom-with-Bevy | High maint. (silent-miss bug class), small perf × every panel/frame | S (typed keys) / M per panel |
| Counters & polling (ECS-F4) | `Selection::changed` `selection/mod.rs:186`, `DocumentRegistry::changed` `document/mod.rs:157`, `CadMeshes::epoch` `cad/mesh.rs:88`, `Builder::seen_selection` `builder.rs:176`; systems wake per frame to compare | `Changed<T>`, `RemovedComponents<T>`, `On<Add/Insert/Remove>` (zero local uses), or `Message` at mutation site (0.17 split, `bevy-modern.md:105,115`). Job/`stat` polling stays idiomatic | REPLACE-custom-with-Bevy (internal edges only; external HTTP push keeps counter) | Med. maint., small-med. perf | M incremental |
| Action bus (ECS-F5) | Generic `Act<A>: Message` `app/actions.rs:121-126`, one consumer per type in `ViewerSet::Actions`, rationale `app/actions.rs:10-24`; caveats: `messages.drain().collect()` `app/actions.rs:308`, `InFlight<A>` re-apply per frame `app/actions.rs:265-298` | `Message`/`MessageWriter`/`MessageReader` since 0.17 (`bevy-modern.md:115-116`) — this *is* the modern way | ALREADY-GOOD (caveats low priority) | — (S to use reader cursor; M for completion messages) | S/M optional |
| Activation hops (ECS-F6 = UI-F7 partial) | `Activate` observer → `Activated` marker `ui_kit/activation.rs:120-126` → poll `With<Activated>` → `Act` message → `clear` `ui_kit/activation.rs:136-138` | Entity-event `On<Activate>` observer writing `Act<A>` via `MessageWriter` directly (0.17, `bevy-modern.md:106`); keep CAD `Validate` gate while rebuilds churn | REPLACE-custom-with-Bevy (phased) | Med. maint., small perf/latency | M, risky near focus paths |
| Ordering (ECS-F7) | Public sets `ViewerSet Input→Actions→JobResults→SimSync→Present` `app/mod.rs:155-171,279`, guard test `ordering_guard.rs:1` — but 15/12/11-system `.chain()`s inside sets `robot/mod.rs:263-282`, `inspect_view/mod.rs:358-377` | Keep sets; explicit `before/after` only for real deps now; `chain_weak`/`before_weak`/`after_weak` in **0.20** (`bevy-modern.md:133`) + `shuffle_seed` tests (`bevy-modern.md:134`) | ALREADY-GOOD (sets) / ADOPT (weak ordering post-bump) | Sets: —. Chains: med. perf on multicore (`multi_threaded` underused) | M (needs frame-time checks) |
| Sub-mode state (ECS-F8) | Tab/tool/section as fields in god-resources: builder `Tab`/`Mode` `builder.rs:28-48`, robot `Section` `robot/mod.rs:132-145`, `CadTool`, lesson `PageMode`; inactive panels run and early-return | `SubStates` (present in bevy_state 0.19.1 per ECS audit docs.rs check; **since-version unverified**) → `OnEnter(Tab::…)`, `DespawnOnExit`, `in_state` gating (`bevy-modern.md:141`) | ADOPT (pilot: robot `Section`) | Med. maint., small perf | M per mode |
| Exclusive systems (ECS-F9) | `serve(&mut World)` `app/actions.rs:459` (type-erased registry — defensible narrow), `switch handle(&mut World)` `app/switch/mod.rs:373` (accumulated); `DeferredWorld` zero uses | Ordinary param systems + derived `SystemParam` (**derive since-version unverified**); exclusive systems unified into plain `add_systems` in **0.20** (`bevy-modern.md:137`) | ADOPT (shrink `serve`; decompose `handle` last) | Med. maint., small perf | M / L |
| Query shapes (ECS-F10) | 418 `Option<Res<…>>` + 514 `Option<&…>` (double-guard next to `in_state`); `roots.iter().next()` singleton picks `cad/panel.rs:403`, `app/picker/mod.rs:547`; 15-param systems `ui_kit/activation.rs:153`, `cad/panel.rs:390` | `Single`/`Option<Single>` for singletons; `Populated<T>` (**version unverified**, ECS audit); default resources in test harnesses instead of taxing signatures | ADOPT | Med. maint. (arbitrary-entity footgun), perf ~nil | S per system, M per mode sweep |
| Errors (ECS-F11) | No fallible systems / `BevyError`; global `set_error_handler(warn)` `app/mod.rs:230` (spot-read; rationale :225-229), tests keep strict default | Fallible `fn sys(..) -> Result` + `Single?`/`get()?` since 0.16, per-world handler 0.17/renamed 0.19 (`bevy-modern.md:87-88`); `try_insert`/`try_despawn` at rebuild sites | ADOPT (sequenced with retained-UI row) | Med. correctness — real errors visible again | M |
| Required components (ECS-F12) | Exactly two `#[require]`: `Ordinary` `ui_kit/activation.rs:10` (spot-read), `ModalFocus` `:24`; else manual tuples — `Kit::button` 10+ components `ui_kit/widgets.rs:108-120`. No `#[derive(Bundle)]` (aligned) | `#[require(…)]` since 0.15, expression syntax 0.16 (`bevy-modern.md:65`) on markers/roots (`CardRoot`, panel roots) | ADOPT (opportunistic, with panel work) | Med. maint. (forgotten-component bugs) | M |
| Relationships (ECS-F13) | Only built-in `ChildOf`/`Children`; `CadMeshes: HashMap<String,Entry>` + epoch `cad/mesh.rs:47-104`; hand `DocumentId(u64)` + linear Vec `document/mod.rs:29-171`; selection as strings/Vec `selection/mod.rs:51-191`; per-frame `HashMap<String,Entity>` `physics_view.rs:516` | `#[relationship]` one-to-many + `ChildOf` since 0.16 (`bevy-modern.md:67-68`); `Entity` as the never-reused id. String map stays *at the file/service boundary only* | ADOPT (pilot `CadMeshes` only) | High maint. in CAD/inspect sync, small-med. perf | L |

Dropped / not carried as findings: ECS-F3 is the same defect as UI-F6 and
appears once in §3b; `DeferredWorld`/hooks absence is not a finding (ECS-F9
says so explicitly); BSN-as-fix for rebuilds is not recommended on current
evidence (macro-only in 0.19/0.20, `bevy-modern.md:259` — pilot at most,
§5).

### 3b. UI, text, picking

All paths in §3 tables are under `crates/sim-spatial/src/` unless a fuller
path is given.

| Area | Current pattern (file:line) | Modern Bevy way (API + since) | Verdict | Impact | Cost |
|---|---|---|---|---|---|
| Kit architecture (UI §1) | Skin over Bevy: layout 100% `Node` `ui_kit/widgets.rs:108-130`; headless `ui_widgets::Button`+`ActivateOnPress` via `Ordinary` `ui_kit/activation.rs:9-11`; real `Slider`+`slider_self_update` `ui_kit/slider.rs:38-58`; `InputFocus`/`TabNavigationPlugin` `ui_kit/activation.rs:51-54` | Headless `bevy_ui_widgets` (exp. 0.17, stable + in `DefaultPlugins` 0.19, `bevy-modern.md:183`); picking observers not `Interaction` polling (`bevy-modern.md:184`) | ALREADY-GOOD | — | — |
| Panel rebuilds (UI-F6 = ECS-F3) | Builder despawns every root + respawns on `panel_dirty` (100+ set sites) `builder/ui.rs:116-148`; lesson same `lesson/ui/mod.rs:63-112`; scroll rescue, deferred-while-pressed, focus rebind `ui_kit/activation.rs:168-256`. Good exemplar exists: phenomena targeted refresh `phenomena/panel.rs:315-460` | Retained updates: persistent roots, mutate `Text`/`Node`/`Look`, `Changed<T>` repaint (already proven `ui_kit/widgets.rs:335-400`); `Disabled`/default-filter toggling since 0.16 (`bevy-modern.md:74`) where hide/show suffices | REPLACE-custom-with-Bevy (incremental; §4.6) | **High** perf+complexity; root cause of activation rebind, `warn` handler, `EditableText` blocker | L (S–M per panel) |
| Text editing (UI-F1) | `TextDraft{text,select_all}` append/backspace-pop only `ui_kit/text/draft.rs:17-63` (spot-read); no caret/selection/clipboard/IME/mouse caret; caret is literal `\|` `ui_kit/widgets.rs:248-254`. Rejection of Bevy editor reasoned `docs/architecture/native-viewer.md:5251-5311` | Interim (custom, contained): caret+selection+motion keys in `TextDraft`. Bevy: `EditableText` 0.19 / split `EditableText`+`TextInput` 0.20 (`bevy-modern.md:182`) — re-evaluate only after retained panels + placeholder/AccessKit gaps close | KEEP-custom-justified (for now) + ADOPT caret step | **High** UX/a11y ceiling; M-step is most-touched papercut | M (caret) / L (full `EditableText`) |
| Checkbox/radio (UI-F2) | Toggle chips; state parsed from draft text `"true"/"false"` `ui_kit/form.rs:101-105,248-279`; AT sees buttons | Headless `Checkbox`+`Checked`+`ValueChange<bool>`, `RadioButton`/`RadioGroup` in 0.19.1 `bevy_ui_widgets` (verified in UI audit; crate since 0.17/stable 0.19, `bevy-modern.md:183`); `checkbox_self_update` mirroring existing slider precedent | REPLACE-custom-with-Bevy (`Kit::checkbox`/`radio_group`, keep chip paint) | Med. (a11y semantics, kills string round-trip) | M |
| Lists (UI-F4) | Every row a focusable `Button` `ui_kit/widgets.rs:170-188`; Tab walks every row; arrow nav hand-wired (`FieldEvent::Arrow` `ui_kit/text/mod.rs:143-145`) | `ListBox`/`ListItem` + `ActiveDescendant` in 0.19.1 `bevy_ui_widgets` (verified in UI audit; `bevy-modern.md:183`) | REPLACE-custom-with-Bevy (pilot: palette) | Med. keyboard UX + a11y roles | M |
| Menus/popovers (UI-F5) | Palette `ui_kit/palette.rs`, pie `ui_kit/pie.rs`, CAD popups: hand-placed absolute trees, per-surface dismissal/flipping | `ui_widgets::popover` + headless `MenuButton`/menus in 0.19.1 (verified in UI audit; `bevy-modern.md:183`) | ADOPT opportunistically per surface, no sweep | Low-med. | M per family |
| Scrolling (UI-F3) | Native `Overflow::scroll_y`+`ScrollPosition` `ui_kit/scroll.rs:15-17` but per-mode wheel routing `builder/ui.rs:356`, `cad/panel.rs:353`, `lesson/ui/mod.rs:377`…; 0.16-style `clamp_scroll_positions` shim `ui_kit/scroll.rs:36-45`; **no visible scrollbars** | `ScrollPosition` logical-px, layout no longer clobbers (0.17, `bevy-modern.md:177`); native wheel-via-picking (**nested-scroller equivalence unverified** — audit step, §6); headless `Scrollbar` in 0.19.1 widgets | REPLACE-custom-with-Bevy (wheel+shim, after per-mode check) + ADOPT `Scrollbar` | Med. (affordance, deleted geometry code) | M (S–M scrollbar) |
| Activation layer (UI-F7) | 309-line `ui_kit/activation.rs`: eligibility ancestor-walk every `PreUpdate` `:75-118`, modal stack, identity rebind — exists for real Bevy gaps (tab filter, secondary-button, rebuild survival) | No single Bevy replacement; shrink *consequences*: retained panels delete rebind third; eligibility becomes `Changed<Node/Visibility/Enabled>`-driven. Upstream churn risk: widgets API "likely to change substantially" (UI audit) | KEEP-custom-justified (shrink via panel row, not before) | Med. maintenance | M (as part of panel row) |
| Display text (UI §2) | `Kit::text` `(Text,TextFont,TextColor,TextLayout)` `ui_kit/widgets.rs:70-77`; real `TextSpan` (`markdown.rs:131`); in-place `Text.0` mutation (66 sites); no `Text2d` (UI-projected labels `builder/markers.rs:110`) | `Text`/`TextFont`/`TextSection` model 0.19 + Parley (`bevy-modern.md:179-181`); direct `&mut Text` is idiomatic — no `TextUiWriter` needed | ALREADY-GOOD | — (watch 0.20 `TextFont` default/`Font::from_bytes`, §2) | — |
| Accessibility (UI-F13) | `AccessibleLabel` everywhere, maintained through Bevy pass `ui_kit/widgets.rs:396-406`; node-less field `AccessibilityNode` `ui_kit/text/mod.rs:315-323` — but **no roles** ("`accesskit` crate is not a dependency" `ui_kit/text/mod.rs:315-317`); chart images no text alt | `AccessibleLabel` since 0.19 (`bevy-modern.md:285`) + AccessKit roles via headless-widget semantics (needs direct `accesskit` dep, already transitive) | ADOPT (roles in kit constructors; `TextInput` first; chart `AccessibleLabel`) | Med.; highest value-per-cost a11y item | S (kit) / M (mode sweep) |
| Picking (UI §3) | `MeshPickingPlugin`+`require_markers:true` `app/mod.rs:395-396`; `MeshPickingCamera`, `Pickable::IGNORE` discipline, `On<Pointer<Click/Over/Out>>` observers `inspect_view/scene.rs:135-219`; `MeshRayCast` triangle→face `cad/pick.rs:74-75,183-215` | `bevy_picking` first-party since 0.15, pointer observers (`bevy-modern.md:199-200`); flat events in 0.20 will re-shape signatures | ALREADY-GOOD | — (S cleanups: shared `HoverMap` `SystemParam` for dup at `cad/scene.rs:162`/`camera/input.rs:30`; dup `segment_distance`) | — / S |
| CAD pick pipeline (UI-F8) | Custom: slop/box logic, 9-ray 7×7px ring `cad/pick.rs:217-235`, screen-space edge/vertex search `cad/pick.rs:19-63,246-260`, 33 ms coalesced off-thread search `:45-52` | No Bevy API picks sampled B-rep edges/candidate lists; a custom picking *backend* would fit worse (UI audit §3) | KEEP-custom-justified | Low | — |
| Projection math (UI-F9) | `CadView` reimplements `world_to_viewport`/`viewport_to_world` in model frame `cad/view.rs:48-132` — copyable snapshot for jobs, no one-frame `GlobalTransform` lag; other modes use native calls `builder/placement.rs:484`, `camera/input.rs:218` | `Camera::world_to_viewport`/`viewport_to_world` (long-standing) — cannot meet same-frame/job constraints | KEEP-custom-justified (cosmetic S trim at most) | Low | S optional |
| Feathers (UI-F15) | Rejected with reason `crates/sim-spatial/Cargo.toml:25-27` + `docs/architecture/native-viewer.md:461`; kit duplicates only its token layer (`Look`/`Tint` repaint `ui_kit/widgets.rs:333-382`) | Feathers non-exp. 0.19, theming 0.20 (`bevy-modern.md:185`) — worth it only if a runtime UI theme is ever required (tokens are compile-time today) | KEEP-custom-justified | Low | — (M if theme resource ever needed) |

### 3c. Rendering, cameras, meshes/materials, assets

| Area | Current pattern (file:line) | Modern Bevy way (API + since) | Verdict | Impact | Cost |
|---|---|---|---|---|---|
| Cameras (UI §5) | Custom `Orbit`/`OrbitRules`/`ViewArea` shared by all modes `camera/mod.rs:78-231,474-491`; native `Camera::viewport`+`SubCameraView` write-on-change `camera/viewport.rs:30-87`; `Camera2d` UI camera order 3 `cad/scene.rs:70-92`; `Projection` swap `camera/orbit.rs:338-356` | Cameras-as-components (`bevy-modern.md:215`) — already used. First-party controllers catalogued 0.18/pan-orbit 0.20 (`bevy-modern.md:216`) vs UI audit: no in-tree orbit in 0.19.1 — **discrepancy noted, immaterial**: RoboCAD turntable/zoom-to-cursor/REST actions exceed any stock controller | KEEP-custom-justified (controller) / ALREADY-GOOD (viewport plumbing) | — | — |
| Layers / AA (UI-F14, §5 nit) | `RenderLayers` 0 uses — 3D gizmos also draw on UI `Camera2d` (known wart `cad/overlay.rs:33`); `Msaa` 0 uses; `Tonemapping::None` everywhere `cad/scene.rs:73-75` (LUT features off) | `RenderLayers` masks (long-standing, `bevy-modern.md:231`); `Msaa::Sample4` per 3D camera. Tonemapping: leave (LUT features + colour re-tune) | ADOPT (`RenderLayers` for gizmos, MSAA) | Low-med. visual (CAD edges), fixes double-draw | S each |
| Mesh apply (UI §6) | CAD: off-thread tessellation `cad/mesh.rs:274-320,413-417`, handle swap `Mesh3d(handle)` `cad/mesh.rs:381-393`, despawn only on hide/delete/doc-change `:336-367` | `Mesh3d`/`MeshMaterial3d` handle components (`bevy-modern.md:219,221`) | ALREADY-GOOD (exemplary) | — | — |
| Materials, inspect (UI-F10) | `materials.add` per piece/part `inspect_view/scene.rs:126-161`, then per-part `materials.get_mut` recolour `inspect_view/scene.rs:243-281` (same `linked.rs:143`, `phenomena/scene.rs:157`); safe only because handles are unshared. CAD does it right: colour + `(AssetId,MaterialLook)` caches `cad/mesh.rs:234-252`, `cad/display/draw.rs:101-160` | Cache by `(colour,metallic)` + swap `MeshMaterial3d` handles for state, mirroring in-house `CadMaterials`; lazy per-part material on first tint | REPLACE-custom-with-Bevy-pattern (in-house template exists) | Med. correctness/perf risk | S–M |
| Custom materials (UI-F12) | Zero `Material`/`AsBindGroup`/WGSL; section view CPU-clips tessellation copies `cad/display/section.rs:9-22,372-465` (preserves triangle↔face picking); matcap approximated by tint `cad/display/draw.rs:11-17` | `ExtendedMaterial<StandardMaterial>` fragment-discard clip plane / custom `Material` matcap (`bevy-modern.md:219`) — but clip shader breaks raycast face invariant without a shader-aware pick filter | KEEP-custom-justified (revisit only on section-latency complaints / matcap parity audit) | Med. if triggered, else virtue (upgrade surface) | M each if triggered |
| Gizmos (UI §6) | Immediate `Gizmos` (129 sites) + grouped `GizmoConfig`s `cad/overlay.rs:63-79` + **retained `GizmoAsset`** for grid/axes/edges `cad/display/draw.rs:283-471`; nit: per-frame immediate grid `inspect_view/scene.rs:285-319` | Immediate + retained gizmos, retained since 0.16 (`bevy-modern.md:228`) | ALREADY-GOOD (nit S if profiled) | — | — |
| Charts/icons (UI-F11/F17) | Charts CPU-rasterized 720×200 into persistent `Handle<Image>`, `data` replaced per update `chart.rs:10-116`, `builder/graphs.rs:157-204` (576 KB re-upload/update); some producers allocate fresh `Image` per result `lesson/watch.rs:233-297`, `cad/experiment_review/plots.rs:64-80`. Icons: per-name 48×48 `Image`s `ui_kit/theme.rs:89-104` | No in-tree charting/vector API in `bevy-modern.md`; GPU line pipeline unjustified at current counts. Normalize producers to handle-reuse pattern | KEEP-custom-justified + ADOPT handle-reuse normalization; icons: no action | Med. (charts), low (icons) | S (normalize) / L GPU (not recommended) |
| Asset loading (P-F17/F18) | `AssetServer` **0 uses**; `include_bytes!` fonts/icons `ui_kit/theme.rs:88-113`; manual `std::fs` + OBJ parse `models.rs:46-141`; images via `Image::from_buffer` `cad/references/planes.rs:98`; no hot reload — docs poll stamps 0.4 s `builder/background.rs:42-66` | `AssetServer::load` + builder (0.19, `bevy-modern.md:237`), hot reload `watch_for_changes_override` (`bevy-modern.md:246`) — requires custom `AssetSource`s for arbitrary user paths, async font timing vs first `OnEnter`, for mostly *generated* assets | KEEP-custom-justified (S path-keyed decode cache only if profiling shows dup decodes) | Low-med. (dev ergonomics, dup decode) | L to adopt (not recommended as-is) |
| Offline render (UI-F16) | `sim-render` deterministic CPU PNG/MP4 `crates/sim-render/src/lib.rs:1`, own canvas `crates/sim-render/src/canvas.rs:17-99`; live-window `Screenshot::primary_window()`+observe already used `app/switch/mod.rs:478` | Bevy `Screenshot` (0.18, `bevy-modern.md:230`) — for live window only; wrong tool for byte-deterministic headless artifacts | KEEP-custom-justified (stacks are complementary; optional S–M: unify chart rasterizer behind `sim-render`) | Info | — |

### 3d. Input, time, tasks, platform, diagnostics

| Area | Current pattern (file:line) | Modern Bevy way (API + since) | Verdict | Impact | Cost |
|---|---|---|---|---|---|
| Windowing core (P-F1) | `DefaultPlugins.set(WindowPlugin{close_when_requested:false})` `app/mod.rs:387-391`; per-mode `Look` on `OnEnter` `app/mod.rs:317-430`; reactive `WinitSettings` 60 Hz/40 ms `app/mod.rs:331-333`, `Continuous` for Place/Phenomena `:349,364`; `RequestRedraw` wakes `app/actions.rs:508` | `WinitSettings`/`UpdateMode::reactive` (documented 0.19; `bevy-modern.md:282-284` context) | ALREADY-GOOD | — | — |
| Present mode (P-F2) | `present_mode` never set (`..default()` = AutoVsync) `app/mod.rs:388`; one-frame click lag worked around narratively `app/mod.rs:328-330` | Per-mode `Window.present_mode = PresentMode::Mailbox/AutoNoVsync` (0.19.1 API, platform audit) in `Look`/`apply_look` | ADOPT | Low-med. latency/power on high-refresh | S |
| Cursor (P-F3) | `CursorIcon`/`CursorOptions`/`CursorGrabMode` zero uses; orbit/fly drags never set grab cursor or confine pointer `camera/input.rs:124-175`, `camera/fly.rs:72-78` | `CursorIcon` on window entity during drag; `CursorOptions{grab_mode,visible}` for fly — `CursorOptions` split from `Window` 0.17 (`bevy-modern.md:204,279-280`) | ADOPT | Med. for Place fly, cosmetic elsewhere | S |
| macOS occlusion (P-F4) | Raw `objc2` `NSWindow.occlusionState` `rest.rs:36-70` (winit sends no event for hidden-created window; `WindowOccluded` used elsewhere) | No Bevy occlusion query API exists in catalogue | KEEP-custom-justified (drop when winit emits it) | — | — |
| Input stack (P-F5/F10) | `ButtonInput<KeyCode/Key/MouseButton>` `camera/input.rs:124-128`; `Messages<MouseMotion>`+`MessageCursor` `camera/input.rs:45`; sole `KeyboardInput` reader via `InputFocus` `ui_kit/text/mod.rs:332,370`; `FileDragAndDrop::DroppedFile` `cad/references/drop.rs:7-60`; custom picker (no Bevy dialog API; REST parity) `app/picker/mod.rs:1-20` | `ButtonInput` + message-based raw input since 0.17 (`bevy-modern.md:195-196`); focused-input dispatch | ALREADY-GOOD (picker: KEEP-custom-justified) | — | — |
| Gamepad/keymap (P-F6/F7) | `bevy_gilrs` ON (`crates/sim-spatial/Cargo.toml:27`) despite "no gamepad" comment `:18-21` (spot-read); `&Gamepad` query (modern) `drive_input/input.rs:195`; hand deadzone shared with browser via `sim.drive-bindings/1` JSON `crates/sim-runtime/src/drive_bindings.rs:35` | Gamepad entity model, NonSend replaced 0.17 (`bevy-modern.md:198`); Bevy has no keymap crate. Native `AxisSettings` deadzone would fork web/native semantics | KEEP-custom-justified + fix stale comment | Doc drift now, low runtime | S (comment) |
| IME (P-F8, UI-F1) | Zero `Ime` refs; text only from `KeyboardInput.text` `ui_kit/text/input.rs:47-141` — CJK/dead-key composition lost | `Window.ime_enabled` while field focused + `Ime::Preedit/Commit` (platform audit, docs.rs 0.19.1; **not covered by any guide section read** — `bevy-modern.md:285`, §6) | ADOPT (with/after caret step) | Med. (international text entry broken) | M (preedit rendering is the bulk) |
| Gestures (P-F9) | No `TouchInput`/`PinchGesture`/`PanGesture`; wheel zoom only (Line+Pixel handled) `camera/input.rs:45` | `PinchGesture`/`PanGesture` messages in `camera/input.rs` (**exact 0.19 module path unverified**, §6) | ADOPT | Low-med. laptop UX | S |
| Time (P-F11/F13) | `Res<Time>` presentation-only (15+ systems); scattered `delta_secs().min(0.1)` `physics_view.rs:392`; one `Time<Real>` `robot/video.rs:143`; **zero `FixedUpdate`/`Timer`**; settings retry on raw `Instant` `app/settings/mod.rs:174` (tests mutate it `app/settings/tests.rs:131`) | `Time<Virtual>::set_max_delta` once; `Timer`/`Time<Real>` for UI retry (time model `bevy-modern.md:140`; `Timer::is_finished` 0.17 `bevy-modern.md:293`). Hardware `Instant` deadlines: keep (pausable clock would be wrong) | ADOPT (max-delta + retry Timer) / KEEP (hardware) | Low (consolidation, testability) | S |
| Sim pacing (P-F12) | Hand accumulator, per-exhibit grids, on run threads `phenomena/run.rs:132-228,389-416`; robot/builder runs in `RunThread` workers | `FixedUpdate`+`Time<Fixed>` (`bevy-modern.md:140`) — **cannot serve**: shared non-Bevy code, per-exhibit different grids, solver chunks must not block UI thread | KEEP-custom-justified (do not migrate) | — | L (not recommended) |
| Jobs/tasks (P-F14–F16) | `Job` thin layer over `IoTaskPool`/`AsyncComputeTaskPool` `jobs/mod.rs:46,156-222` + cancel/progress/streaming/panic-capture; `Pool::Dedicated`/`RunThread` mpsc+snapshot `jobs/run_thread.rs:37-126`; results polled in `ViewerSet::JobResults` `app/mod.rs:279`; no blocking in systems | Task pools + `Task<T>` poll-back (`bevy-modern.md:268`) — raw use would *lose* progress/cancel/streaming; pools are for finite tasks, not permanent threads | ALREADY-GOOD / KEEP-custom-justified (nit: `Vec::remove(0)` stream `jobs/mod.rs:246`, S if ever deep) | — | — |
| Reflect/settings (P-F19) | Sole `register_type::<PreferenceGroup>` `app/settings/plugin.rs:14`; 4 JSON-*string* fields `app/settings/mod.rs:28-37`; nothing reads it reflectively; real persistence hand JSON `app/settings/jobs.rs`; other Reflect = gizmo groups only | Either typed `#[derive(Reflect)]` fields + `bevy-settings` persistence (App Settings 0.19, `bevy-modern.md:144`; 0.20 useless-group error looms) **or** drop seam for plain serde | ADOPT (resolve seam one way or the other) | Low (double serialization, dead contract) | M either way |
| Documents (P-F20) | Versioned serde schemas (`sim.system/2`, `.simrobot.json`, `.rcad`) owned outside Bevy; ECS is a rebuilt view; `ViewerMode` serde for REST `app/mod.rs:54` | `DynamicScene`/`bevy_world_serialization` 0.19 (`bevy-modern.md:256-258`) would couple file formats to ECS layout + Bevy version — wrong boundary (browser/REST/headless sharing) | KEEP-custom-justified | — | L (not recommended) |
| Diagnostics (P-F21) | No `FrameTimeDiagnosticsPlugin`/`DiagnosticsStore`; hand worst-frame stats `builder/background.rs:67-79` + `eprintln!` stats `phenomena/run.rs:144-215` | `FrameTimeDiagnosticsPlugin` + `DiagnosticsStore` (+ optional `LogDiagnosticsPlugin`/overlay 0.19, `bevy-modern.md:271`) feeding existing REST fields | REPLACE-custom-with-Bevy | Low-med. (percentiles, all modes) | S |
| Logging (P-F22) | `LogPlugin` never `.set()` (only `WindowPlugin` is, `app/mod.rs:387`); in-app `bevy::log::warn!` correct; CLI/headless `println!`×20+ `main.rs:187-585` runs pre-`App` | `.set(LogPlugin{filter:"wgpu=error,sim_spatial=info"})` (`bevy-modern.md:272`); leave pre-App prints (CLI stdout contracts — `--validate-only` scraping **unverified**, §6) | ADOPT (filter only) | Low (supportability) | S |
| Exit/headless (P-F23/F24) | Guarded close: `WindowCloseRequested`→`Act<CloseAction>`, STOP-before-refusal, `ClosingWindow`/`ExitSystems` two-frame arm `app/close.rs:71-153`; `AppExit` consumed `robot/hardware/actions.rs:478-510`; `--headless` bypasses `App` `main.rs:580-587` | This *is* the modern close pattern (exit systems in `Last` 0.19, `bevy-modern.md:283`); `ScheduleRunnerPlugin` unification not worth plugin surgery | ALREADY-GOOD / KEEP (headless fork) | — | — (M if ever unified) |

## 4. Top 10 prioritized actions

Ordered by leverage-per-cost, respecting dependencies (resource splits
before change detection, retained panels before error-handler tightening,
0.20 bump after the cheap 0.19 wins).

1. **Typed rebuild keys + AccessKit roles (S, do first).** First step:
   replace `cad/activation.rs:28-30` string `render_key` with the typed
   `SourceStamp` tuple already defined above it; sweep `project/panel.rs:134`,
   `robot/threads/panel.rs:131`. Same PR-sized change: add direct
   `accesskit` dep, set roles in kit constructors (`TextInput` role for
   fields at `ui_kit/text/mod.rs:315` first), `AccessibleLabel` summary on
   `chart_image`. APIs: `Changed<T>` (existing), `AccessibleLabel`,
   `AccessibilityNode`.
2. **Diagnostics/logging/platform S-bundle (S).** First step:
   `app.add_plugins(FrameTimeDiagnosticsPlugin::default())` in
   `app/mod.rs` CorePlugin, feed REST `system_state` from
   `DiagnosticsStore`, delete `builder/background.rs:67-79` hand stats;
   `.set(LogPlugin{filter:"wgpu=error,sim_spatial=info"})`; add
   `present_mode: Mailbox` to Place/Phenomena `Look`; `CursorIcon`
   Grab/Grabbing in `camera/input.rs` drag, `CursorOptions` grab in fly;
   `Time<Virtual>::set_max_delta`; fix stale "no gamepad" comment
   `crates/sim-spatial/Cargo.toml:18-21`. APIs: `FrameTimeDiagnosticsPlugin`,
   `DiagnosticsStore`, `LogPlugin`, `PresentMode`, `CursorIcon`,
   `CursorOptions`, `Time<Virtual>`.
3. **Text caret + selection (M).** First step: add `caret: usize` +
   selection range to `TextDraft` (`ui_kit/text/draft.rs:17-63`, pure
   struct with pure tests) with Left/Right/Home/End/Shift-select/Ctrl-word
   in `TextDraft::key`; draw caret as separate node (removes `|` from
   string/`AccessibleLabel` value). Clipboard decision (`system_clipboard`
   feature) recorded as its own S–M follow-up. APIs: `Key`/`KeyboardInput`
   (existing), `InputFocus` (existing); preserves `typing` condition and
   `release_held` hardware-jog safety (`ui_kit/text/input.rs:22-32`).
4. **`Single` + fallible-systems sweep, one mode (S–M).** First step:
   convert `roots.iter().next()` at `cad/panel.rs:403` and
   `app/picker/mod.rs:547` to `Single`/`Option<Single>`, make those
   systems `-> Result<(), BevyError>` with `single()?`/`get()?`;
   `try_insert`/`try_despawn` at that mode's rebuild sites so benign
   dead-entity commands stop reaching the `warn` handler
   (`app/mod.rs:230`). APIs: `Single`, `bevy::ecs::error::Result`,
   `set_error_handler` (tighten per-mode, not globally yet).
5. **Kit widgets: Checkbox, Scrollbar, ListBox pilot (M each).** First
   step: `Kit::checkbox` wrapping `bevy::ui_widgets::Checkbox` +
   `Checked` + `checkbox_self_update` observer (mirror `Slider` at
   `ui_kit/slider.rs:38-58`), convert `FieldKind::Check`
   (`ui_kit/form.rs:274-279`); then kit `Scrollbar` inside `scroll_area`
   (`ui_kit/scroll.rs:15`); `ListBox`+`ActiveDescendant` pilot on command
   palette only. APIs: `Checkbox`, `Checked`, `ValueChange<bool>`,
   `RadioButton`/`RadioGroup`, `Scrollbar`, `ListBox`, `ListItem`,
   `ActiveDescendant`, `ScrollPosition`.
6. **Retained-panel pilot + first resource split (M pilot, unlocks L).**
   First step: lift one nested state out of `CadDocument`
   (e.g. `print` at `cad/document/mod.rs:83-85`, already a separate
   struct) into its own `Resource`, gate its panel on
   `Res::is_changed()` instead of `revision`; convert builder
   inspector/status-bar to phenomena-style in-place refresh
   (`phenomena/panel.rs:367-460` `Mark` pattern) leaving schematic as
   child-replacement. Measure frame time before/after (item 2 gives the
   tool). APIs: `Resource`, `Changed<T>`, `Added<T>`, `On<Add>`,
   `bypass_change_detection` (to delete).
7. **Un-chain one Present stage with measurement (M).** First step: robot
   Present 11-system chain (`robot/mod.rs:263-282`): keep only
   comment-named `before/after` edges, drop `.chain()`, compare
   `FrameTimeDiagnosticsPlugin` percentiles; on 0.20 repeat with
   `chain_weak` + `shuffle_seed` test seed. APIs: `SystemSet`,
   `configure_sets`, `before`/`after`, **0.20:** `chain_weak`,
   `ScheduleBuildSettings.shuffle_seed`.
8. **Bump bevy 0.19.1 → 0.20.0 (M, compiler-guided).** First step: bump
   `Cargo.toml:60`, `crates/sim-spatial/Cargo.toml:10,27`
   (`bevy` + `bevy-settings` lockstep), work `bevy-modern.md` §15
   top-to-bottom: flat pointer events / `On<…>` signatures,
   `Interaction` deprecation in repaint (`ui_kit/widgets.rs:335`),
   exclusive-system unification (`app/actions.rs:459`,
   `app/switch/mod.rs:373`), `Tonemapping`/`bevy_shape` imports,
   `Font::from_bytes`/`TextFont` default. Precondition: resolve
   `PreferenceGroup` seam (item 9-lite: at minimum confirm it survives
   0.20 useless-group error). APIs: per §2 list.
9. **Resolve the `bevy-settings`/Reflect seam (M).** First step: decide
   and record — typed `#[derive(Reflect)]` fields + `bevy-settings`
   own load/save (delete JSON-strings at `app/settings/mod.rs:28-37` +
   parallel hand JSON), *or* drop `register_type`/registry assert
   (`app/settings/plugin.rs:14-26`) for plain serde. APIs: `Reflect`,
   `SettingsGroup`, `AppTypeRegistry`.
10. **SubStates pilot + inspect material cache (M / S–M, parallelizable).**
    First steps: robot `Section` (`robot/mod.rs:132-145`) as first
    `SubStates` with `OnEnter(Section::…)` spawns + `DespawnOnExit`;
    separately, `CadMaterials`-style cache in `inspect_view/scene.rs:126-281`
    swapping `MeshMaterial3d` handles instead of per-part `materials.add`
    + `get_mut`. APIs: `SubStates` (verify signature in bevy_state
    0.19.1/0.20 docs at implementation time), `OnEnter`, `Assets<StandardMaterial>`,
    `MeshMaterial3d`, `AssetId`.

Explicitly *not* in the top 10 (real but lower leverage or trigger-gated):
IME (M, after item 3), `ListBox` beyond pilot, popover/Menu per surface,
activation one-hop `On<Activate>` (after item 6), `#[require]` sweep
(opportunistic with item 6), `CadMeshes` relationship pilot (L, after
items 6–8 prove the component direction), MSAA/`RenderLayers` (S,
opportunistic), chart handle normalization (S), pinch/pan gestures (S).

## 5. Explicit non-recommendations — keep custom

- **Physics/exhibit pacing on `RunThread` + hand accumulators**
  (`phenomena/run.rs:132-228`, `jobs/run_thread.rs:37-126`). Not
  `FixedUpdate`/`Time<Fixed>`: exhibits are shared non-Bevy code with
  *different* grids each, solver chunks run seconds, and Bevy has one
  fixed rate. Migrating would block the UI thread for no gain.
- **`jobs::Job` over Bevy task pools** (`jobs/mod.rs:46-319`). It is
  already `IoTaskPool`/`AsyncComputeTaskPool` underneath; raw `Task`
  poll-back would delete cancel tokens, progress, streaming, panic
  naming. `Pool::Dedicated` OS threads likewise: pools are for finite
  tasks.
- **Versioned serde-JSON documents** (`sim.system/2`, `.rcad`, …), ECS as
  rebuilt view. Not `DynamicScene`/BSN serialization: files are shared
  with browser/REST/headless tools and must not couple to ECS layout or
  Bevy version. BSN as *authoring* syntax: pilot at most on a new static
  panel post-0.20; no evidence it fits data-driven rebuilding panels.
- **Procedural assets, no `AssetServer`** (`ui_kit/theme.rs:88-113`,
  `models.rs:46-141`). Assets are generated or live at arbitrary user
  paths; fonts are needed before first `OnEnter`. `AssetServer` + hot
  reload is L for dev-ergonomics only.
- **Custom orbit/fly camera** (`camera/mod.rs`, `camera/fly.rs`) and
  **`CadView` projection snapshot** (`cad/view.rs:48-132`). RoboCAD
  parity, named views, Z-up model frame, REST actions, same-frame
  correctness — no stock controller or `Camera::viewport_to_world`
  call meets the contract (other modes already use the native calls
  where they fit).
- **CAD sub-body picking pipeline** (`cad/pick.rs` whole file) on top of
  `MeshRayCast`. Screen-space B-rep edge/vertex search, candidate
  enumeration, section filtering have no Bevy equivalent; a custom
  picking backend fits the candidate-list design worse.
- **`ui_kit` token/paint layer; `bevy_feathers` rejection**
  (`crates/sim-spatial/Cargo.toml:25-27`). Headless widgets ship no
  styling by design; Feathers' look was evaluated and unwanted.
  Revisit only for runtime theming.
- **Custom text editor — for now** (`ui_kit/text/`). The `EditableText`
  rejection is reasoned and recorded
  (`docs/architecture/native-viewer.md:5251-5311`): no placeholder/
  submit/AccessKit in 0.19.1, persistent-node requirement vs panel
  rebuilds, windowless-test cost, clipboard feature off. Grow the
  custom draft (item 3); re-evaluate on the stated triggers only.
- **`sim-render` CPU renderer and `sim-viewer` egui app.** Deterministic
  headless PNG/MP4 and the schematic preview are product surfaces, not
  failed Bevy adoption. Bevy `Screenshot` already covers the live window.
- **CPU chart rasterizer, CPU section-clipping, tint-matcap, per-name
  icon textures, custom in-window file picker, JSON drive keymap,
  macOS objc2 occlusion, hardware `Instant` deadlines, `--headless`
  no-`App` loop.** Each is justified in §3 by a missing Bevy API, a
  cross-surface contract (browser/REST), a determinism/safety
  requirement, or an invariant (triangle↔face picking) the Bevy
  alternative would break. Revisit triggers are named in their rows.
- **The `Act<A>` message bus and public-`SystemSet` ordering guard.**
  Listed here to forestall "modernization": both already *are* modern
  Bevy practice (0.17 `Message` split; sets-as-contract), enforced
  better than typical. Do more of them, don't replace them.

## 6. Gaps & unverified

Carried unchanged from the five artifacts (wording preserved):

- bevy-modern-guide: 0.16->0.17 and 0.18->0.19 guide tail sections beyond ~100k-char fetch window verified by TOC heading + matching release post only (marked TOC in artifact) — re-read before acting on those rows
- bevy-modern-guide: GitHub releases pages not fetched separately; version/date triangulated from docs.rs + bevy.org/news + lib.rs instead
- bevy-modern-guide: IME specifics not covered by any guide section read; flagged verify-per-app in §13
- bevy-modern-guide: No sim-spatial source audit performed (out of scope for reference-list task)
- repo-bevy-inventory: Cargo.lock feature resolution inferred from manifests+lock crate list, not cargo tree (cargo runs prohibited)
- repo-bevy-inventory: Grep counts are line counts including tests, not AST-exact per-type inventories for all 291 components
- audit-ecs-architecture: lesson/ internals beyond PageMode and jobs thread internals not inspected in depth; render-world/extract and reflection usage not audited (named in artifact Method and limits)
- audit-ecs-architecture: SubStates/SystemParam-derive/Populated/BSN since-versions marked version-unverified in artifact where not fetched
- audit-ui-render-picking: ~40 mode panel files calling the kit sampled via scripted grep + rebuild sites, not read line-by-line
- audit-ui-render-picking: CAD tool internals beyond pick/snap/measure/transform headers not fully read
- audit-ui-render-picking: No build/run/profiling per brief: rebuild-cost and wheel-routing impact claims are structural, not measured
- audit-ui-render-picking: Bevy native wheel-to-ScrollPosition equivalence for nested scrollers not runtime-verified (flagged as audit step in F3)
- audit-platform-tasks-assets: Whether main.rs --validate-only println! output is scraped by scripts (affects F22 scope) — not checked
- audit-platform-tasks-assets: Exact 0.19 gestures module path for PinchGesture/PanGesture cited in F9 — page not fetched, flagged in artifact

Additional gaps from this synthesis:

- No build, test, run, or profiling was performed for this final pass
  (per brief): every perf impact above (rebuild churn, chain
  serialization, wheel routing, chart re-upload) is structural.
  Items 2/6/7 in §4 exist partly to create the measurement base.
- First-party camera-controller availability in 0.19.1 is contradictory
  between artifacts (`bevy-modern.md:216` catalogues 0.18 `PanCamera` /
  0.20 pan-orbit; UI audit §5 found no in-tree orbit in 0.19.1). The
  KEEP verdict does not depend on resolving it; do not cite a stock
  orbit controller for 0.19.1 without checking 0.19.1 docs first.
- Whether the deprecated `ui::widgets::Button` component (vs the
  headless `ui_widgets::Button` confirmed at `ui_kit/activation.rs:10`)
  appears anywhere else, whether `NextState::set_if_neq` is used, and
  whether `MeshTag`/custom `WorldQuery` impls exist (other 0.20
  breakers) were not exhaustively grepped in any artifact read — the
  0.20 bump (item 8) must grep for each `bevy-modern.md` §15 item
  rather than assume absence.
- `bevy-settings` 0.20's useless-`SettingsGroup` error against the
  current `PreferenceGroup` shape is inferred from the guide heading
  (`bevy-modern.md:144,330-332`), not tested; item 9 may be forced by
  item 8 rather than optional.
- Spot-checks in this pass covered 5 source bodies only; all other
  `file:line` citations are taken from the originating artifacts,
  whose authors report reading those bodies (methods/limits in each
  artifact). Line numbers are as of 2026-10-12 and will drift.
- Findings whose evidence could not be seen in the artifacts were
  dropped rather than carried: specifically, no standalone finding is
  made for hot-patching (0.17), BRP/remote inspection, GPU-driven
  rendering/occlusion, Solari/DLSS, web-asset sources, or `no_std`
  (all catalogued in `bevy-modern.md` but with no local usage evidence
  either way in the audits read) — absence from §3 means "not audited",
  not "not applicable".

## 7. Execution log

Updated as each item of §4 lands (2026-10-12 execution pass; small
commits, targeted tests only — the runs named per item, nothing broader).

- **Baseline**: `cargo check -p sim-cad` found one error in the previous
  day's flex port (an `FnOnce` closure where `Fn` was required); fixed,
  and `cargo test -p sim-cad --lib -- flex component_derivation
  component_graph` passes (8). `cargo check -p sim-spatial` clean.
- **Item 1 (typed rebuild keys + AccessKit roles) — done.**
  `render_key` returns a typed `RenderKey` compared as values in the
  CAD panel, display `Shown`, thread pins and toolbar (its `Display`
  keeps the historical string byte-identical for the panels whose keys
  are still strings); the project card and comments panel keys are typed
  tuples. Direct `accesskit =0.24.1` dependency added; text fields carry
  `Role::TextInput`/`MultilineTextInput`; `chart_image` requires an
  accessible label (15 call sites). Note: `Button` and `Slider` headless
  widgets already carried their roles through `#[require]`, so no kit
  button change was needed. Targeted:
  `cargo test -p sim-spatial --lib -- render_key_display activation_tests ui_kit::tests`
  (31 passed).
- **Item 2 (diagnostics/logging/platform bundle) — done.**
  `FrameTimeDiagnosticsPlugin` in `CorePlugin`; the builder's
  `frame_timing` reads Bevy's `frame_time` diagnostic instead of `Time`.
  `LogPlugin` filter `wgpu=error,sim_spatial=info`; `PresentMode::Mailbox`
  for Place and Phenomena through `Look`/`apply_look` (AutoVsync
  elsewhere); a grabbing `CursorIcon` while a camera drag button is held
  outside the UI; `Time<Virtual>` capped at 100 ms and the three
  scattered `delta_secs().min(0.1)` clamps retired; the stale
  "no gamepad" feature comment corrected. Pointer-lock for the fly
  camera was *not* added: `camera/fly.rs` records a deliberate decision
  against cursor grabbing, which stands. Targeted:
  `cargo test -p sim-spatial --lib -- camera::tests app::tests` —
  green except five `rest::bind` tests that the shell sandbox cannot
  run (`PermissionDenied` binding a socket); the same five pass outside
  the sandbox.
- **Item 3 (text caret + selection) — done.** `TextDraft` carries a byte
  caret (char-boundary clamped) and an anchor; Left/Right/Home/End move
  it (Home/End line-wise), Shift extends a range selection, typing and
  Backspace/Delete act on the selection or the char at the caret, and
  `select_all` focus semantics are unchanged. `Kit::input_at` draws the
  caret at its position and the selection as an accent span; wired
  through `FormRow`, `PathView` and the CAD inspector's physical rows
  (callers whose forms keep their own text storage pass no caret yet and
  keep the end caret; the materials search, tree rename and references
  row fields still use their own append-only mini-drafts). IME
  composition remains future work, as the audit sized it. Targeted:
  `cargo test -p sim-spatial --lib -- ui_kit::tests text::tests text_drafts`
  (26 passed, including the new caret/selection test).
- **Item 4 (Single + fallible sweep, Inspect) — done.** All eight
  `Single`-using Inspect systems (`scroll_inspector`, `sync_camera`,
  `draw_pins`, `inset`, `split`, physics `labels`, `overlay_bar`,
  `publish`) now take plain queries and return
  `bevy::ecs::error::Result<()>` with `single()?`/`single_mut()?`, so a
  missing or duplicated singleton is a routed error, not a panic or an
  arbitrary pick. The pooled-label and companion-copy despawns in those
  systems are `try_despawn`. The two `.iter().next()` singleton sites
  F10 named outside Inspect went with it (CAD panel root is
  `Option<Single>`; the picker's existence check is `is_empty`). The
  global warn handler stays for now — tightening it wants this pattern
  in the remaining modes first. Targeted:
  `cargo test -p sim-spatial --lib -- inspect_view::tests view::tests cad::panel`
  (33 passed).
- **Item 5 (Checkbox, ListBox pilot, Scrollbar) — done.** `Kit::checkbox`
  is a chip-painted kit button that is also Bevy's headless `Checkbox`
  (CheckBox role, `Checked` mirrored at spawn; no `checkbox_self_update`
  — the caller's draft is authoritative and panels rebuild on toggle, so
  self-update would double-flip the AT state on a refused toggle); form
  `Check` rows use it. `Kit::scrollbar` wraps the headless `Scrollbar`;
  piloted on the CAD inspector dock, whose scroll area fills it.
  The phenomena exhibit list is a `ListBox`: rows carry `ListItem` +
  ListItem role, the current row is `Selected` and the
  `ActiveDescendant`, and a `ValueChange<Entity>` observer writes the
  same select action a press does (skipping the already-current row a
  click also produces), giving the list arrow-key navigation.
  Targeted: `cargo test -p sim-spatial --lib -- ui_kit::tests phenomena::`
  (26 passed).
- **Item 6 (retained pilot + first resource split) — done.**
  `PhysicalEdit` (the inspector rows' draft and exact-measurement job)
  is its own `Resource`, split out of `CadDocument` on the
  `ComponentsState` pattern (init in the inspector's `CoreParts`,
  threaded through `Cx`, `exact::*`, `entry`, the rows, the panel
  refresh and `cad_state`; reset in `cad::clear`). The History part is
  the retained pilot: spawned once per document source with a constant
  part key, its undo/redo text blocks are updated in place by
  `sections::sync_history` on the document's change tick — the part's
  `format!("{:?}", history)` rebuild key is deleted. (Visual note: the
  history labels now render as two multi-line text blocks instead of
  one row per label.) Targeted:
  `cargo test -p sim-spatial --lib -- cad::inspector cad::snapshot cad::panel`
  (18 passed; the panel test world inits the new resource like its
  siblings).
- **Item 7 (un-chain Robot Present) — done, measurement caveated.**
  The two chained Present tuples (11 + 10 systems) are one un-chained
  registration: every system fills its own panel root or text from state
  written in earlier stages, and no pair reads another's same-frame
  output, so access analysis now decides parallelism. No explicit edges
  were needed. Measurement: the schedule builds and the targeted tests
  pass, but a before/after frame-time A/B was **not** run — this
  environment has no windowed Robot session, and windowless test apps
  do not exercise Present rendering. Run the viewer in Robot mode and
  compare `frame_ms` in `system_state` (now fed by
  `FrameTimeDiagnosticsPlugin`, item 2) against the pre-change build;
  treat any panel that lags a frame as a named edge to add back, not a
  re-chain. Targeted:
  `cargo test -p sim-spatial --lib -- ordering_guard robot::tests`
  (11 passed).
- **Item 9 (`bevy-settings`/Reflect seam) — done by removal.** The
  registered `PreferenceGroup` had no consumer, so it, per-frame JSON
  publication into it, `SettingsGroup` registration/assertion, and the
  `bevy-settings` dependency are removed. The former group and file names
  are now typed constants (`preferences` and `viewer-preferences`), so the
  existing on-disk contract is byte-identical. A focused regression test
  pins both names and the derived `.json` filename. Targeted settings
  execution: first run 36 passed with only the new name test expecting the
  production filename from an isolated fixture path; corrected and the
  single regression test then passed.
- **Item 8 (bevy 0.19.1 → 0.20.0) — done.** Versions bumped in the root
  and `sim-spatial` manifests (the 0.19 feature list resolves unchanged;
  `accesskit =0.24.1` still matches `bevy_a11y` 0.20). The 139 first-check
  errors were three classes: flat pointer events (`Pointer<Press>` →
  `PointerPress`, etc.; tree-row `pointer_location` → `pointer.position`;
  test triggers rebuilt as flat event literals), the deprecated `bevy_ui`
  `Button` (every kit control now names `bevy::ui_widgets::Button` — one
  shadowing import per file, queries included; zero deprecated-Button
  warnings), and `CalculatedClip` (the lesson viewport now un-projects
  each inherited clip rect through `world_to_clip_local`). `Ordinary`
  carries `Interaction` + `FocusPolicy::Block` as required components —
  exactly what the deprecated bundle used to add — so the existing
  `Interaction` polling consumers keep working; the two repaint systems
  the audit names migrated to `Hovered`/`Pressed` (`Look`/`Tint` require
  `Hovered`; `Pressed` removal is collected via `RemovedComponents`).
  Remaining `Interaction` consumers (sliders, jog, text press, double-click
  rows; 54 deprecation warnings) migrate consumer-by-consumer, not in this
  bump. §15 greps: no `TextScroll`, `Sprite`, manual `WorldQuery`,
  `DeferredWorld`, `MeshTag`, bind-group, `FilteredResources`,
  `AssetId::invalid` or `bevy_shape` use; `NextState` only ever `.set()`;
  `Tonemapping` paths and one-arg `Font::from_bytes` compile unchanged;
  kit text sets `FontSize::Px` explicitly, so the `Rem(1.)` default is
  moot; the two `&mut World` systems already register through plain
  `add_systems`. One behavioral find: 0.20's tab navigation skips
  `InheritedVisibility=false` subtrees, which broke four windowless
  fixtures that never ran visibility propagation (production gets it from
  `DefaultPlugins` in PostUpdate); each fixture now stands in for
  propagation with a PreUpdate marker system. Verified against the
  pre-bump base in a scratch worktree (the modal tests pass there without
  the stand-in). Targeted: lib + tests compile with 0 errors;
  `ui_kit:: inspect_view::tests builder::placement::tests
  app::picker::activation_tests` (59 passed after the fixture stand-in),
  `cad::tree::tests robot::hardware::actions::input::tests
  ordering_guard app::settings::` (pass), `app::close` (17),
  `cad::surfaces::form cad::files::form` (11 with picker batch).
- **Post-bump weak ordering (item 7 repeat on 0.20) — done;
  frame-time caveated.** The audited robot SimSync 12-system tuple, both
  inspect-view SimSync tuples, and the inspect, phenomena, CAD,
  and robot-hardware Present tuples now use `chain_weak()`: their
  conflicting pairs (shared scene/view writes and Commands producers)
  keep tuple order, while disjoint presenters can run in parallel.
  The Input and Focus/key-gating chains and the public stage chains
  stay strict because their comments name same-frame focus/key semantics.
  The windowless public ordering test mirrors the robot/inspect weak
  tuples and initializes its Update schedule with
  `ScheduleBuildSettings.shuffle_seed = 0x5EED_2020`; `bevy_ecs`'s
  `debug` feature is enabled only through dev-dependencies. No
  windowed frame-time A/B was available in this environment, so the
  performance effect is unverified; correctness evidence is schedule
  construction only. Targeted:
  `cargo test -p sim-spatial --lib -- app::ordering_tests ordering_guard`
  (10 passed).
- **Item 10a (inspect material-handle cache) — done.** A
  `PartMaterials` resource now caches one shared `StandardMaterial`
  per untinted spawn look (sRGBA colour, metallic, roughness), in
  the `CadMaterials` pattern. Each spawned part carries a
  `PartMaterial` with that shared handle and a private handle created
  lazily on its first tint (thermal colour, selection,
  ghost/emphasis/internals alpha, or heat glow); `update_parts` swaps
  `MeshMaterial3d` handles and mutates only the private asset, so the
  exact prior colour/alpha/emissive formulas and values are preserved
  and same-look parts no longer each allocate a material. Parts not
  spawned by `spawn_parts` keep the legacy mutate-own-asset path (the
  existing animation test pins it). Split-view copies record their
  source entity and follow its current handle, preserving the old
  shared-asset mirroring. Spawning now also includes `Visibility`
  explicitly: on 0.20 `Mesh3d` gains it only from `VisibilityPlugin`'s
  runtime registration, while `update_parts` writes it. Focused test:
  two same-look parts share one handle until one is selected, the
  selection tint lands on a private handle without recolouring the
  shared asset, and clearing selection swaps back. Targeted:
  `cargo test -p sim-spatial --lib -- inspect_view::tests animation::`
  (7 passed) and `cargo check -p sim-spatial --lib --tests` (clean).
- **Item 10b (robot `Section` SubStates pilot) — done as a one-way ECS
  projection.** `Section` now derives `SubStates` sourced on
  `ViewerMode::Robot` (default `Link`), registered by
  `robot::section_state` in `RobotPlugin`. `RobotView::section` stays
  authoritative — the data-level constructors, action handlers and
  `robot_state`'s same-frame answer all read/write it as before — and
  `sync_section_state` (SimSync, after Actions) mirrors it into
  `NextState<Section>`; the pilot deliberately does not invert that
  ownership, since a SubState transition lands a frame later than the
  REST reply that must already name the new section. First lifecycle
  consumer: entering `Section::Comments` rereads the CAD source (as
  the manual Refresh act does) and spawns a `ThreadsSectionBody`
  under `ThreadsRoot` carrying `DespawnOnExit(Section::Comments)`;
  the panel draw is gated on `in_state(Section::Comments)` and draws
  into that body (still honoring the view field during the one-pass
  mirror lag so leaving clears the same frame), while thread reads
  and landed changes keep running in every Robot section for REST
  callers waiting on them. Focused test: no `Section` state outside
  Robot mode, the file default mirrors to `Link`, entering Comments
  spawns and draws the scoped body and clears the stale error,
  leaving the section despawns it, and leaving Robot mode removes
  the state. Targeted:
  `cargo test -p sim-spatial --lib -- robot::tests:: robot::threads::tests robot::actions::tests`
  (21 passed).

