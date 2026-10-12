# Bevy ECS / App Architecture Audit — `sim-spatial`

Audit date: 2026-10-12. Scope: Bevy usage in the workspace, judged from source
bodies plus spot-checks of current Bevy docs. No code was changed; `cargo`
was not run.

## Scope correction: only one Bevy app crate

The brief assumed Bevy in `sim-spatial`, `sim-render`, `sim-viewer` and
`sim-web`. Inspection of the manifests shows only **`crates/sim-spatial`**
depends on Bevy (`crates/sim-spatial/Cargo.toml`: `bevy = "=0.19.1"` with a
hand-picked feature set; workspace root `Cargo.toml:60` pins `bevy = "0.19.1"`):

- `crates/sim-viewer/Cargo.toml` — `eframe = "=0.36.2"` (egui), no Bevy.
- `crates/sim-render/Cargo.toml` — `glam`, `epaint_default_fonts`, no Bevy;
  no `bevy::` use in `crates/sim-render/src`.
- `crates/sim-web/Cargo.toml` — `wasm-bindgen` cdylib, no Bevy.

So this audit covers `crates/sim-spatial` only: 541 `.rs` files, ~156k LOC
(counted with `wc -l`). It is one Bevy `App` (`crates/sim-spatial/src/app/mod.rs:221`
`run(launch)`) hosting seven viewer modes as Bevy states. Pattern counts
below come from `grep` over `crates/sim-spatial/src`; every finding cites a
body that was opened and read.

## Verdict in one paragraph

The skeleton is modern and unusually disciplined for a codebase this size:
Bevy 0.19.1, `States` + `ComputedStates` + `DespawnOnExit`, public
`SystemSet` pipeline enforced by a source-scanning guard test, typed
`Message`-based actions, picking observers, `Changed<>`-driven widget
repaint, reactive winit. The debt is concentrated in three places:
(1) a handful of god-`Resource`s that hold whole documents plus all panel
state, which forces manual revision counters, string rebuild keys and
`bypass_change_detection` workarounds; (2) immediate-mode-style UI rebuilds
(`despawn_related::<Children>()` + respawn) gated by those string keys,
which is where focus-rebinding machinery and the global `warn` error
handler come from; (3) long `.chain()` schedules that serialise work the
`multi_threaded` scheduler could overlap. Custom Relationships, fallible
systems and `SubStates` are unused; required components are used twice.

## How to read the findings

Each finding gives: **Current** pattern with `file:line`, **Modern
alternative** (with since-version only where verified by fetching docs;
otherwise marked *version-unverified*), **Impact**, and **Cost** (S/M/L =
small/medium/large migration). "Already good" notes are collected at the end.

---

## F1 — God-Resources hold documents + all feature state

**Current.**

- `CadDocument` (`crates/sim-spatial/src/cad/document/mod.rs:18-152`): one
  `#[derive(Resource)]` with ~50 fields — connection, health, the whole
  `DocState`, selection echo, tool state, plus nested per-feature states
  (`robot`, `materials`, `physical_edit`, `results`, `print`, `tree`,
  `threads`, `references` at lines 74-94), in-flight jobs (lines 109-113),
  and a hand-rolled change counter: `pub revision: u64` — "Bumped whenever
  anything the panels show changes" (line 106-107).
- `Builder` (`crates/sim-spatial/src/builder.rs:161-260`): ~50 fields —
  document, store, registry, palette, drag state, live run, jobs, and
  again nested feature states (`graphs`, `study`, `composition`,
  `calibration`, `schematic`, …), plus manual dirty flags `scene_dirty` /
  `panel_dirty` (lines 191-192).
- `SpatialScene` (`crates/sim-spatial/src/inspect_view/mod.rs:32-77`): the
  inspected `SystemDescription` + `SpatialDescription` + view state +
  selection projection + lesson directives in one resource.
- `RobotView` (`crates/sim-spatial/src/robot/mod.rs:69-130`): model, run
  controller, preset, source watch, stress, planar view, scroll, `ui_revision`.

