//! Build mode for the schematic: edit a `sim.system/1` file with the same
//! shared commands, validation and undo history as the physical viewer, REST
//! and the CLI. Levels map onto description groups: drilling into a
//! subsystem focuses its group and collapses the rest. Reference images for
//! the schematic view are drawn behind the diagram.
use super::*;
use sim_runtime::system_builder;
use sim_system::library::{self, Alternative};
use sim_system::{Command as SystemCommand, InstanceKind, InstanceSpec, ParameterBinding, ReferenceView, Resolver, SystemDocument, SystemStore, Terminal};
use std::path::PathBuf;

pub(super) struct SystemBuilder {
    store: SystemStore,
    registry: sim_core::BehaviorRegistry,
    pub(super) document: SystemDocument,
    stamp: Option<std::time::SystemTime>,
    checked: std::time::Instant,
    pub(super) level: String,
    pub(super) selected: BTreeSet<String>,
    filter: String,
    status: String,
    library_dir: PathBuf,
    alternatives: Option<(String, Vec<Alternative>)>,
    connect_from: Option<Terminal>,
    edits: std::collections::BTreeMap<(String, String), String>,
    rename: String,
    image_path: String,
    findings: Vec<sim_system::Finding>,
    compile_error: Option<String>,
    build_dir: PathBuf,
    live_path: Option<PathBuf>,
    textures: std::collections::BTreeMap<String, egui::TextureHandle>,
    needs_view: bool,
    /// Computed once; the registry does not change while running.
    elements: Vec<library::ElementEntry>,
    /// Refreshed on reload, not every frame.
    library_entries: Vec<library::LibraryEntry>,
}

impl SystemBuilder {
    pub(super) fn open(path: PathBuf, library_dir: PathBuf) -> Result<Self, String> {
        let registry = sim_runtime::registry();
        let store = SystemStore::new(path.clone());
        let document = store.load_valid(&registry).map_err(|e| e.to_string())?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let build_dir = path.with_file_name(format!(".{}.build", name.trim_end_matches(".system.json")));
        Ok(Self {
            stamp: store.stamp(),
            store,
            registry,
            document,
            checked: std::time::Instant::now(),
            level: String::new(),
            selected: BTreeSet::new(),
            filter: String::new(),
            status: "Build mode. Place parts from the palette; edits are shared with every open editor.".into(),
            library_dir,
            alternatives: None,
            connect_from: None,
            edits: Default::default(),
            rename: String::new(),
            image_path: String::new(),
            findings: Vec::new(),
            compile_error: None,
            build_dir,
            live_path: None,
            textures: Default::default(),
            needs_view: true,
            elements: Vec::new(),
            library_entries: Vec::new(),
        })
        .map(|mut b: Self| {
            b.elements = library::elements(&b.registry);
            b.library_entries = library::list(&b.library_dir, &b.registry).unwrap_or_default();
            b
        })
    }

    /// Compile the current document into a description and runnable capture.
    pub(super) fn compile(&mut self) -> Result<SystemDescription, String> {
        let config = system_builder::config_for(&self.document);
        let compiled = system_builder::compile(&self.document, &self.registry, config.clone())?;
        self.findings = compiled.flat.findings.clone();
        self.compile_error = sim_compile::Runtime::new(compiled.flat.model.clone(), &self.registry, config.integrator).err().map(|e| system_builder::locate(&compiled.flat, e.to_string()));
        let bundle = system_builder::write_bundle(&compiled, &self.build_dir, "system")?;
        self.live_path = Some(bundle.live);
        Ok(compiled.description)
    }

