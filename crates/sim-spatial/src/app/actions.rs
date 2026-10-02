//! The one action layer (native-viewer.md §3).
//!
//! Every user intent is a typed action: one enum per mode or feature
//! ([`Action`]), registered here once ([`registry`]) with its REST commands
//! (name, modes, example args, description, next to its variants) and the
//! `system_ui` control ids it answers. Buttons, keys, `system_ui`
//! activations and REST commands only produce actions; each action type is
//! validated and applied by one system in `ViewerSet::Actions`.
//!
//! - **Transport.** An action travels as a Bevy Message, [`Act`]: the value
//!   and where it came from ([`Origin`]). Messages, not observer triggers:
//!   each action type has exactly one consumer, its mode's apply system,
//!   which runs once per frame in `ViewerSet::Actions` after every input
//!   system (Input) has written, and before job results and the scene sync.
//!   The apply system drains `Messages<Act<A>>` ([`apply`]).
//! - **Reply tokens.** The one REST poll ([`serve`], Input) parses a command
//!   into its action (the command's JSON shape, unchanged) and writes it
//!   once with a [`Reply`] token held in the `sim_api` continuation, then
//!   answers Pending until the handler has written the outcome for that
//!   token ([`Replies`]; the mode switch's submit/outcome/waiting/cancel,
//!   generalized). A handler that answers Pending (a load, a study, a
//!   capture, a render) is re-applied every frame with its own
//!   continuation ([`InFlight`]) until it finishes; a REST cancel reaches
//!   it as [`Call::cancelled`].
//! - **Generated capabilities.** `GET /v1/capabilities` is [`capabilities`]
//!   and the router's modes table is [`command_modes`], both read from the
//!   registry; the lib tests cross-check it against what the action types
//!   parse ([`variants`]).
use super::ViewerMode;
use crate::app::ViewerMode::{Build, Cad, Inspect, Lessons, Phenomena, Place, Robot};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sim_api::Outcome;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Every mode.
pub const ALL: &[ViewerMode] = &ViewerMode::ALL;
/// The spatial assembly view's modes (its scene commands).
pub const SPATIAL: &[ViewerMode] = &[Inspect, Build, Lessons];
/// The builder's modes (build, and lessons over it).
pub const BUILDER: &[ViewerMode] = &[Build, Lessons];
/// The lesson commands: lessons, and build while the builder is shown over a lesson.
pub const LESSON: &[ViewerMode] = &[Lessons, Build];
pub const ROBOT: &[ViewerMode] = &[Robot];
pub const PLACE: &[ViewerMode] = &[Place];
pub const PHENOMENA: &[ViewerMode] = &[Phenomena];
/// Modes without controls of their own (`system_ui` is the mode switcher's).
pub const SWITCHER_ONLY: &[ViewerMode] = &[Inspect, Place];

/// One REST command of an action type: what `GET /v1/capabilities` lists.
pub struct Spec {
    pub name: &'static str,
    pub modes: &'static [ViewerMode],
    pub example: Value,
    pub description: String,
}
pub fn spec(name: &'static str, modes: &'static [ViewerMode], example: Value, description: impl Into<String>) -> Spec {
    Spec { name, modes, example, description: description.into() }
}

/// A mode's (or feature's) action type.
pub trait Action: DeserializeOwned + Send + Sync + 'static {
    /// Its REST commands, in the order `GET /v1/capabilities` lists them.
    fn commands() -> Vec<Spec>;
    /// A REST command as this action. The default is the action's own serde
    /// form (`{"command": name, ...args}`), which keeps each command's JSON
    /// shape and error text.
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        sim_api::decode::<Self>(command)
    }
    /// The REST command names its parser accepts, read from serde (the
    /// action's own form by default; an action with its own REST form names
    /// that form's type, as [`variants`] reads it).
    fn accepts() -> Vec<&'static str> {
        variants::<Self>()
    }
    /// The `system_ui` control ids it answers: exact ids, or patterns where
    /// `<…>` stands for a name, index or number the controls list fills in.
    fn controls() -> &'static [&'static str] {
        &[]
    }
}