**Consequences visible in the code.** Any `ResMut<CadDocument>` write marks
the *whole* resource changed, so change detection is too coarse to gate
panels. The codebase compensates with (a) manual counters (`revision`,
`ui_revision`, `Selection::changed` at `selection/mod.rs:186-187`,
`DocumentRegistry::changed` at `document/mod.rs:157-158`), (b) string
rebuild keys (F2), and (c) widespread `bypass_change_detection()` calls to
write without triggering the coarse signal (e.g.
`crates/sim-spatial/src/cad/references/reads.rs:250`,
`crates/sim-spatial/src/lesson/ui/mod.rs:423`,
`crates/sim-spatial/src/builder/background.rs:70`). A single `ResMut` also
serialises every system that touches any part of the document.

**Modern alternative.** Split per-concern resources (the codebase already
does this well at the edges — `CadMeshes`, `Replies`, `Selection`) so
`Res::is_changed()` / `Changed<T>` become precise; represent per-item
viewer state (CAD nodes, builder instances, robot links) as
entities/components where systems need to query subsets, keeping the
*external* document payloads (`SystemDocument`, `DocState` — file formats
owned by other crates) as plain data inside a slim resource. This is a
design pattern, not a versioned API.

**Impact.** Maintainability high (precise invalidation deletes F2's keys);
perf medium (fewer full-panel rebuilds, more scheduler parallelism).

**Cost.** L overall; M per feature state lifted out (e.g. `print`, `tree`
are already separate structs, just nested).

## F2 — Manual string rebuild keys instead of change detection

**Current.**

- Whole CAD panel gate is a nine-condition hand-written predicate plus a
  string stamp: `crates/sim-spatial/src/cad/panel.rs:404-411`
  (`stamp == drawn.stamp && !added && !draft.is_changed() && … &&
  !motion.is_changed()`), then a per-part `format!` key per list entity
  (`crates/sim-spatial/src/cad/panel.rs:425-437`).
- The stamp itself is a debug-formatted string:
  `render_key` = `format!("{:?}", (doc.generation, &doc.doc_key, doc.revision))`
  (`crates/sim-spatial/src/cad/activation.rs:28-30`) — allocated and
  compared as text every frame, even though a perfectly good typed
  `SourceStamp` struct exists right above it
  (`crates/sim-spatial/src/cad/activation.rs:7-18`).
- Same shape elsewhere: `crates/sim-spatial/src/project/panel.rs:134-140`
  (`format!("{}|{}|{focus:?}|{}|{}", st.revision, …)` compared against
  `Local<Option<String>>`), `crates/sim-spatial/src/robot/threads/panel.rs:131-132`,
  `crates/sim-spatial/src/app/picker/mod.rs:539-546`,
  `Local<String>` in `crates/sim-spatial/src/physics_view.rs:619`.

**Modern alternative.** `Changed<T>` / `Added<T>` query filters and
`Res::is_changed()` on *split* resources (F1), lifecycle observers
(`On<Add>`/`On<Insert>`, observers since 0.14 — verified via Bevy 0.14
release notes, <https://github.com/bevyengine/bevy-website/blob/HEAD/release-content/0.14/release-notes/10756_hooks_and_observers.md>),
or at minimum typed tuple keys (`(u64, u64, bool)`) instead of `format!`
strings. The codebase already proves the pattern works where state is
component-shaped: `repaint_buttons` is gated by
`Or<(Changed<Look>, Changed<Enabled>, Changed<Interaction>)>`
(`crates/sim-spatial/src/ui_kit/widgets.rs:335`).

**Impact.** Perf small per key but multiplied by every panel every frame
(allocations); maintainability high — string keys silently miss a field
when state grows, which is exactly the bug class the nine-condition gate
at `cad/panel.rs:409` is fighting.

**Cost.** S for typed keys; M per panel for real change detection (depends
on F1 splits).

## F3 — UI trees are despawned and respawned on every rebuild

**Current.** 86 `despawn_related` / `despawn()` sites (grep count). The
canonical shape: compare key → `despawn_related::<Children>()` →
`with_children(|p| …)` respawn the whole subtree
(`crates/sim-spatial/src/cad/panel.rs:438-468`). The project card despawns
its entire root and respawns it (`crates/sim-spatial/src/project/panel.rs:141-161`).
Robot panels do the same (`crates/sim-spatial/src/robot/controls.rs:104`,
`crates/sim-spatial/src/robot/hardware/sync_panel.rs:105`, …).

**Consequences visible in the code.**

- Commands queued for just-despawned entities are common enough that the
  app downgrades Bevy's default panic to a warning:
  `app.set_error_handler(bevy::ecs::error::warn)` with the explanation at
  `crates/sim-spatial/src/app/mod.rs:225-230`.
- Text focus must survive rebuilds, so durable `TextField` entities live
  outside the panels and a whole rebinding system re-anchors focus by
  `InputIdentity` string after respawns
  (`crates/sim-spatial/src/ui_kit/activation.rs:153-180`, contract in
  `crates/sim-spatial/src/ui_kit/mod.rs:93-95`).
- Activation occurrences can land on dead entities and must be made to
  "expire" (`crates/sim-spatial/src/ui_kit/activation.rs:120-126`).

**Modern alternative.** Retained-mode updates: spawn rows once, mutate
`Text` / `Node` / `Look` / `Enabled` components, use `Changed<>` systems to
repaint (already the design for buttons — `ui_kit/widgets.rs:335-400`).
Bevy 0.19's scene/BSN work may eventually offer a declarative retained UI
path, but that is **version-unverified** here and not needed to fix this.

**Impact.** Perf medium (archetype churn, layout invalidation);
maintainability high (deletes focus-rebind and stamp/expire machinery).

**Cost.** L. Migrate panel by panel, starting with high-churn ones (robot
controls, CAD lists); the key-gated parts that change rarely can stay.

## F4 — Per-frame polling with manual counters, where observers/messages fit

**Current.** Cross-feature propagation is "compare a counter every frame":
`Selection::changed: u64` "bumped on every change; highlights, inspectors
and RoboCAD's `/selection` push compare it with the count they last saw"
(`crates/sim-spatial/src/selection/mod.rs:32-34`, field at :186-187);
`DocumentRegistry::changed` (`crates/sim-spatial/src/document/mod.rs:157-158`);
`Builder::seen_selection` (`crates/sim-spatial/src/builder.rs:176-178`);
`CadDocument::published_selection` (`crates/sim-spatial/src/cad/document/mod.rs:135-138`);
`CadMeshes::epoch` "bumped when an entity is spawned, replaced or
despawned" (`crates/sim-spatial/src/cad/mesh.rs:88-89`). Present-stage
systems then run every frame and early-return when nothing changed.

Job results are also polled per frame (`job.poll()` in SimSync/JobResults
systems, e.g. `crates/sim-spatial/src/robot/scene.rs:44-49`). That part is
idiomatic for Bevy (no completion callback into the ECS), and the file
watch poll (`crates/sim-spatial/src/robot/scene.rs:9-19`) is a filesystem
stat — polling is the right tool there. Reactive winit
(`crates/sim-spatial/src/app/mod.rs:331-333`, 60 Hz focused / low-power
unfocused, `RequestRedraw` writers) bounds the cost.

**Modern alternative.** For *internal* state edges: `Changed<T>`,
`RemovedComponents<T>`, lifecycle observers (`On<Add>`, `On<Insert>`,
`On<Remove>` — none are used anywhere; grep finds zero), or a `Message`
written at the mutation site. Counters are only needed where the consumer
is outside the ECS (the RoboCAD HTTP push genuinely is).

**Impact.** Perf small-medium (systems still wake every frame to compare);
maintainability medium (each counter is a hand-maintained invalidation
edge that can be forgotten — the failure mode F2's gates defend against).

**Cost.** M, incremental per counter once F1 splits land.

## F5 — `Act<A>` action bus: actually modern — keep, with two caveats

**Current.** Every user intent is a typed enum per mode, transported as a
Bevy `Message`: `Act<A> { action, origin }` with
`impl<A> Message for Act<A>` (`crates/sim-spatial/src/app/actions.rs:121-126`),
registered via `add_message` (:300-303), written by input systems with
`MessageWriter<Act<A>>`, and applied by exactly one handler per type in
`ViewerSet::Actions`. The module documents *why* messages rather than
observer triggers: one consumer, deterministic frame ordering, REST reply
correlation (`crates/sim-spatial/src/app/actions.rs:10-24`). That reasoning
is sound and matches current Bevy guidance (buffered `Message` for
queue-like single-consumer flows; observers for entity-targeted immediate
reactions — the `Event`→`Message` rename that made this distinction
explicit landed in 0.17; the codebase already uses the new names
everywhere). **This is a keep/do-more-of, not a finding against it.**

Caveats worth recording:

1. Apply systems bypass `MessageReader` and drain the buffer directly —
   `messages.drain().collect()` into a `Vec` each frame
   (`crates/sim-spatial/src/app/actions.rs:308-311`; same in the switch
   handler, `crates/sim-spatial/src/app/switch/mod.rs:375`). Works, but
   allocates per frame and forfeits reader-cursor semantics if a second
   consumer ever appears.
2. Asynchronous REST actions are re-applied every frame while `Pending`
   via the `InFlight<A>` resource (`crates/sim-spatial/src/app/actions.rs:265-298`),
   i.e. polling continuations with `serde_json::Value` state. This is a
   reasonable fit for "handler owns a background job", but it means every
   long action re-runs its handler prologue per frame; a completion
   `Message` from the job side would be the more event-driven shape.

**Impact / Cost.** Low priority. S for (1) where a single consumer is
guaranteed; M for (2), and only worth it if Pending handlers get heavier.

## F6 — Activation is observer → marker component → poll → message

**Current.** A click travels four hops: `bevy_ui_widgets::Activate`
observer `capture` inserts an `Activated` marker
(`crates/sim-spatial/src/ui_kit/activation.rs:120-126`), validation systems
may remove it (`crates/sim-spatial/src/cad/activation.rs:78-95`), then each
mode polls `Query<&SomeAction, With<Activated>>` in `InputSet::Window` and
converts to `MessageWriter<Act<A>>` (e.g.
`crates/sim-spatial/src/app/switcher.rs:66`,
`crates/sim-spatial/src/phenomena/panel.rs:249`), then `clear` removes the
marker (`crates/sim-spatial/src/ui_kit/activation.rs:136-138`).

The indirection exists for real reasons documented in the code:
eligibility checks, stale-source refusal (F2's `SourceStamp`), and
REST-before-window ordering. But it re-implements, with a marker component
and a clear system, what an entity-event observer writing the `Act`
message directly (with the validation as an observer or a preceding
system) would do in one hop.

**Modern alternative.** Entity-targeted observers (`On<Activate>` observers
are already registered — `crates/sim-spatial/src/ui_kit/activation.rs:66-68`)
writing `Act<A>` via `MessageWriter`, keeping the `ActivationSet::Validate`
gate as a message-filtering step only where stale-source refusal is
required (CAD). Picking already uses direct observers well
(`crates/sim-spatial/src/linked.rs:111-116`,
`crates/sim-spatial/src/inspect_view/scene.rs:135`,
`crates/sim-spatial/src/robot/actions/keys.rs:154`) — do more of that.

**Impact.** Maintainability medium; perf small (one frame-part less
latency, fewer archetype moves from marker insert/remove).

**Cost.** M, and risky around focus/keyboard paths — migrate one mode's
buttons first, keep CAD's validated path until F3 reduces rebuild churn.

## F7 — Ordering: excellent public sets, but over-chained inside them

**Current (the good part).** One pipeline of sets —
`ViewerSet::Input → Actions → JobResults → SimSync → Present`
(`crates/sim-spatial/src/app/mod.rs:155-171`, configured :279-280) — with
sub-sets per feature (`CameraSet`, `RobotSet` at
`crates/sim-spatial/src/robot/mod.rs:211-223`, `InspectViewSet` at
`crates/sim-spatial/src/inspect_view/mod.rs:309-326`, `ActivationSet`).
Cross-feature ordering against concrete functions is *banned and
machine-checked*: `crates/sim-spatial/src/ordering_guard.rs:1` — "Public
sets are the only ordering contract between top-level features." This is
beyond typical Bevy practice; keep it.

**Current (the cost).** Inside the sets, long `.chain()` tuples serialise
everything: Robot mode chains 15 input systems, 12 SimSync systems and two
11/10-system Present chains (`crates/sim-spatial/src/robot/mod.rs:263-282`);
the spatial view chains its layout, camera-readers and Present stages
(`crates/sim-spatial/src/inspect_view/mod.rs:358-377`). 51 `.chain()` and
241 `in_set` sites overall (grep). Chained systems cannot run in parallel
even when their access is disjoint, so the `multi_threaded` feature
(enabled in `crates/sim-spatial/Cargo.toml`) buys little inside a mode.

**Modern alternative.** Keep sets as the contract; drop `.chain()` to
explicit `.before()/.after()` edges only where a comment names a real data
dependency (several chains already carry such comments, e.g.
`robot/mod.rs:271-274` — those edges stay). Let the scheduler parallelise
the rest; Bevy reports ambiguities if access conflicts appear.

**Impact.** Perf medium on multi-core (Present-stage panels are mostly
independent); maintainability small win (adding a system stops being a
position-in-a-tuple decision).

**Cost.** M — mechanical, but needs frame-time measurement per mode to
confirm no hidden dependency was relying on chain order.

## F8 — Modes: States done right at the top, plain enums underneath

**Current (good).** `ViewerMode` is a real `#[derive(States)]`
(`crates/sim-spatial/src/app/mod.rs:54-74`); `ModeScope` and
`SpatialScreen` are `ComputedStates` (:125-151) so Build↔Lessons keeps the
builder alive; mode entities get `DespawnOnExit<ModeScope>` via a single
`Last`-schedule sweep, `scope_new_entities` (:301-312); `OnEnter`/`OnExit`
drive setup/teardown (e.g. `crates/sim-spatial/src/app/switch/mod.rs:354-367`).
Mode *resources* are removed manually in `OnExit` systems — correct,
because Bevy state-scoping covers entities, not resources.

**Gaps.**

- Sub-mode state is plain fields in god-resources, polled by systems:
  builder `Tab`/`Mode` (`crates/sim-spatial/src/builder.rs:28-48`, fields
  at :205-207), robot `Section` (`crates/sim-spatial/src/robot/mod.rs:132-145`),
  `CadTool`, lesson `PageMode`. Panels for inactive tabs still run and
  early-return instead of being state-gated.
- The `scope_new_entities` sweep is clever but implicit: *any* root
  `Node`/`Transform` spawned during `Update` silently becomes
  mode-scoped unless marked `Persistent`; correctness rests on the
  scheduling argument in its comment (`app/mod.rs:301-306`).

**Modern alternative.** `SubStates` (present in bevy_state 0.19.1 —
verified at <https://docs.rs/bevy_state/0.19.1/bevy_state/state/index.html>:
"a state that exists only when the source state meet certain conditions…
while they exist they can be manually modified"; since-version
*unverified*) for tab/tool state, giving `OnEnter(Tab::…)` spawns and
`DespawnOnExit` cleanup for free, plus `run_if(in_state(..))` gating
instead of field checks.

**Impact.** Maintainability medium; perf small.

**Cost.** M per mode (robot `Section` is the cleanest first candidate).

## F9 — Two exclusive `&mut World` systems at the centre

**Current.** The REST poll `serve(world: &mut World)`
(`crates/sim-spatial/src/app/actions.rs:459`) and the mode-switch handler
`handle(world: &mut World)` (`crates/sim-spatial/src/app/switch/mod.rs:373`)
are exclusive systems: they take the whole world, do
`world.resource_mut` / `resource_scope` gymnastics
(`actions.rs:463-495`, `switch/mod.rs:374-419`), and block all parallel
execution while running. `serve` is exclusive because the action registry
is type-erased — `Feature.submit: fn(&mut World, …)`
(`actions.rs:345-353`) — which is a defensible reason for *one* narrow
exclusive system. `handle` is exclusive by accumulation: it orchestrates
the switcher, replies, settings, picker and documents in one body.
`DeferredWorld` is used nowhere (grep: zero) — that is fine; it is only
needed inside hooks/observers, not a goal in itself.

**Modern alternative.** Ordinary systems with declared params (the
scheduler then sees conflicts and can overlap them), grouping params with
a derived `SystemParam` struct where signatures get wide (*version for
the derive unverified*). Keep `serve` exclusive but shrink it to
parse-and-write-message; move switch bookkeeping into param systems.

**Impact.** Perf small (both run once per frame, briefly);
maintainability medium (access conflicts become compile/schedule-visible
instead of runtime `resource_scope` dances).

**Cost.** M for `serve` narrowing; L for decomposing `handle`.

## F10 — Query shapes: defensive `Option<>` everywhere, a few very wide systems

**Current.** Grep counts: 418 `Option<Res<…>>` / `Option<ResMut<…>>` and
514 `Option<&…>` occurrences. Many systems are both state-gated
(`run_if(in_state(ViewerMode::Cad))`, 91 `in_state` uses) *and* take
`Option<Res<CadDocument>>` (e.g. `crates/sim-spatial/src/cad/activation.rs:42`,
`:78`) — double guarding because tests build partial apps without the
mode's resources. A few systems are very wide: the ancestry query with
nine fetched components (`cad/activation.rs:45`), `modal_focus` with 15
params (`crates/sim-spatial/src/ui_kit/activation.rs:153-167`), the CAD
panel draw with ~15 params (`crates/sim-spatial/src/cad/panel.rs:390-402`).
Singleton access is inconsistent: `Single<>` is used well in places
(`crates/sim-spatial/src/robot/scene.rs:30-33`,
`crates/sim-spatial/src/physics_view.rs:515`) but elsewhere it is
`roots.iter().next()` over a query (`crates/sim-spatial/src/cad/panel.rs:403`,
`crates/sim-spatial/src/app/picker/mod.rs:547`), which silently picks an
arbitrary entity if two roots ever exist.

To be fair to the code: `Option<&T>` fetches do not fragment archetypes,
and most wide queries are wide because UI work genuinely touches many
components. The real costs are readability, missed `Changed<>` filtering
(F2), and the silent-arbitrary-entity pattern.

**Modern alternative.** `Single` / `Option<Single>` for singletons
(panics/`None` loudly instead of arbitrary choice), `Populated<T>` where a
system should skip when empty (*version unverified*), and splitting
15-param systems along the same seams as F1's resource splits. Where
partial-app tests are the only reason for `Option<Res<…>>`, prefer
inserting a default resource in the test harness over taxing every
production signature.

**Impact.** Maintainability medium; perf negligible except via F2.

**Cost.** S per system; M to sweep a mode.

## F11 — Error handling: domain errors are values; ECS errors are global-warn

**Current.** Domain failures flow as `Result<_, String>` through action
`Outcome`s to REST replies and status lines — appropriate and consistent.
At the ECS level there are **no fallible systems**: grep finds no system
returning `bevy::ecs::error::Result` and no `BevyError` use; instead the
app installs one global handler,
`app.set_error_handler(bevy::ecs::error::warn)`
(`crates/sim-spatial/src/app/mod.rs:230`, rationale :225-229: a panel
rebuilt the frame its mode tears down queues commands for dead entities;
tests keep the strict default). Genuine panics in non-test code are rare
and mostly programmer-error guards, e.g. `named()` panicking on an
unregistered action feature (`crates/sim-spatial/src/app/actions.rs:386-388`).

The risk is that `warn` also swallows *real* command/system errors in
production — the handler cannot distinguish F3's benign dead-entity
rebuild from a logic bug, and the warning goes to a log nobody watches in
a desktop window.

**Modern alternative.** Fallible systems: a system returning
`Result<(), BevyError>` has its `Err` routed to the error handler, and
query fallibles (`single()?`, `get()?`) compose with `?` — verified at
<https://docs.rs/bevy/0.19.1/bevy/ecs/error/index.html> ("When a system…
its return type is that of `Result`, then Bevy considers those systems to
be 'fallible'… All `BevyError`s returned by a system, observer or command
are handled by an 'error handler'"). Combined with `try_insert` /
`try_despawn` at rebuild sites (already used in places, e.g.
`crates/sim-spatial/src/ui_kit/activation.rs:124`), the benign case stops
producing errors at all, and the global handler can go back to
panic-in-debug / log-in-release severity matching.

**Impact.** Maintainability/correctness medium (real errors become
visible again); perf none.

**Cost.** M — mostly mechanical `-> Result` + `?` in systems that use
`Single`/`get`, plus the F3 work that removes the original excuse.

## F12 — Required components used twice; manual bundle tuples everywhere else

**Current.** Exactly two `#[require(...)]` in the crate, both exemplary:
`Ordinary` requiring the widget `Button`/`ActivateOnPress`/`Outline`
(`crates/sim-spatial/src/ui_kit/activation.rs:9-11`) and `ModalFocus`
requiring `TabGroup::modal()` (:23-25). Everywhere else, composition is
manual tuples returned from `Kit` methods —
`Kit::button` assembles 10+ components
(`crates/sim-spatial/src/ui_kit/widgets.rs:108-120`) — and call sites
hand-write spawn tuples, e.g. the project card root
`(node, BackgroundColor, BorderColor, GlobalZIndex, Persistent, CardRoot)`
(`crates/sim-spatial/src/project/panel.rs:161`). No `#[derive(Bundle)]`
anywhere (grep: zero) — which is *aligned* with modern Bevy direction;
the gap is not using required components to replace the tuples.

**Modern alternative.** Required components (headline feature of Bevy
0.15 — verified at <https://bevy.org/news/bevy-0-15/>): put
`#[require(Node, BackgroundColor = …, …)]` on marker components like
`CardRoot`, `ListRoot`, panel roots, so spawn sites shrink to the marker
plus genuinely variable data, and "every X always has Y" becomes a type
invariant instead of a `Kit` convention. Keep `Kit` functions for
parameterised construction (labels, looks, actions).

**Impact.** Maintainability medium (fewer forgotten-component bugs, e.g. a
button missing `AccessibleLabel`); perf none.

**Cost.** M — mechanical per widget/root type; do opportunistically with F3.

## F13 — No custom Relationships; identity bookkeeping duplicates ECS indexing

**Current.** The only relationships in use are the built-in
`ChildOf`/`Children` hierarchy (which *is* a relationship since Bevy
0.16). No `#[relationship]` types exist (grep: zero). Cross-references are
manual:

- `CadMeshes.entries: HashMap<String, Entry>` maps RoboCAD node-id strings
  to state plus `entity: Option<Entity>`
  (`crates/sim-spatial/src/cad/mesh.rs:47-61`, resource at :79-104), with
  a hand-bumped `epoch` so consumers know the mapping changed (:88-89).
- The document registry re-implements entity identity:
  `DocumentId(pub u64)` with a never-reused counter and a `Vec` of entries
  searched linearly (`crates/sim-spatial/src/document/mod.rs:29-32`,
  `:151-171`), plus its own `changed` counter (F4).
- Selection items are stringly-typed values — `Item::Component { id: String }`,
  `Item::Cad(SelectionItem)` (`crates/sim-spatial/src/selection/mod.rs:51-66`)
  — stored in a `Vec<Selected>` in a resource (:183-191), never as entity
  references, so "what is selected" cannot be a query.
- 3D parts carry a Vec index, `Part { index: usize }`
  (`crates/sim-spatial/src/inspect_view/mod.rs:411-414`), indexing back
  into `SpatialScene.spatial.parts`.
- Physics labels rebuild a `HashMap<String, Entity>` from a query every
  frame (`crates/sim-spatial/src/physics_view.rs:516`).

**Fair caveat.** The *boundary* ids are real: RoboCAD node ids, file
component ids and document sources exist outside the ECS, so a string→
entity map at the sync boundary is unavoidable. The finding is that the
map then becomes the *internal* representation too, with manual epochs
and linear searches, where ECS indexing would do.

**Modern alternative.** Entity Relationships (Bevy 0.16 — verified via
the 0.16 release notes,
<https://github.com/bevyengine/bevy-website/blob/HEAD/release-content/0.16/release-notes/17398_Relationships_nonfragmenting_onetomany.md>,
and <https://docs.rs/bevy/0.19.1/bevy/ecs/relationship/index.html>):
e.g. a `CadNodeOf(Entity)` relationship from body entity → document
entity, selection as a relationship or marker component on the selected
entity, and documents as entities (`Entity` *is* the never-reused id,
with generation built in — exactly what `DocumentId` hand-rolls).
Internal consumers then query/traverse instead of consulting HashMaps;
the string map survives only at the file/service boundary.

**Impact.** Maintainability high in CAD/inspect sync code (deletes epoch
counters and parallel Vecs); perf small-medium (query iteration vs hash
lookups per item).

**Cost.** L. This is the deepest change on the list; pilot it on
`CadMeshes` (bounded, already entity-centric) before touching selection
or the document registry, whose string ids are also REST-visible.

---

## Already modern — keep / do more of

- **One App, plugin-per-mode, states for modes.** `run()` builds the only
  `App` (`app/mod.rs:221-275`); `ViewerMode: States` + `ModeScope` /
  `SpatialScreen: ComputedStates` + `DespawnOnExit` (`app/mod.rs:54-151`,
  `:307-312`). Textbook current Bevy.
- **Public SystemSets as the only cross-feature ordering contract**,
  enforced by a test that scans the source
  (`app/mod.rs:155-185`, `ordering_guard.rs:1`). Keep; relax chains
  *inside* sets per F7.
- **Messages, not events, for actions** — `Act<A>: Message`,
  `add_message`, one consumer per type, with the rationale written down
  (`app/actions.rs:10-24`, `:121-126`). Fully on the post-rename API.
- **Observers for picking and one-shot reactions** — `On<Pointer<Click>>`
  pick handlers (`linked.rs:116`, `robot/actions/keys.rs:154`,
  `builder/placement.rs:360`), screenshot-captured observers
  (`robot/video.rs:166`), `slider_self_update` observer
  (`ui_kit/mod.rs:129`), activation observers (`ui_kit/activation.rs:66-68`).
- **Change detection where widgets are components** —
  `Changed<Look/Enabled/Interaction>` repaint, `Changed<Text>` label
  following (`ui_kit/widgets.rs:335-400`); `Added<CadBody>` /
  `Changed<Mesh3d>` in CAD mesh sync (`cad/mesh.rs:505`,
  `cad/results/overlay.rs:132`). F1/F2 are about extending this to panels.
- **Required components where used** (`ui_kit/activation.rs:10`, `:24`) —
  the model for F12.
- **`Single` for true singletons** in robot/physics views
  (`robot/scene.rs:30-33`); `try_insert` for racy UI targets
  (`ui_kit/activation.rs:124`).
- **Reactive rendering discipline** — per-mode `WinitSettings`
  (`app/mod.rs:328-333`), `RequestRedraw` written only when a job lands or
  a frame applies (`robot/scene.rs:45-48`), background work on job threads
  applied in `JobResults`. This is why per-frame polling (F4) is bounded
  today.
- **Strict-in-tests error policy** — tests build their own `App` without
  the `warn` handler (`app/mod.rs:228-230`), so F11's production leniency
  does not hide failures from CI.

## Suggested order (cheapest leverage first)

1. Typed rebuild keys instead of `format!` strings (F2, S).
2. `Single` sweep + fallible systems in `Single`/`get` users (F10/F11, S-M).
3. Required components on panel roots as panels are touched (F12, M).
4. Un-chain Present stages with frame-time checks (F7, M).
5. Lift nested feature states out of `CadDocument`/`Builder` into their
   own resources; delete the counters they fed (F1+F4, M each).
6. Retained-mode conversion of the highest-churn panels (F3, L).
7. `SubStates` for robot sections / builder tabs (F8, M).
8. Relationship pilot in `CadMeshes` (F13, L).
9. Decompose the exclusive switch handler last (F9, L) — it is central
   and currently correct.

## Method and limits

- Source bodies opened and read for every `file:line` above; counts
  (resources, despawns, `Option<>`, chains) are `grep` totals over
  `crates/sim-spatial/src` and are labelled as such.
- Version claims verified by fetching: Bevy 0.15 release notes (required
  components), Bevy 0.16 release notes + docs.rs 0.19.1 (relationships),
  docs.rs 0.19.1 `bevy::ecs::error` (fallible systems), docs.rs
  bevy_state 0.19.1 (`SubStates`/`ComputedStates` presence), Bevy 0.14
  release notes (observers). Anything else is marked *version-unverified*.
- Not inspected in depth: `crates/sim-spatial/src/lesson` internals beyond
  `PageMode`, the `jobs` thread internals, render-world/extract usage
  (the crate uses `DefaultPlugins` rendering; no custom render graph was
  searched for), and reflection/serialization of ECS types. `sim-viewer`,
  `sim-render`, `sim-web` were checked only at manifest level to confirm
  they are not Bevy crates. No build or test run was performed.


