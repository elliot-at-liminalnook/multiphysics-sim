//! Document editing: the palette, the shared command path with undo, levels,
//! placement, port snapping, selection edits, reference images and the
//! library (publish, updates, where used).
use super::*;

impl Builder {
    pub(super) fn refresh_palette(&mut self) {
        let mut items = Vec::new();
        for id in self.document.definitions.keys() {
            if *id == self.document.root || self.definition_id().as_deref() == Some(id.as_str()) {
                continue;
            }
            let d = &self.document.definitions[id];
            items.push(PaletteItem { label: d.label.clone(), detail: format!("subsystem - {id}"), kind: InstanceKind::Subsystem { definition: id.clone() }, library_path: None, domain: ui::interface_category(d.interface.as_deref()).into() });
        }
        if let Ok(entries) = library::list(&self.library_dir, &self.registry) {
            for e in entries {
                if self.document.definitions.contains_key(&e.id) {
                    continue;
                }
                items.push(PaletteItem { label: e.label, detail: format!("library - {}", e.id), kind: InstanceKind::Subsystem { definition: e.id }, library_path: Some(e.path), domain: ui::interface_category(e.interface.as_deref()).into() });
            }
        }
        for e in &self.elements {
            items.push(PaletteItem { label: e.display_name.clone(), detail: e.component_type.clone(), kind: InstanceKind::Element { component_type: e.component_type.clone() }, library_path: None, domain: e.domain.clone() });
        }
        self.palette = items;
    }

    pub(super) fn filtered(&self) -> Vec<&PaletteItem> {
        let f = self.filter.to_lowercase();
        self.palette
            .iter()
            .filter(|p| self.category.is_none_or(|c| ui::category(&p.domain) == c || (c == "Subsystems" && matches!(p.kind, InstanceKind::Subsystem { .. }))))
            .filter(|p| f.is_empty() || p.label.to_lowercase().contains(&f) || p.detail.to_lowercase().contains(&f))
            .collect()
    }

    pub(super) fn definition(&self) -> Option<sim_system::Definition> {
        self.document.definitions.get(&self.definition_id()?).cloned()
    }

