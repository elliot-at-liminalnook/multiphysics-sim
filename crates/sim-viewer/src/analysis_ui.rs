use super::*;
use sim_diagram::CardDecoration;
impl Viewer {
    pub(super) fn sync_view(&mut self, record_edit: bool) {
        if self.diagram.state.positions.is_empty() {
            return;
        }
        let id = self.projection.view.id.clone();
        let changed = self.journal.current.views.get(&id).is_some_and(|old| {
            old.state.positions != self.diagram.state.positions
                || old.state.pinned != self.diagram.state.pinned
        });
        if record_edit && changed {
            let mut next = self.journal.current.clone();
            next.views.insert(
                id,
                SavedView {
                    state: self.diagram.state.clone(),
                    selected: self.diagram.selected.clone(),
                },
            );
            if let Err(e) = self.journal.commit(next, &self.description) {
                self.error = Some(e);
            }
        } else {
            self.journal.current.views.insert(
                id,
                SavedView {
                    state: self.diagram.state.clone(),
                    selected: self.diagram.selected.clone(),
                },
            );
        }
        self.journal.current.collapsed = self.collapsed.clone();
        self.journal.current.focus = self.focus.clone();
        self.journal.current.dimmed_domains = self.diagram.dimmed_domains.clone();
    }
    pub(super) fn decorate(&mut self) {
        self.diagram.decorations.clear();
        for (id, source) in &self.projection.nodes {
            let target = match source {
                NodeSource::Component(id) => Target::Component(id.clone()),
                NodeSource::Group(id) => Target::Group(id.clone()),
            };
            let group = match source {
                NodeSource::Group(id) => self.journal.current.groups.get(id),
                NodeSource::Component(id) => self
                    .journal
                    .current
                    .groups
                    .values()
                    .find(|g| g.members.contains(id)),
            };
            let note = self
                .journal
                .current
                .annotation(&target)
                .map(|a| a.text.clone())
                .unwrap_or_default();
            if group.is_some() || !note.is_empty() {
                self.diagram.decorations.insert(
                    id.clone(),
                    CardDecoration {
                        note,
                        group_label: group.map(|g| format!("Analysis group: {}", g.label)),
                        color: group.map(|g| g.color),
                    },
                );
            }
        }
    }
    fn target_label(&self, target: &Target) -> String {
        match target {
            Target::Component(id) => self.description.components[id].label.clone(),
            Target::Group(id) => self.presentation.groups[id].label.clone(),
            Target::Net(_) => String::new(),
        }
    }
    pub(super) fn begin_annotation(&mut self, target: Target) {
        if self.annotation_editor.is_some() || self.group_editor.is_some() {
            self.error = Some("Apply or close the current editor first.".into());
            return;
        }
        let existing = self.journal.current.annotation(&target);
        self.annotation_editor = Some(AnnotationEditor {
            label: existing
                .and_then(|n| n.label.clone())
                .unwrap_or_else(|| self.target_label(&target)),
            text: existing.map(|n| n.text.clone()).unwrap_or_default(),
            target,
        });
    }
    pub(super) fn begin_group(&mut self, id: Option<String>) {
        if self.annotation_editor.is_some() || self.group_editor.is_some() {
            self.error = Some("Apply or close the current editor first.".into());
            return;
        }
        if let Some(group) = id
            .as_ref()
            .and_then(|id| self.journal.current.groups.get(id))
        {
            self.group_editor = Some(GroupEditor {
                id: Some(group.id.clone()),
                label: group.label.clone(),
                members: group.members.clone(),
                color: group.color,
                search: String::new(),
                note: self
                    .journal
                    .current
                    .annotation(&Target::Group(group.id.clone()))
                    .map(|n| n.text.clone())
                    .unwrap_or_default(),
            });
            return;
        }
        let mut visible = self.diagram.marked_components.clone();
        if visible.is_empty() {
            if let Some(Selection::Component(id)) = &self.diagram.selected {
                visible.insert(id.clone());
            }
        }
        let members = visible
            .iter()
            .filter_map(|id| self.projection.node_members.get(id))
            .flatten()
            .cloned()
            .collect();
        self.group_editor = Some(GroupEditor {
            id: None,
            label: String::new(),
            members,
            color: [165, 112, 35],
            search: String::new(),
            note: String::new(),
        });
    }
    pub(super) fn restore_analysis(&mut self) {
        self.history.retain_mut(|(collapsed, focus)| {
            collapsed.retain(|id| {
                self.description.groups.contains_key(id)
                    || self.journal.current.groups.contains_key(id)
            });
            focus.as_ref().is_none_or(|f| match f {
                NodeSource::Component(id) => self.description.components.contains_key(id),
                NodeSource::Group(id) => {
                    self.description.groups.contains_key(id)
                        || self.journal.current.groups.contains_key(id)
                }
            })
        });
        self.collapsed = self.journal.current.collapsed.clone();
        self.focus = self.journal.current.focus.clone();
        self.diagram.state.positions.clear();
        self.reproject();
        self.diagram.dimmed_domains = self.journal.current.dimmed_domains.clone();
    }
    pub(super) fn apply_analysis(&mut self, action: AnalysisAction) {
        self.sync_view(false);
        let mut next = self.journal.current.clone();
        match action {
            AnalysisAction::SaveAndClose => {
                let annotation_draft = self.annotation_editor.clone();
                if let Some(editor) = self.group_editor.clone() {
                    self.apply_analysis(AnalysisAction::Group(editor));
                    if self.error.is_some() {
                        return;
                    }
                }
                if let Some(editor) = annotation_draft.clone() {
                    self.apply_analysis(AnalysisAction::Annotation(Annotation {
                        target: editor.target,
                        label: Some(editor.label),
                        text: editor.text,
                    }));
                    if self.error.is_some() {
                        self.annotation_editor = annotation_draft;
                        return;
                    }
                }
                self.sync_view(false);
                self.save_workspace();
                if self.error.is_none() {
                    for (id, model) in &mut self.model_workspaces {
                        if model.saved.as_ref() == Some(&model.journal.current) {
                            continue;
                        }
                        let Some(example) = self.examples.iter().find(|e| &e.description.id == id)
                        else {
                            self.error = Some("Cannot find a previously opened model; keep working to recover its workspace.".into());
                            return;
                        };
                        let path = std::path::PathBuf::from(&model.path);
                        if model.file.as_ref().is_none_or(|file| file.path != path) {
                            model.file = Some(WorkspaceFile::new_path(path));
                        }
                        let file = model.file.as_mut().unwrap();
                        if let Err(error) = file.save(&model.journal.current, &example.description)
                        {
                            self.error = Some(format!(
                                "Could not save {} to {}: {error}. Keep working and switch to that model to choose another filename.",
                                example.label, model.path
                            ));
                            return;
                        }
                        model.saved = Some(model.journal.current.clone());
                    }
                    self.allow_close = true;
                }
                return;
            }
            AnalysisAction::Save => {
                self.save_workspace();
                return;
            }
            AnalysisAction::Undo => {
                if self.journal.undo() {
                    self.restore_analysis();
                }
                return;
            }
            AnalysisAction::Redo => {
                if self.journal.redo() {
                    self.restore_analysis();
                }
                return;
            }
            AnalysisAction::Annotation(mut annotation) => {
                annotation.text = annotation.text.trim().into();
                annotation.label = annotation
                    .label
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().into());
                if annotation.text.is_empty() && annotation.label.is_none() {
                    next.annotations.remove(&annotation.target.key());
                } else {
                    next.annotations.insert(annotation.target.key(), annotation);
                }
            }
            AnalysisAction::Group(editor) => {
                let id = if let Some(id) = editor.id {
                    id
                } else {
                    loop {
                        let id = format!("analysis/group/{}", next.next_group);
                        let Some(counter) =
                            next.next_group.checked_add(1).filter(|n| *n < u64::MAX)
                        else {
                            self.error = Some("Analysis group identities exhausted".into());
                            return;
                        };
                        next.next_group = counter;
                        if !next.groups.contains_key(&id)
                            && !self.description.groups.contains_key(&id)
                        {
                            break id;
                        }
                    }
                };
                next.groups.insert(
                    id.clone(),
                    AnalysisGroup {
                        id: id.clone(),
                        label: editor.label.trim().into(),
                        members: editor.members,
                        color: editor.color,
                    },
                );
                let target = Target::Group(id.clone());
                if editor.note.trim().is_empty() {
                    next.annotations.remove(&target.key());
                } else {
                    next.annotations.insert(
                        target.key(),
                        Annotation {
                            target,
                            label: None,
                            text: editor.note.trim().into(),
                        },
                    );
                }
                next.collapsed.insert(id.clone());
                next.focus = Some(NodeSource::Group(id));
            }
            AnalysisAction::DeleteGroup(id) => {
                next.groups.remove(&id);
                next.annotations.remove(&Target::Group(id.clone()).key());
                next.collapsed.remove(&id);
                if next.focus == Some(NodeSource::Group(id)) {
                    next.focus = None;
                }
            }
        }
        match self.journal.commit(next, &self.description) {
            Ok(()) => {
                self.error = None;
                self.annotation_editor = None;
                self.group_editor = None;
                self.restore_analysis();
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn load_workspace(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        let (file, workspace) = WorkspaceFile::open(path.clone(), &self.description)?;
        self.workspace_path = path.to_string_lossy().into();
        self.workspace_file = Some(file);
        self.saved_workspace = Some(workspace.clone());
        self.journal = Journal::new(workspace);
        self.restore_analysis();
        Ok(())
    }
    fn save_workspace(&mut self) {
        let path = std::path::PathBuf::from(self.workspace_path.trim());
        if path.as_os_str().is_empty() {
            self.error = Some("Choose a workspace filename".into());
            return;
        }
        if self
            .workspace_file
            .as_ref()
            .is_none_or(|file| file.path != path)
        {
            self.workspace_file = Some(WorkspaceFile::new_path(path));
        }
        match self
            .workspace_file
            .as_mut()
            .unwrap()
            .save(&self.journal.current, &self.description)
        {
            Ok(()) => {
                self.saved_workspace = Some(self.journal.current.clone());
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn analysis_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.journal.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                self.pending_analysis = Some(AnalysisAction::Undo);
            }
            if ui
                .add_enabled(self.journal.can_redo(), egui::Button::new("Redo"))
                .clicked()
            {
                self.pending_analysis = Some(AnalysisAction::Redo);
            }
            if ui
                .button("New analysis group")
                .on_hover_text("Shift-click cards to select several, then group them")
                .clicked()
            {
                self.begin_group(None);
            }
            let dirty = self.saved_workspace.as_ref() != Some(&self.journal.current);
            if ui
                .button(if dirty {
                    "Save workspace •"
                } else {
                    "Save workspace"
                })
                .clicked()
            {
                self.pending_analysis = Some(AnalysisAction::Save);
            }
            if ui.small_button("File…").clicked() {
                self.show_workspace_path = !self.show_workspace_path;
            }
            if !self.diagram.marked_components.is_empty() {
                ui.label(format!("{} selected", self.diagram.marked_components.len()));
                if ui.small_button("Clear selection").clicked() {
                    self.diagram.marked_components.clear();
                    self.diagram.selected = None;
                }
            }
            ui.small(format!(
                "{} notes · {} analysis groups",
                self.journal.current.annotations.len(),
                self.journal.current.groups.len()
            ));
        });
        if self.show_workspace_path {
            ui.horizontal(|ui| {
                ui.label("Workspace file");
                ui.text_edit_singleline(&mut self.workspace_path);
            });
            ui.small(
                "Saved separately from CAD and the model. A changed file will not be overwritten.",
            );
        }
        if ui
            .ctx()
            .input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
        {
            self.pending_analysis = Some(AnalysisAction::Save);
        }
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui
                .ctx()
                .input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z))
            {
                self.pending_analysis = Some(if ui.ctx().input(|i| i.modifiers.shift) {
                    AnalysisAction::Redo
                } else {
                    AnalysisAction::Undo
                });
            }
        }
    }
    pub(super) fn analysis_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut editor) = self.annotation_editor.take() {
            let mut open = true;
            egui::Window::new("Analysis note")
                .id(egui::Id::new("annotation-editor"))
                .open(&mut open)
                .default_width(420.)
                .show(ctx, |ui| {
                    if !matches!(editor.target, Target::Net(_)) {
                        ui.label("Display name");
                        ui.text_edit_singleline(&mut editor.label);
                    }
                    ui.label("Notes");
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.text)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    );
                    ui.small("These annotations do not change the captured physical model.");
                    ui.horizontal(|ui| {
                        if ui.button("Apply note").clicked() {
                            self.pending_analysis = Some(AnalysisAction::Annotation(Annotation {
                                target: editor.target.clone(),
                                label: if matches!(editor.target, Target::Net(_)) {
                                    None
                                } else {
                                    Some(editor.label.clone())
                                },
                                text: editor.text.clone(),
                            }));
                        }
                        if ui.button("Remove annotation").clicked() {
                            self.pending_analysis = Some(AnalysisAction::Annotation(Annotation {
                                target: editor.target.clone(),
                                label: None,
                                text: String::new(),
                            }));
                        }
                    });
                });
            if open {
                self.annotation_editor = Some(editor);
            }
        }
        if let Some(mut editor) = self.group_editor.take() {
            let mut open = true;
            egui::Window::new("Analysis group")
                .id(egui::Id::new("group-editor"))
                .open(&mut open)
                .default_width(480.)
                .show(ctx, |ui| {
                    ui.label("Group name");
                    ui.text_edit_singleline(&mut editor.label);
                    ui.horizontal(|ui| {
                        ui.label("Group color");
                        ui.color_edit_button_srgb(&mut editor.color);
                    });
                    ui.label("Notes");
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.note)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                    ui.separator();
                    ui.label(format!("{} components selected", editor.members.len()));
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.search)
                            .hint_text("Find components to include…"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(260.)
                        .show(ui, |ui| {
                            let needle = editor.search.to_lowercase();
                            let mut components: Vec<_> = self
                                .description
                                .components
                                .values()
                                .filter(|c| {
                                    c.label.to_lowercase().contains(&needle)
                                        || c.component_type.to_lowercase().contains(&needle)
                                })
                                .collect();
                            components.sort_by(|a, b| a.label.cmp(&b.label));
                            for component in components {
                                let mut checked = editor.members.contains(&component.id);
                                if ui.checkbox(&mut checked, &component.label).changed() {
                                    if checked {
                                        editor.members.insert(component.id.clone());
                                    } else {
                                        editor.members.remove(&component.id);
                                    }
                                }
                            }
                        });
                    ui.small("Analysis groups are disjoint. Source CAD groups remain unchanged.");
                    if ui.button("Apply group").clicked() {
                        self.pending_analysis = Some(AnalysisAction::Group(editor.clone()));
                    }
                    if let Some(id) = &editor.id {
                        if ui.button("Remove group (keep components)").clicked() {
                            self.pending_analysis = Some(AnalysisAction::DeleteGroup(id.clone()));
                        }
                    }
                });
            if open {
                self.group_editor = Some(editor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewer() -> Viewer {
        Viewer::new(vec![Example {
            label: "Quadruped".into(),
            description: serde_json::from_str(include_str!(
                "../../../examples/systems-viewer/full-robot.description.json"
            ))
            .unwrap(),
        }])
        .unwrap()
    }
    fn settle(v: &mut Viewer) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while v.diagram.is_layout_pending() {
            v.diagram.poll_layout();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(v.diagram.layout_error.is_none());
        v.sync_view(false);
    }
    #[test]
    fn annotate_group_undo_save_and_reopen_preserves_engineering_work() {
        let mut v = viewer();
        settle(&mut v);
        let original = serde_json::to_vec(&v.description).unwrap();
        let component = "cad/motor/0702845b41d3/winding".to_string();
        assert!(v.description.components.contains_key(&component));
        v.apply_analysis(AnalysisAction::Annotation(Annotation {
            target: Target::Component(component.clone()),
            label: Some("Winding thermal mass".into()),
            text: "Review the winding-to-case path.".into(),
        }));
        settle(&mut v);
        let members: BTreeSet<_> = v
            .description
            .components
            .keys()
            .filter(|id| {
                id.starts_with("cad/motor/0702845b41d3/")
                    && ["/winding", "/case", "/winding_case"]
                        .iter()
                        .any(|end| id.ends_with(end))
            })
            .cloned()
            .collect();
        assert_eq!(members.len(), 3);
        v.apply_analysis(AnalysisAction::Group(GroupEditor {
            id: None,
            label: "Thermal investigation".into(),
            members: members.clone(),
            color: [161, 112, 30],
            search: String::new(),
            note: "Check this path before increasing the current limit.".into(),
        }));
        settle(&mut v);
        assert_eq!(v.journal.current.groups.len(), 1);
        assert_eq!(v.diagram.decorations.len(), 3);
        assert_eq!(serde_json::to_vec(&v.description).unwrap(), original);
        v.apply_analysis(AnalysisAction::Undo);
        settle(&mut v);
        assert!(v.journal.current.groups.is_empty());
        assert!(
            v.journal
                .current
                .annotation(&Target::Component(component.clone()))
                .is_some()
        );
        v.apply_analysis(AnalysisAction::Redo);
        settle(&mut v);
        assert_eq!(v.journal.current.groups.len(), 1);
        let path = std::env::temp_dir().join(format!(
            "sim-viewer-analysis-roundtrip-{}.json",
            std::process::id()
        ));
        assert!(!path.exists());
        v.workspace_path = path.to_string_lossy().into();
        v.apply_analysis(AnalysisAction::Save);
        assert!(v.error.is_none());
        let saved = v.journal.current.clone();
        let mut reopened = viewer();
        reopened.load_workspace(path.clone()).unwrap();
        settle(&mut reopened);
        assert_eq!(reopened.journal.current, saved);
        assert_eq!(
            reopened.presentation.components[&component].label,
            "Winding thermal mass"
        );
        assert_eq!(
            reopened.diagram.decorations[&component].note,
            "Review the winding-to-case path."
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn switching_models_retains_file_bindings_and_close_saves_each_analysis() {
        let mut v = viewer();
        let root =
            std::env::temp_dir().join(format!("sim-viewer-multiple-models-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let first_path = root.join("first.json");
        v.load_workspace(first_path.clone()).unwrap();
        settle(&mut v);
        let first_id = v.description.id.clone();
        let component = v.description.components.keys().next().unwrap().clone();
        v.begin_annotation(Target::Component(component.clone()));
        v.annotation_editor.as_mut().unwrap().text = "First model note".into();
        // Give a second capture independent identity for this persistence test.
        let mut second = v.description.clone();
        second.id = "second-captured-model".into();
        v.examples.push(Example {
            label: "Second capture".into(),
            description: second,
        });
        v.switch(1);
        assert_eq!(v.current, 0, "switching must not discard an open editor");
        assert_eq!(
            v.annotation_editor.as_ref().unwrap().text,
            "First model note"
        );
        v.apply_analysis(AnalysisAction::Annotation(Annotation {
            target: Target::Component(component.clone()),
            label: None,
            text: "First model note".into(),
        }));
        settle(&mut v);
        v.switch(1);
        settle(&mut v);
        let initial_second_path = std::path::PathBuf::from(&v.workspace_path);
        assert_ne!(initial_second_path, first_path);
        let second_path = root.join("second-chosen-filename.json");
        v.workspace_path = second_path.to_string_lossy().into();
        v.apply_analysis(AnalysisAction::Annotation(Annotation {
            target: Target::Component(component.clone()),
            label: None,
            text: "Second model note".into(),
        }));
        settle(&mut v);
        v.switch(0);
        settle(&mut v);
        assert_eq!(v.description.id, first_id);
        assert_eq!(std::path::PathBuf::from(&v.workspace_path), first_path);
        assert_eq!(
            v.journal
                .current
                .annotation(&Target::Component(component.clone()))
                .unwrap()
                .text,
            "First model note"
        );
        v.apply_analysis(AnalysisAction::SaveAndClose);
        assert!(v.error.is_none(), "{:?}", v.error);
        assert!(v.allow_close);
        let (_, first) = WorkspaceFile::open(first_path, &v.examples[0].description).unwrap();
        assert!(
            !initial_second_path.exists(),
            "save must use the edited filename"
        );
        let (_, second) = WorkspaceFile::open(second_path, &v.examples[1].description).unwrap();
        assert_eq!(
            first
                .annotation(&Target::Component(component.clone()))
                .unwrap()
                .text,
            "First model note"
        );
        assert_eq!(
            second
                .annotation(&Target::Component(component))
                .unwrap()
                .text,
            "Second model note"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_group_never_reuses_an_existing_identity_from_a_sidecar() {
        let mut v = viewer();
        let ids: Vec<_> = v.description.components.keys().take(2).cloned().collect();
        for (i, component) in ids.iter().enumerate() {
            v.journal.current.next_group = 1;
            v.apply_analysis(AnalysisAction::Group(GroupEditor {
                id: None,
                label: format!("Review {i}"),
                members: [component.clone()].into(),
                color: [100, 100, 100],
                search: String::new(),
                note: String::new(),
            }));
            assert!(v.error.is_none(), "{:?}", v.error);
        }
        assert_eq!(v.journal.current.groups.len(), 2);
        assert!(
            v.journal.current.groups["analysis/group/1"]
                .members
                .contains(&ids[0])
        );
        assert!(
            v.journal.current.groups["analysis/group/2"]
                .members
                .contains(&ids[1])
        );
    }

    #[test]
    fn invalid_group_keeps_its_draft_and_prevents_closing() {
        let mut v = viewer();
        v.group_editor = Some(GroupEditor {
            id: None,
            label: "Draft".into(),
            members: BTreeSet::new(),
            color: [1, 2, 3],
            search: String::new(),
            note: "Do not lose this draft".into(),
        });
        v.apply_analysis(AnalysisAction::SaveAndClose);
        assert!(!v.allow_close);
        assert!(v.error.is_some());
        assert_eq!(
            v.group_editor.as_ref().unwrap().note,
            "Do not lose this draft"
        );
        assert!(v.journal.current.groups.is_empty());
    }
}