/// A REST reply token: the key of an open slot in [`Replies`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reply(u64);
impl Reply {
    /// The headless server answers synchronously; its calls carry this token.
    pub const HEADLESS: Reply = Reply(0);
    /// As kept in a continuation.
    pub fn id(self) -> u64 {
        self.0
    }
    pub fn from_id(id: u64) -> Self {
        Reply(id)
    }
}

/// Where an action came from: how its handler reports the outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A REST command: the outcome goes to its reply slot.
    Rest(Reply),
    /// A button, key or `system_ui`-free click: a refusal is shown where the
    /// mode shows it (its status line).
    Ui,
    /// Not shown to anyone (a file watch, a 3D pick): the outcome is dropped.
    Quiet,
    /// A `system_ui` activation that one mode's handler passed on to another
    /// action type (robot mode's `hardware:<name>` controls): remote like
    /// `Rest`, but its REST command was answered by the handler that passed
    /// it on, so the outcome is dropped. Hardware motion is refused from it
    /// (`robot::hardware::HardwareAction::starts_motion`).
    SystemUi,
}

/// An action on its way to its handler (a Bevy Message).
pub struct Act<A> {
    pub action: A,
    pub origin: Origin,
}
impl<A: Send + Sync + 'static> Message for Act<A> {}
impl<A> Act<A> {
    pub fn ui(action: A) -> Self {
        Self { action, origin: Origin::Ui }
    }
    pub fn quiet(action: A) -> Self {
        Self { action, origin: Origin::Quiet }
    }
}

/// One application of an action by its handler.
pub struct Call<'a> {
    pub origin: Origin,
    /// The handler's own state across frames while it answers Pending
    /// (Null on the first application).
    pub continuation: &'a mut Value,
    /// A REST cancel was requested (or the reply was abandoned).
    pub cancelled: bool,
    /// For handlers that wait on another action (a lesson waiting for the mode switch).
    pub replies: &'a mut Replies,
}
impl Call<'_> {
    pub fn rest(&self) -> bool {
        matches!(self.origin, Origin::Rest(_))
    }
    /// From automation (REST, or a `system_ui` activation passed on), not a
    /// pointer or key in the window.
    pub fn remote(&self) -> bool {
        matches!(self.origin, Origin::Rest(_) | Origin::SystemUi)
    }
}

/// Frames a REST action may wait unapplied before its reply is abandoned:
/// its mode's handler did not run (the mode changed between the poll and the
/// handler, which the state machine does not allow within a frame; a guard).
const UNPICKED_FRAMES: u64 = 3;

struct Slot {
    opened: u64,
    picked: bool,
    cancelled: bool,
    outcome: Option<Outcome>,
}

/// Open REST replies: the one mechanism behind every asynchronous answer
/// (generalized from the mode switch's submit/outcome/waiting/cancel).
#[derive(Resource, Default)]
pub struct Replies {
    next: u64,
    /// Bumped once per frame by [`serve`].
    frame: u64,
    slots: BTreeMap<u64, Slot>,
}
/// The continuation key the REST poll keeps a token under.
const REPLY: &str = "reply";
/// And the action type ([`Feature::name`]) that holds it.
const FEATURE: &str = "feature";

