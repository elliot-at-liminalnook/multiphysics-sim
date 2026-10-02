//! The builder chrome's actions: [`BuildAction`] (buttons, keys, markers;
//! `system_ui` activations resolve to it), its handler [`dispatch`], and the
//! input systems that write it. It travels inside
//! `system_actions::SystemAction::Ui` and is applied by
//! `system_actions::apply`, the builder's one handler. Edits go through
//! `Builder::apply` (the validated command path and the shared undo history).
use super::*;
use super::system_actions::SystemAction;
use crate::app::actions::Act;
use crate::app::switch::{ModeSwitch, WindowAction};

/// A builder button, key or marker: the chrome's actions. The serialized
/// value names its `system_ui` control (`ui_api::collect`), so it is unchanged.
#[derive(Component, Clone, Debug, serde::Serialize)]
#[serde(rename_all="snake_case")]
pub(crate) enum BuildAction {
    /// Back to the lesson this builder was opened from ("‹ lesson"): the mode
    /// switch to Lessons, refused, naming the blocker, while a text or
    /// discussion draft, placement drag, study, replay, Codex answer or open
    /// is in progress (`Builder::switch_blockers`); a live run is paused and
    /// kept (Run resumes it back in build mode).
    Lessons,
    OpenReference(String),
    CloseReference,
    Agent(agent::Request),
    Discussion(discussion::Action),
    ImportNotes,
    GridSnap, GridVisible, GridPlane, GridSpacing, GridOrigin, Position,
    Tab(Tab),
    Category(Option<&'static str>),
    SetMode(Mode),
    /// Live run back to t = 0, paused (`Builder::run_reset`).
    Reset,
    /// Advance the paused live run one timestep (`Builder::run_step`).
    Step,
    Up,
    Level(String),
    Select(String),
    /// A schematic box: the same selection path as `Select` (the Outline).
    SchematicSelect(String),
    /// Show or hide the schematic pane.
    ToggleSchematic,
    Open(String),
    Filter,
    Group,
    Ungroup,
    Swap,
    SwapTo(usize),
    MakeUnique,
    Delete,
    Rename,
    Terminal(Terminal),
    CancelConnect,
    Disconnect(Terminal),
    Parameter(String, String),
    Undo,
    Redo,
    Run,
    Pause,
    ImportImage,
    Opacity(String, f32),
    Lock(String),
    Calibrate(String),
    Width(String),
    RemoveReference(String),
    SaveToLibrary,
    SyncLibrary,
    /// Expose (instance, parameter) of this level as a level parameter.
    Expose(String, String),
    /// Show a library item's card.
    Preview(usize),
    PreviewKind(InstanceKind),
    ClosePreview,
    PlacePreview,
    /// Attach the preview to the selected instance's port.
    AttachPreview(String),
    /// Attach suggestion `index` to the selected instance's `port`.
    Snap(String, usize),
    SnapMore(String),
    ToggleNotes,
    ToggleGraphs,
    RunStudy(String),
    RemoveStudy(String),
    CompareSelected,
    SweepParameter(String, String),
    CancelStudy,
    ClearStudy,
    SaveRun,
    ToggleRealtime,
    PickRun(String),
    CompareRuns,
    ReplayRun(String),
    CancelReplay,
    Pin(String),
    Unpin(String),
    /// Open this system file in the window (Systems tab list).
    OpenSystem(PathBuf),
    /// Type a system file path to open.
    OpenSystemPath,
    CancelOpen,
    /// Type an actuator registry path (Actuators tab).
    ActuatorRegistryPath,
    /// Type a consumer file to check against the registry.
    ActuatorConsumerPath,
    /// Reload the registry and recheck the previous consumer files.
    ActuatorReload,
    CancelActuators,
    /// Type a gait-lab results folder (Gait lab tab).
    GaitResultsPath,
    /// Reread the current results folder.
    GaitReload,
    CancelGaitReports,
    /// Show this results entry (directory name) in detail.
    GaitReportSelect(String),
    /// Registry or Measured evidence part of the Actuators tab.
    ActuatorView(calibration::ActuatorView),
    /// Type an identification archive folder (Measured evidence).
    CalibrationPath,
    /// Reload the current identification archive.
    CalibrationReload,
    CancelCalibration,
    CalibrationSplit(calibration::SplitFilter),
    CalibrationOutcome(calibration::OutcomeFilter),
    /// Page of the filtered trial list (0-based).
    CalibrationPage(usize),
    /// Select a trial of the shown archive and chart it.
    CalibrationTrial(String),
    /// Arrow keys and Page Up/Down: move the selection by a display-only step (metres).
    Nudge([f32; 3]),
    /// A primary click on a rendered part (`pick_part` in `inspect_view/scene.rs`; no button
    /// carries it, so it is not a `system_ui` control): in Annotate mode a
    /// comment draft pinned at `world` (display metres) on part `index`, else
    /// the same selection as `system_ui` `click_part` (`add`: shift held).
    PickPart { index: usize, component: String, add: bool, world: Option<[f32; 3]> },
    /// Enter in an open text draft (its kit field, `drafts::sync_field`): post a comment or thread
    /// title, else commit the field. Not a button, so not a `system_ui` control.
    SubmitDraft,
    /// Escape in an open text draft: drop it.
    DropDraft,
    /// A click on reference image `id` at `world` (display metres; `pick_reference`):
    /// a calibration point while calibrating it, else the hint to calibrate.
    ReferencePoint { id: String, world: [f32; 3] },
}

/// The chrome's handler: one `BuildAction` (a button, key, marker, or a
/// `system_ui` activation). Refusals are the status line (`Builder::report`).
/// What is selected is the shared selection (`pick`, `picked`).
pub(super) fn dispatch(builder: &mut Builder, scene: &mut SpatialScene, orbit: &mut Orbit, pick: &mut Picked, action: BuildAction) {
    builder.action_error=None;
    builder.panel_dirty = true;
    match action {
        BuildAction::Lessons => {}
        BuildAction::OpenReference(target)=>{let r=builder.reference.open(target);builder.report(r);builder.panel_dirty=true;},
        BuildAction::CloseReference=>{builder.reference=Default::default();builder.panel_dirty=true;},
        BuildAction::Agent(action)=>{let r=builder.agent_request(action);builder.report(r);},
        BuildAction::Discussion(action)=>discussion::act(builder,scene,orbit,pick,action),
        BuildAction::ImportNotes=>{let r=builder.discussion_request(discussion::Request::ImportLegacy,None,scene,orbit,pick);builder.report(r);},
        BuildAction::GridSnap | BuildAction::GridVisible | BuildAction::GridPlane => {
            let mut grid=builder.grid(); match action {BuildAction::GridSnap=>grid.snap=!grid.snap, BuildAction::GridVisible=>grid.visible=!grid.visible,_=>grid.plane=match grid.plane {sim_system::display::Plane::Xz=>sim_system::display::Plane::Xy,sim_system::display::Plane::Xy=>sim_system::display::Plane::Yz,_=>sim_system::display::Plane::Xz}};
            let r=builder.set_grid(grid);builder.report(r);
        }
        BuildAction::GridSpacing=>builder.start_input(Purpose::GridSpacing,builder.grid().spacing_m.to_string()),
        BuildAction::GridOrigin=>builder.start_input(Purpose::GridOrigin,builder.grid().origin_m.iter().map(|v|v.to_string()).collect::<Vec<_>>().join(" ")),
        BuildAction::Position=>{if let Some(s)=pick.first().and_then(|n|builder.spec(&n)){builder.start_input(Purpose::Position,s.placement.position.iter().map(|v|v.to_string()).collect::<Vec<_>>().join(" "));}},
        BuildAction::Tab(tab) => {
            if tab == Tab::Systems && builder.open.shell.is_some() {
                builder.open.systems = open::discover(&builder.store.path, &builder.library_dir);
            }
            // First visit: load the default registry (off the UI thread).
            if tab == Tab::Actuators && builder.actuators.shown.is_none() && builder.actuators.error.is_none() && builder.actuators.pending().is_none() {
                let r = builder.actuators_request(None, None);
                builder.report(r);
            }
            // First visit: read the default results folder (off the UI thread).
            if tab == Tab::GaitLab && builder.gait_lab.shown.is_none() && builder.gait_lab.error.is_none() && builder.gait_lab.pending().is_none() {
                let r = builder.gait_reports_request(None);
                builder.report(r);
            }
            builder.tab = tab;
        }
        BuildAction::Category(category) => {
            builder.category = category;
            builder.page = 0;
        }
        BuildAction::SetMode(mode) => {
            if builder.input.is_some(){builder.status="Finish or cancel the current draft first.".into();return;}
            builder.mode = mode;
            builder.connect_from = None;
            builder.status = match mode {
                Mode::Annotate => "Annotate: click a rendered surface to place a comment; Escape cancels.".into(),
                Mode::Connect => "Connect: select a part, pick a port in the inspector, then pick the other port.".into(),
                Mode::Select => "Select: click parts; shift-click adds to the selection.".into(),
            };
            if let Some(name) = pick.only() {
                builder.port_menu = (mode == Mode::Connect).then_some(name);
            }
        }
        BuildAction::Reset => {
            let r = builder.run_reset();
            builder.report(r);
        }
        BuildAction::Step => {
            let r = builder.run_step();
            builder.report(r);
        }
        BuildAction::Up => {
            let parent = builder.level.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
            let child = builder.level.rsplit('/').next().unwrap_or("").to_string();
            // Back up, with the subsystem just left selected.
            let r = builder.enter_level(pick, &parent);
            if r.is_ok() && !child.is_empty() {
                let _ = pick.set([child]);
            }
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Level(path) => {
            let r = builder.enter_level(pick, &path);
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Select(name) | BuildAction::SchematicSelect(name) => {
            let _ = builder.suggestions(&name);
            let r = builder.select(pick, vec![name]);
            builder.report(r);
        }
        BuildAction::Open(name) => {
            let path = builder.full_path(&name);
            let r = builder.enter_level(pick, &path);
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Preview(index) => {
            builder.preview = builder.filtered().get(index).cloned().cloned();
            builder.load_preview_sheet();
            if let Some(name) = pick.only() {
                let _ = builder.suggestions(&name);
            }
        }
        BuildAction::PreviewKind(kind) => {
            builder.preview = builder.palette_item(&kind);
            builder.load_preview_sheet();
            if builder.preview.is_none() {
                builder.status = format!("{} is not in the palette at this level", sim_system::commands::kind_label(&kind));
            }
        }
        BuildAction::ClosePreview => builder.preview = None,
        BuildAction::PlacePreview => {
            if let Some(item) = builder.preview.take() {
                builder.place(pick, item);
            }
        }
        BuildAction::AttachPreview(port) => {
            let (Some(name), Some(item)) = (pick.only(), builder.preview.clone()) else { return };
            let candidate = builder.suggestions(&name).ok().and_then(|all| all.into_iter().find(|p| p.port == port)).and_then(|p| p.candidates.into_iter().find(|c| c.kind == item.kind));
            let r = match candidate {
                Some(c) => builder.snap(pick, &name, &port, &c).map(|n| builder.status = format!("Snapped {n} onto {name}.{port}")),
                None => Err(format!("{} has no port that fits {name}.{port}", item.label)),
            };
            builder.report(r);
        }
        BuildAction::Snap(port, index) => {
            let Some(name) = pick.only() else { return };
            let candidate = builder.suggestions(&name).ok().and_then(|all| all.into_iter().find(|p| p.port == port)).and_then(|p| p.candidates.into_iter().nth(index));
            if let Some(c) = candidate {
                let r = builder.snap(pick, &name, &port, &c).map(|n| builder.status = format!("Snapped {n} ({}) onto {name}.{port}", c.label));
                builder.report(r);
            }
        }
        BuildAction::SnapMore(port) => {
            if !builder.snap_expanded.remove(&port) {
                builder.snap_expanded.insert(port);
            }
        }
        BuildAction::ToggleNotes => builder.show_notes = !builder.show_notes,
        BuildAction::RunStudy(name) => {
            let r = builder.run_study(&name);
            builder.report(r);
        }
        BuildAction::RemoveStudy(name) => {
            let r = builder.apply("Remove study", vec![SystemCommand::SetStudy { name, study: None }]);
            builder.report(r);
        }
        BuildAction::CompareSelected => {
            if let Some(name) = pick.only() {
                let r = builder.compare_alternatives(scene, &name);
                builder.report(r);
            }
        }
        BuildAction::SweepParameter(name, parameter) => {
            let observe = builder.default_observe(scene, &name);
            builder.start_input(Purpose::Sweep { name, parameter, observe }, String::new());
            builder.status = "Sweep: type from, to and count (for example 1 4 4), then Enter.".into();
        }
        BuildAction::CancelStudy => {
            if let Some(job) = builder.study.job.take() {
                job.work.cancel();
                builder.status = "Study cancelled.".into();
            }
        }
        BuildAction::ClearStudy => builder.study.result = None,
        BuildAction::OpenSystem(path) => {
            let result = builder.open_system(path);
            builder.report(result);
        }
        BuildAction::OpenSystemPath => builder.start_input(Purpose::OpenSystem, String::new()),
        BuildAction::CancelOpen => {
            builder.cancel_open();
        }
        BuildAction::ActuatorRegistryPath => {
            let shown = builder.actuators.registry.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            builder.start_input(Purpose::ActuatorRegistry, shown);
        }
        BuildAction::ActuatorConsumerPath => builder.start_input(Purpose::ActuatorConsumer, String::new()),
        BuildAction::ActuatorReload => {
            let r = builder.actuators_request(None, None);
            builder.report(r);
        }
        BuildAction::CancelActuators => {
            builder.cancel_actuators();
        }
        BuildAction::ActuatorView(view) => {
            builder.actuator_view = view;
        }
        BuildAction::CalibrationPath => {
            let shown = builder.calibration.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            builder.start_input(Purpose::CalibrationArchive, shown);
        }
        BuildAction::CalibrationReload => {
            let r = builder.calibration_request(None);
            builder.report(r);
        }
        BuildAction::CancelCalibration => {
            builder.cancel_calibration();
        }
        BuildAction::CalibrationSplit(f) => builder.set_calibration_filter(Some(f), None),
        BuildAction::CalibrationOutcome(f) => builder.set_calibration_filter(None, Some(f)),
        BuildAction::CalibrationPage(page) => builder.set_calibration_page(page),
        BuildAction::CalibrationTrial(id) => {
            let r = builder.select_calibration_trial(&id);
            builder.report(r);
        }
        BuildAction::GaitResultsPath => {
            let shown = builder.gait_lab.root.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            builder.start_input(Purpose::GaitResults, shown);
        }
        BuildAction::GaitReload => {
            let r = builder.gait_reports_request(None);
            builder.report(r);
        }
        BuildAction::CancelGaitReports => {
            builder.cancel_gait_reports();
        }
        BuildAction::GaitReportSelect(name) => {
            let r = builder.select_gait_report(name);
            builder.report(r);
        }
        BuildAction::ToggleRealtime => {
            builder.realtime = !builder.realtime;
            builder.stop_run();
            builder.status = if builder.realtime { "Realtime profile: every part's realtime model at the profile's step. Press Run.".into() } else { "Detailed model. Press Run.".into() };
        }
        BuildAction::SaveRun => {
            let r = builder.save_run("saved by hand").map(|_| ());
            builder.report(r);
        }
        BuildAction::PickRun(id) => {
            if !builder.run_picks.remove(&id) {
                builder.run_picks.insert(id);
            }
        }
        BuildAction::ReplayRun(id) => {
            let r = builder.replay_run(&id);
            builder.report(r);
        }
        BuildAction::CancelReplay => {
            builder.cancel_replay();
        }
        BuildAction::CompareRuns => {
            let ids: Vec<String> = builder.run_picks.iter().cloned().collect();
            let r = builder.compare_runs(&ids);
            builder.report(r);
        }
        BuildAction::ToggleGraphs => builder.graphs.visible = !builder.graphs.visible,
        BuildAction::ToggleSchematic => builder.schematic.visible = !builder.schematic.visible,
        BuildAction::Pin(id) => {
            if !builder.graphs.pinned.contains(&id) {
                builder.graphs.pinned.push(id);
                if builder.graphs.pinned.len() > graphs::MAX_CHARTS {
                    builder.graphs.pinned.remove(0);
                }
            }
            builder.graphs.visible = true;
            builder.observe(scene);
        }
        BuildAction::Unpin(id) => builder.graphs.pinned.retain(|p| *p != id),
        BuildAction::Filter => {
            let initial = builder.filter.clone();
            builder.start_input(Purpose::Filter, initial);
        }
        BuildAction::Group => builder.group_selected(pick),
        BuildAction::Ungroup => {
            if let Some(name) = pick.only() {
                let r = builder.apply("Ungroup", vec![SystemCommand::Ungroup { at: builder.level.clone(), name }]).map(|_| {
                    let _ = pick.clear();
                });
                builder.report(r);
            }
        }
        BuildAction::Swap => {
            if let Some(name) = pick.only() {
                match library::alternatives(&builder.document, &builder.registry, Some(&builder.library_dir), &builder.level, &name) {
                    Ok(list) => {
                        builder.status = format!("{} implementations fit {name}'s connected ports", list.len());
                        builder.alternatives = Some((name, list));
                    }
                    Err(e) => builder.status = e.to_string(),
                }
            }
        }
        BuildAction::SwapTo(index) => {
            if let Some((name, list)) = builder.alternatives.clone() {
                if let Some(alt) = list.get(index) {
                    let mut commands = Vec::new();
                    if let Some(path) = &alt.library_path {
                        match library::import(std::path::Path::new(path)) {
                            Ok(definitions) => commands.push(SystemCommand::AddDefinitions { definitions }),
                            Err(e) => {
                                builder.status = e.to_string();
                                return;
                            }
                        }
                    }
                    commands.push(SystemCommand::Swap { at: builder.level.clone(), name, kind: alt.kind.clone(), keep_parameters: true });
                    let r = builder.apply(&format!("Swap to {}", alt.label), commands);
                    builder.report(r);
                }
            }
        }
        BuildAction::MakeUnique => {
            if let Some(name) = pick.only() {
                if let Some(InstanceKind::Subsystem { definition }) = builder.spec(&name).map(|s| s.kind) {
                    let mut id = format!("{definition}_{name}");
                    let mut n = 2;
                    while builder.document.definitions.contains_key(&id) {
                        id = format!("{definition}_{name}_{n}");
                        n += 1;
                    }
                    let r = builder.apply("Make unique", vec![SystemCommand::MakeUnique { at: builder.level.clone(), name, definition: id }]);
                    builder.report(r);
                }
            }
        }
        BuildAction::Delete => builder.remove_selected(pick),
        BuildAction::Rename => {
            if let Some(name) = pick.only() {
                builder.start_input(Purpose::Rename(name.clone()), name);
            }
        }
        BuildAction::Terminal(t) => match builder.connect_from.take() {
            None => {
                builder.status = format!("Connecting from {t}: pick the other terminal (select another part, then its port).");
                builder.connect_from = Some(t);
            }
            Some(from) if from == t => builder.status = "Connection cancelled.".into(),
            Some(from) => {
                let r = builder.apply("Connect", vec![SystemCommand::Connect { at: builder.level.clone(), terminals: vec![from, t], label: String::new() }]);
                builder.report(r);
            }
        },
        BuildAction::CancelConnect => {
            builder.connect_from = None;
            builder.status = "Connection cancelled.".into();
        }
        BuildAction::Disconnect(t) => {
            let r = builder.apply("Disconnect", vec![SystemCommand::Disconnect { at: builder.level.clone(), terminal: t }]);
            builder.report(r);
        }
        BuildAction::Parameter(name, parameter) => {
            let current = builder.spec(&name).and_then(|s| s.parameters.get(&parameter).cloned()).map(|b| match b {
                sim_system::ParameterBinding::Value { value, .. } => value.to_string(),
                sim_system::ParameterBinding::Parameter { parameter } => format!("${parameter}"),
            });
            builder.start_input(Purpose::Parameter { name, parameter }, current.unwrap_or_default());
        }
        BuildAction::Undo => {
            let _ = builder.undo();
        }
        BuildAction::Redo => {
            let _ = builder.redo();
        }
        BuildAction::Run => builder.start_run(scene),
        BuildAction::Pause => builder.pause_run(),
        BuildAction::ImportImage => builder.start_input(Purpose::ImportImage, String::new()),
        BuildAction::Opacity(id, delta) => {
            if let Some(mut r) = builder.reference(&id) {
                r.opacity = (r.opacity + delta).clamp(0.05, 1.0);
                let result = builder.apply("Reference opacity", vec![SystemCommand::SetReference { at: builder.level.clone(), id, reference: r }]);
                builder.report(result);
            }
        }
        BuildAction::Lock(id) => {
            if let Some(mut r) = builder.reference(&id) {
                r.locked = !r.locked;
                let result = builder.apply(if r.locked { "Lock reference" } else { "Unlock reference" }, vec![SystemCommand::SetReference { at: builder.level.clone(), id, reference: r }]);
                builder.report(result);
            }
        }
        BuildAction::Calibrate(id) => {
            builder.calibrating = Some((id, Vec::new()));
            builder.status = "Calibrate: click two points on the image a known distance apart.".into();
        }
        BuildAction::Width(id) => {
            let width = builder.reference(&id).map(|r| r.width.to_string()).unwrap_or_default();
            builder.start_input(Purpose::ReferenceWidth(id), width);
        }
        BuildAction::RemoveReference(id) => {
            let r = builder.apply("Remove reference", vec![SystemCommand::RemoveReference { at: builder.level.clone(), id }]);
            builder.report(r);
        }
        BuildAction::SaveToLibrary => {
            if let Some(name) = pick.only() {
                if let Some(InstanceKind::Subsystem { definition }) = builder.spec(&name).map(|s| s.kind) {
                    let r = builder.publish(&definition).map(|_| ());
                    builder.report(r);
                }
            }
        }
        BuildAction::SyncLibrary => {
            let r = builder.sync_library().map(|_| ());
            builder.report(r);
        }
        BuildAction::Expose(instance, parameter) => {
            let r = builder.expose(&instance, &parameter).map(|_| ());
            builder.report(r);
        }
        BuildAction::Nudge(delta) => builder.nudge(pick.names(), delta),
        BuildAction::PickPart { index, component, add, world } => {
            if builder.mode == Mode::Annotate {
                // The part index is the clicked mesh's in this frame's scene; checked in case the scene was rebuilt.
                if let Some(world) = world.filter(|_| scene.spatial.parts.get(index).is_some_and(|p| p.component == component)) {
                    discussion::begin_surface(builder, scene, index, Vec3::from_array(world));
                }
            } else {
                click_part(builder, pick, &component, add);
            }
        }
        BuildAction::SubmitDraft => {
            if builder.input.as_ref().is_some_and(|i| matches!(i.purpose, Purpose::Comment | Purpose::ThreadTitle)) {
                discussion::submit(builder, scene, orbit, pick);
            } else if builder.input.is_some() {
                builder.commit_picking(pick);
            }
        }
        // The Cancel button's and REST cancel_input's handler (it also ends a comment edit).
        BuildAction::DropDraft => discussion::act(builder, scene, orbit, pick, discussion::Action::CancelDraft),
        BuildAction::ReferencePoint { id: quad, world } => {
            let frame = builder.subsystems.get(&builder.level).copied().unwrap_or(sim_system::flatten::WorldPlacement::IDENTITY);
            // Points are recorded in the level's frame, like the reference origin.
            let inverse = Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(Quat::from_array(frame.rotation_xyzw)).to_matrix().inverse();
            let local = inverse.transform_point3(Vec3::from_array(world)).to_array();
            let Some((id, points)) = builder.calibrating.as_mut() else {
                builder.status = format!("Reference {quad} (click Calibrate to scale it from two points)");
                return;
            };
            if *id != quad {
                return;
            }
            points.push(local);
            if points.len() == 2 {
                let (id, points) = builder.calibrating.take().expect("checked above");
                builder.start_input(Purpose::Distance { id, first: points[0], second: points[1] }, String::new());
                builder.status = "Type the real distance between the two points (m) and press Enter.".into();
            } else {
                builder.status = "Now click the second point.".into();
            }
        }
    }
}

/// Rendered source identity, retained with the panel instead of resolving a row
/// against the current palette after REST or another action changed it.
#[derive(Component, Clone, Debug)]
pub(super) struct RenderStamp {
    pub(super) document: Option<crate::document::DocumentId>,
    path: std::path::PathBuf,
    revision: u64,
    level: String,
    palette: String,
}
impl RenderStamp {
    pub(super) fn capture(builder: &Builder) -> Self {
        Self { document: None, path: builder.path().to_path_buf(), revision: builder.document.revision,
            level: builder.level.clone(), palette: format!("{:?}",builder.palette) }
    }
    pub(super) fn matches(&self,builder:&Builder)->bool {
        self.path==builder.path() && self.revision==builder.document.revision
            && self.level==builder.level && self.palette==format!("{:?}",builder.palette)
    }
}

/// Input: a pressed, enabled builder button's action. The Lessons button
/// ("‹ lesson") asks the mode switch for Lessons: refused, naming the
/// blocker, while a draft, drag, study, replay, Codex answer or open is in
/// progress (`Builder::switch_blockers`); a live run is paused and kept.
pub(super) fn buttons(
    actions: Query<(Entity, &BuildAction, Option<&ui_api::Enabled>), (With<crate::ui_kit::activation::Activated>, With<Button>)>,
    parents: Query<&ChildOf>,
    stamps: Query<&RenderStamp>,
    builder: Res<Builder>,
    registry: Res<DocumentRegistry>,
    learn: Option<Res<crate::lesson::Learn>>,
    mut out: MessageWriter<Act<SystemAction>>,
    mut switch: MessageWriter<Act<WindowAction>>,
) {
    for (entity, action, enabled) in &actions {
        if enabled.is_some_and(|e| !e.0) {
            continue;
        }
        let mut cursor=Some(entity);
        let mut source=None;
        while let Some(entity)=cursor {
            if let Ok(stamp)=stamps.get(entity) {source=Some(stamp.clone());break;}
            cursor=parents.get(entity).ok().map(|parent|parent.parent());
        }
        let mut source=source.unwrap_or_else(||RenderStamp::capture(&builder));
        source.document=picked::document(&registry);
        if !source.matches(&builder) {continue;}
        if matches!(action, BuildAction::Lessons) {
            if learn.is_some() {
                switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Lessons, document: None })));
            }
            continue;
        }
        out.write(Act::ui(SystemAction::RenderedUi { stamp: source, action: action.clone() }));
    }
}

