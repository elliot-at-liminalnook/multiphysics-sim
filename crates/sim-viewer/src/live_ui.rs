//! Process-backed live measurements. The GUI never advances physics.
use super::*;
use sim_inspect::{
    Availability, SampleFrame,
    plot::{self, History},
    selection::SelectionTarget,
};
use sim_runtime::{
    system_session::{Command, Phase, Reply, SessionStatus},
    system_worker::{Client, Event, Launch},
};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_PLOTS: usize = 8;
const HISTORY_FRAMES: usize = 2000;
pub(super) struct LivePanel {
    api_replies: std::collections::BTreeMap<u64, sim_api::Result>,
    publisher: Option<sim_inspect::live::native::Publisher>,
    animation_observables: BTreeSet<String>,
    launch: Launch,
    source: SystemDescription,
    client: Option<Client>,
    description: Option<SystemDescription>,
    status: Option<SessionStatus>,
    history: Option<History>,
    selected: BTreeSet<String>,
    subscribed: BTreeSet<String>,
    pending: Option<u64>,
    error: Option<String>,
    cursor: Option<f64>,
    last_sample: Option<std::time::Instant>,
}
impl LivePanel {
    pub fn new(launch: Launch, source: SystemDescription) -> Result<Self, String> {
        let binding = launch
            .binding
            .as_ref()
            .ok_or("live preview requires an exact source binding")?;
        if binding.description_id != source.id
            || launch.source_hash != source.source_hash
            || launch.revision != source.model_revision
        {
            return Err("live capture belongs to another description".into());
        }
        let mut panel = Self {
            api_replies: Default::default(),
            publisher: None,
            animation_observables: BTreeSet::new(),
            launch,
            source,
            client: None,
            description: None,
            status: None,
            history: None,
            selected: BTreeSet::new(),
            subscribed: BTreeSet::new(),
            pending: None,
            error: None,
            cursor: None,
            last_sample: None,
        };
        panel.restart();
        Ok(panel)
    }
    fn restart(&mut self) {
        self.api_replies.clear();
        self.client = None;
        self.status = None;
        self.description = None;
        self.history = None;
        self.pending = None;
        self.subscribed.clear();
        self.cursor = None;
        self.last_sample = None;
        self.error = None;
        self.launch.run_id = format!(
            "viewer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let result = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| {
                Client::spawn(
                    &exe.with_file_name("sim-system-worker"),
                    &[],
                    self.launch.clone(),
                )
            });
        match result {
            Ok(c) => self.client = Some(c),
            Err(e) => {
                self.error = Some(format!(
                    "Cannot start simulation worker: {e}. Build sim-system-worker first."
                ))
            }
        }
    }
    fn send(&mut self, command: Command) {
        if self.pending.is_some() {
            return;
        }
        if let (Some(client), Some(status)) = (&mut self.client, &self.status) {
            match client.command(status.generation, command) {
                Ok(id) => self.pending = Some(id),
                Err(e) => self.error = Some(e),
            }
        }
    }
    fn accept_reply(&mut self, reply: Reply) -> Result<(), String> {
        if let Some(d) = reply.description {
            sim_inspect::live::validate_runtime_description(&self.source, &d)
                .map_err(|e| e.to_string())?;
            self.description = Some(d);
        }
        if self
            .status
            .as_ref()
            .is_none_or(|s| s.generation != reply.status.generation)
        {
            self.history = Some(History::new(
                reply.status.run_id.clone(),
                reply.status.generation,
                HISTORY_FRAMES,
            ));
            self.cursor = None;
        }
        self.status = Some(reply.status);
        if let Some(frame) = reply.frame {
            self.accept_frame(frame)?;
        }
        Ok(())
    }
    fn accept_frame(&mut self, frame: SampleFrame) -> Result<(), String> {
        let (Some(status), Some(description), Some(history)) =
            (&self.status, &self.description, &mut self.history)
        else {
            return Ok(());
        };
        // Reliable replies can overtake an older coalesced display frame.
        if frame.run_id != status.run_id
            || frame.generation != status.generation
            || history
                .frames()
                .back()
                .is_some_and(|f| frame.sequence <= f.sequence)
        {
            return Ok(());
        }
        history
            .push(description, frame)
            .map_err(|e| e.to_string())?;
        self.last_sample = Some(std::time::Instant::now());
        Ok(())
    }
    fn poll(&mut self) {
        let events = self.client.as_mut().map(Client::poll).unwrap_or_default();
        for event in events {
            let result = match event {
                Event::Building => Ok(()),
                Event::Ready { reply } => self.accept_reply(reply),
                Event::Reply { id, reply } => {
                    let value = serde_json::json!(reply);
                    if self.pending == Some(id) {
                        self.pending = None;
                    }
                    let result = self.accept_reply(reply);
                    if self.api_replies.len() >= 32 {
                        self.api_replies.pop_first();
                    }
                    self.api_replies
                        .insert(id, result.as_ref().map(|_| value).map_err(Clone::clone));
                    result
                }
                Event::Frame { status, frame } => {
                    if self.status.as_ref().is_some_and(|s| {
                        s.run_id == status.run_id
                            && s.generation == status.generation
                            && status.sequence > s.sequence
                    }) {
                        self.status = Some(status);
                    }
                    self.accept_frame(frame)
                }
                Event::Error {
                    id,
                    status,
                    message,
                } => {
                    if let Some(id) = id {
                        self.api_replies.insert(id, Err(message.clone()));
                    }
                    if id == self.pending {
                        self.pending = None;
                    }
                    if let Some(status) = status {
                        self.status = Some(status);
                    }
                    Err(message)
                }
                Event::Exited { code } => {
                    self.client = None;
                    self.pending = None;
                    Err(format!(
                        "Simulation worker exited ({code:?}); retained graphs are still available."
                    ))
                }
            };
            if let Err(e) = result {
                self.error = Some(e);
            }
        }
        let wanted: BTreeSet<_> = self
            .selected
            .union(&self.animation_observables)
            .cloned()
            .collect();
        if self.pending.is_none()
            && self.status.is_some()
            && self.client.is_some()
            && self.subscribed != wanted
        {
            self.send(Command::Subscribe {
                observables: wanted.iter().cloned().collect(),
            });
            if self.pending.is_some() {
                self.subscribed = wanted;
            }
        }
    }
    fn publish(&mut self) {
        let frame = self
            .history
            .as_ref()
            .and_then(|h| h.frames().back())
            .filter(|f| {
                self.status.as_ref().is_some_and(|s| {
                    f.run_id == s.run_id && f.generation == s.generation && f.sequence == s.sequence
                })
            })
            .cloned();
        if let Some(publisher) = &mut self.publisher {
            let snapshot = sim_inspect::live::LiveSnapshot {
                version: 1,
                source_description_id: self.source.id.clone(),
                description: self.description.clone(),
                status: self.status.clone(),
                frame,
                error: self.error.clone().or_else(|| {
                    (self
                        .status
                        .as_ref()
                        .is_some_and(|s| s.phase == Phase::Running)
                        && self
                            .last_sample
                            .is_some_and(|t| t.elapsed() > Duration::from_secs(2)))
                    .then(|| "Waiting for simulation — holding last sample".into())
                }),
            };
            if let Err(e) = publisher.publish(snapshot) {
                self.error = Some(e);
            }
        }
    }
    pub(super) fn summary(&self) -> String {
        self.status
            .as_ref()
            .map(|s| format!("{:?} · {:.3} s", s.phase, s.time))
            .unwrap_or_else(|| "Simulation worker starting".into())
    }
    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Live simulation");
            let phase = self.status.as_ref().map(|s| s.phase);
            let ready = self.pending.is_none() && self.client.is_some();
            if ui
                .add_enabled(
                    ready && phase == Some(Phase::Paused),
                    egui::Button::new("Run"),
                )
                .clicked()
            {
                self.send(Command::Start);
            }
            if ui
                .add_enabled(
                    ready && phase == Some(Phase::Running),
                    egui::Button::new("Pause"),
                )
                .clicked()
            {
                self.send(Command::Pause);
            }
            if ui
                .add_enabled(
                    ready && phase == Some(Phase::Paused),
                    egui::Button::new("Step"),
                )
                .clicked()
            {
                self.send(Command::Step);
            }
            if ui
                .add_enabled(ready && phase.is_some(), egui::Button::new("Reset"))
                .on_hover_text("Rebuild the captured system and clear this run's graphs")
                .clicked()
            {
                self.send(Command::Reset);
            }
            if ui
                .add_enabled(self.client.is_some(), egui::Button::new("Cancel"))
                .clicked()
            {
                if let Some(mut client) = self.client.take() {
                    if let Err(e) = client.terminate() {
                        self.error = Some(e);
                    }
                }
                self.pending = None;
                if let Some(status) = &mut self.status {
                    status.phase = Phase::Cancelled;
                }
            }
            if ui
                .add_enabled(self.client.is_none(), egui::Button::new("Restart worker"))
                .clicked()
            {
                self.restart();
            }
            if let Some(status) = &self.status {
                ui.label(format!(
                    "{:?} · {:.3} s · step {}",
                    status.phase, status.time, status.step
                ));
            } else {
                ui.label(if self.client.is_some() {
                    "Building…"
                } else {
                    "Offline"
                });
            }
            ui.small(format!(
                "Δt {} s · seed {}",
                self.launch.config.interval, self.launch.config.seed
            ));
        });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::DARK_RED, e);
        }
    }
    fn picker(&mut self, ui: &mut egui::Ui, target: &SelectionTarget) {
        ui.strong("Graph over time");
        let d = self.description.as_ref().unwrap_or(&self.source);
        let choices = plot::options(d, target).unwrap_or_default();
        if choices.is_empty() {
            ui.small("Select a connection, port, or component to see its quantities.");
        }
        for o in choices {
            let mut checked = self.selected.contains(&o.id);
            let enabled = self.description.is_some()
                && o.availability == Availability::Available
                && (checked || self.selected.len() < MAX_PLOTS);
            let reason = match &o.availability {
                Availability::Available => o
                    .sign_convention
                    .as_deref()
                    .unwrap_or("Committed runtime observation"),
                Availability::Unavailable { reason } => reason,
            };
            let response = ui
                .add_enabled(
                    enabled,
                    egui::Checkbox::new(&mut checked, plot::label(d, o)),
                )
                .on_hover_text(format!("{reason}\n{}", o.id));
            if response.changed() {
                if checked {
                    self.selected.insert(o.id.clone());
                } else {
                    self.selected.remove(&o.id);
                }
            }
            if let Availability::Unavailable { reason } = &o.availability {
                ui.small(reason);
            }
        }
        if self.description.is_none() {
            ui.small("Measurements become selectable when the worker is ready.");
        }
        ui.small(format!(
            "Graphs stay selected while you inspect other parts. Up to {MAX_PLOTS} at once."
        ));
        ui.separator();
    }
    fn graphs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Time graphs");
            if ui.small_button("Clear graphs").clicked() {
                self.selected.clear();
            }
            ui.small("Latest 2,000 display frames · decimated, not a full recording");
        });
        if self.selected.is_empty() {
            ui.label("Click a connection and choose quantities under “Graph over time”.");
            return;
        }
        let (Some(d), Some(history)) = (&self.description, &self.history) else {
            return;
        };
        let xmin = self
            .selected
            .iter()
            .flat_map(|id| history.series(id).into_iter().flatten().map(|p| p.time))
            .fold(history.frames().front().map_or(0., |f| f.time), f64::min);
        let xmax = history
            .frames()
            .back()
            .map_or(1., |f| f.time)
            .max(xmin + self.launch.config.interval);
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("live-graphs")
            .show(ui, |ui| {
                for id in &self.selected {
                    let Some(o) = d.observables.get(id) else {
                        continue;
                    };
                    ui.horizontal(|ui| {
                        ui.strong(plot::label(d, o));
                        if ui.small_button("×").on_hover_text("Remove graph").clicked() {
                            remove = Some(id.clone());
                        }
                    });
                    if let Some(sign) = &o.sign_convention {
                        ui.small(sign);
                    }
                    sim_diagram::plot::TimeGraph::new(&history.series(id))
                        .unit(plot::unit(d, o))
                        .time_range([xmin, xmax])
                        .show(ui, &mut self.cursor);
                }
            });
        if let Some(id) = remove {
            self.selected.remove(&id);
        }
    }
}

