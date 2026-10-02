# Bevy 0.19.1 practice for this codebase

The workspace pins Bevy 0.19.1, and nothing is compiled during normal work. Most
Bevy code in model training data predates 0.17, so the main risk is writing an
old API that only a compiler would catch. These rules apply to every role:
the Director when ranking epics, the orchestrator when assigning and reviewing,
and the worker and its subagents when writing and reviewing code.

Authority, highest first:
1. `docs/architecture/native-viewer.md`: this project's decided shape.
2. This file.
3. The pinned source: `~/.cargo/registry/src/*/bevy_*-0.19.1/src`. Read the
   signature before you use any Bevy API; don't write one from memory. Official
   examples are at `github.com/bevyengine/bevy/tree/v0.19.1/examples`.
4. The user's full study guide, `/Users/elliot/Projects/bevy-practice/README.md`.
   It's written for games; where it differs from 1 or 2, they win.

## Never write these (pre-0.19 patterns)

Each was checked against the 0.19.1 source: the old name is gone.

| Old | Current |
|---|---|
| `EventReader` / `EventWriter`, `add_event` | `MessageReader` / `MessageWriter`, `app.add_message::<M>()`, `#[derive(Message)]` |
| Observer `Trigger<E>`, `trigger_targets` | `On<E>`; `Event` / `EntityEvent`; the current trigger APIs |
| `despawn_recursive()` | `despawn()` (follows linked relationships) |
| `StateScoped(s)` | `DespawnOnExit(s)` / `DespawnOnEnter(s)` |
| `NodeBundle`, `TextBundle`, `PbrBundle` and other `*Bundle` structs | The defining component; its required components fill defaults. Add explicit components to override. (`ui_kit::widgets::TextBundle` is our own tuple alias, not Bevy's.) |
| `Parent`, hand-kept child lists | `ChildOf` / `Children` |
| `.add_system`, `.system()`, stages, base sets | `add_systems(Schedule, ...)` with system sets and `run_if` |
| `query.single()` panicking | `single()` / `single_mut()` return `Result<_, QuerySingleError>`. Handle absence deliberately, or use the `Single<D>` param (the system is skipped unless exactly one entity matches). |
| `#[derive(Component, Resource)]` on one type | `Resource` is a `Component` in 0.19; derive `Resource` only. Use separate types for per-entity and global data. |
| Assuming `World::clear_entities()` keeps resources | It clears them in 0.19. Clean up deliberately. |
| `RenderGraph` nodes and edges | Render schedules and systems in the render world |
| `DynamicScene` for world serialization | `bevy_world_serialization`. The new `bevy_scene` (BSN) is for composition. |
| Old text and font layout types | 0.19 Parley text APIs; follow the migration guide |

## Engine rules (true regardless of style)

- **Registration order is not execution order.** Unordered systems run in any
  order or in parallel. Order by system sets or `.chain()` wherever dataflow
  needs it, and leave independent work unordered.
- **Commands are deferred.** A spawn or insert is not visible to queries in the
  same system, and an observer triggered through `Commands` runs when the
  commands are applied. A consumer that must see a producer's spawns in the
  same frame needs an ordering edge.
- **Messages expire** after about two `Messages::update` calls. A reader gated
  off by `run_if` for longer misses them. Work that must not be lost goes in
  state or an explicit queue, not a message.
- **Resources are components.** A broad entity query can now match resource
  entities. Narrow it, using `Without<IsResource>` if needed.
- **Change detection** fires on any mutable dereference, not on a changed
  value. Use `set_if_neq` to avoid false changes. `Changed<T>` merges several
  changes into one and still scans the archetype. If every change matters, use
  a message or a history.
- **Required components** are filled in at insertion only. They don't stop
  later removal, and they don't receive the spawner's field values.
- **`ChildOf`** brings transform and visibility propagation and linked despawn.
  A custom relationship gets neither unless declared (`linked_spawn`). Never
  edit a relationship target's collection by hand.
- **`GlobalTransform`** is valid only after propagation. A same-frame reader
  must run after it.
- **Assets:** a handle from `AssetServer::load` is not a loaded asset. Keep
  strong handles, and don't `Assets::add` unchanged content every frame.
- **The main and render worlds are separate.** Main-world data reaches render
  systems only through extraction.
- **Signature limits:** a system takes at most 16 parameters (nest tuples, or
  use a `SystemParam` derive), a query's data tuple at most 15 elements, and a
  `ParamSet` at most 8.
- **Disjoint access:** two queries that could alias mutably must be proven
  disjoint with `With` / `Without`, or wrapped in a `ParamSet`. Don't use
  `&mut World` or `EntityMut` just to avoid stating access; it serializes the
  schedule.
- **Colors:** use the typed constructors (`Color::srgb(..)` for display
  colors) and do lighting math in linear space.

## How Bevy's constructs map to this codebase

Where general Bevy advice differs from this table, the table wins. Reasons are
in `native-viewer.md`.

| Construct | Here |
|---|---|
| `States` | `ViewerMode` with the computed states `ModeScope` and `SpatialScreen`. Setup goes in `OnEnter`; teardown uses `DespawnOnExit<ModeScope>`. Per-object status is a component enum, not a state. |
| System sets | One pipeline, `ViewerSet`: Input → Actions → JobResults → SimSync → Present. A feature orders itself only against public sets (`ViewerSet`, `camera::CameraSet`), never against another feature's private systems. |
| Plugins | One plugin per feature, in its own folder (`mod.rs`, `plugin.rs`, `actions.rs`, `ui.rs`, `jobs.rs`). Files over about 800 lines are a smell. |
| Messages | The action layer: `app::actions::Act<A>` is written in Input and drained by the action type's one apply system in Actions. |
| Observers (`On<..>`) | Pointer events on entities (picks, drags, screenshots). An observer only writes its mode's action. It holds no logic. |
| Hooks | Structural invariants only, such as maintaining an index. Not for setting up behavior. |
| Task pools | Only through the `jobs` module (`Job`, `Latest`, `RunThread`, `Pool::Io` / `Compute` / `Dedicated`). No `thread::spawn`, and no raw pool spawns in features. |
| `FixedUpdate` | **Not used for physics.** Physics advances in the shared runtime crates on simulation time; the viewer observes their frames (§5). Don't move simulation into Bevy's schedules. |
| Widgets | `ui_kit` on `bevy::ui::Button`, `Interaction` and `bevy_ui_widgets` (e.g. `Slider`). Feathers is deliberately not used. |
| Keyboard focus and text entry | `bevy_input_focus::InputFocus` and the kit text field (`bevy_text::EditableText`), being adopted by the `one-text-entry` epic. Check `native-viewer.md` for its status. New code never reads `MessageReader<KeyboardInput>` for text. |
| Picking | Bevy picking events feed the one `Selection` (§7). Don't add a separate selection path. |
| Settings | Bevy 0.19 app settings (`#[derive(SettingsGroup)]`) for persisted viewer preferences. The crate behind it isn't in this workspace's registry yet, so read its docs.rs page for 0.19.1 before adopting it. |
| BSN | Evaluate for mode setup and teardown. Don't rewrite working spawns just to use it. |
| Camera controllers, `InfiniteGrid` | Deliberately not adopted; see `native-viewer.md`. |

## Before writing or approving Bevy code

Answer these concretely; an unclear answer means the design isn't finished:
- Which data is per entity, which is global (a resource), and which is a
  durable asset or a CAD source? Who is the one writer of each value?
- Which systems need ordering, and through which public set?
- Is each communication an occurrence (a message), current state (change
  detection), an entity interaction (an observer), or durable pending work
  (state or a queue)?
- What spawns this feature's transient entities, what despawns them, and what
  happens when either end of a relationship goes away?
- Does the work belong to a frame system, a job, or the simulation runtime?
- Was every Bevy signature read in the 0.19.1 source, and does no row of the
  "Never write these" table appear?

**For the orchestrator:** an outdated pattern, or a hand-rolled version of a
0.19 facility this table names, is a reason to revise, like architectural
drift.

**For the Director:** a recurring Bevy-practice cost counts as a named,
recurring cost when ranking candidates. Examples: hand-rolled input loops,
private ordering edges, per-feature threads, bespoke widgets.