impl Replies {
    /// A new slot.
    pub fn open(&mut self) -> Reply {
        self.next += 1;
        self.slots.insert(self.next, Slot { opened: self.frame, picked: false, cancelled: false, outcome: None });
        Reply(self.next)
    }
    /// The handler's answer (dropped if nobody waits for it any more).
    pub fn answer(&mut self, reply: Reply, outcome: Outcome) {
        if let Some(slot) = self.slots.get_mut(&reply.0) {
            slot.outcome = Some(outcome);
        }
    }
    /// The answer, once written; the slot closes.
    pub fn take(&mut self, reply: Reply) -> Option<Outcome> {
        if self.slots.get(&reply.0)?.outcome.is_none() {
            return None;
        }
        self.slots.remove(&reply.0).and_then(|s| s.outcome)
    }
    /// Ask the handler to stop (it sees `Call::cancelled`).
    pub fn cancel(&mut self, reply: Reply) {
        if let Some(slot) = self.slots.get_mut(&reply.0) {
            slot.cancelled = true;
        }
    }
    /// Cancelled, or no longer waited for.
    pub fn cancelled(&self, reply: Reply) -> bool {
        self.slots.get(&reply.0).is_none_or(|s| s.cancelled)
    }
    pub fn waiting(&self, reply: Reply) -> bool {
        self.slots.get(&reply.0).is_some_and(|s| s.outcome.is_none())
    }
    /// Nobody waits for this reply any more.
    pub fn forget(&mut self, reply: Reply) {
        self.slots.remove(&reply.0);
    }
    /// The handler has taken the action (it is no longer at risk of not being applied).
    pub(crate) fn pick(&mut self, reply: Reply) {
        if let Some(slot) = self.slots.get_mut(&reply.0) {
            slot.picked = true;
        }
    }

    /// The REST side of an action: on the first poll parse the command and
    /// write the action once, keeping its token in the continuation; on
    /// later polls answer Pending until the handler has answered. A cancel
    /// is passed on to the handler, which settles the command.
    pub fn submit<A>(&mut self, continuation: &mut Value, cancelled: bool, parse: impl FnOnce() -> Result<A, String>, emit: impl FnOnce(Act<A>) -> bool) -> Outcome {
        let Some(token) = continuation.get(REPLY).and_then(Value::as_u64) else {
            let action = match parse() {
                Ok(action) => action,
                Err(e) => return Outcome::Done(Err(e)),
            };
            let reply = self.open();
            if !emit(Act { action, origin: Origin::Rest(reply) }) {
                self.forget(reply);
                return Outcome::Done(Err("this command's handler is not part of this window".into()));
            }
            *continuation = json!({REPLY: reply.0});
            return Outcome::Pending;
        };
        let reply = Reply(token);
        if let Some(outcome) = self.take(reply) {
            return outcome;
        }
        let frame = self.frame;
        let Some(slot) = self.slots.get_mut(&token) else {
            return Outcome::Done(Err("the command's reply was lost".into()));
        };
        if cancelled {
            slot.cancelled = true;
        }
        if !slot.picked && frame > slot.opened + UNPICKED_FRAMES {
            self.forget(reply);
            return Outcome::Done(Err("the command was not applied: its mode's handler did not run".into()));
        }
        Outcome::Pending
    }
}

/// REST actions whose handler answered Pending, re-applied every frame with
/// their continuation until they finish.
#[derive(Resource)]
pub struct InFlight<A>(Vec<(A, Origin, Value)>);
impl<A> Default for InFlight<A> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<A> InFlight<A> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Answer every REST call still carried here with `reason` and empty the
    /// queue. A mode's OnExit calls this: its apply system does not run
    /// outside the mode, so a carried call would otherwise hang, or be
    /// answered against the next session's state on return.
    pub fn abandon(&mut self, replies: &mut Replies, reason: &str) {
        for (_, origin, _) in self.0.drain(..) {
            if let Origin::Rest(r) = origin {
                replies.answer(r, Outcome::Done(Err(reason.to_string())));
            }
        }
    }
}

/// An action type's message and in-flight queue (each mode plugin registers its own).
pub fn register<A: Send + Sync + 'static>(app: &mut App) {
    app.add_message::<Act<A>>().init_resource::<InFlight<A>>();
}