    fn definition_id(&self) -> Option<String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(&self.level).ok()
    }

    fn definition(&self) -> Option<sim_system::Definition> {
        self.document.definitions.get(&self.definition_id()?).cloned()
    }

    pub(super) fn apply(&mut self, label: &str, commands: Vec<SystemCommand>) -> Result<sim_system::store::Applied, String> {
        match self.store.apply(&self.registry, label, &commands, Some(self.document.revision)) {
            Ok(applied) => {
                self.status = applied.outcomes.iter().map(|o| if o.shared_by > 1 { format!("{} (shared by {} placements)", o.message, o.shared_by) } else { o.message.clone() }).collect::<Vec<_>>().join("; ");
                self.reload();
                Ok(applied)
            }
            Err(sim_system::SystemError::Stale { .. }) => {
                self.reload();
                Err("Another editor changed the system; reloaded. Try again.".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub(super) fn reload(&mut self) {
        match self.store.load_valid(&self.registry) {
            Ok(document) => {
                self.document = document;
                self.stamp = self.store.stamp();
                if self.definition_id().is_none() {
                    self.level.clear();
                }
                let names: BTreeSet<String> = self.definition().map(|d| d.instances.keys().cloned().collect()).unwrap_or_default();
                self.selected.retain(|s| names.contains(s));
                self.alternatives = None;
                self.edits.clear();
                self.needs_view = true;
                self.library_entries = library::list(&self.library_dir, &self.registry).unwrap_or_default();
            }
            Err(e) => self.status = format!("Could not load the system: {e}"),
        }
    }

    pub(super) fn set_level(&mut self, path: &str) -> Result<(), String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(path).map_err(|e| e.to_string())?;
        self.level = path.trim_matches('/').to_string();
        self.selected.clear();
        self.alternatives = None;
        self.connect_from = None;
        self.needs_view = true;
        Ok(())
    }

    pub(super) fn instance_for_component(&self, component: &str) -> Option<String> {
        let rest = if self.level.is_empty() { component } else { component.strip_prefix(&format!("{}/", self.level))? };
        rest.split('/').next().map(str::to_string)
    }

    fn unique(&self, base: &str) -> String {
        let taken: BTreeSet<String> = self.definition().map(|d| d.instances.keys().cloned().collect()).unwrap_or_default();
        let base: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).take(24).collect();
        (1..).map(|n| format!("{base}{n}")).find(|c| !taken.contains(c)).unwrap()
    }

    pub(super) fn state_json(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.store.path, "title": self.document.title, "revision": self.document.revision,
            "level": self.level, "definition": self.definition_id(), "selected": self.selected,
            "status": self.status, "findings": self.findings, "compile_error": self.compile_error,
            "history": self.store.history(),
        })
    }
}

impl Viewer {
    /// Poll the shared file and keep the diagram on the current level.
    pub(super) fn system_tick(&mut self, ctx: &egui::Context) {
        let Some(b) = self.system.as_mut() else { return };
        if b.checked.elapsed().as_millis() >= 400 {
            b.checked = std::time::Instant::now();
            if b.store.stamp() != b.stamp {
                b.reload();
                b.status = "Reloaded: the system file changed in another editor.".into();
            }
        }
        if b.needs_view {
            b.needs_view = false;
            let compiled = b.compile();
            match compiled {
                Ok(description) => self.system_replace(description),
                Err(e) => {
                    if let Some(b) = self.system.as_mut() {
                        b.status = format!("Does not compile yet: {e}");
                    }
                }
            }
        }
        self.system_backdrops(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(400));
    }

    fn system_replace(&mut self, description: SystemDescription) {
        let Some(b) = self.system.as_ref() else { return };
        let level = b.level.clone();
        let title = b.document.title.clone();
        let live_path = b.live_path.clone();
        if description.id != self.description.id {
            let had_live = self.live.is_some();
            self.annotation_editor = None;
            self.group_editor = None;
            self.examples[self.current] = Example { label: title, description };
            self.switch(self.current);
            self.model_workspaces.clear();
            self.saved_workspace = Some(self.journal.current.clone());
            if had_live {
                if let Some(path) = &live_path {
                    if let Err(e) = self.load_live(path) {
                        self.error = Some(e);
                    }
                }
            }
        }
        // Level view: this level expanded, everything else collapsed.
        let mut keep = BTreeSet::new();
        let mut path = String::new();
        for part in sim_system::split_path(&level) {
            path = sim_system::join_path(&path, part);
            keep.insert(path.clone());
        }
        self.collapsed = self.description.groups.keys().filter(|g| !keep.contains(*g)).cloned().collect();
        self.focus = (!level.is_empty() && self.description.groups.contains_key(&level)).then(|| NodeSource::Group(level));
        self.history.clear();
        self.reproject();
    }

