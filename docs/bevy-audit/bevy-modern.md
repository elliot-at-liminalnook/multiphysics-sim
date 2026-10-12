# Modern Bevy: how Bevy wants features implemented (reference catalogue)

Compiled 2026-10-12 for the physics-simulator workspace. Every pattern below was
read in the official migration guide or release post cited for its row; search
results were only used to locate those pages. "Canonical usage" is a one-line
reminder, not a tutorial — follow the citation for full code.

## 1. Versions

| Fact | Value | Evidence |
|---|---|---|
| Workspace pin (declared) | `bevy = "0.19.1"` | `Cargo.toml:60` (`[workspace.dependencies]`) |
| Workspace pin (resolved) | `bevy 0.19.1`, all `bevy_*` subcrates `0.19.1` | `Cargo.lock` (`name = "bevy"`, `version = "0.19.1"`, checksum `4bfadbeb…`) |
| Effective Bevy consumer | `crates/sim-spatial` only: `bevy = { version = "=0.19.1", default-features = false, features = [...] }`, plus `bevy-settings = "=0.19.1"` | `crates/sim-spatial/Cargo.toml:10,27` |
| Latest stable (Oct 2026) | **Bevy 0.20.0, released 2026-10-08** | [docs.rs/crate/bevy](https://docs.rs/crate/bevy) shows `bevy 0.20.0` as latest; [bevy.org/news/bevy-0-20](https://bevy.org/news/bevy-0-20/) "Posted on October 8, 2026"; [lib.rs/crates/bevy](https://lib.rs/crates/bevy) lists `0.20.0 Oct 8, 2026` above `0.19.1 Aug 13, 2026` |
| Release cadence in window | 0.16 (2025) → 0.17 → 0.18 (0.18.1 Mar 4, 2026) → 0.19.0 Jun 19, 2026 / 0.19.1 Aug 13, 2026 → 0.20.0 Oct 8, 2026 | lib.rs version table above; bevy.org/news posts per release |

**So: the workspace is exactly one minor behind latest.** The 0.19→0.20 guide is
the upgrade checklist; the 0.16–0.19 guides explain why 0.19.1 code looks the
way the `crates/sim-spatial/Cargo.toml:18-27` comment describes (0.17 crate
split, picking feature names, `bevy_state`, `bevy_ui_widgets`).

Note `crates/sim-render`, `crates/sim-viewer`, `crates/sim-web` do **not**
depend on Bevy at all today (inspected: sim-render = glam/image/ab_glyph;
sim-viewer = eframe/egui 0.36.2; sim-web = wasm-bindgen). The task brief's
"chiefly" list overstates Bevy's footprint — only sim-spatial uses it.

## 2. Sources actually read (bodies fetched, not snippets)

Migration guides (5, covering pinned ± context as required):

1. [0.15 to 0.16](https://bevy.org/learn/migration-guides/0-15-to-0-16/) — context (relationships, fallible systems era)
2. [0.16 to 0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/) — one of two pre-pin guides
3. [0.17 to 0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/) — second pre-pin guide
4. [0.18 to 0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/) — guide *into* the pin
5. [0.19 to 0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/) — guide *out of* the pin to latest

Release posts (5):

- [Bevy 0.16](https://bevy.org/news/bevy-0-16/) · [Bevy 0.17](https://bevy.org/news/bevy-0-17/) · [Bevy 0.18](https://bevy.org/news/bevy-0-18/) · [Bevy 0.19](https://bevy.org/news/bevy-0-19/) · [Bevy 0.20](https://bevy.org/news/bevy-0-20/)

Plus [docs.rs/crate/bevy](https://docs.rs/crate/bevy) (0.20.0 latest) and the
workspace manifests above. The old `bevyengine.org/learn/migration-guides/`
URLs now redirect to `bevy.org/learn/migration-guides/introduction/`.

Caveat on method: bevy.org guide pages exceed the fetch window (~100k chars),
so for 0.16→0.17 and 0.18→0.19 the table-of-contents plus the first ~2/3 of
sections were read verbatim and tail sections were cross-checked against the
matching release post. Items only seen in a TOC are marked "(TOC)" below and
should be re-read before acting on them.

## How to read the catalogue

- **Since** = first release where the pattern is the recommended one.
- **Replaces / for** = the older API it supersedes, or its purpose.
- ⚠️ = deprecated/removed API that code written against ≤0.19.1 (or older
  habits) may still contain; §15 collects the 0.19.1→0.20 breakers.

---

## 3. ECS core

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| Required components (expression syntax) | 0.15, syntax reworked 0.16, priority/safety refactor 0.17 | Hand-written `Bundle`s that only bundle defaults [0.15→0.16](https://bevy.org/learn/migration-guides/0-15-to-0-16/#improve-required-component-syntax) | `#[derive(Component)] #[require(Transform, Visibility)] struct Foo;` custom: `#[require(Name = Name::new("x"))]` |
| Runtime required components are `unsafe` | 0.17 | Depth-first priority; `RequiredComponentsRegistrator` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#required-components-refactor) | Prefer derive; runtime registration only `unsafe` via registrator |
| Relationships (one-to-many) | 0.16 | Hand-rolled bidirectional links; hooks keep target side in O(1) [0.16 news](https://bevy.org/news/bevy-0-16/#ecs-relationships) | `#[derive(Component)] #[relationship(relationship_target = LikedBy)] struct Likes(pub Entity);` + target `LikedBy(Vec<Entity>)` |
| Hierarchy is a relationship: `ChildOf`/`Children` | 0.16 | ⚠️ `Parent`, `AddChild`/`RemoveParent` commands [0.15→0.16](https://bevy.org/learn/migration-guides/0-15-to-0-16/#rename-parent-to-childof-and-change-how-the-parent-is-accessed) | `commands.spawn(ChildOf(parent));` reparent: `.insert(ChildOf(new))`; detach: `.remove::<ChildOf>()` |
| `children![…]` + bundle effects | 0.16 | Nested `.with_children(|p| …)` [0.16 news](https://bevy.org/news/bevy-0-16/#improved-spawn-api) | `commands.spawn((Player, children![(RightHand, children![Glove])]));` also `fn player() -> impl Bundle` |
| `detach_*` renames | 0.18 | ⚠️ `clear_children`/`clear_related` [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#renamed-clear-children-and-clear-related-methods-to-detach) | `detach_children()` / `detach_related::<T>()` |
| Self-referential relationships | 0.19 | Previously forbidden self-links [0.19 news](https://bevy.org/news/bevy-0-19/#self-referential-relationships) | Relationship may target its own entity |
| Immutable components | 0.16 | Mutable-by-default; integrity + perf [0.16 news](https://bevy.org/news/bevy-0-16/#immutable-components) | `#[component(immutable)] struct Id(u64);` entry API: `OccupiedEntry<T>` → `.into_mut()` |
| Resources as Components | **0.19 (pin)** | `Resource: Component`; derive impls both; type can't be both — split types; broad queries see resources, filter `Without<IsResource>`; `ReflectResource` now ZST, use `ReflectComponent`; Non-Send Resources→Non-Send Data [guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/#resources-as-components) | `#[derive(Resource)] struct Score(u32);` + `Res<Score>`; distinct component/resource types |
| Entity disabling / default query filters | 0.16 | Despawn/respawn churn [0.16 news](https://bevy.org/news/bevy-0-16/#entity-disabling-default-query-filters) | `insert(Disabled)` … `remove::<Disabled>()` |
| `EntityRef`/`EntityMut` respect default filters | 0.17 | ⚠️ Previously ignored filters [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#entityref-entitymut-and-option-disabled-no-longer-ignore-default-query-filters) | Explicit `Allow<Disabled>` when meaning "everything" |
| Entity cloning | 0.16, builder split 0.17 | Manual copy [0.16 news](https://bevy.org/news/bevy-0-16/#entity-cloning); split (TOC) [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#entityclonerbuilder-split) | `world.clone_entity(src)` / `EntityCloner::build_opt_out(…)` |
| Entity spawn ticks | 0.17 | Hand-tracked "new" flags [0.17 news](https://bevy.org/news/bevy-0-17/#entity-spawn-ticks) | Query `SpawnDetails`, filter `Spawned` |
| Remote entity reservation | 0.19 | Unsafe id reservation (net/BRP) [0.19 news](https://bevy.org/news/bevy-0-19/#remote-entity-reservation) | Reserve `Entity` ids before spawn |
| `Entity::PLACEHOLDER` → `Option<Entity>` | 0.20 | ⚠️ Sentinel values [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#some-entity-placeholder-values-have-been-replaced-with-option-entity) | Model absence as `None` |
| `Bundle::component_ids` → iterator | 0.18 | ⚠️ Slice returns [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#change-bundle-component-ids-and-bundle-get-component-ids-to-return-an-iterator) | Iterate / `.collect()` |
| Contiguous + nested query access | 0.19 | Combination workarounds [0.19 news](https://bevy.org/news/bevy-0-19/#contiguous-query-access) | Grouped contiguous iteration; nested `Query` in `QueryData` |
| Safe disjoint mutable access | 0.18 | `unsafe` multi-get [0.18 news](https://bevy.org/news/bevy-0-18/#safe-mutable-access-to-multiple-arbitrary-components) | Safe `get_many`-family world APIs |
| Per-column change ticks | 0.20 | Coarser tick bookkeeping [0.20 news](https://bevy.org/news/bevy-0-20/#per-column-change-ticks) | No change; `Changed<T>` precision improves |
| Location tracking (`MaybeLocation`) | 0.16 | ⚠️ `track_change_detection` flag, `&'static Location` returns (0.15→0.16 guide) | `Ref::changed_by()` → `MaybeLocation`; feature `track_location` |
| ⚠️ Batch spawn removed | removed 0.17 | `insert_or_spawn_batch{,_with_caller}` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#removed-deprecated-batch-spawning-methods) | `Disabled`+`try_insert_batch`, `spawn_batch`, or own map via `EntityMapper` |
| ⚠️ `iter_entities{,_mut}` deprecated | 0.17 | Direct world iteration [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#deprecate-iter-entities-and-iter-entities-mut) | `Query<EntityRef>` / `Query<EntityMut>` |
| Fallible systems + unified errors | 0.16 | Panics as control flow [0.16 news](https://bevy.org/news/bevy-0-16/#unified-ecs-error-handling) | `fn sys(…) -> Result { … }` with `Single`/`?`; `App::set_error_handler` |
| Per-world error handler, always on | 0.17; renamed 0.19 | ⚠️ `configurable_error_handler` + `GLOBAL_ERROR_HANDLER`; ⚠️ `DefaultErrorHandler`→`FallbackErrorHandler` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#changes-to-the-default-error-handler-mechanism), [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#defaulterrorhandler-renamed-to-fallbackerrorhandler) | `app.set_error_handler(h);` / insert `FallbackErrorHandler(h)` |
| Error context messages | 0.20 | Bare errors [0.20 news](https://bevy.org/news/bevy-0-20/#bevy-error-context-messages) | Attach context on fallible paths |
| Delayed commands | 0.19 | Hand-rolled timers [0.19 news](https://bevy.org/news/bevy-0-19/#delayed-commands) | `commands.delayed().secs(1.0).spawn(Marker);` no cancellation — embed origin entity |
| Catching panics | 0.20 | Panic kills app [0.20 news](https://bevy.org/news/bevy-0-20/#catching-panics) | Opt-in panic catching in schedule config |
| `FilteredResources` deprecated | 0.20 | ⚠️ Dynamic resource structs [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#filteredresources-and-similar-structs-have-been-deprecated) | Query resources as components (0.19 model) |

---

## 4. Observers, lifecycle, messaging (the 0.17 split — read this first)

The single biggest conceptual change in the window. Before 0.17 one `Event`
trait served both observers and buffered queues. Since 0.17 they are
statically distinct ([0.17 news #event-observer-overhaul](https://bevy.org/news/bevy-0-17/#event-observer-overhaul),
[0.16→0.17 #event-trait-split-rename](https://bevy.org/learn/migration-guides/0-16-to-0-17/#event-trait-split-rename)):

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `Event` = observed only; `On<E>` param | 0.17 | ⚠️ `Trigger<E>` observer param | `#[derive(Event)] struct GameOver{score:u32}` · `world.add_observer(|e: On<GameOver>| info!("{}", e.score));` · `world.trigger(GameOver{score:100});` |
| `EntityEvent` (target in the type) | 0.17 | ⚠️ `world.trigger_targets(ev, entity)` | `#[derive(EntityEvent)] struct Click{entity: Entity}` · `world.trigger(Click{entity});` · per-entity: `world.entity_mut(e).observe(|c: On<Click>| …)` |
| `#[event_target]` field attr | 0.17 | Ambiguous multi-entity events | `#[derive(EntityEvent)] struct Attack{#[event_target] attacker: Entity, target: Entity}` |
| Propagation opt-in | 0.17 | ⚠️ `Traversal`/`AUTO_PROPAGATE` on every event; `#[event(traversal, auto_propagate)]` style | `#[derive(EntityEvent)] #[entity_event(propagate)] struct Click{entity:Entity}` then `click.propagate(false);` `click.original_event_target()`; defaults to bubbling via `ChildOf` |
| Custom `Trigger` impls | 0.17 | One-size dispatch; usually unneeded | `#[derive(Event)] #[event(trigger = MyTrigger)] struct Jump;` (triggers: `GlobalTrigger`, `EntityTrigger`, `PropagateEntityTrigger`, `EntityComponentsTrigger`) |
| Lifecycle events renamed | 0.17 | ⚠️ `Trigger<OnAdd, T>` etc. Types moved `bevy_ecs::world`→`bevy_ecs::lifecycle`; `bevy_ecs::removal_detection`→`lifecycle` | `app.add_observer(|add: On<Add, Player>| info!("{}", add.entity));` — likewise `Insert`, `Replace`, `Remove`, `Despawn` |
| Component hooks (still the low-level tool) | 0.14, reorganised 0.17 | Observers when logic must run *during* mutation with `DeferredWorld`; hooks code now in `bevy_ecs::lifecycle` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#component-lifecycle-reorganization) | `#[component(on_add = my_on_add, on_remove = …)]` with `fn my_on_add(mut w: DeferredWorld, ctx: HookContext)` |
| Lifecycle observers see old/new archetypes; lifecycle event changes | 0.19 | Richer hook/observer context [0.18→0.19 guide TOC](https://bevy.org/learn/migration-guides/0-18-to-0-19/) (TOC) | Read before writing replace/remove observers on 0.19+ |
| Observer run conditions | 0.19 | Guard clauses inside every observer [0.19 news](https://bevy.org/news/bevy-0-19/#observer-run-conditions) | Attach `.run_if(…)`-style conditions to observers like systems |
| Observer `B` generic moved into event type | 0.20 | ⚠️ `On<E, B>`-style bundle generic on the observer [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#moved-observer-events-b-bundle-generic-into-the-event-type) | Update `On<Add, T>` signatures to the 0.20 encoding (compiler-guided) |
| `Message` = buffered queue | 0.17 | ⚠️ `#[derive(Event)]` + `EventReader`/`EventWriter` for buffered flow | `#[derive(Message)] struct Hit;` · write: `fn w(mut m: MessageWriter<Hit>) { m.write(Hit); }` · read: `fn r(mut r: MessageReader<Hit>) { for _ in r.read() {} }` · register: `app.add_message::<Hit>();` |
| `write_message` renames | 0.17 (send→write started 0.16) | ⚠️ `World::send_event*`, `Commands::send_event`, `Events::send*`, `command::send_event`, `SendBatchIds` (deprecated aliases) [table](https://bevy.org/learn/migration-guides/0-16-to-0-17/#rename-send-event-and-similar-methods-to-write-message) | `world.write_message(m);` `commands.write_message(m);` `Messages::write/write_default/write_batch` |
| `RemovedComponents` renamed methods | 0.17 | ⚠️ Event-named removal APIs [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#removedcomponents-methods-renamed-to-match-event-to-message-rename) | `RemovedComponents<T>` param, `.read()` messages |
| Immutable entity events | 0.18 | Mutable entity-event payloads (aliasing hazards) [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#immutable-entity-events) | Treat `On<E>` payload as read-only; mutate via `Commands`/queries |
| `Ready` event (scenes) | 0.20 | (Mis)using `Add` (top-down, children absent) for "scene fully spawned" [0.20 news](https://bevy.org/news/bevy-0-20/#ready-event) | In BSN: `on(|ready: On<Ready>| …)` — fires bottom-up after full spawn incl. descendants/deps |
| ⚠️ Exclusive systems as observers | forbidden 0.17; systems unified 0.20 | `&mut World` observers; see §5 [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#exclusive-function-systems-have-been-unified-with-regular-function-systems) | Observer systems are ordinary systems with `On<E>` first param |

Rule of thumb the guides teach: **need it handled immediately, possibly
per-entity, with propagation? `Event`+observer. Need it queued, polled,
countable, or read next frame? `Message`.** A type may derive both, rarely.

## 5. App, plugins, schedules, states

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `Plugin` + plugin groups (`DefaultPlugins`, `.set(…)`, `.disable::<P>()`) | long-standing, unchanged in window | Monolithic `App` setup | `App::new().add_plugins(DefaultPlugins.set(WindowPlugin{..default()})).run();` |
| Schedules: `PreStartup/Startup/PostStartup`, `First/PreUpdate/Update/PostUpdate/Last`, `Fixed*` | long-standing | — | `app.add_systems(Update, sys);` startup one-shots in `Startup` |
| System sets & ordering: `in_set`, `before/after`, `chain`, `ApplyDeferred` | long-standing; naming normalised 0.17 (`*Systems` suffix) [0.17 news](https://bevy.org/news/bevy-0-17/#consistent-naming-conventions-for-system-sets) | ⚠️ Inconsistently named first-party sets | `#[derive(SystemSet,…)] enum SimSystems{Step,Sync}` · `.configure_sets(Update, (SimSystems::Step, SimSystems::Sync).chain())` |
| Weak ordering `chain_weak`/`before_weak`/`after_weak` | **0.20** | Over-strict `chain()` leaving threads idle; ordering kept only on conflicting accesses (+ deferred/exclusive always ordered). Built-in schedules now order sets weakly [0.20 news](https://bevy.org/news/bevy-0-20/#weak-system-ordering-with-chain-weak), [guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/#several-built-in-schedules-now-order-their-system-sets-weakly) | `schedule.configure_sets((A, B, C).chain_weak());` — unsafe if ordering hides in interior mutability/globals |
| Schedule randomisation (`shuffle_seed`) | 0.20 | Hidden order-dependence bugs [0.20 news](https://bevy.org/news/bevy-0-20/#schedule-randomization); `ScheduleBuildSettings.shuffle_seed` [guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/#schedulebuildsettings-now-includes-a-shuffle-seed-field) | Set a seed in tests to shake out undeclared ordering |
| System combinators (`And/Or/Xor/Not` conditions etc.) | 0.18 | Hand-composed condition systems (TOC) [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#system-combinators) (TOC) | Compose `SystemCondition`s algebraically in `.run_if(…)` |
| `Condition` → `SystemCondition` | 0.17 | ⚠️ Old trait name [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#renamed-condition-to-systemcondition) | `fn cond() -> bool` used as `SystemCondition` via `.run_if(cond)` |
| Exclusive systems unified with regular systems | **0.20** | ⚠️ Separate `ExclusiveSystem`/`.exclusive_systems` machinery [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#exclusive-function-systems-have-been-unified-with-regular-function-systems) | `fn sys(world: &mut World)` added with plain `add_systems` |
| Remove systems from schedules | 0.18 | Rebuilding whole schedules [0.18 news](https://bevy.org/news/bevy-0-18/#remove-systems-from-schedules) | Schedule system-removal API at build time |
| `set_executor` replaces `ExecutorKind`; `SimpleExecutor` removed | executor: 0.19; SimpleExecutor deprecated 0.17, removed 0.18 | ⚠️ `ExecutorKind::*`, `SimpleExecutor` [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#set-executor-replaced-executorkind) | `schedule.set_executor(…)`; default multi-threaded executor otherwise |
| Fixed timestep: `FixedUpdate`, `Time<Fixed>`, `FixedMain` | long-standing (0.10+) | Physics in `Update` with raw `delta` | `app.add_systems(FixedUpdate, step_physics);` `Res<Time<Fixed>>`; tune via `Time::from_hz(…)` resource init |
| States + sub-states | states long-standing; sub-states 0.14+ | ⚠️ `State<T>` resource polling everywhere | `#[derive(States,…)] enum AppState{…}` · `app.init_state::<AppState>()` · `OnEnter(AppState::X)`/`OnExit`/`OnTransition` · `NextState::set_if_different` (⚠️ `set_if_neq` renamed 0.20 [guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/#nextstate-set-if-neq-renamed-to-set-if-different)) |
| State-scoped entities always on | 0.17 | ⚠️ `app.enable_state_scoped_entities::<S>()`, `#[states(scoped_entities)]` (both removed/deprecated no-ops) [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#state-scoped-entities-are-now-always-enabled-implicitly) | Just spawn `DespawnOnExit(AppState::Menu)` / `DespawnOnEnter(…)` |
| State-scoped renames; same-state transitions | renames 0.17; same-state 0.18, despawn-on-same 0.19 | ⚠️ `StateScoped`/`DespawnOnExitState` spellings [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#renamed-state-scoped-entities-and-events); `StateTransition` on same→same [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#same-state-transitions) | `DespawnOnEnter`/`DespawnOnExit`; don't assume enter/exit only fire on change |
| App Settings (`bevy-settings`, `SettingsPlugin`) | 0.19 | Ad-hoc config resources; this workspace pins `bevy-settings = "=0.19.1"` [0.19 news](https://bevy.org/news/bevy-0-19/#app-settings) | Derive settings groups; ⚠️ 0.20 errors on useless `SettingsGroup`s [guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/#bevy-settings-errors-on-useless-settingsgroups) |
| `RenderStartup` schedule | 0.17 | ⚠️ Creating render resources in `Plugin::finish`/`build` (many resources no longer exist there) [0.17 news](https://bevy.org/news/bevy-0-17/#renderstartup-schedule); [guide list](https://bevy.org/learn/migration-guides/0-16-to-0-17/#many-render-resources-now-initialized-in-renderstartup) | `render_app.add_systems(RenderStartup, init_my_pipeline_resource);` (0.19: `MeshPipeline*` etc. also move here) |
| Feature collections (`2d`, `3d`, `ui`) + decoupled features | collections 0.18; decoupling 0.19 | ⚠️ `3d`/`2d` implying `ui` and `audio` [0.18 news](https://bevy.org/news/bevy-0-18/#cargo-feature-collections); [0.18→0.19 audio/ui sections](https://bevy.org/learn/migration-guides/0-18-to-0-19/) | Name every domain explicitly: `features = ["3d","ui","audio"]` as needed (or the crate-level list sim-spatial uses) |
| Hot patching systems | 0.17 (experimental) | Full rebuild to tweak a system [0.17 news](https://bevy.org/news/bevy-0-17/#hot-patching-systems-in-a-running-app) | `hotpatching` feature + Dioxus subsecond tooling |

---

## 6. Transforms & hierarchy propagation

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `Transform` + `GlobalTransform` (required components pull both) | long-standing | Manual world-matrix maths | Spawn `Transform::from_xyz(…)`; read `GlobalTransform` after `PostUpdate` propagation |
| Faster propagation, static optimisations | 0.16 faster propagation; `StaticTransformOptimizations` changes 0.19 | Recomputing static subtrees [0.16 news](https://bevy.org/news/bevy-0-16/#faster-transform-propagation); 0.19 guide §StaticTransformOptimizations (TOC) | Mark static hierarchies; no threshold field any more (0.19) |
| Propagation via `ChildOf` relationship | 0.16 | ⚠️ Separate hierarchy bookkeeping | Reparent = insert `ChildOf`; propagation follows automatically |
| Component propagation (generic) | 0.17 | Hand-written "copy parent value to children" systems [0.17 news](https://bevy.org/news/bevy-0-17/#component-propagation) | Register a propagated component; framework copies it down the hierarchy |
| `UiTransform` (specialised UI transform) | 0.17 | World `Transform` for UI nodes [0.17 news](https://bevy.org/news/bevy-0-17/#specialized-ui-transform) | UI nodes use `UiTransform`/`UiGlobalTransform`, not `Transform` |
| `validate_parent_has_component` → plugin | 0.19 | ⚠️ Free function checks [0.18→0.19 guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/#the-validate-parent-has-component-is-superseded-by-validateparenthascomponentplugin) | `ValidateParentHasComponentPlugin::<T>::default()` in dev builds |

## 7. UI (`bevy_ui`, widgets, Feathers, BSN)

Direction of travel is unmistakable across 0.17→0.20: hand-spawned `Node`
trees remain valid, but new code is expected to use **headless widgets
(`bevy_ui_widgets`) for behaviour, Feathers or own styling for looks, and BSN
scenes for structure**, with interaction via **picking observers**, not
`Interaction` polling.

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `Node` layout component (flex/grid via fields) | 0.15 (`Node` replaces `Style`) and earlier line; `Node::direction` added 0.19 | ⚠️ `Style`, `NodeBundle`/`ButtonBundle` (bundles removed in the required-components era) | `commands.spawn((Node{width: percent(100), flex_direction: FlexDirection::Column, ..default()}, BackgroundColor(c)));` |
| `BorderRadius` inside `Node`; elliptical (2D) radii | folded into `Node` 0.18; 2D fields 0.20 | ⚠️ Standalone `BorderRadius` component; scalar corners [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#borderradius-has-been-added-to-node-and-is-no-longer-a-component); [0.20 news](https://bevy.org/news/bevy-0-20/#elliptical-border-radius) | `Node{border_radius: BorderRadius::all(Val::Px(8.0)), ..default()}` |
| Per-side border colours, gradients | 0.17 | Single border colour, texture hacks [0.17 news](https://bevy.org/news/bevy-0-17/#ui-gradients) | `BorderColor` per-side constructors; gradient background nodes |
| `Val` helpers + `Val::Em`/`Val::Rem` | helpers 0.17; Em/Rem 0.20 | Raw `Val::Px` everywhere; font-relative sizing [0.20 news](https://bevy.org/news/bevy-0-20/#val-em-and-val-rem) | `px(12)`, `percent(50)`, `em(10)`, `rem(1.5)`; `EmSize` component / `RemSize` resource |
| `FixedNode` | 0.20 | Absolutely-positioned overlay hacks [0.20 news](https://bevy.org/news/bevy-0-20/#fixednode) | Layout-exempt fixed-position UI (HUD overlays) |
| `ScrollPosition` (logical px, not clobbered by layout) | 0.17 | ⚠️ Physical-unit scroll, layout overwriting it [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#scrollposition-now-uses-logical-pixel-units-and-is-no-longer-overwritten-during-layout-updates) | Write `ScrollPosition(Vec2)` freely; 0.18: nodes may ignore parent scroll [0.18 news](https://bevy.org/news/bevy-0-18/#ui-nodes-can-ignore-parent-scroll-position) |
| `ComputedNode`/`ComputedStackIndex` etc. | stack split 0.19 | ⚠️ `ComputedNode::stack_index` field [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#computednode-stack-index-has-been-replaced-by-computedstackindex) | Read computed layout from the split components; never write them |
| Text: `Text`, `TextFont`, `TextColor`, sections | `TextSection` model 0.19 (replaces `TextRoot`/`TextSpan*`); Parley backend 0.19 | ⚠️ cosmic-text re-exports (removed 0.17), `TextBundle`, span components [0.18→0.19 Parley + TextSection sections](https://bevy.org/learn/migration-guides/0-18-to-0-19/#bevy-text-migration-from-cosmic-text-to-parley) | `commands.spawn((Text::new("hi"), TextFont::from_font_size(20.0)));` rich runs via `TextSection`s; `LineHeight` own component (0.18) |
| Rich text: variations, backgrounds, inline boxes/images | font variations + pickable sections 0.18; richer text 0.19; `InlineBox`/`InlineImage` 0.20 | Multiple text entities for mixed styling [0.18 news](https://bevy.org/news/bevy-0-18/#font-variations); [0.20 news](https://bevy.org/news/bevy-0-20/#inlinebox-and-inlineimage) | Inline entities flowed in text layout; `FontSource` generic families (0.20) |
| `TextFont` defaults & constructors | `From` impls 0.17; fields changed 0.19; default `FontSize::Rem(1.)` 0.20 | ⚠️ `TextFont::from_font(…)`-era constructors, px default [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#textfont-default-font-size-is-now-fontsize-rem-1) | `TextFont::from_font_size(20.0)`; set `RemSize` for scalable default |
| Text input: `EditableText` + `TextInput` | text input 0.19; split into two components 0.20; Escape releases focus 0.20 | Hand-rolled `ReceivedCharacter` editors; ⚠️ `TextScroll` → `EditableText::viewport` [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#editabletext-and-textinput-are-now-separate-components) | Spawn `TextInput` (behaviour) with `EditableText` (buffer/viewport) |
| Headless widgets (`bevy_ui_widgets`) | experimental 0.17; **stable + in `DefaultPlugins` 0.19** (`UiWidgetsPlugins`, `InputDispatchPlugin`) | ⚠️ `Interaction`-polling `Button` logic; custom slider maths. `Core*` prefix dropped 0.19 [0.17 news](https://bevy.org/news/bevy-0-17/#headless-bevy-ui-widgets-experimental); [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#uiwidgetsplugins-and-inputdispatchplugin-are-now-in-defaultplugins) | `commands.spawn((Node{..}, Slider{…}, …)).observe(|v: On<ValueChange<f32>>| …);` tabs headless in 0.20 |
| ⚠️ `ui::widgets::Button` + `Interaction` deprecated | 0.20 | The old button component and polled interaction state [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#ui-widgets-button-and-ui-interaction-are-deprecated) | Headless `Button` from `bevy_ui_widgets` + picking observers (`On<Pointer<Press>>`…) |
| Feathers (editor widget kit) | experimental 0.17; non-experimental 0.19; many widgets + contextual theming 0.20 | Rolling own tool UI; `FeathersPlugin`→`FeathersCorePlugin` (0.19) [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#the-experimental-bevy-feathers-feature-is-no-longer-experimental); [0.20 news](https://bevy.org/news/bevy-0-20/#contextual-theming) | `FeathersCorePlugin` + `feathers::*` controls; theme via `ThemeContext`/`SemanticToken`. Workspace deliberately does **not** use it (`crates/sim-spatial/Cargo.toml:25-27`) |
| Retained UI rendering | 0.20 | Extract-every-frame UI [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#ui-rendering-is-now-retained) | Mostly internal; `CalculatedClip` now stores transformed rects |
| `ViewportNode` | 0.17; `camera: Option<Entity>` 0.19 | Manual render-to-texture in UI [0.17 news](https://bevy.org/news/bevy-0-17/#viewportnode) | Embed a camera view inside UI layout |
| Focus: `InputFocus`, directional navigation | input-focus crate 0.16-era; automatic directional nav 0.18; `InputFocus` fields private 0.19; `FocusCause::Auto` 0.20 | ⚠️ `bevy::a11y::Focus` [0.15→0.16](https://bevy.org/learn/migration-guides/0-15-to-0-16/#replace-bevy-a11y-focus-with-bevy-input-focus-inputfocus); manual tab order [0.18 news](https://bevy.org/news/bevy-0-18/#automatic-directional-navigation) | `ResMut<InputFocus>` set/clear via methods; `TabIndex`/`TabGroup` for order; arrow-key nav automatic |
| BSN scenes (`bsn!`, `SceneComponent`, `.spawn()`) | **0.19** (no `.bsn` asset loader yet); syntax improved 0.20 | Imperative spawn trees for UI/scenes [0.19 news #next-generation-scenes](https://bevy.org/news/bevy-0-19/#next-generation-scenes); [0.20 news](https://bevy.org/news/bevy-0-20/#bsn-syntax-improvements) | `fn level() -> impl SceneList { bsn_list![Camera2d, Sprite{image:"p.png"}] }` + `.add_systems(Startup, level.spawn())`; `#[derive(SceneComponent)] #[scene("player.bsn")]`-ready types; props via `@Type { @prop: v, field: v }`; 0.20: no `template_value`, enums/chained methods work |

## 8. Input & picking

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `ButtonInput<KeyCode>` / `ButtonInput<MouseButton>` / `ButtonInput<Key>` | `Key` variant 0.17 | ⚠️ `Input<T>` (long renamed); scancode-only logic [0.17 news](https://bevy.org/news/bevy-0-17/#buttoninput-for-key) | `keys.just_pressed(KeyCode::Space)`; layout-aware text keys via `ButtonInput<Key>` |
| Message-based raw input | 0.17 split | ⚠️ `EventReader<KeyboardInput>` etc. | `MessageReader<KeyboardInput>`, `MessageReader<MouseMotion>`, `MessageReader<TouchInput>`… |
| Input sources behind features | 0.18 | Always-on gilrs etc. [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#put-input-sources-for-bevy-input-under-features) | Enable only used sources (`keyboard`, `mouse`, `gamepad`, …) — sim-spatial already does |
| Gamepad via `bevy_gilrs` entities | entity model pre-window; NonSend replaced 0.17 | ⚠️ `Gilrs` NonSend resource [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#replace-gilrs-accesskitadapters-and-winitwindows-non-send-resources) | Query `(&Gamepad, &GamepadSettings)` per gamepad entity |
| Picking: `bevy_picking` core + backends | first-party 0.15; `PickingSettings`/`PointerInputSettings` extracted 0.17 | `bevy_mod_picking`, manual raycasts for UI/mesh hover [0.16→0.17 settings sections](https://bevy.org/learn/migration-guides/0-16-to-0-17/) | Features `mesh_picking`/`ui_picking`/`sprite_picking`; entities are pickable by default (`Pickable` to tune) |
| Pointer observers (`Pointer<E>`) | renames 0.17: `Press`/`Release`; **flat pointer events 0.20** | ⚠️ `Pointer<Pressed>`/`Pointer<Released>` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#rename-pointer-pressed-and-pointer-released-to-pointer-press-and-pointer-release); ⚠️ nested `Pointer<…>` wrapper → flat events [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#flat-pointer-events) | `.observe(|ev: On<Pointer<Click>>| …)` (0.19 form) → flat `On<Click>`-style in 0.20; `ev.original_event_target()` |
| Drag events carry dragged entity; `DragEnter` on drag start | 0.17 / 0.18 | Inferring drag source [guides](https://bevy.org/learn/migration-guides/0-17-to-0-18/#dragenter-now-fires-on-drag-starts) | Read dragged entity from the event payload |
| Pickable text sections; non-text area not pickable | 0.18 | Whole-text-node hit boxes [0.18 news](https://bevy.org/news/bevy-0-18/#pick-able-text-sections); guide §non-text areas | Per-section picking for links/selection |
| `RelativeCursorPosition` object-centred | 0.17 | ⚠️ Corner-origin semantics [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#relativecursorposition-is-now-object-centered) | Normalised coords centred on the node |
| Cursor: `CursorOptions` component, custom cursor | split from `Window` 0.17; cursor module `bevy_feathers`→`bevy_picking` 0.20 | ⚠️ `window.cursor_options` field [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#window-is-now-split-into-multiple-components) | `Single<&mut CursorOptions, With<PrimaryWindow>>` |

---

## 9. Rendering, cameras, materials, gizmos

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| Crate split decoupled from `bevy_render` | 0.17 (`bevy_camera`, `bevy_light`, `bevy_mesh`, `bevy_shader`, `bevy_image`, `bevy_color`, `bevy_material` 0.19, `bevy_ui_render`, `bevy_gizmos_render`, `bevy_shape`/`bevy_curve` 0.20) | `bevy_render` monolith imports [0.17 news](https://bevy.org/news/bevy-0-17/#decoupling-bevy-s-public-api-from-bevy-render); [0.18→0.19 `bevy_material`](https://bevy.org/learn/migration-guides/0-18-to-0-19/#new-crate-bevy-material); [0.19→0.20 shape/curve](https://bevy.org/learn/migration-guides/0-19-to-0-20/#bevy-shape-split-out-from-bevy-math) | Depend/import the narrow crate; sim-spatial feature list mirrors this split |
| Render graph **as systems** | **0.19** | `Node` trait + `RenderLabel` + graph edges [0.19 news](https://bevy.org/news/bevy-0-19/#render-graph-as-systems) | `render_app.add_systems(Core3d, my_pass.after(Core3dSystems::MainPass));` with `fn my_pass(mut ctx: RenderContext, …)` |
| `RenderSystems` set splits | `ManageViews`→3 sets 0.19; weak ordering 0.20 | Coarse render ordering [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#rendersystems-manageviews-has-been-split-into-three-system-sets) | Order against the fine-grained `RenderSystems::*` |
| Cameras as components: `Camera3d`/`Camera2d`, `Projection`, `RenderTarget` component | `RenderTarget` component 0.18; `Hdr` split from `Camera` 0.17, moved to `bevy_camera` 0.19 | `Camera3dBundle`; `camera.hdr` field [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#split-hdr-from-camera); [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#rendertarget-is-now-a-component) | `commands.spawn((Camera3d::default(), Hdr, Tonemapping::TonyMcMapface, Transform::…));` |
| First-party camera controllers | 0.18 (`PanCamera` etc.); pan-orbit 0.20; mouse panning 0.19 | Every project rewriting orbit/pan [0.18 news](https://bevy.org/news/bevy-0-18/#first-party-camera-controllers); [0.20 news](https://bevy.org/news/bevy-0-20/#pan-orbit-camera) | Add the controller component to the camera entity |
| `Tonemapping`/`DebandDither` in `bevy_render::view`; `Tonemapping::None` passthrough | 0.20 | Old module paths; partial passthrough [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#tonemapping-and-debanddither-moved-to-bevy-render-view) | Update imports; `None` = full passthrough |
| Post-processing split + more effects; `FullscreenMaterial` | split 0.19; fullscreen material 0.18 | Monolithic core-pipeline effects, custom nodes for fullscreen passes [0.18 news](https://bevy.org/news/bevy-0-18/#fullscreen-material); [0.18→0.19 post-processing + FullscreenMaterial sections](https://bevy.org/learn/migration-guides/0-18-to-0-19/) | Per-effect components on camera (`Bloom`, …); derive `FullscreenMaterial` for custom passes |
| Materials: `Material`/`MaterialPlugin`, `MeshMaterial3d`, extended/data-driven | data-driven 0.17; 2D extended + sprite materials 0.20; `enable_prepass/shadows` are `Material` methods 0.18 | `Handle<StandardMaterial>` component directly (pre-0.15 habit) [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#enable-prepass-and-enable-shadows-are-now-material-methods); [0.20 news](https://bevy.org/news/bevy-0-20/#2d-extended-materials) | `commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat)));` custom: `#[derive(Asset, AsBindGroup, …)]` + `MaterialPlugin::<M>::default()` |
| Sprite = `Mesh2d` + `SpriteMaterial` | **0.20** | Dedicated sprite pipeline/`Sprite` rendering path; backend unified [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#migrate-sprite-to-use-mesh2d-spritematerial); [0.20 news](https://bevy.org/news/bevy-0-20/#sprite-materials) | Migrate sprite entities per guide; custom sprite shaders via sprite materials |
| Meshes: `Mesh3d`/`Mesh2d`, `MeshTag::new(x)`, auto AABB | tags 0.16, ctor 0.20; auto AABB 0.18; `MeshAabb::get_aabb` 0.20 | `MeshTag(x)` tuple ctor; manual `Aabb` upkeep; `compute_aabb` name [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#meshtag-x-is-now-meshtag-new-x) | `MeshTag::new(3)`; prefer `Mesh::try_*` when mesh may be render-world-only (0.18) |
| GPU-driven rendering, virtual geometry, occlusion culling | GPU-driven 0.16; VG BVH 0.17; occlusion experimental 0.16 → non-experimental 0.19 | CPU draw submission for huge scenes [0.16 news](https://bevy.org/news/bevy-0-16/#gpu-driven-rendering) | Opt-in components/settings; no code change for standard scenes |
| Solari (raytraced GI) + DLSS | Solari experimental 0.17, improved 0.18–0.20; DLSS 0.17/0.20 | Baked lightmaps only [0.20 news](https://bevy.org/news/bevy-0-20/#solari-and-dlss) | `Solari*` components/plugins behind features; treat as experimental |
| Atmosphere as entity in `bevy_light`; multi-camera | entity 0.19; multi-camera 0.20; generalised media 0.18 | Resource-style atmosphere [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#atmosphere-is-now-an-entity) | Spawn `Atmosphere` entity; `Skybox` also in `bevy_light`, image optional |
| Lights: clustered, light textures, rect area, contact shadows | light textures 0.17; rect area + contact shadows 0.19; `AmbientLight` split component/resource 0.18 | Single ambient resource assumption [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#ambientlight-split-into-a-component-and-a-resource) | Per-camera `AmbientLight` component overrides global resource |
| Order-independent transparency per-material opt-out; opt-in transmission | 0.20 | Global OIT behaviour; always-on transmission cost [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#opt-in-screenspacetransmission) | `ScreenSpaceTransmission` component where needed |
| WESL shaders (supersedes Bevy WGSL extensions) | experimental 0.16 → standard path 0.20; mesh shaders 0.20 | `#import` Bevy-extended WGSL [0.20 news](https://bevy.org/news/bevy-0-20/#wesl-shaders); guide §WESL; `view_transformations.wgsl` deprecated 0.17 → `bevy_render::view` | Author `.wesl`; in WGSL imports use `view::position_view_to_world(pos, view_bindings::view.world_from_view)` |
| Gizmos: immediate + retained; text gizmos | retained 0.16; text 0.19; gizmos render split 0.18; `cuboid`→`cube` 0.18 | Retained gizmo meshes by hand [0.16 news](https://bevy.org/news/bevy-0-16/#retained-gizmos); [0.19 news](https://bevy.org/news/bevy-0-19/#text-gizmos) | `gizmos.cuboid(…)`→`gizmos.cube(…)`; `GizmoAsset` retained; `gizmos.text_2d(Isometry2d::IDENTITY,"hi",40.,Vec2::ZERO,Color::WHITE)` (ASCII debug only) |
| `RenderDebugOverlay` in DefaultPlugins | 0.20 | Manual debug overlay plugin [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#renderdebugoverlay-added-to-default-plugins-and-now-has-optional-keybindings) | Toggle via its settings/keybindings |
| Easy screenshot / video | screenshot+recording 0.18 | Manual readback code [0.18 news](https://bevy.org/news/bevy-0-18/#easy-screenshot-and-video-recording) | `Screenshot` component + observers; `EasyScreenRecordPlugin` (output_dir field 0.19) |
| Visibility: `Visibility`/`InheritedVisibility`/`ViewVisibility`, `RenderLayers` | long-standing | — | Toggle `Visibility::Hidden`; camera/layer masks via `RenderLayers` |

## 10. Assets

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `AssetServer::load` + `Handle<T>`; builder for advanced loads | builder 0.19 | Many `load_*` variants [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#advanced-assetserver-load-variants-are-now-exposed-through-a-builder-pattern) | `let h: Handle<Image> = assets.load("a.png");` advanced: load builder with settings/meta transforms |
| `Handle::Uuid` (not `Weak`) | 0.17 | `Handle::Weak`, `weak_from_u128` (deprecated 0.16) [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#handle-weak-has-been-replaced-by-handle-uuid) | Persist `AssetId`/uuid handles; `handle.untyped()`/`typed()` conversions |
| Serialisable handles | 0.19 | Path-string bookkeeping [0.19 news](https://bevy.org/news/bevy-0-19/#serializing-and-deserializing-asset-handles) | Serde/reflect round-trip of `Handle<T>` in scenes/saves |
| `Assets::insert` returns `Result`; `AssetEvent` messages | 0.17; event hygiene 0.19 | Infallible insert; spurious `Modified` storms [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#assets-insert-and-assets-get-or-insert-with-now-return-result) | `meshes.insert(&h, mesh)?;` read `MessageReader<AssetEvent<Mesh>>` |
| `AssetChanged<T>` query filter | 0.16 | Polling asset versions [0.16 news](https://bevy.org/news/bevy-0-16/#assetchanged-query-filter) | `Query<…, AssetChanged<Image>>` |
| Processed assets: no auto `.meta`; `Process`/`AssetLoader` need `TypePath`; short type paths | 0.16 meta; 0.18 traits/paths | Auto-generated meta noise [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#traits-assetloader-assettransformer-assetsaver-and-process-all-now-require-typepath) | `AssetPlugin{mode: AssetMode::ProcessedDev, …}`; derive `TypePath` on loaders/processors |
| Asset saving (`AssetSaver`, `SavedAsset`) | saving story 0.19; signature 0.19 | Write-only asset pipeline [0.19 news](https://bevy.org/news/bevy-0-19/#asset-saving) | Implement `AssetSaver` taking `AssetPath`; compressed saver improved 0.20 |
| Custom sources/readers: seekable, async_channel, required reader | 0.18 | `crossbeam_channel` senders, `AsyncSeekForward` [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/) | `Reader::seekable`; register source with reader + watcher |
| Web assets (HTTP source) | 0.17 | Bundling everything [0.17 news](https://bevy.org/news/bevy-0-17/#web-assets) | `http://`/`https://` asset source behind feature |
| Hot reload | long-standing | — | `AssetPlugin{watch_for_changes_override: Some(true), …}` / `use_asset_processor_override` (0.18) in dev |
| Texture compression/mipmaps automatic | 0.20 | Manual basis/mip pipelines [0.20 news](https://bevy.org/news/bevy-0-20/#better-texture-compression) | Loader settings; `Image::pixel_bytes()` returns `Result` (0.19) |
| glTF: extensions, material names, forward config, coord conversion | names/forward 0.17; extensions 0.18; conversion 0.18; dep inverted with `bevy_pbr` 0.19 | Forked loaders [0.18 news](https://bevy.org/news/bevy-0-18/#gltf-extensions) | `GltfPlugin`/loader settings; scene spawn via `SceneRoot(assets.load("a.gltf#Scene0"))` |

## 11. Reflection, scenes, serialisation

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `#[derive(Reflect)]` + `#[reflect(Component)]`, app registration | long-standing; auto-registration 0.17 | Manual `register_type` lists everywhere [0.17 news](https://bevy.org/news/bevy-0-17/#reflect-auto-registration) | `app.register_type::<T>();` or auto-register feature; `#[reflect(...)]` parens-only syntax (0.18) |
| `bevy_reflect` root decluttered; `CreateTypeData` | 0.19 declutter; `FromType`→`CreateTypeData` 0.20 | Flat imports; `FromType` name [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#bevy-reflect-reorganized-to-de-clutter-the-crate-root) | Import from submodules; `PartialReflect::to_dynamic()` now `Result` (0.20) |
| Old `bevy_scene` → `bevy_world_serialization`; new `bevy_scene` = BSN | **0.19** | Name collision resolved [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#the-old-bevy-scene-is-now-bevy-world-serialization) | RON world snapshots: `bevy_world_serialization`; BSN: `bevy_scene` |
| `DynamicScene`/`DynamicSceneBuilder` need `&TypeRegistry` | 0.19 | Ambient registry [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#dynamicscenebuilder-and-dynamicscene-from-scene-now-require-a-typeregistry) | Pass `app.world().resource::<AppTypeRegistry>()` read guard explicitly |
| `SceneSpawner` renamed methods | 0.17 | Old spawner names [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#scenespawner-methods-have-been-renamed-and-replaced) | Follow compiler to new `spawn_dynamic_*`/`despawn_*` names; scene entities via `SceneRoot`/`DynamicSceneRoot` components (0.15+ model) |
| BSN `Scene`/`SceneList`/`SceneComponent` | 0.19, syntax 0.20 | `DynamicScene` RON as authoring format | See §7 BSN row; `.bsn` file loader not shipped in 0.19/0.20 (macro `bsn!` only + proposals) |
| Reflected functions (overloading) | 0.16 | — [0.16 news](https://bevy.org/news/bevy-0-16/#reflection-function-overloading-generic-variadic-functions) | Register `Function` traits for scripting/BRP |
| Bevy Remote Protocol (BRP) | pre-window, evolving; `schedule.graph` exposes accesses 0.20 | Custom debug protocols [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#expose-system-accesses-and-filters-in-brp-schedule-graph) | `RemotePlugin` + `bevy_brp` tooling; mind Resources-as-Components in queries (0.19) |


## 12. Tasks, diagnostics, logging

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| Task pools: `AsyncComputeTaskPool`, `ComputeTaskPool`, `IoTaskPool` | long-standing | `std::thread` per job, external executors in systems | `let task = AsyncComputeTaskPool::get().spawn(async move { … });` store `Task<T>` in a component, poll with `future::poll_once` |
| Scoped parallelism: `ComputeTaskPool::scope`, `par_iter` on queries | long-standing | Sequential iteration of large queries | `query.par_iter_mut().for_each(|…| …);` (respects pool; `multi_threaded` feature) |
| Cancellable web tasks (`web-task`) | 0.19 | Dropped tasks leaking on WASM [0.19 news](https://bevy.org/news/bevy-0-19/#cancellable-web-tasks) | No API change: dropping `Task` now cancels on web too; guide §Dropping Tasks in Web Builds |
| Diagnostics: `DiagnosticsPlugin` family, overlay | overlay 0.19; frame-time graph 0.17; history config 0.16 | Printf FPS [0.19 news](https://bevy.org/news/bevy-0-19/#diagnostics-overlay); [0.17 news](https://bevy.org/news/bevy-0-17/#frame-time-graph) | `FrameTimeDiagnosticsPlugin::default()` + `LogDiagnosticsPlugin` or on-screen `DiagnosticsOverlay` |
| Logging: `LogPlugin`, `tracing` spans, GPU timestamps | GPU timestamps 0.16 | `println!` debugging [0.16 news](https://bevy.org/news/bevy-0-16/#gpu-timestamps-in-traces) | `LogPlugin{filter:"…,my_crate=debug".into(), ..default()}`; `info!`/`warn!` from `bevy::log` |
| Interactive transform gizmo, infinite grid (dev tools) | 0.19 | Editor-less scene poking [0.19 news](https://bevy.org/news/bevy-0-19/#interactive-transform-gizmo) | Dev-only plugins; do not ship in release UI |

## 13. Window, winit, accessibility

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `Window` split into components (`Window`, `CursorOptions`, …) | 0.17 | ⚠️ One giant `Window` struct incl. `cursor_options` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#window-is-now-split-into-multiple-components) | `WindowPlugin{primary_window: Some(Window{..}), primary_cursor_options: Some(CursorOptions{..}), ..default()}`; query each component |
| Cursor types in `bevy_window` (was `bevy_winit`) | 0.17 | ⚠️ Old paths [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#move-cursor-related-types-from-bevy-winit-to-bevy-window) | `bevy::window::{CursorGrabMode, CursorOptions, …}` |
| `WinitWindows`/`AccessKitAdapters` NonSend replaced | 0.17 | ⚠️ NonSend resources for platform handles — now NonSend *Data* components on entities (0.19 model) [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#replace-gilrs-accesskitadapters-and-winitwindows-non-send-resources) | Use the entity-component APIs; mark systems `NonSend` only when touching them |
| Winit user events removed | 0.18 | ⚠️ `EventLoopProxy` user-event wakeups [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#winit-user-events-removed) | Use Bevy messages/channels + `WinitSettings` wake policies |
| Wayland on by default; exit systems in `Last` | 0.17 / 0.19 | X11-only assumptions; early exit handling [guides](https://bevy.org/learn/migration-guides/0-18-to-0-19/#windowplugin-exit-systems-moved-to-last) | Keep both `x11`/`wayland` features unless shipping single-backend |
| macOS activation follows `Window::focused`; render-world window data via ECS | 0.20 | Surprise focus steals; side-channel window data [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#use-ecs-for-render-world-window-data) | Set `focused: false` for background tools |
| Accessibility: `AccessibleLabel`, AccessKit via input-focus/widgets | label 0.19 | Unlabelled custom widgets [0.19 news](https://bevy.org/news/bevy-0-19/#accessible-label-component) | Add `AccessibleLabel` + headless widget semantics; IME handled by `TextInput`/winit (`Window::ime_enabled` area — verify per-app, not covered by a guide section read here) |

## 14. Math, colour, miscellany

| Pattern | Since | Replaces / for | Canonical usage |
|---|---|---|---|
| `bevy_math` splits: `bevy_shape`, `bevy_curve` | 0.20 | ⚠️ `bevy::math::{primitives, curve}` paths [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#bevy-curve-split-out-from-bevy-math) | Update imports; curve derivatives since 0.16 |
| `Color` API; `with_luminance` no clamp | 0.20 behaviour | ⚠️ Clamping assumption [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#with-luminance-no-longer-clamps-in-bevy-color) | Clamp explicitly if needed; colour/layout interpolation helpers since 0.18 |
| `Timer::{is_paused, is_finished}` | 0.17 | ⚠️ `paused()`/`finished()` [0.16→0.17](https://bevy.org/learn/migration-guides/0-16-to-0-17/#renamed-timer-paused-to-timer-is-paused-and-timer-finished-to-timer-is-finished) | `timer.is_finished()`; prefer delayed commands (0.19) for one-shot futures |
| `Name` from `&'static str` only | 0.20 | ⚠️ `Name::from(&non_static_str)` [0.19→0.20](https://bevy.org/learn/migration-guides/0-19-to-0-20/#strs-must-now-have-a-static-lifetime-to-be-converted-to-name) | `Name::new(owned_string)` for dynamic names |
| `no_std` support; Rust 2024 edition | 0.16 | std-only assumptions [0.15→0.16 guide](https://bevy.org/learn/migration-guides/0-15-to-0-16/) | `default-features = false` + explicit features (workspace already on edition 2024) |
| Animation: `AnimationTarget` split, target rename | 0.18 | ⚠️ Monolithic `AnimationTarget`, `animation_player` field [0.17→0.18](https://bevy.org/learn/migration-guides/0-17-to-0-18/#animationtarget-replaced-by-separate-components) | Separate target components; `AnimationEventTrigger::target` (workspace disables animation features, listed for completeness) |
| Audio: explicit `audio` feature, rodio 0.22 | 0.19 | ⚠️ `audio` implied by `2d/3d/ui` [0.18→0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/#audio-feature-is-now-no-longer-implied-by-the-3d-2d-or-ui-features) | Opt in explicitly; `AudioSinkPlayback::toggle_playback()` (renamed 0.16) |

## 15. What breaks going 0.19.1 → 0.20 (the pinned-code watchlist)

From the [0.19→0.20 guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/)
headings read in full TOC order; highest-risk for a UI-heavy viewer like
sim-spatial first:

1. **Interaction model:** `ui::widgets::Button` + `Interaction` deprecated;
   flat pointer events replace `Pointer<E>` nesting; observer `B` generic
   moved into event types — every `.observe(…)` signature may need edits.
2. **Text:** `EditableText`/`TextInput` split, `TextScroll` gone,
   `TextReader` changed for inline boxes, `Font::from_bytes` loses family
   arg, `resolve_font_source` removed, `TextFont::default()` now `Rem(1.)`.
3. **Sprites/2D:** `Sprite` → `Mesh2d + SpriteMaterial` migration; sprite
   backend unified (less relevant: sim-spatial is 3D/UI, but chart/2D code
   should be checked).
4. **Systems:** exclusive systems unified (remove exclusive registration);
   built-in schedules weakly ordered (audit any reliance on strict set
   ordering — relevant to `ordering_guard.rs`-style tests); `NextState::
   set_if_neq` → `set_if_different`; `WorldQuery` loses default impls
   (manual `WorldQuery` impls break); `DeferredWorld::query` deprecated.
5. **Rendering imports/types:** `Tonemapping`/`DebandDither` moved;
   `MeshTag::new`; `MeshAabb::get_aabb`; `BindGroupBuilder` (was
   `UnpreparedBindGroup`); `bevy_shape`/`bevy_curve` splits; WESL for custom
   shaders; `WgpuWrapper` removed; stencil-related depth type changes.
6. **ECS data:** `Entity::PLACEHOLDER` → `Option<Entity>` in places;
   `FilteredResources` deprecated; `FromType` → `CreateTypeData`;
   `PartialReflect::to_dynamic` fallible; `ComponentId` consts type-safer;
   `AssetId::invalid()` deprecated; `UntypedHandle::meta_transform` deleted.
7. **UI metrics:** `Val::Em/Rem` available; `BorderRadius` fields 2D;
   `CalculatedClip` transformed rects; retained UI rendering (custom UI
   extraction code breaks); `FeathersNumberInput` uses `NumberInputValue`.
8. **Settings:** `bevy-settings` errors on useless `SettingsGroup`s —
   workspace uses `bevy-settings = "=0.19.1"` (`crates/sim-spatial/Cargo.toml:10`),
   bump in lockstep.
9. **Windowing:** macOS activation follows `Window::focused`; render-world
   window data via ECS (custom extract code breaks).

## 16. Reading order for an engineer upgrading or writing new Bevy code here

1. §4 of this file + [0.17 Event/Observer overhaul](https://bevy.org/news/bevy-0-17/#event-observer-overhaul) — the Event/Message/Observer vocabulary everything else uses.
2. [0.18→0.19 Resources as Components](https://bevy.org/learn/migration-guides/0-18-to-0-19/#resources-as-components) — the data model of the pinned version.
3. [0.19 Next Generation Scenes](https://bevy.org/news/bevy-0-19/#next-generation-scenes) + [0.20 BSN/Ready](https://bevy.org/news/bevy-0-20/#bsn-syntax-improvements) — where UI/scene authoring is going; evaluate before writing more imperative spawn trees.
4. [0.19→0.20 migration guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/) top-to-bottom when bumping the pin; then [docs.rs bevy 0.20.0](https://docs.rs/bevy/0.20.0/bevy/) for exact signatures.
5. `crates/sim-spatial/Cargo.toml:18-27` — this workspace's own record of which split-crate features it intentionally enables/excludes; keep it in sync on any bump.

*Unresolved / not inspected: guide sections beyond the ~100k-char fetch
window on the 0.16→0.17 and 0.18→0.19 pages were verified by heading + the
matching release post only (marked TOC inline); GitHub release pages were not
fetched separately because docs.rs + bevy.org/news + lib.rs agree on
0.20.0 / 2026-10-08; no sim-spatial source audit was performed (out of scope —
this file is the reference list, not the audit).*