/// The body of every apply system: the actions still in flight, then this
/// frame's messages (in the order they were written), each given to the one
/// handler; outcomes go to their REST replies.
pub fn apply<A: Send + Sync + 'static>(messages: &mut Messages<Act<A>>, in_flight: &mut InFlight<A>, replies: &mut Replies, mut handle: impl FnMut(&A, &mut Call) -> Outcome) {
    let fresh: Vec<Act<A>> = messages.drain().collect();
    let carried = std::mem::take(&mut in_flight.0);
    for (action, origin, mut continuation) in carried.into_iter().chain(fresh.into_iter().map(|a| (a.action, a.origin, Value::Null))) {
        let (cancelled, abandoned) = match origin {
            Origin::Rest(r) => {
                replies.pick(r);
                (replies.cancelled(r), !replies.waiting(r))
            }
            _ => (false, false),
        };
        let outcome = handle(&action, &mut Call { origin, continuation: &mut continuation, cancelled, replies: &mut *replies });
        match outcome {
            // Only a REST caller waits: a click's work continues in the
            // feature's own jobs, as it always did. An abandoned reply had its
            // one cancelled application.
            Outcome::Pending if matches!(origin, Origin::Rest(_)) && !abandoned => in_flight.0.push((action, origin, continuation)),
            Outcome::Pending => {}
            done => {
                if let Origin::Rest(r) = origin {
                    replies.answer(r, done);
                }
            }
        }
    }
}

/// A registered action type: its REST commands and how they are parsed.
pub struct Feature {
    pub name: &'static str,
    /// The action type (features sharing one type are cross-checked together).
    pub action: &'static str,
    pub commands: fn() -> Vec<Spec>,
    /// The REST command names the action type's serde form accepts.
    pub accepts: fn() -> Vec<&'static str>,
    pub parses: fn(&sim_api::Command) -> Result<(), String>,
    pub controls: fn() -> &'static [&'static str],
    submit: fn(&mut World, &sim_api::Command, &mut Value, bool) -> Outcome,
}
fn feature<A: Action>(name: &'static str, commands: fn() -> Vec<Spec>) -> Feature {
    Feature { name, action: std::any::type_name::<A>(), commands, accepts: A::accepts, parses: |c| A::parse(c).map(drop), controls: A::controls, submit: submit_to::<A> }
}

/// REST: parse into `A` and write it to its handler, with a reply token.
fn submit_to<A: Action>(world: &mut World, command: &sim_api::Command, continuation: &mut Value, cancelled: bool) -> Outcome {
    world.resource_scope(|world, mut replies: Mut<Replies>| replies.submit(continuation, cancelled, || A::parse(command), |act| world.write_message(act).is_some()))
}

/// Every action type, in the order `GET /v1/capabilities` lists their commands.
pub fn registry() -> &'static [Feature] {
    static REGISTRY: OnceLock<Vec<Feature>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        use super::switch::WindowAction;
        vec![
            feature::<super::settings::actions::SettingsAction>("settings", <super::settings::actions::SettingsAction as Action>::commands),
            feature::<WindowAction>("window", <WindowAction as Action>::commands),
            feature::<crate::inspect::InspectAction>("inspect", <crate::inspect::InspectAction as Action>::commands),
            feature::<crate::builder::system_actions::SystemAction>("build", <crate::builder::system_actions::SystemAction as Action>::commands),
            feature::<crate::builder::calibration::study::StudyAction>("measured_study", <crate::builder::calibration::study::StudyAction as Action>::commands),
            feature::<crate::lesson::actions::LessonCommand>("lessons", <crate::lesson::actions::LessonCommand as Action>::commands),
            feature::<crate::robot::RobotAction>("robot", <crate::robot::RobotAction as Action>::commands),
            feature::<crate::robot::hardware::HardwareAction>("hardware", <crate::robot::hardware::HardwareAction as Action>::commands),
            feature::<crate::place_view::PlaceAction>("place", <crate::place_view::PlaceAction as Action>::commands),
            feature::<crate::cad::CadAction>("cad", <crate::cad::CadAction as Action>::commands),
            feature::<crate::phenomena::PhenomenaAction>("phenomena", <crate::phenomena::PhenomenaAction as Action>::commands),
            // The shared camera's `camera_*` commands, in every orbit mode.
            feature::<crate::camera::CameraAction>("camera", <crate::camera::CameraAction as Action>::commands),
            // The one selection (native-viewer.md §7): no REST command of its
            // own; each mode's selection command is an adapter onto it.
            feature::<crate::selection::SelectionAction>("selection", <crate::selection::SelectionAction as Action>::commands),
            // `system_ui` in inspect and place mode: the switcher's controls (listed last, as before).
            feature::<WindowAction>("switcher", WindowAction::switcher_commands),
        ]
    })
}

