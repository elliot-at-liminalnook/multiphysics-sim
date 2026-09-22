//! Native schematic preview over actual shared model descriptions.
mod analysis_ui;
mod annotations_ui;
mod experiments_ui;
mod live_ui;
mod render;
mod rest;
mod selection_ui;
mod workspace_file;
use eframe::egui;
use sim_diagram::analysis::{AnalysisGroup, Annotation, Journal, SavedView, Target, Workspace};
use sim_diagram::{
    Diagram, Selection,
    projection::{self, NodeSource, Projection},
};
use sim_inspect::{
    ObservationLocation, SystemDescription,
    model::{IdentityBindings, describe},
};
use std::collections::BTreeSet;
use workspace_file::WorkspaceFile;

struct Example {
    label: String,
    description: SystemDescription,
}
enum Action {
    Expand(String),
    Focus(NodeSource),
    Overview,
    Back,
    Annotate(Target),
    EditGroup(String),
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationEditor {
    target: Target,
    label: String,
    text: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupEditor {
    id: Option<String>,
    label: String,
    members: BTreeSet<String>,
    color: [u8; 3],
    search: String,
    note: String,
}
enum AnalysisAction {
    Annotation(Annotation),
    Group(GroupEditor),
    DeleteGroup(String),
    Undo,
    Redo,
    Save,
    SaveAndClose,
}

struct ModelWorkspace {
    journal: Journal,
    saved: Option<Workspace>,
    file: Option<WorkspaceFile>,
    path: String,
}

struct Viewer {
    api: Option<sim_api::Server>,
    image_task: Option<sim_api::ImageTask>,
    annotations: Option<sim_inspect::annotations::native::Store>,
    note_editor: Option<sim_inspect::annotations::Note>,
    note_base: Option<sim_inspect::annotations::Note>,
    note_pending: Option<(u64, sim_inspect::annotations::Note)>,
    note_navigation: u64,
    note_hover: sim_inspect::selection::SelectionTarget,
    note_pointer_hover: sim_inspect::selection::SelectionTarget,
    live: Option<live_ui::LivePanel>,
    link: Option<selection_ui::LinkedSelection>,
    compact: bool,
    show_browser: bool,
    experiments: experiments_ui::ExperimentsPanel,
    examples: Vec<Example>,
    current: usize,
    description: SystemDescription,
    diagram: Diagram,
    projection: Projection,
    collapsed: BTreeSet<String>,
    focus: Option<NodeSource>,
    requested: Option<Action>,
    show_notes: bool,
    search: String,
    error: Option<String>,
    frames: usize,
    screenshot_requested: bool,
    history: Vec<(BTreeSet<String>, Option<NodeSource>)>,
    screenshot: Option<String>,
    presentation: SystemDescription,
    journal: Journal,
    model_workspaces: std::collections::BTreeMap<String, ModelWorkspace>,
    annotation_paths: std::collections::BTreeMap<String, std::path::PathBuf>,
    saved_workspace: Option<Workspace>,
    workspace_file: Option<WorkspaceFile>,
    workspace_path: String,
    annotation_editor: Option<AnnotationEditor>,
    group_editor: Option<GroupEditor>,
    show_workspace_path: bool,
    pending_analysis: Option<AnalysisAction>,
    confirm_close: bool,
    allow_close: bool,
}

fn describe_model(model: &sim_core::ModelWorld, hash: &str) -> Result<SystemDescription, String> {
    describe(
        model,
        &sim_runtime::registry(),
        hash,
        1,
        &IdentityBindings::default(),
    )
    .map(|v| v.description)
    .map_err(|e| e.to_string())
}

impl Viewer {
    fn new(examples: Vec<Example>) -> Result<Self, String> {
        let description = examples[0].description.clone();
        let workspace = Workspace::new(&description);
        let presentation = workspace.presentation(&description)?;
        let collapsed = description.groups.keys().cloned().collect();
        let projection = projection::project(&description, &collapsed, None);
        let mut diagram = Diagram::new(&projection.view);
        diagram.bundles = projection
            .connections
            .iter()
            .map(|(id, nets)| (id.clone(), nets.len()))
            .collect();
        Ok(Self {
            api: None,
            image_task: None,
            annotations: None,
            note_editor: None,
            note_base: None,
            note_pending: None,
            note_navigation: 0,
            note_hover: sim_inspect::selection::SelectionTarget::None,
            note_pointer_hover: sim_inspect::selection::SelectionTarget::None,
            live: None,
            link: None,
            compact: false,
            show_browser: true,
            experiments: Default::default(),
            examples,
            current: 0,
            description,
            diagram,
            projection,
            collapsed,
            focus: None,
            requested: None,
            show_notes: false,
            search: String::new(),
            error: None,
            frames: 0,
            screenshot_requested: false,
            history: vec![],
            screenshot: std::env::var("SIM_VIEWER_SCREENSHOT").ok(),
            presentation,
            journal: Journal::new(workspace),
            model_workspaces: Default::default(),
            annotation_paths: Default::default(),
            saved_workspace: None,
            workspace_file: None,
            workspace_path: "systems-viewer.workspace.json".into(),
            annotation_editor: None,
            group_editor: None,
            show_workspace_path: false,
            pending_analysis: None,
            confirm_close: false,
            allow_close: false,
        })
    }

    fn reproject(&mut self) {
        if !self.diagram.state.positions.is_empty() {
            self.journal.current.views.insert(
                self.projection.view.id.clone(),
                SavedView {
                    state: self.diagram.state.clone(),
                    selected: self.diagram.selected.clone(),
                },
            );
        }
        let dimmed = self.diagram.dimmed_domains.clone();

        self.presentation = self
            .journal
            .current
            .presentation(&self.description)
            .expect("validated analysis workspace");
        self.projection =
            projection::project(&self.presentation, &self.collapsed, self.focus.as_ref());
        self.diagram = Diagram::new(&self.projection.view);
        self.diagram.dimmed_domains = dimmed;
        if let Some(view) = self.journal.current.views.get(&self.projection.view.id) {
            if self.diagram.restore_state(view.state.clone()).is_ok() {
                self.diagram.selected = view.selected.clone();
            }
        }
        self.diagram.bundles = self
            .projection
            .connections
            .iter()
            .map(|(id, nets)| (id.clone(), nets.len()))
            .collect();
        self.decorate();
    }

    fn switch(&mut self, index: usize) {
        if self.annotation_editor.is_some()
            || self.group_editor.is_some()
            || self.note_editor.is_some()
            || self.note_pending.is_some()
        {
            self.error = Some("Apply or close the current editor before switching models.".into());
            return;
        }
        self.sync_view(false);
        let next_description = self.examples[index].description.clone();
        let default_path = std::path::PathBuf::from(&self.workspace_path).with_file_name(format!(
            "systems-{}.workspace.json",
            &blake3::hash(next_description.id.as_bytes()).to_hex()[..12]
        ));
        let next = self
            .model_workspaces
            .remove(&next_description.id)
            .unwrap_or_else(|| ModelWorkspace {
                journal: Journal::new(Workspace::new(&next_description)),
                saved: None,
                file: Some(WorkspaceFile::new_path(default_path.clone())),
                path: default_path.to_string_lossy().into(),
            });
        let previous = ModelWorkspace {
            journal: std::mem::replace(&mut self.journal, next.journal),
            saved: std::mem::replace(&mut self.saved_workspace, next.saved),
            file: std::mem::replace(&mut self.workspace_file, next.file),
            path: std::mem::replace(&mut self.workspace_path, next.path),
        };
        self.model_workspaces
            .insert(self.description.id.clone(), previous);
        self.description = next_description;
        self.diagram.state.positions.clear();
        self.collapsed = self.journal.current.collapsed.clone();
        self.focus = self.journal.current.focus.clone();
        self.current = index;
        if self.annotations.is_some() {
            self.note_navigation = 0;
            self.note_editor = None;
            self.note_pending = None;
            let path = self
                .annotation_paths
                .get(&self.description.id)
                .cloned()
                .unwrap_or_else(|| {
                    std::path::PathBuf::from(&self.workspace_path)
                        .with_extension("annotations.json")
                });
            self.connect_annotations(path);
        }
        self.history.clear();
        self.error = None;
        self.reproject();
    }

    fn apply(&mut self, action: Action) {
        match &action {
            Action::Annotate(target) => {
                self.begin_annotation(target.clone());
                return;
            }
            Action::EditGroup(id) => {
                self.begin_group(Some(id.clone()));
                return;
            }
            _ => {}
        }
        if !matches!(action, Action::Back) {
            self.history
                .push((self.collapsed.clone(), self.focus.clone()));
            if self.history.len() > 64 {
                self.history.remove(0);
            }
        }

        match action {
            Action::Annotate(_) | Action::EditGroup(_) => unreachable!(),
            Action::Back => {
                if let Some((collapsed, focus)) = self.history.pop() {
                    self.collapsed = collapsed;
                    self.focus = focus;
                }
            }
            Action::Expand(id) => {
                if !self.collapsed.remove(&id) {
                    self.collapsed.insert(id);
                }
            }
            Action::Focus(source) => self.focus = Some(source),
            Action::Overview => {
                self.focus = None;
                self.collapsed = self.presentation.groups.keys().cloned().collect();
            }
        }
        self.reproject();
        if let Some(NodeSource::Component(id)) = &self.focus {
            self.diagram.selected = Some(Selection::Component(id.clone()));
        }
        self.journal.current.collapsed = self.collapsed.clone();
        self.journal.current.focus = self.focus.clone();
    }

    fn group_tree(&mut self, ui: &mut egui::Ui, id: &str) {
        let group = self.presentation.groups[id].clone();
        let open = !self.collapsed.contains(id);
        let header = egui::CollapsingHeader::new(&group.label)
            .id_salt(id)
            .open(Some(open))
            .show(ui, |ui| {
                let children: Vec<_> = self
                    .presentation
                    .groups
                    .values()
                    .filter(|g| g.parent.as_deref() == Some(id))
                    .map(|g| g.id.clone())
                    .collect();
                for child in children {
                    self.group_tree(ui, &child);
                }
                let members: Vec<_> = self
                    .presentation
                    .components
                    .values()
                    .filter(|c| c.group.as_deref() == Some(id))
                    .map(|c| (c.id.clone(), c.label.clone()))
                    .collect();
                for (component, label) in members {
                    if ui.selectable_label(false, label).clicked() {
                        self.select_source(sim_inspect::selection::SelectionTarget::component(
                            component.clone(),
                        ));
                        self.requested = Some(Action::Focus(NodeSource::Component(component)));
                    }
                }
                if ui.small_button("Focus this subsystem").clicked() {
                    self.requested = Some(Action::Focus(NodeSource::Group(id.into())));
                }
            });
        if header.header_response.clicked() {
            self.requested = Some(Action::Expand(id.into()));
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        self.shared_notes_ui(ui);
        self.live_picker(ui);
        self.linked_inspector(ui);
        ui.add_space(12.);
        ui.label(
            egui::RichText::new("INSPECTOR")
                .small()
                .color(egui::Color32::from_rgb(105, 118, 139)),
        );
        ui.add_space(12.);
        let visible_component = match &self.diagram.selected {
            Some(Selection::Component(id)) => Some(id.clone()),
            Some(Selection::Port(id)) => self
                .projection
                .view
                .ports
                .get(id)
                .map(|p| p.component.clone()),
            _ => None,
        };
        let exact_component = self
            .link
            .as_ref()
            .filter(|link| link.client.description_id() == self.description.id)
            .and_then(|link| match &link.target {
                sim_inspect::selection::SelectionTarget::Components { ids } if ids.len() == 1 => {
                    ids.first().cloned()
                }
                sim_inspect::selection::SelectionTarget::Ports { ids } if ids.len() == 1 => ids
                    .first()
                    .and_then(|id| self.description.ports.get(id))
                    .map(|p| p.component.clone()),
                _ => None,
            });
        let source = exact_component.map(NodeSource::Component).or_else(|| {
            visible_component
                .as_ref()
                .and_then(|id| self.projection.nodes.get(id))
                .cloned()
                .or_else(|| {
                    if self.diagram.selected.is_none() {
                        self.focus.clone()
                    } else {
                        None
                    }
                })
        });
        if let Some(NodeSource::Group(group)) = &source {
            ui.heading(&self.presentation.groups[group].label);
            if ui.button("Annotate / rename").clicked() {
                self.requested = Some(Action::Annotate(Target::Group(group.clone())));
            }
            if self.journal.current.groups.contains_key(group) {
                ui.label("Engineer-owned analysis group");
                if ui.button("Edit analysis group").clicked() {
                    self.requested = Some(Action::EditGroup(group.clone()));
                }
            }
            if let Some(note) = self
                .journal
                .current
                .annotation(&Target::Group(group.clone()))
            {
                ui.label(&note.text);
            }
            let count = visible_component
                .as_ref()
                .and_then(|id| self.projection.node_members.get(id))
                .map_or_else(
                    || {
                        self.presentation
                            .components
                            .keys()
                            .filter(|id| projection::in_group(&self.presentation, id, group))
                            .count()
                    },
                    Vec::len,
                );
            ui.label(format!("{count} components in this subsystem"));
            ui.add_space(12.);
            if ui.button("Expand in overview").clicked() {
                self.requested = Some(Action::Expand(group.clone()));
            }
            if ui.button("Focus connections").clicked() {
                self.requested = Some(Action::Focus(NodeSource::Group(group.clone())));
            }
            ui.add_space(12.);
            ui.label("Dashed bundles represent separate connections. Expand or focus to inspect their individual terminals.");
            return;
        }
        let component = match &source {
            Some(NodeSource::Component(id)) => self.description.components.get(id),
            _ => None,
        };
        if let Some(component) = component {
            ui.heading(&self.presentation.components[&component.id].label);
            if ui.button("Annotate / rename").clicked() {
                self.requested = Some(Action::Annotate(Target::Component(component.id.clone())));
            }
            if let Some(note) = self
                .journal
                .current
                .annotation(&Target::Component(component.id.clone()))
            {
                ui.label(&note.text);
            }
            ui.small(format!("Captured name: {}", component.label));
            ui.label(
                egui::RichText::new(&component.component_type)
                    .monospace()
                    .small(),
            );
            if ui.button("Focus connections").clicked() {
                self.requested = Some(Action::Focus(NodeSource::Component(component.id.clone())));
            }
            if let Some(cad) = &component.cad {
                ui.label(format!("CAD body: {}", cad.body_id));
            }
            if let Some(source) = &component.source {
                ui.collapsing("Captured source", |ui| {
                    ui.label(&source.path);
                    ui.label(&source.artifact_hash);
                });
            }
            ui.add_space(16.);
            ui.strong("Parameters");
            ui.separator();
            for (name, parameter) in &component.parameters {
                ui.label(
                    egui::RichText::new(name)
                        .small()
                        .color(egui::Color32::from_rgb(105, 118, 139)),
                );
                ui.label(format!(
                    "{} {}",
                    parameter.value,
                    parameter.unit.as_deref().unwrap_or("")
                ));
                ui.add_space(6.);
            }
            if component.parameters.is_empty() {
                ui.label("No parameters");
            }
            ui.add_space(12.);
            ui.strong("Ports and quantities");
            ui.separator();
            for port in self
                .description
                .ports
                .values()
                .filter(|p| p.component == component.id)
            {
                ui.label(egui::RichText::new(&port.name).strong());
                for observable in
                    self.description
                        .observables
                        .values()
                        .filter(|o| match &o.location {
                            ObservationLocation::Across { port: p, .. }
                            | ObservationLocation::Through { port: p, .. }
                            | ObservationLocation::Signal { port: p } => p == &port.id,
                            _ => false,
                        })
                {
                    let unit = self
                        .description
                        .definitions
                        .quantities
                        .iter()
                        .find(|q| q.id == observable.quantity)
                        .map(|q| q.canonical_unit.as_str())
                        .unwrap_or("?");
                    let name = match &observable.location {
                        ObservationLocation::Across { lane, .. }
                        | ObservationLocation::Through { lane, .. } => lane.as_str(),
                        _ => "signal",
                    };
                    ui.label(format!("{name}  ·  {unit}"));
                }
                ui.add_space(6.);
            }
            ui.add_space(12.);
            ui.label(
                egui::RichText::new("Parameter provenance has not been supplied for this example.")
                    .small()
                    .weak(),
            );
        } else if let Some(Selection::Net(id)) = &self.diagram.selected {
            let nets = self.inspected_nets(id);
            ui.heading(if nets.len() > 1 {
                "Connection bundle"
            } else {
                "Connection"
            });
            ui.label(format!("{} distinct connections", nets.len()));
            for net_id in nets {
                let net = &self.description.nets[&net_id];
                ui.separator();
                if ui.small_button("Annotate this connection").clicked() {
                    self.requested = Some(Action::Annotate(Target::Net(net_id.clone())));
                }
                if let Some(note) = self
                    .journal
                    .current
                    .annotation(&Target::Net(net_id.clone()))
                {
                    ui.label(&note.text);
                }
                for port in &net.ports {
                    let p = &self.description.ports[port];
                    ui.label(format!(
                        "{} · {}",
                        self.description.components[&p.component].label, p.name
                    ));
                }
            }
        } else {
            ui.heading("Explore the system");
            ui.label("Select a component, terminal, or connection to inspect it.");
        }
    }
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.api_tick(ui.ctx());
        self.sync_selection();
        if self.link.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(25));
        }
        self.frames += 1;
        let discussion_blocks_close = self.note_editor.is_some() || self.note_pending.is_some();
        if discussion_blocks_close && ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error=Some("Save or cancel the open discussion edit before closing. A pending save must finish first.".into());
        }
        if self.screenshot.is_none()
            && !discussion_blocks_close
            && self.experiments.has_unsaved()
            && ui.ctx().input(|i| i.viewport().close_requested())
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.experiments.confirm_exit = true;
        }
        if self.screenshot.is_none()
            && !discussion_blocks_close
            && !self.experiments.has_unsaved()
            && !self.allow_close
            && ui.ctx().input(|i| i.viewport().close_requested())
            && (self.saved_workspace.as_ref() != Some(&self.journal.current)
                || self
                    .model_workspaces
                    .values()
                    .any(|w| w.saved.as_ref() != Some(&w.journal.current))
                || self.annotation_editor.is_some()
                || self.group_editor.is_some())
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if let Some(path) = &self.screenshot {
            ui.ctx().request_repaint();
            if self.frames >= 4
                && !self.diagram.is_layout_pending()
                && !self.experiments.loading()
                && !self.screenshot_requested
            {
                self.screenshot_requested = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            for screenshot in ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| {
                        if let egui::Event::Screenshot { image, .. } = e {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            }) {
                if let Err(error) = image::save_buffer(
                    path,
                    screenshot.as_raw(),
                    screenshot.width() as u32,
                    screenshot.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Could not save requested screenshot to {path}: {error}");
                    std::process::exit(1);
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::Panel::top("toolbar").show(ui, |ui| {
            if self.link.is_some() {
                ui.horizontal(|ui| {
                    ui.label(self.link_status());
                    ui.checkbox(&mut self.show_browser, "Browser");
                });
            }
            ui.add_space(7.);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Systems");
                ui.add_space(24.);
                let mut selected = self.current;
                egui::ComboBox::from_id_salt("model")
                    .selected_text(&self.examples[self.current].label)
                    .show_ui(ui, |ui| {
                        for (i, example) in self.examples.iter().enumerate() {
                            ui.selectable_value(&mut selected, i, &example.label);
                        }
                    });
                if selected != self.current {
                    self.switch(selected);
                }
                ui.add_space(16.);
                if ui
                    .add_enabled(!self.history.is_empty(), egui::Button::new("Back"))
                    .clicked()
                {
                    self.requested = Some(Action::Back);
                }
                if let Some(Selection::Component(id)) = self.diagram.selected.clone() {
                    if self.diagram.state.pinned.contains(&id)
                        && ui.button("Unpin selected").clicked()
                    {
                        self.diagram.state.pinned.remove(&id);
                        self.diagram.state.revision += 1;
                    }
                }
                if ui.button("Fit diagram").clicked() {
                    self.diagram.fit();
                }
                if self.diagram.is_layout_pending() {
                    ui.spinner();
                    if ui.button("Cancel layout").clicked() {
                        self.diagram.cancel_layout();
                    }
                }
                if ui.button("Arrange").clicked() {
                    self.diagram.reset_layout(&self.projection.view);
                }
                if !self.description.groups.is_empty() && ui.button("System overview").clicked() {
                    self.requested = Some(Action::Overview);
                }
                if self.focus.is_some() {
                    ui.label("Focused connections");
                }
                if !self.description.diagnostics.is_empty()
                    && ui
                        .button(format!(
                            "Model notes ({})",
                            self.description.diagnostics.len()
                        ))
                        .clicked()
                {
                    self.show_notes = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("STRUCTURE PREVIEW").small().weak());
                });
            });
            ui.add_space(7.);
            self.analysis_toolbar(ui);
            if ui.button("Experiments").clicked() {
                if self.experiments.open {
                    self.experiments.open = false;
                } else {
                    self.experiments.launch(None, Some(ui.ctx()));
                }
            }
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} components  ·  {} connections",
                    self.description.components.len(),
                    self.description.nets.len()
                ));
                ui.separator();
                ui.label(self.live.as_ref().map(|l| l.summary()).unwrap_or_else(|| if self.experiments.loading() { "Experiment task running".into() } else { "No simulation running".into() }));
                if !self.description.groups.is_empty() {
                    ui.label(format!(
                        "{} internal nets hidden",
                        self.projection.hidden_internal_nets
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(if self.compact { "Right-drag: pan · Scroll: zoom" } else { "Shift-click to select several · drag selection · right-drag to pan · scroll to zoom" });
                });
            });
        });
        if self.show_browser {
            egui::Panel::left("components")
                .default_size(190.)
                .resizable(true)
                .show(ui, |ui| {
                    ui.add_space(12.);
                    ui.strong("System browser");
                    ui.add_space(12.);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search).hint_text("Find a component…"),
                    );
                    ui.add_space(12.);
                    egui::ScrollArea::vertical()
                        .max_height(ui.available_height() - 140.)
                        .show(ui, |ui| {
                            let needle = self.search.to_lowercase();
                            if needle.is_empty() && !self.description.groups.is_empty() {
                                let roots: Vec<_> = self
                                    .presentation
                                    .groups
                                    .values()
                                    .filter(|g| g.parent.is_none())
                                    .map(|g| g.id.clone())
                                    .collect();
                                for group in roots {
                                    self.group_tree(ui, &group);
                                }
                            } else {
                                let mut components: Vec<_> = self
                                    .presentation
                                    .components
                                    .values()
                                    .filter(|c| {
                                        c.label.to_lowercase().contains(&needle)
                                            || c.component_type.to_lowercase().contains(&needle)
                                    })
                                    .map(|c| (c.id.clone(), c.label.clone()))
                                    .collect();
                                components.sort_by(|a, b| a.1.cmp(&b.1));
                                for (id, label) in components {
                                    let selected = self.diagram.selected
                                        == Some(Selection::Component(id.clone()));
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.diagram.marked_components.clear();
                                        self.select_source(
                                            sim_inspect::selection::SelectionTarget::component(
                                                id.clone(),
                                            ),
                                        );
                                        if self.projection.view.components.contains_key(&id) {
                                            self.diagram.selected = Some(Selection::Component(id));
                                        } else {
                                            self.requested =
                                                Some(Action::Focus(NodeSource::Component(id)));
                                        }
                                    }
                                }
                            }
                        });
                    ui.add_space(24.);
                    ui.separator();
                    ui.add_space(8.);
                    ui.strong("Connection domains");
                    if !self.journal.current.groups.is_empty() {
                        ui.small("Card stripes mark your analysis groups.");
                    }
                    ui.small("Dim domains to follow one path. All connections remain present.");
                    let mut domains = std::collections::BTreeMap::new();
                    for port in self.projection.view.ports.values() {
                        let domain =
                            sim_diagram::style::port_domain(&self.projection.view, &port.schema);
                        domains.insert(domain.key.clone(), domain);
                    }
                    for domain in domains.values() {
                        let mut enabled = !self.diagram.dimmed_domains.contains(&domain.key);
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(12., 12.), egui::Sense::hover());
                            if domain.key == "signals" {
                                ui.painter().rect_filled(rect.shrink(2.), 1., domain.color);
                            } else {
                                ui.painter().circle_filled(rect.center(), 4., domain.color);
                            }
                            if ui.checkbox(&mut enabled, &domain.label).changed() {
                                if enabled {
                                    self.diagram.dimmed_domains.remove(&domain.key);
                                } else {
                                    self.diagram.dimmed_domains.insert(domain.key.clone());
                                }
                            }
                        });
                    }
                    if !self.diagram.dimmed_domains.is_empty()
                        && ui.small_button("Emphasize all domains").clicked()
                    {
                        self.diagram.dimmed_domains.clear();
                    }
                    if let Some(error) = &self.diagram.layout_error {
                        ui.colored_label(egui::Color32::DARK_RED, error);
                    }
                    if !self.diagram.layout().unrouted.is_empty() {
                        ui.colored_label(
                            egui::Color32::DARK_RED,
                            format!(
                                "{} terminals could not be routed",
                                self.diagram.layout().unrouted.len()
                            ),
                        );
                    }
                });
        }
        self.live_panel(ui);
        egui::Panel::right("inspector")
            .default_size(if self.compact { 225. } else { 270. })
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(241, 244, 249)))
            .show(ui, |ui| {
                if let Some(source) = &self.focus {
                    let label = match source {
                        NodeSource::Group(id) => &self.presentation.groups[id].label,
                        NodeSource::Component(id) => &self.presentation.components[id].label,
                    };
                    ui.label(
                        egui::RichText::new(format!("  {label}  ·  connected context retained"))
                            .strong(),
                    );
                }
                self.diagram.show(ui, &self.projection.view)
            });
        if !ui.input(|i| i.pointer.primary_down()) && !self.diagram.is_layout_pending() {
            self.sync_view(true);
        }
        self.analysis_windows(ui.ctx());
        if self.confirm_close {
            egui::Window::new("Save analysis before closing?")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Save notes, groups and layouts for all opened models. Open editors will be applied first.");
                    ui.text_edit_singleline(&mut self.workspace_path);
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::DARK_RED, error);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Save and close").clicked() {
                            self.pending_analysis = Some(AnalysisAction::SaveAndClose);
                        }
                        if ui.button("Discard and close").clicked() {
                            self.allow_close = true;
                        }
                        if ui.button("Keep working").clicked() {
                            self.confirm_close = false;
                        }
                    });
                });
        }
        if let Some(action) = self.pending_analysis.take() {
            self.apply_analysis(action);
        }
        if self.allow_close {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(action) = self.requested.take() {
            self.apply(action);
        }
        self.sync_selection();
        self.experiments.show(ui.ctx());
        egui::Window::new("Captured model notes")
            .open(&mut self.show_notes)
            .default_width(600.)
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(600.)
                    .show(ui, |ui| {
                        for diagnostic in &self.description.diagnostics {
                            ui.strong(&diagnostic.code);
                            ui.label(&diagnostic.message);
                            ui.separator();
                        }
                    });
            });
        if let Some(error) = self.error.clone() {
            let mut open = true;
            egui::Window::new("Workspace or model needs attention")
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(error);
                });
            if !open {
                self.error = None;
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    // Keep existing source loading branches, but accept switches in any order.
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--headless" | "--compact" => i += 1,
            "--annotations" | "--description" | "--model" | "--experiments" | "--workspace"
            | "--focus-group" | "--selection-link" | "--live" | "--animation" | "--spatial"
            | "--api-port" => {
                if args.get(i + 1).is_none_or(|v| v.starts_with("--")) {
                    return Err(format!("{} requires a value", args[i]).into());
                }
                i += 2;
            }
            "--help" | "-h" => {
                println!(
                    "sim-viewer [--description FILE | --model FILE | --experiments DIRECTORY] [--workspace FILE] [--live FILE] [--animation FILE --spatial FILE] [--selection-link DIRECTORY] [--compact] [--focus-group ID] [--api-port PORT] [--headless]"
                );
                return Ok(());
            }
            arg => return Err(format!("unknown option {arg}").into()),
        }
    }
    if let Some(i) = args
        .iter()
        .position(|a| a == "--description" || a == "--model" || a == "--experiments")
    {
        let source: Vec<_> = args.drain(i..i + 2).collect();
        args.splice(0..0, source);
    }

    let examples = if args.first().map(String::as_str) == Some("--description") {
        let path = args
            .get(1)
            .ok_or("usage: sim-viewer --description description.json")?;
        let description: SystemDescription = serde_json::from_slice(&std::fs::read(path)?)?;
        description.validate()?;
        vec![Example {
            label: "Captured system".into(),
            description,
        }]
    } else if args.first().map(String::as_str) == Some("--model") {
        let path = args
            .get(1)
            .ok_or("usage: sim-viewer --model model-world.json")?;
        let bytes = std::fs::read(path)?;
        let model = serde_json::from_slice(&bytes)?;
        vec![Example {
            label: path.clone(),
            description: describe_model(&model, &blake3::hash(&bytes).to_hex())?,
        }]
    } else if !args.iter().any(|a| a == "--description" || a == "--model") {
        let captured: serde_json::Value = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/evidence/pre-migration-baseline.json"
        ))?;
        let mut examples = Vec::new();
        for case in captured["cases"].as_array().ok_or("missing examples")? {
            let bytes = serde_json::to_vec(&case["model"])?;
            let model = serde_json::from_slice(&bytes)?;
            let label = match case["name"].as_str().unwrap_or("System") {
                "rc" => "RC circuit",
                "thermal" => "Thermal network",
                "motor_composite" => "Motor + heat",
                "planar_frame" => "Planar body",
                "spatial_frame" => "Spatial body",
                name => name,
            };
            examples.push(Example {
                label: label.into(),
                description: describe_model(&model, &blake3::hash(&bytes).to_hex())?,
            });
        }
        examples
    } else {
        return Err(
            "usage: sim-viewer [--model model-world.json | --description description.json | --experiments archive-directory]".into(),
        );
    };
    let mut viewer = Viewer::new(examples)?;
    if let Some(i) = args.iter().position(|a| a == "--experiments") {
        let path = args
            .get(i + 1)
            .ok_or("--experiments requires an archive directory")?;
        viewer.experiments.launch(Some(path.clone()), None);
    }
    let workspace_path = if let Some(i) = args.iter().position(|a| a == "--workspace") {
        args.get(i + 1)
            .ok_or("--workspace requires a path")?
            .clone()
    } else if args.first().map(String::as_str) == Some("--experiments") {
        "systems-viewer.workspace.json".into()
    } else if matches!(
        args.first().map(String::as_str),
        Some("--description" | "--model")
    ) {
        let path = &args[1];
        format!("{path}.workspace.json")
    } else {
        "systems-viewer.workspace.json".into()
    };
    viewer.workspace_path = workspace_path.clone();
    if let Err(error) = viewer.load_workspace(workspace_path.into()) {
        viewer.error = Some(error);
    }
    let annotation_path = if let Some(i) = args.iter().position(|a| a == "--annotations") {
        args.get(i + 1)
            .ok_or("--annotations requires a path")?
            .clone()
    } else if matches!(
        args.first().map(String::as_str),
        Some("--description" | "--model")
    ) {
        format!("{}.annotations.json", args[1])
    } else {
        format!("{}.annotations.json", viewer.workspace_path)
    };
    viewer.connect_annotations(annotation_path.into());
    if let Some(index) = args.iter().position(|arg| arg == "--focus-group") {
        let group = args
            .get(index + 1)
            .ok_or("--focus-group requires a declared group ID")?;
        if !viewer.description.groups.contains_key(group) {
            return Err(format!("unknown group {group}").into());
        }
        viewer.apply(Action::Focus(NodeSource::Group(group.clone())));
    }
    viewer.compact = args.iter().any(|a| a == "--compact");
    viewer.show_browser = !viewer.compact;
    if let Some(i) = args.iter().position(|a| a == "--selection-link") {
        viewer.connect_selection(
            args.get(i + 1)
                .ok_or("--selection-link requires a session directory")?
                .into(),
        )?;
    }
    if let Some(i) = args.iter().position(|a| a == "--live") {
        viewer.load_live(std::path::Path::new(
            args.get(i + 1).ok_or("--live requires a capture path")?,
        ))?;
    }
    if let Some(i) = args.iter().position(|a| a == "--animation") {
        let j = args
            .iter()
            .position(|a| a == "--spatial")
            .ok_or("--animation requires --spatial")?;
        viewer.link_animation(
            std::path::Path::new(args.get(i + 1).ok_or("missing animation path")?),
            std::path::Path::new(args.get(j + 1).ok_or("missing spatial path")?),
        )?;
    }
    let port = if let Some(i) = args.iter().position(|a| a == "--api-port") {
        args.get(i + 1).ok_or("missing API port")?.parse::<u16>()?
    } else {
        8422
    };
    let api = rest::server(port)?;
    eprintln!("Schematic REST: http://{}", api.address);
    viewer.api = Some(api);
    if args.iter().any(|a| a == "--headless") {
        let ctx = egui::Context::default();
        loop {
            viewer.api_tick(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
    let size = if viewer.compact {
        [880., 850.]
    } else {
        [1440., 900.]
    };
    let window_title = format!("Systems — {}", viewer.examples[viewer.current].label);
    eframe::run_native(
        &window_title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size(size),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(viewer))
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn settle(viewer: &mut Viewer) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while viewer.diagram.is_layout_pending() {
            viewer.diagram.poll_layout();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(viewer.diagram.layout_error.is_none());
    }
    #[test]
    fn focus_and_back_restore_the_engineers_overview_and_selection() {
        let description = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/full-robot.description.json"
        ))
        .unwrap();
        let mut viewer = Viewer::new(vec![Example {
            label: "Quadruped".into(),
            description,
        }])
        .unwrap();
        settle(&mut viewer);
        let id = viewer
            .projection
            .view
            .components
            .keys()
            .next()
            .unwrap()
            .clone();
        viewer.diagram.selected = Some(Selection::Component(id));
        viewer.diagram.state.camera = sim_inspect::Point { x: 100., y: 200. };
        viewer.diagram.state.zoom = 0.7;
        let state = viewer.diagram.state.clone();
        let selected = viewer.diagram.selected.clone();
        viewer.apply(Action::Focus(NodeSource::Group(
            "cad/motor/0702845b41d3".into(),
        )));
        settle(&mut viewer);
        viewer.apply(Action::Back);
        settle(&mut viewer);
        assert!(viewer.focus.is_none());
        assert_eq!(viewer.diagram.state, state);
        assert_eq!(viewer.diagram.selected, selected);
    }
}