    fn system_backdrops(&mut self, ctx: &egui::Context) {
        let Some(b) = self.system.as_mut() else { return };
        let Some(definition) = b.definition() else { return };
        let mut backdrops = Vec::new();
        for reference in definition.references.values().filter(|r| r.view == ReferenceView::Schematic && r.visible) {
            let Some(asset) = b.document.assets.get(&reference.asset).cloned() else { continue };
            let texture = match b.textures.get(&reference.asset) {
                Some(t) => t.clone(),
                None => {
                    let path = sim_system::assets::resolve(&b.store.path, &asset);
                    let Ok(decoded) = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| image::load_from_memory(&bytes).map_err(|e| e.to_string())) else {
                        b.status = format!("Could not decode {}", path.display());
                        continue;
                    };
                    let rgba = decoded.to_rgba8();
                    let image = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    let handle = ctx.load_texture(format!("reference-{}", reference.asset), image, egui::TextureOptions::LINEAR);
                    b.textures.insert(reference.asset.clone(), handle.clone());
                    handle
                }
            };
            backdrops.push(sim_diagram::Backdrop { texture: texture.id(), center: sim_inspect::Point { x: reference.origin[0], y: reference.origin[1] }, size: [reference.width, reference.height(&asset)], opacity: reference.opacity });
        }
        self.diagram.backdrops = backdrops;
    }

    /// Diagram clicks select the instance at the current level.
    pub(super) fn system_sync_selection(&mut self) {
        let Some(Selection::Component(id)) = self.diagram.selected.clone() else { return };
        let Some(b) = self.system.as_mut() else { return };
        if let Some(name) = b.instance_for_component(&id) {
            if !b.selected.contains(&name) || b.selected.len() != 1 {
                if b.connect_from.is_none() {
                    b.selected = BTreeSet::from([name]);
                    b.alternatives = None;
                }
            }
        }
    }

    pub(super) fn system_api(&mut self, command: &sim_api::Command) -> sim_api::Result {
        #[derive(serde::Deserialize)]
        #[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
        enum Request {
            System {
                #[serde(default)]
                label: Option<String>,
                commands: Vec<SystemCommand>,
            },
            SystemState,
            SystemLevel {
                path: String,
            },
            SystemSelect {
                names: Vec<String>,
            },
            SystemUndo,
            SystemRedo,
        }
        let b = self.system.as_mut().ok_or("start the schematic with --system FILE to edit systems")?;
        let result = match sim_api::decode::<Request>(command)? {
            Request::System { label, commands } => {
                let label = label.unwrap_or_else(|| format!("{} command(s) via REST", commands.len()));
                b.apply(&label, commands).map(|a| serde_json::json!(a))
            }
            Request::SystemState => Ok(b.state_json()),
            Request::SystemLevel { path } => b.set_level(&path).map(|_| b.state_json()),
            Request::SystemSelect { names } => {
                b.selected = names.into_iter().collect();
                Ok(b.state_json())
            }
            Request::SystemUndo => b.store.undo().map(|a| serde_json::json!(a)).map_err(|e| e.to_string()).inspect(|_| b.reload()),
            Request::SystemRedo => b.store.redo().map(|a| serde_json::json!(a)).map_err(|e| e.to_string()).inspect(|_| b.reload()),
        };
        result
    }

    pub(super) fn system_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut b) = self.system.take() else { return };
        let mut run_requested = false;
        egui::Panel::left("system-builder").default_size(320.).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    ui.strong("Build");
                    ui.weak(format!("rev {}", b.document.revision));
                    if ui.button("Undo").clicked() {
                        match b.store.undo() {
                            Ok(a) => {
                                b.status = a.outcomes[0].message.clone();
                                b.reload();
                            }
                            Err(e) => b.status = e.to_string(),
                        }
                    }
                    if ui.button("Redo").clicked() {
                        match b.store.redo() {
                            Ok(a) => {
                                b.status = a.outcomes[0].message.clone();
                                b.reload();
                            }
                            Err(e) => b.status = e.to_string(),
                        }
                    }
                    if ui.button("Run").on_hover_text("Load the compiled system into the live panel").clicked() {
                        run_requested = true;
                    }
                });
                ui.small(b.store.path.display().to_string());
                ui.label(&b.status);
                if let Some(e) = &b.compile_error {
                    ui.colored_label(egui::Color32::from_rgb(180, 80, 20), format!("Compile: {e}"));
                }
                ui.separator();
                // Breadcrumb.
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(b.level.is_empty(), &b.document.title).clicked() {
                        let _ = b.set_level("");
                    }
                    let mut path = String::new();
                    for part in sim_system::split_path(&b.level.clone()) {
                        path = sim_system::join_path(&path, part);
                        ui.label("›");
                        if ui.selectable_label(path == b.level, part).clicked() {
                            let _ = b.set_level(&path);
                        }
                    }
                    if !b.level.is_empty() && ui.button("Up").clicked() {
                        let parent = b.level.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
                        let _ = b.set_level(&parent);
                    }
                });
                let definition_id = b.definition_id().unwrap_or_default();
                let definition = b.definition();
                if let Some(d) = &definition {
                    let shared = Resolver::new(&b.document, &b.registry).placements(&definition_id);
                    ui.weak(format!("{} · {definition_id}{}", d.label, if shared > 1 { format!(" · shared by {shared}") } else { String::new() }));
                }
                ui.separator();
                ui.strong("Instances");
                if let Some(d) = &definition {
                    for (name, spec) in &d.instances {
                        ui.horizontal(|ui| {
                            let mut on = b.selected.contains(name);
                            if ui.checkbox(&mut on, "").changed() {
                                if on { b.selected.insert(name.clone()); } else { b.selected.remove(name); }
                                b.alternatives = None;
                            }
                            let label = format!("{name} · {}", if spec.label.is_empty() { sim_system::commands::kind_label(&spec.kind) } else { spec.label.clone() });
                            if ui.selectable_label(b.selected.contains(name) && b.selected.len() == 1, label).clicked() {
                                b.selected = BTreeSet::from([name.clone()]);
                                b.alternatives = None;
                                b.rename = name.clone();
                            }
                            if matches!(spec.kind, InstanceKind::Subsystem { .. }) && ui.small_button("Open").clicked() {
                                let path = sim_system::join_path(&b.level.clone(), name);
                                let _ = b.set_level(&path);
                            }
                        });
                    }
                }
                if !b.selected.is_empty() {
                    ui.separator();
                    ui.strong(format!("Selected ({})", b.selected.len()));
                    let only = (b.selected.len() == 1).then(|| b.selected.iter().next().unwrap().clone());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Group").clicked() {
                            let name = b.unique("group");
                            let mut definition = format!("{}.{name}", b.document.root);
                            let mut n = 2;
                            while b.document.definitions.contains_key(&definition) {
                                definition = format!("{}.{name}_{n}", b.document.root);
                                n += 1;
                            }
                            let instances: Vec<String> = b.selected.iter().cloned().collect();
                            let at = b.level.clone();
                            match b.apply("Group", vec![SystemCommand::Group { at, instances, name: name.clone(), definition, label: format!("Group {name}") }]) {
                                Ok(_) => b.selected = BTreeSet::from([name]),
                                Err(e) => b.status = e,
                            }
                        }
                        if ui.button("Delete").clicked() {
                            let at = b.level.clone();
                            let commands = b.selected.iter().map(|n| SystemCommand::RemoveInstance { at: at.clone(), name: n.clone() }).collect();
                            if let Err(e) = b.apply("Delete", commands) {
                                b.status = e;
                            }
                        }
                        if let Some(name) = &only {
                            let spec = definition.as_ref().and_then(|d| d.instances.get(name)).cloned();
                            let subsystem = matches!(spec.as_ref().map(|s| &s.kind), Some(InstanceKind::Subsystem { .. }));
                            if subsystem {
                                if ui.button("Ungroup").clicked() {
                                    let at = b.level.clone();
                                    if let Err(e) = b.apply("Ungroup", vec![SystemCommand::Ungroup { at, name: name.clone() }]) {
                                        b.status = e;
                                    }
                                }
                                if ui.button("Make unique").clicked() {
                                    if let Some(InstanceKind::Subsystem { definition }) = spec.as_ref().map(|s| s.kind.clone()) {
                                        let mut id = format!("{definition}_{name}");
                                        let mut n = 2;
                                        while b.document.definitions.contains_key(&id) {
                                            id = format!("{definition}_{name}_{n}");
                                            n += 1;
                                        }
                                        let at = b.level.clone();
                                        if let Err(e) = b.apply("Make unique", vec![SystemCommand::MakeUnique { at, name: name.clone(), definition: id }]) {
                                            b.status = e;
                                        }
                                    }
                                }
                                if ui.button("Save to library").clicked() {
                                    if let Some(InstanceKind::Subsystem { definition }) = spec.as_ref().map(|s| s.kind.clone()) {
                                        b.status = match library::save(&b.document, &definition, &b.library_dir) {
                                            Ok(p) => format!("Saved {definition} to {}", p.display()),
                                            Err(e) => e.to_string(),
                                        };
                                    }
                                }
                            }
                            if ui.button("Swap…").clicked() {
                                match library::alternatives(&b.document, &b.registry, Some(&b.library_dir), &b.level, name) {
                                    Ok(list) => b.alternatives = Some((name.clone(), list)),
                                    Err(e) => b.status = e.to_string(),
                                }
                            }
                        }
                    });
                    if let Some(name) = only.clone() {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            let r = ui.text_edit_singleline(&mut b.rename);
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && b.rename != name {
                                let at = b.level.clone();
                                let new_name = b.rename.clone();
                                match b.apply("Rename", vec![SystemCommand::RenameInstance { at, name: name.clone(), new_name: new_name.clone() }]) {
                                    Ok(_) => b.selected = BTreeSet::from([new_name]),
                                    Err(e) => b.status = e,
                                }
                            }
                        });
                    }
                    if let Some((name, list)) = b.alternatives.clone() {
                        ui.small(format!("Implementations for {name} (same interface first):"));
                        for alt in list.iter().take(20) {
                            if ui.button(format!("{}{} · {}", if alt.same_interface { "★ " } else { "" }, alt.label, sim_system::commands::kind_label(&alt.kind))).clicked() {
                                let mut commands = Vec::new();
                                if let Some(path) = &alt.library_path {
                                    match library::import(std::path::Path::new(path)) {
                                        Ok(definitions) => commands.push(SystemCommand::AddDefinitions { definitions }),
                                        Err(e) => b.status = e.to_string(),
                                    }
                                }
                                commands.push(SystemCommand::Swap { at: b.level.clone(), name: name.clone(), kind: alt.kind.clone(), keep_parameters: true });
                                if let Err(e) = b.apply(&format!("Swap to {}", alt.label), commands) {
                                    b.status = e;
                                }
                            }
                        }
                    }
                    if let Some(name) = only {
                        if let Some(spec) = definition.as_ref().and_then(|d| d.instances.get(&name)).cloned() {
                            // Parameters.
                            let declared: Vec<(String, String, Option<f64>)> = match &spec.kind {
                                InstanceKind::Element { component_type } => b.registry.get(&component_type.as_str().into()).ok().and_then(|d| d.parameters.clone()).unwrap_or_default().into_iter()
                                    .filter(|p| !p.implementation_reference && !p.name.starts_with("initial.") && !p.name.contains('*')).map(|p| (p.name, p.unit, p.default)).collect(),
                                InstanceKind::Subsystem { definition } => b.document.definitions.get(definition).map(|d| d.parameters.iter().map(|(k, p)| (k.clone(), p.unit.clone(), p.default)).collect()).unwrap_or_default(),
                            };
                            if !declared.is_empty() {
                                ui.small("Parameters (Enter applies; empty clears; $name inherits)");
                            }
                            for (parameter, unit, default) in declared {
                                let current = match spec.parameters.get(&parameter) {
                                    Some(ParameterBinding::Value { value, .. }) => value.to_string(),
                                    Some(ParameterBinding::Parameter { parameter }) => format!("${parameter}"),
                                    None => String::new(),
                                };
                                let key = (name.clone(), parameter.clone());
                                let text = b.edits.entry(key.clone()).or_insert(current.clone());
                                let mut commit = None;
                                ui.horizontal(|ui| {
                                    ui.label(&parameter);
                                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(90.).hint_text(default.map(|d| format!("{d}")).unwrap_or_else(|| "required".into())));
                                    ui.weak(&unit);
                                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && *text != current {
                                        commit = Some(text.trim().to_string());
                                    }
                                });
                                if let Some(value) = commit {
                                    let binding = if value.is_empty() {
                                        Ok(None)
                                    } else if let Some(from) = value.strip_prefix('$') {
                                        Ok(Some(ParameterBinding::Parameter { parameter: from.into() }))
                                    } else {
                                        value.parse::<f64>().map(|v| Some(ParameterBinding::value(v))).map_err(|_| format!("`{value}` is not a number"))
                                    };
                                    let at = b.level.clone();
                                    let result = binding.and_then(|binding| b.apply("Set parameter", vec![SystemCommand::SetParameter { at, name: name.clone(), parameter: parameter.clone(), binding }]).map(|_| ()));
                                    if let Err(e) = result {
                                        b.status = e;
                                    }
                                }
                            }
                            // Ports and connections.
                            ui.small(if b.connect_from.is_some() { "Pick the other terminal" } else { "Ports (click to start a connection)" });
                            if let Ok(ports) = Resolver::new(&b.document, &b.registry).instance_ports(&spec) {
                                let connected: BTreeSet<Terminal> = definition.as_ref().map(|d| d.nets.iter().flat_map(|n| n.terminals.clone()).collect()).unwrap_or_default();
                                for (port, schema) in ports {
                                    let t = Terminal::port(&name, &port);
                                    ui.horizontal(|ui| {
                                        let label = format!("{port} · {}", schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()));
                                        if ui.selectable_label(b.connect_from.as_ref() == Some(&t), label).clicked() {
                                            match b.connect_from.take() {
                                                None => b.connect_from = Some(t.clone()),
                                                Some(from) if from == t => {}
                                                Some(from) => {
                                                    let at = b.level.clone();
                                                    if let Err(e) = b.apply("Connect", vec![SystemCommand::Connect { at, terminals: vec![from, t.clone()], label: String::new() }]) {
                                                        b.status = e;
                                                    }
                                                }
                                            }
                                        }
                                        if connected.contains(&t) && ui.small_button("×").on_hover_text("Disconnect").clicked() {
                                            let at = b.level.clone();
                                            if let Err(e) = b.apply("Disconnect", vec![SystemCommand::Disconnect { at, terminal: t.clone() }]) {
                                                b.status = e;
                                            }
                                        }
                                    });
                                }
                            }
                            if let Some(d) = &definition {
                                if !d.ports.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.weak("Boundary:");
                                        for port in d.ports.keys() {
                                            let t = Terminal::boundary(port);
                                            if ui.selectable_label(b.connect_from.as_ref() == Some(&t), format!("⇱ {port}")).clicked() {
                                                match b.connect_from.take() {
                                                    None => b.connect_from = Some(t),
                                                    Some(from) => {
                                                        let at = b.level.clone();
                                                        if let Err(e) = b.apply("Connect", vec![SystemCommand::Connect { at, terminals: vec![from, t], label: String::new() }]) {
                                                            b.status = e;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                            if b.connect_from.is_some() && ui.button("Cancel connection").clicked() {
                                b.connect_from = None;
                            }
                        }
                    }
                }
                ui.separator();
                ui.strong("Palette");
                ui.add(egui::TextEdit::singleline(&mut b.filter).hint_text("Search components and library…"));
                let needle = b.filter.to_lowercase();
                let mut items: Vec<(String, String, InstanceKind, Option<String>)> = Vec::new();
                for (id, d) in &b.document.definitions {
                    if *id != b.document.root && Some(id) != b.definition_id().as_ref() {
                        items.push((d.label.clone(), format!("subsystem · {id}"), InstanceKind::Subsystem { definition: id.clone() }, None));
                    }
                }
                for e in &b.library_entries {
                    if !b.document.definitions.contains_key(&e.id) {
                        items.push((e.label.clone(), format!("library · {}", e.id), InstanceKind::Subsystem { definition: e.id.clone() }, Some(e.path.clone())));
                    }
                }
                for e in &b.elements {
                    items.push((e.display_name.clone(), e.component_type.clone(), InstanceKind::Element { component_type: e.component_type.clone() }, None));
                }
                egui::ScrollArea::vertical().id_salt("palette").max_height(260.).show(ui, |ui| {
                    for (label, detail, kind, library_path) in items.into_iter().filter(|(l, d, _, _)| needle.is_empty() || l.to_lowercase().contains(&needle) || d.to_lowercase().contains(&needle)).take(120) {
                        if ui.button(format!("+ {label}")).on_hover_text(&detail).clicked() {
                            let mut commands = Vec::new();
                            if let Some(path) = &library_path {
                                match library::import(std::path::Path::new(path)) {
                                    Ok(definitions) => commands.push(SystemCommand::AddDefinitions { definitions }),
                                    Err(e) => b.status = e.to_string(),
                                }
                            }
                            let base = match &kind {
                                InstanceKind::Element { component_type } => component_type.rsplit('.').next().unwrap_or("part").to_string(),
                                InstanceKind::Subsystem { definition } => definition.rsplit('.').next().unwrap_or("sub").to_string(),
                            };
                            let name = b.unique(&base);
                            let count = b.definition().map(|d| d.instances.len()).unwrap_or(0);
                            let mut spec = InstanceSpec::new(kind).at([(count % 6) as f32 * 0.03, 0., (count / 6) as f32 * 0.03]);
                            spec.label = label.clone();
                            commands.push(SystemCommand::AddInstance { at: b.level.clone(), name: name.clone(), instance: spec });
                            match b.apply(&format!("Place {label}"), commands) {
                                Ok(_) => b.selected = BTreeSet::from([name]),
                                Err(e) => b.status = e,
                            }
                        }
                    }
                });
                ui.separator();
                ui.strong("Reference images (schematic)");
                if let Some(d) = &definition {
                    for (id, r) in d.references.iter().filter(|(_, r)| r.view == ReferenceView::Schematic) {
                        let mut reference = r.clone();
                        ui.label(format!("{id}: {}", r.label));
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= ui.add(egui::Slider::new(&mut reference.opacity, 0.05..=1.0).text("opacity")).drag_stopped();
                        });
                        ui.horizontal(|ui| {
                            changed |= ui.add(egui::DragValue::new(&mut reference.width).range(10.0..=5000.0).prefix("width ")).drag_stopped();
                            changed |= ui.add(egui::DragValue::new(&mut reference.origin[0]).prefix("x ")).drag_stopped();
                            changed |= ui.add(egui::DragValue::new(&mut reference.origin[1]).prefix("y ")).drag_stopped();
                        });
                        if changed {
                            let at = b.level.clone();
                            if let Err(e) = b.apply("Adjust reference", vec![SystemCommand::SetReference { at, id: id.clone(), reference }]) {
                                b.status = e;
                            }
                        }
                        if ui.small_button("Remove").clicked() {
                            let at = b.level.clone();
                            if let Err(e) = b.apply("Remove reference", vec![SystemCommand::RemoveReference { at, id: id.clone() }]) {
                                b.status = e;
                            }
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut b.image_path).hint_text("/path/to/image.png").desired_width(180.));
                    if ui.button("Import").clicked() {
                        let path = PathBuf::from(b.image_path.trim());
                        let stem: String = path.file_stem().map(|s| s.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).take(24).collect()).unwrap_or_else(|| "image".into());
                        let result = b.store.import_reference(&b.registry, &b.level.clone(), &stem, &path, ReferenceView::Schematic, [0., 0., 0.], 600.);
                        match result {
                            Ok(_) => {
                                b.status = format!("Imported {} behind the schematic", path.display());
                                b.reload();
                            }
                            Err(e) => b.status = e.to_string(),
                        }
                    }
                });
                if !b.findings.is_empty() {
                    ui.separator();
                    ui.strong(format!("Review ({})", b.findings.len()));
                    for f in b.findings.iter().take(30) {
                        ui.small(format!("• {}", f.message));
                    }
                }
            });
        });
        let live_path = b.live_path.clone();
        self.system = Some(b);
        if run_requested {
            if let Some(path) = live_path {
                match self.load_live(&path) {
                    Ok(()) => {
                        if let Some(b) = self.system.as_mut() {
                            b.status = "Loaded into the live panel below; press Run there.".into();
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }
}