impl Viewer {
    pub(super) fn load_live(&mut self, path: &Path) -> Result<(), String> {
        let launch = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.live = Some(LivePanel::new(launch, self.description.clone())?);
        Ok(())
    }
    pub(super) fn link_animation(
        &mut self,
        path: &Path,
        spatial_path: &Path,
    ) -> Result<(), String> {
        let animation: sim_inspect::animation::AnimationDescription =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let spatial =
            serde_json::from_slice(&std::fs::read(spatial_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        animation
            .validate(&self.description, &spatial)
            .map_err(|e| e.to_string())?;
        let link = self
            .link
            .as_ref()
            .ok_or("animation requires a linked selection session")?;
        let live = self
            .live
            .as_mut()
            .ok_or("animation requires a live session")?;
        live.animation_observables = animation.observables();
        live.publisher = Some(sim_inspect::live::native::Publisher::new(
            link.client.directory.clone(),
        ));
        Ok(())
    }
    pub(super) fn live_target(&self) -> SelectionTarget {
        self.link
            .as_ref()
            .filter(|l| l.client.description_id() == self.description.id)
            .map(|l| l.target.clone())
            .unwrap_or_else(|| {
                self.projection.source_selection(
                    self.diagram.selected.as_ref(),
                    &self.diagram.marked_components,
                )
            })
    }
    pub(super) fn live_picker(&mut self, ui: &mut egui::Ui) {
        let target = self.live_target();
        if let Some(live) = &mut self.live {
            if live.source.id == self.description.id {
                live.picker(ui, &target);
            }
        }
    }
    pub(super) fn live_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(live) = &mut self.live {
            live.poll();
            if live.client.is_some() || live.publisher.is_some() {
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
            egui::Panel::bottom("live-simulation")
                .default_size(260.)
                .min_size(if live.selected.is_empty() { 60. } else { 220. })
                .resizable(true)
                .show(ui, |ui| {
                    live.controls(ui);
                    if live.source.id == self.description.id {
                        live.graphs(ui);
                    } else {
                        ui.label("Live worker belongs to another open model.");
                    }
                });
            live.publish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panel() -> LivePanel {
        let source: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
        ))
        .unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/systems-viewer/spatial/motor-thermal.live.json");
        let launch = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut d = source.clone();
        for o in d.observables.values_mut() {
            o.availability = Availability::Available;
        }
        d.seal().unwrap();
        LivePanel {
            api_replies: Default::default(),
            publisher: None,
            animation_observables: BTreeSet::new(),
            launch,
            source,
            client: None,
            description: Some(d),
            status: None,
            history: None,
            selected: BTreeSet::new(),
            subscribed: BTreeSet::new(),
            pending: None,
            error: None,
            cursor: None,
            last_sample: None,
        }
    }
    #[test]
    fn clicking_a_connection_option_pins_graph_across_selection_changes() {
        let mut panel = panel();
        let net = panel
            .source
            .nets
            .values()
            .find(|n| n.ports.len() == 3)
            .unwrap()
            .id
            .clone();
        let target = SelectionTarget::net(&net);
        let expected = plot::options(panel.description.as_ref().unwrap(), &target).unwrap()[0]
            .id
            .clone();
        let label = plot::label(
            panel.description.as_ref().unwrap(),
            &panel.description.as_ref().unwrap().observables[&expected],
        );
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600., 900.),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input(vec![]), |ui| panel.picker(ui, &target));
        out.textures_delta.clear();
        let pos = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.pos + egui::vec2(3., 5.))
                }
                _ => None,
            })
            .expect("connection graph choice must be rendered");
        let mut output = ctx.run_ui(
            input(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]),
            |ui| panel.picker(ui, &target),
        );
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            input(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]),
            |ui| panel.picker(ui, &target),
        );
        output.textures_delta.clear();
        assert!(panel.selected.contains(&expected));
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            panel.picker(
                ui,
                &SelectionTarget::component("example/motor-thermal/motor"),
            );
            panel.graphs(ui);
        });
        output.textures_delta.clear();
        assert!(
            panel.selected.contains(&expected),
            "inspecting another part must retain graphs"
        );
    }
    #[test]
    fn reset_clears_graph_history_and_old_transport_frames_cannot_restore_it() {
        let mut panel = panel();
        let id = panel
            .description
            .as_ref()
            .unwrap()
            .observables
            .keys()
            .next()
            .unwrap()
            .clone();
        let status = SessionStatus {
            phase: Phase::Paused,
            run_id: "test".into(),
            generation: 0,
            step: 1,
            time: 0.01,
            sequence: 1,
            step_wall_seconds: 0.,
            events: 0,
            message: None,
        };
        let frame = SampleFrame {
            version: sim_inspect::SAMPLE_FRAME_VERSION,
            description_id: panel.description.as_ref().unwrap().id.clone(),
            model_revision: 1,
            run_id: "test".into(),
            generation: 0,
            sequence: 1,
            step: 1,
            time: 0.01,
            values: std::collections::BTreeMap::from([(
                id,
                sim_inspect::SampleValue::Committed {
                    value: 3.,
                    sample_time: 0.01,
                },
            )]),
        };
        panel
            .accept_reply(Reply {
                status: status.clone(),
                description: None,
                frame: Some(frame.clone()),
                recording: None,
            })
            .unwrap();
        assert_eq!(panel.history.as_ref().unwrap().frames().len(), 1);
        let mut reset = status;
        reset.generation = 1;
        reset.step = 0;
        reset.time = 0.;
        reset.sequence = 0;
        panel
            .accept_reply(Reply {
                status: reset,
                description: None,
                frame: None,
                recording: None,
            })
            .unwrap();
        panel.accept_frame(frame).unwrap();
        assert!(panel.history.as_ref().unwrap().frames().is_empty());
    }
}