pub fn named(name: &str) -> &'static Feature {
    registry().iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no action feature `{name}`"))
}

/// `GET /v1/capabilities`: every registered command, each with its `modes`.
pub fn capabilities() -> Vec<Value> {
    registry()
        .iter()
        .flat_map(|f| (f.commands)())
        .map(|s| {
            let mut c = sim_api::capability(s.name, s.example, &s.description);
            c["modes"] = json!(s.modes);
            c
        })
        .collect()
}

/// The modes a command applies to, from the registry; None for a command no
/// action type registers.
pub fn command_modes(name: &str) -> Option<&'static [ViewerMode]> {
    static TABLE: OnceLock<BTreeMap<&'static str, Vec<ViewerMode>>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut table: BTreeMap<&'static str, Vec<ViewerMode>> = BTreeMap::new();
            for f in registry() {
                for s in (f.commands)() {
                    let modes = table.entry(s.name).or_default();
                    for m in s.modes {
                        if !modes.contains(m) {
                            modes.push(*m);
                        }
                    }
                }
            }
            table
        })
        .get(name)
        .map(Vec::as_slice)
}

/// The action type that answers `name` in `mode`.
pub fn feature_for(mode: ViewerMode, name: &str) -> Option<&'static Feature> {
    static TABLE: OnceLock<BTreeMap<(ViewerMode, &'static str), usize>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = BTreeMap::new();
        for (i, f) in registry().iter().enumerate() {
            for s in (f.commands)() {
                for m in s.modes {
                    table.entry((*m, s.name)).or_insert(i);
                }
            }
        }
        table
    });
    table.get(&(mode, name)).map(|&i| &registry()[i])
}

/// Who answers a command no action type registers (so its error names what
/// the mode's parser expects, as each mode's handler did).
pub fn fallback(mode: ViewerMode, name: &str) -> &'static Feature {
    match mode {
        Robot => named("robot"),
        Place => named("place"),
        Cad => named("cad"),
        Phenomena => named("phenomena"),
        Build | Lessons if name.starts_with("lesson_") => named("lessons"),
        Build | Lessons if name.starts_with("system") => named("build"),
        _ => named("inspect"),
    }
}