/// Input: build mode's keys, as the same actions as their buttons. Not run
/// while a kit field has the keyboard (`BuilderPlugin`); an open draft also
/// stops them in the frame before its field is refocused (`drafts::sync_field`).
pub(super) fn keys(keys: Res<ButtonInput<KeyCode>>, builder: Res<Builder>, selection: Res<Selection>, registry: Res<DocumentRegistry>, mut out: MessageWriter<Act<SystemAction>>) {
    if builder.drag.is_some() || builder.input.is_some() {
        return;
    }
    let mut send = |action: BuildAction| {
        out.write(Act::ui(SystemAction::Ui(action)));
    };
    let command = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight) || keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if command && keys.just_pressed(KeyCode::KeyZ) {
        send(if shift { BuildAction::Redo } else { BuildAction::Undo });
        return;
    }
    let step = if shift { 0.001 } else { 0.005 };
    for (key, delta) in [
        (KeyCode::ArrowLeft, [-step, 0., 0.]),
        (KeyCode::ArrowRight, [step, 0., 0.]),
        (KeyCode::ArrowUp, [0., 0., -step]),
        (KeyCode::ArrowDown, [0., 0., step]),
        (KeyCode::PageUp, [0., step, 0.]),
        (KeyCode::PageDown, [0., -step, 0.]),
    ] {
        if keys.just_pressed(key) {
            send(BuildAction::Nudge(delta));
        }
    }
    let action = if keys.just_pressed(KeyCode::KeyN) {
        Some(BuildAction::SetMode(Mode::Annotate))
    } else if keys.just_pressed(KeyCode::Escape) {
        Some(BuildAction::SetMode(Mode::Select))
    } else if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        Some(BuildAction::Delete)
    } else if keys.just_pressed(KeyCode::KeyG) {
        Some(BuildAction::Group)
    } else if keys.just_pressed(KeyCode::KeyU) {
        Some(BuildAction::Up)
    } else if keys.just_pressed(KeyCode::Enter) {
        picked::only(&picked::names(&selection, &registry)).filter(|n| matches!(builder.spec(n).map(|s| s.kind), Some(InstanceKind::Subsystem { .. }))).map(BuildAction::Open)
    } else if keys.just_pressed(KeyCode::Slash) {
        Some(BuildAction::Filter)
    } else if keys.just_pressed(KeyCode::KeyR) {
        Some(if builder.running() { BuildAction::Pause } else { BuildAction::Run })
    } else {
        None
    };
    if let Some(action) = action {
        send(action);
    }
}