    pub(super) fn definition_id(&self) -> Option<String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(&self.level).ok()
    }

    /// Apply shared commands through the file store (with undo).
    pub fn apply(&mut self, label: &str, commands: Vec<SystemCommand>) -> Result<sim_system::store::Applied, String> {
        let result = self.store.apply(&self.registry, label, &commands, Some(self.document.revision));
        match result {
            Ok(applied) => {
                self.status = applied.outcomes.iter().map(|o| {
                    if o.shared_by > 1 { format!("{} (shared by {} placements)", o.message, o.shared_by) } else { o.message.clone() }
                }).collect::<Vec<_>>().join("; ");
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

    pub(super) fn report(&mut self, result: Result<impl Sized, String>) {
        if let Err(e) = result {
            self.action_error=Some(e.clone());
            self.status = e;
            self.panel_dirty = true;
        }
    }

    pub fn reload(&mut self) {
        match self.store.load_valid(&self.registry) {
            Ok(document) => {
                let needs_scene=sim_system::display::scene_hash(&document)!=sim_system::display::scene_hash(&self.document);
                let previous = self.document.revision;
                self.document = document;
                if self.input.is_none() && self.discussion.selected.as_ref().is_some_and(|id|!self.document.discussions.threads.contains_key(id)){self.discussion.selected=None;}
                self.stamp = self.store.stamp();
                if self.definition_id().is_none() {
                    self.level.clear();
                }
                // The selection's items are re-checked by name against the new
                // revision by `picked::sync` (the handler's answer, then `picked::track`).
                self.alternatives = None;
                self.refresh_palette();
                self.updates = self.library_updates();
                self.used_in = None;
                self.scene_dirty |= needs_scene;
                // No compile follows a scene-neutral edit: carry the schematic's compiled source forward.
                if !self.scene_dirty && self.job.is_none() { self.schematic.advance_revision(previous, self.document.revision); }
                self.panel_dirty = true;
            }
            Err(e) => self.status = format!("Could not load the system: {e}"),
        }
    }

    /// Shared-history step; the outcome is shown in the status bar and
    /// returned so REST callers see refusals ("nothing to undo", stale file).
    pub fn undo(&mut self) -> Result<sim_system::store::Applied, String> {
        let result = self.store.undo().map_err(|e| e.to_string());
        match &result {
            Ok(applied) => {
                self.status = applied.outcomes[0].message.clone();
                self.reload();
            }
            Err(e) => self.status = e.clone(),
        }
        self.panel_dirty = true;
        result
    }

    /// Shared-history step; the outcome is shown in the status bar and
    /// returned so REST callers see refusals ("nothing to redo", stale file).
    pub fn redo(&mut self) -> Result<sim_system::store::Applied, String> {
        let result = self.store.redo().map_err(|e| e.to_string());
        match &result {
            Ok(applied) => {
                self.status = applied.outcomes[0].message.clone();
                self.reload();
            }
            Err(e) => self.status = e.clone(),
        }
        self.panel_dirty = true;
        result
    }

    pub fn set_level(&mut self, path: &str) -> Result<(), String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(path).map_err(|e| e.to_string())?;
        self.level = path.trim_matches('/').to_string();
        // The caller clears the selection (`Builder::enter_level`); a fresh
        // builder (a lesson's sandbox) has none.
        self.alternatives = None;
        self.port_menu = None;
        self.connect_from = None;
        self.refresh_palette();
        self.scene_dirty = true;
        self.panel_dirty = true;
        Ok(())
    }

    pub(super) fn full_path(&self, name: &str) -> String {
        sim_system::join_path(&self.level, name)
    }

    /// The instance at this level containing a flattened component path.
    pub fn instance_for_component(&self, component: &str) -> Option<String> {
        schematic::instance_at(&self.level, component)
    }

    pub(super) fn unique_name(&self, base: &str) -> String {
        let taken: BTreeSet<String> = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.instances.keys().cloned().collect()).unwrap_or_default();
        let base: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let base = base.trim_matches('_').chars().take(24).collect::<String>();
        let base = if base.is_empty() { "part".to_string() } else { base };
        (1..).map(|n| format!("{base}{n}")).find(|c| !taken.contains(c)).unwrap()
    }

    /// Propose the next slot on this level's grid; shared validation checks clearance.
    pub(super) fn place(&mut self, pick: &mut Picked, item: PaletteItem) {
        let count=self.definition().map(|d|d.instances.len()).unwrap_or(0);
        let grid=self.grid(); let (a,b,_)=grid.plane.axes(); let mut p=grid.origin_m;
        p[a]+=(count%6) as f32*grid.spacing_m*3.;p[b]+=(count/6) as f32*grid.spacing_m*3.;
        self.place_at(pick,item,p);
    }
    pub(super) fn placement_commands(&self, item: &PaletteItem, position: [f32;3]) -> Result<(String, Vec<SystemCommand>), String> {
        let mut commands = Vec::new();
        if let Some(path) = &item.library_path {
            let definitions=library::import(std::path::Path::new(path)).map_err(|e|e.to_string())?;
            commands.push(SystemCommand::AddDefinitions { definitions });
        }
        let name = self.unique_name(&sim_system::kind_base_name(&item.kind));
        let spec = sim_system::snap::starter(&self.registry, &item.kind, &item.label).at(position);
        commands.push(SystemCommand::AddInstance { at: self.level.clone(), name: name.clone(), instance: spec });
        Ok((name,commands))
    }
    /// Place `item` at `position` and select it.
    pub(super) fn place_at(&mut self, pick: &mut Picked, item: PaletteItem, position: [f32;3]) {
        let result = self.placement_commands(&item, position).and_then(|(name,commands)| {
            let applied = self.apply(&format!("Place {}", item.label), commands)?;
            // Selecting can only fail without an open Build document (then nothing is selected).
            pick.sync(self);
            let _ = pick.set([name]);
            Ok(applied)
        });
        self.report(result);
    }

    /// Port suggestions for `name` at this level, cached per revision.
    pub(crate) fn suggestions(&mut self, name: &str) -> Result<Vec<sim_system::snap::PortSuggestions>, String> {
        let fresh = self.snaps.as_ref().is_some_and(|(n, r, _)| n == name && *r == self.document.revision) && self.level_of_snaps == self.level;
        if let Some(InstanceKind::Subsystem { definition }) = self.spec(name).map(|s| s.kind) {
            if self.used_in.as_ref().is_none_or(|(d, _)| *d != definition) {
                self.used_in = Some((definition.clone(), self.where_used(&definition)));
            }
        }
        if !fresh {
            let result = sim_system::snap::suggestions(&self.document, &self.registry, Some(&self.library_dir), &self.level, name).map_err(|e| e.to_string());
            self.snaps = Some((name.to_string(), self.document.revision, result));
            self.level_of_snaps = self.level.clone();
        }
        self.snaps.as_ref().unwrap().2.clone()
    }

    pub(super) fn cached_suggestions(&self, name: &str) -> Option<&Vec<sim_system::snap::PortSuggestions>> {
        self.snaps.as_ref().filter(|(n, r, _)| n == name && *r == self.document.revision).and_then(|(_, _, s)| s.as_ref().ok())
    }

    /// Attach a suggested candidate to `name.port` as one undoable edit; the
    /// new instance is selected.
    pub(crate) fn snap(&mut self, pick: &mut Picked, name: &str, port: &str, candidate: &sim_system::snap::Candidate) -> Result<String, String> {
        if let Some(conflict) = &candidate.conflict {
            return Err(conflict.clone());
        }
        let new_name = self.unique_name(&sim_system::kind_base_name(&candidate.kind));
        let commands = sim_system::snap::snap(&self.document, &self.registry, &self.level, name, port, candidate, &new_name).map_err(|e| e.to_string())?;
        self.apply(&format!("Snap {} to {name}.{port}", candidate.label), commands)?;
        self.preview = None;
        self.scene_dirty = true;
        self.panel_dirty = true;
        pick.sync(self);
        let _ = pick.set([new_name.clone()]);
        Ok(new_name)
    }

    /// The palette entry for a registry element or definition, for cards.
    pub(super) fn icon(&self, kind: &InstanceKind) -> String {
        match kind {
            InstanceKind::Element { component_type } => self.elements.iter().find(|e|&e.component_type==component_type).map(|e|e.icon.clone()).unwrap_or_else(||sim_core::icons::for_type(component_type).into()),
            InstanceKind::Subsystem { definition } => sim_core::icons::resolve(self.document.definitions.get(definition).map(|d|d.icon.as_str()).unwrap_or(""), definition).into(),
            InstanceKind::Generated { .. } => sim_core::icons::for_type("robot.articulated").into(),
            InstanceKind::Block { .. } => sim_core::icons::for_type("control.pi").into(),
        }
    }
    pub(super) fn palette_item(&self, kind: &InstanceKind) -> Option<PaletteItem> {
        self.palette.iter().find(|p| p.kind == *kind).cloned()
    }

    pub(super) fn datasheet(&self, component_type: &str) -> Option<sim_runtime::bench::Datasheet> {
        let dir = self.library_dir.parent().map(|p| p.join("datasheets"))?;
        serde_json::from_slice(&std::fs::read(sim_runtime::bench::path(&dir, component_type)).ok()?).ok()
    }

    pub(super) fn load_preview_sheet(&mut self) {
        self.preview_sheet = match self.preview.as_ref().map(|p| p.kind.clone()) {
            Some(InstanceKind::Element { component_type }) => self.datasheet(&component_type),
            _ => None,
        };
    }

    pub(crate) fn element_entry(&self, component_type: &str) -> Option<&library::ElementEntry> {
        self.elements.iter().find(|e| e.component_type == component_type)
    }

    pub(super) fn spec(&self, name: &str) -> Option<InstanceSpec> {
        let id = self.definition_id()?;
        self.document.definitions.get(&id)?.instances.get(name).cloned()
    }

    /// Move `names` (the selection) by a display-only step.
    pub(super) fn nudge(&mut self, names: BTreeSet<String>, delta: [f32; 3]) {
        let mut commands = Vec::new();
        for name in names {
            if let Some(spec) = self.spec(&name) {
                let mut placement = spec.placement.clone();
                for k in 0..3 {
                    placement.position[k] += delta[k];
                }
                commands.push(SystemCommand::MoveInstance { at: self.level.clone(), name, placement });
            }
        }
        if !commands.is_empty() {
            let r = self.apply("Move", commands);
            self.report(r);
        }
    }

    /// Group the selected instances into a new subsystem, which is then selected.
    pub(super) fn group_selected(&mut self, pick: &mut Picked) {
        let selected = pick.names();
        if selected.is_empty() {
            self.status = "Select instances to group (shift-click to add).".into();
            return;
        }
        let name = self.unique_name("group");
        let prefix = self.document.root.clone();
        let mut definition = format!("{prefix}.{name}");
        let mut n = 2;
        while self.document.definitions.contains_key(&definition) {
            definition = format!("{prefix}.{name}_{n}");
            n += 1;
        }
        let instances: Vec<String> = selected.into_iter().collect();
        let result = self.apply("Group", vec![SystemCommand::Group { at: self.level.clone(), instances, name: name.clone(), definition, label: format!("Group {name}") }]).and_then(|_| {
            pick.sync(self);
            let _ = pick.set([name]);
            Ok(())
        });
        self.report(result);
    }

    /// Delete the selected instances (they leave the selection with them).
    pub(super) fn remove_selected(&mut self, pick: &mut Picked) {
        let commands: Vec<_> = pick.names().into_iter().map(|n| SystemCommand::RemoveInstance { at: self.level.clone(), name: n }).collect();
        if commands.is_empty() {
            return;
        }
        let r = self.apply("Delete", commands).map(|_| {
            let _ = pick.clear();
        });
        self.report(r);
    }

    pub(super) fn reference(&self, id: &str) -> Option<sim_system::ReferenceImage> {
        self.document.definitions.get(&self.definition_id()?)?.references.get(id).cloned()
    }

    pub fn import_image(&mut self, path: PathBuf) -> Result<(), String> {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let taken: BTreeSet<String> = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.references.keys().cloned().collect()).unwrap_or_default();
        let base: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).take(24).collect();
        let id = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}_{n}") }).find(|c| !taken.contains(c) && sim_system::valid_name(c)).unwrap_or_else(|| "image".into());
        let origin = self.subsystems.get(&self.level).map(|w| w.position).unwrap_or([0.; 3]);
        let applied = self.store.import_reference(&self.registry, &self.level, &id, &path, ReferenceView::Spatial, [origin[0], origin[1] - 0.002, origin[2]], 0.1).map_err(|e| e.to_string())?;
        self.status = format!("{} - adjust width, opacity or calibration in the References tab", applied.outcomes.last().map(|o| o.message.clone()).unwrap_or_default());
        self.reload();
        Ok(())
    }

    /// Directory recorded library paths resolve against: the workspace root
    /// (`crate::workspace`), or the error naming what was searched.
    pub(super) fn base_dir(&self) -> Result<PathBuf, String> {
        crate::workspace::root().map(std::path::Path::to_path_buf)
    }

    /// Publish a definition (and what it places) as a new library version,
    /// refreshing library files that bundle it.
    pub fn publish(&mut self, definition: &str) -> Result<Vec<library::Published>, String> {
        let published = library::publish(&self.document, definition, &self.library_dir).map_err(|e| e.to_string())?;
        let changed: Vec<String> = published.iter().filter(|p| p.changed).map(|p| format!("{} v{}", p.id, p.version)).collect();
        self.status = if changed.is_empty() { format!("{definition} is already published with these contents") } else { format!("Published {}", changed.join(", ")) };
        self.updates = self.library_updates();
        self.used_in = None;
        self.refresh_palette();
        self.panel_dirty = true;
        Ok(published)
    }

    /// Imported definitions whose library file has changed.
    pub fn library_updates(&self) -> Vec<library::Stale> {
        // Without a workspace root no recorded library path resolves (reported in system_state.workspace).
        self.base_dir().map(|base| library::stale(&self.document, &base)).unwrap_or_default()
    }

    /// Bring every stale import up to date as one undoable edit.
    pub fn sync_library(&mut self) -> Result<sim_system::store::Applied, String> {
        let commands = library::sync(&self.document, &self.base_dir()?).map_err(|e| e.to_string())?;
        if commands.is_empty() {
            return Err("Library imports are up to date".into());
        }
        self.apply("Update from library", commands)
    }

    /// Where a definition is placed in the system files under the workspace's examples/ and next to this file.
    pub fn where_used(&self, definition: &str) -> Vec<(String, usize)> {
        let mut files = self.base_dir().map(|base| library::system_files(&base.join("examples"))).unwrap_or_default();
        if let Some(dir) = self.store.path.parent() {
            files.extend(library::system_files(dir));
        }
        files.sort();
        files.dedup();
        library::where_used(&files, definition)
    }

    /// Make `instance.parameter` of this level a parameter of the level's definition.
    pub fn expose(&mut self, instance: &str, parameter: &str) -> Result<sim_system::store::Applied, String> {
        let definition = self.definition_id().ok_or("no level")?;
        if definition == self.document.root {
            return Err("Expose works inside a subsystem: open one first".into());
        }
        let taken = self.document.definitions[&definition].parameters.keys().cloned().collect::<BTreeSet<_>>();
        let name = std::iter::once(parameter.to_string()).chain((2..).map(|n| format!("{parameter}_{n}"))).find(|n| !taken.contains(n)).unwrap();
        self.apply("Expose parameter", vec![SystemCommand::ExposeParameter { definition, instance: instance.into(), inner: parameter.into(), parameter: name, description: String::new() }])
    }

    /// Library entry for a registry element, with its notes.
    pub fn component_json(&self, component_type: &str) -> Result<serde_json::Value, String> {
        let entry = self.element_entry(component_type).ok_or_else(|| format!("no palette element `{component_type}`"))?;
        Ok(serde_json::json!({"entry": entry, "datasheet": self.datasheet(component_type)}))
    }
}