/// The one REST poll (Input): every mode's commands go through the one
/// dispatch (`route`) to their action type, and wait for its handler.
pub(crate) fn serve(world: &mut World) {
    if !world.contains_resource::<crate::rest::Rest>() {
        return;
    }
    let mode = *world.resource::<State<ViewerMode>>().get();
    world.resource_mut::<Replies>().frame += 1;
    world.resource_scope(|world, mut rest: Mut<crate::rest::Rest>| {
        let server = &mut rest.0;
        server.poll(|command, continuation, cancelled| {
            let waiting = continuation.get(REPLY).and_then(Value::as_u64).map(Reply);
            // Already answered (the handler ran before the mode changed): the
            // answer stands, whatever mode the window is in now.
            if let Some(outcome) = waiting.and_then(|r| world.resource_mut::<Replies>().take(r)) {
                return annotate_studies(world, mode, command, outcome);
            }
            let outcome = match super::route::route(mode, true, command) {
                // A command left waiting by another action type: the mode
                // changed to one where its name is another type's command
                // (`state`, `camera`, `system_ui`), whose handler never saw
                // it. Its own handler no longer runs, so its reply would
                // never come (and the server runs one command at a time).
                Ok(feature) if waiting.is_some() && continuation.get(FEATURE).and_then(Value::as_str).is_some_and(|f| f != feature.action) => {
                    world.resource_mut::<Replies>().forget(waiting.unwrap());
                    Outcome::Done(Err(format!("`{}` was left unfinished: the mode changed to {} while it waited", command.command, mode.name())))
                }
                Ok(feature) => {
                    let outcome = (feature.submit)(world, command, continuation, cancelled);
                    // Remember which action type holds the reply.
                    if let Some(c) = continuation.as_object_mut().filter(|c| c.contains_key(REPLY)) {
                        c.entry(FEATURE).or_insert_with(|| json!(feature.action));
                    }
                    outcome
                }
                Err(e) => match waiting {
                    // A command left waiting by a mode that is no longer active.
                    Some(reply) => {
                        world.resource_mut::<Replies>().forget(reply);
                        Outcome::Done(Err(format!("{e} (it was accepted in the previous mode and left unfinished when the mode changed)")))
                    }
                    None => Outcome::Done(Err(e)),
                },
            };
            // The document picker's controls as they are now (after a
            // `picker:*` activation changed them, the new list).
            annotate_studies(world, mode, command, outcome)
        });
        // Keep frames coming while a command waits; an idle background
        // window otherwise steps only on its slow low-power timer.
        if server.busy() {
            let _ = world.write_message(bevy::window::RequestRedraw);
        }
    });
}

/// Add the actual rendered offline-study controls to the common system_ui list.
fn annotate_studies(world: &World, mode: ViewerMode, command: &sim_api::Command, outcome: Outcome) -> Outcome {
    let mut outcome = super::route::annotate(mode, &super::picker::controls_in(world), command, outcome);
    if command.command == "system_ui" && command.args["action"]["operation"] == "controls" {
        if let Outcome::Done(Ok(value)) = &mut outcome {
            if let Some(controls) = value.get_mut("controls").and_then(Value::as_array_mut) {
                if let Some(ui) = world.get_resource::<crate::builder::calibration::study::forms::StudyUi>() {
                    controls.extend(crate::builder::calibration::study::ui::controls(ui));
                }
            }
        }
    }
    outcome
}

/// Whether a `system_ui` control id fits one of an action type's control
/// patterns ([`Action::controls`]): `<…>` stands for one `:`-separated
/// segment, or the rest of one after a fixed prefix (`control-<hash>`).
pub fn control_matches(pattern: &str, id: &str) -> bool {
    let (pattern, id): (Vec<&str>, Vec<&str>) = (pattern.split(':').collect(), id.split(':').collect());
    pattern.len() == id.len()
        && pattern.iter().zip(&id).all(|(p, i)| match p.find('<') {
            Some(at) if p.ends_with('>') => i.len() > at && i.starts_with(&p[..at]),
            _ => p == i,
        })
}

/// The REST command names a serde-deserialized action type accepts: its
/// variant names as serde lists them (skipped variants excluded), read from
/// the type itself by asking it to parse a command no variant has.
pub fn variants<T: DeserializeOwned>() -> Vec<&'static str> {
    #[derive(Debug)]
    struct Probe(Option<&'static [&'static str]>);
    impl std::fmt::Display for Probe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "probe")
        }
    }
    impl std::error::Error for Probe {}
    impl serde::de::Error for Probe {
        fn custom<M: std::fmt::Display>(_: M) -> Self {
            Probe(None)
        }
        fn unknown_variant(_: &str, expected: &'static [&'static str]) -> Self {
            Probe(Some(expected))
        }
    }
    let probe = serde::de::value::MapDeserializer::<_, Probe>::new(std::iter::once(("command", "\u{1}probe")));
    match T::deserialize(probe) {
        Err(Probe(Some(names))) => names.to_vec(),
        _ => Vec::new(),
    }
}