impl LivePanel {
    pub(super) fn api_tick(&mut self) {
        self.poll();
        self.publish();
    }
    pub(super) fn api_state(&self) -> serde_json::Value {
        serde_json::json!({"status":self.status,"pending":self.pending,"error":self.error,"selected":self.selected,"subscribed":self.subscribed,"cursor":self.cursor,"config":self.launch.config,"frame":self.history.as_ref().and_then(|h|h.frames().back())})
    }
    pub(super) fn api_description(&self) -> serde_json::Value {
        serde_json::json!(self.description)
    }
    pub(super) fn api_history(&self) -> serde_json::Value {
        serde_json::json!(self.history.as_ref().map(|h| h.frames()))
    }
    pub(super) fn api_graphs(
        &mut self,
        observables: BTreeSet<String>,
        cursor: Option<f64>,
    ) -> Result<(), String> {
        if observables.len() > MAX_PLOTS || cursor.is_some_and(|t| !t.is_finite() || t < 0.) {
            return Err("at most 8 graphs and a finite nonnegative cursor are required".into());
        }
        let d = self
            .description
            .as_ref()
            .ok_or("simulation description is not ready")?;
        for id in &observables {
            let observable = d
                .observables
                .get(id)
                .ok_or_else(|| format!("unknown observable {id}"))?;
            if let Availability::Unavailable { reason } = &observable.availability {
                return Err(format!("observable {id} is unavailable: {reason}"));
            }
        }
        self.selected = observables;
        self.cursor = cursor;
        Ok(())
    }
    pub(super) fn api_restart(&mut self) -> Result<(), String> {
        if self.client.is_some() {
            return Err("cancel the existing worker before restarting".into());
        }
        self.restart();
        Ok(())
    }
    pub(super) fn api_cancel(&mut self) -> Result<(), String> {
        if let Some(mut client) = self.client.take() {
            if let Err(error) = client.terminate() {
                self.client = Some(client);
                return Err(error);
            }
        }
        self.pending = None;
        if let Some(status) = &mut self.status {
            status.phase = Phase::Cancelled;
        }
        Ok(())
    }
    pub(super) fn api_command(
        &mut self,
        command: Command,
        continuation: &mut serde_json::Value,
    ) -> sim_api::Outcome {
        use serde_json::json;
        if let Some(id) = continuation.as_u64() {
            if let Some(result) = self.api_replies.remove(&id) {
                return sim_api::Outcome::Done(result);
            }
            if self.client.is_none() {
                return sim_api::Outcome::Done(Err(self
                    .error
                    .clone()
                    .unwrap_or("worker disconnected".into())));
            }
            return sim_api::Outcome::Pending;
        }
        if self.client.is_none() {
            return sim_api::Outcome::Done(Err(
                "no simulation worker; restart or load a capture".into()
            ));
        }
        if self.pending.is_some() || self.status.is_none() {
            return sim_api::Outcome::Pending;
        }
        if let Command::BeginRecording { capacity, .. } = &command {
            if *capacity > 10000 {
                return sim_api::Outcome::Done(Err(
                    "REST recordings are limited to 10000 frames".into()
                ));
            }
        }
        // REST Subscribe uses graph controls instead; otherwise the next UI poll
        // would silently replace that subscription with its selected graphs.
        if matches!(command, Command::Subscribe { .. }) {
            return sim_api::Outcome::Done(Err(
                "use graphs to set the persistent display subscription".into(),
            ));
        }
        self.error = None;
        self.send(command);
        match self.pending {
            Some(id) => {
                *continuation = json!(id);
                sim_api::Outcome::Pending
            }
            None => sim_api::Outcome::Done(Err(self
                .error
                .clone()
                .unwrap_or("command could not be queued".into()))),
        }
    }
}

impl LivePanel {
    pub(super) fn capture_graphs(
        &self,
        options: &sim_render::graphs::Options,
    ) -> Result<(Vec<sim_render::graphs::Panel>, serde_json::Value), String> {
        use sim_render::graphs::{Panel, Series};
        let d = self
            .description
            .as_ref()
            .ok_or("simulation description is not ready")?;
        let history = self
            .history
            .as_ref()
            .ok_or("no simulation measurements yet")?;
        let ids: Vec<_> = if options.observables.is_empty() {
            if self.selected.is_empty() {
                self.animation_observables.iter().cloned().collect()
            } else {
                self.selected.iter().cloned().collect()
            }
        } else {
            options.observables.clone()
        };
        if ids.is_empty() || ids.len() > 8 {
            return Err("select 1 to 8 observable IDs using graphs or render options".into());
        }
        let mut panels = Vec::new();
        for id in &ids {
            let o = d
                .observables
                .get(id)
                .ok_or_else(|| format!("unknown observable {id}"))?;
            let points = history
                .series(id)
                .into_iter()
                .map(|v| v.map(|p| [p.time, p.value]))
                .collect();
            panels.push(Panel {
                title: plot::label(d, o),
                unit: plot::unit(d, o).into(),
                series: vec![Series {
                    label: "Sampled value".into(),
                    color: [16, 133, 151],
                    points,
                }],
            });
        }
        Ok((
            panels,
            serde_json::json!({"source_description_id":self.source.id,"runtime_description_id":d.id,"status":self.status,"observables":ids,"frame_count":history.frames().len(),"sample_timing":"Committed or accepted-stage sample_time, never substituted endpoint times","history":"bounded display samples; use full-rate recording for numerical analysis"}),
        ))
    }
}
