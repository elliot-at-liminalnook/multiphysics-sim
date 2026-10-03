//! Shared discussion cards and view links, separate from the physical definition.
use super::*;
use serde_json::{Value, json};
use sim_inspect::{annotations as notes, selection::SelectionTarget};
impl Viewer {
    fn note_selection(&self) -> SelectionTarget {
        self.link
            .as_ref()
            .map(|l| l.target.clone())
            .unwrap_or_else(|| {
                self.projection.source_selection(
                    self.diagram.selected.as_ref(),
                    &self.diagram.marked_components,
                )
            })
    }
    pub(super) fn note_document(&self) -> notes::Document {
        self.annotations
            .as_ref()
            .map(|s| s.document())
            .unwrap_or_else(|| notes::Document::new(&self.description))
    }
    pub(super) fn connect_annotations(&mut self, path: std::path::PathBuf) {
        self.annotation_paths
            .insert(self.description.id.clone(), path.clone());
        self.annotations = Some(notes::native::Store::new(
            std::sync::Arc::new(self.description.clone()),
            path,
        ));
    }
    pub(super) fn saved_note_view(&self, id: String, label: String) -> notes::SavedView {
        notes::SavedView {
            id,
            label,
            selection: self.note_selection(),
            physical: None,
            schematic: Some(notes::SchematicView {
                state: self.diagram.state.clone(),
                collapsed: self.collapsed.clone(),
                focus: self.focus.as_ref().map(|f| match f {
                    NodeSource::Component(id) => notes::Focus::Component(id.clone()),
                    NodeSource::Group(id) => notes::Focus::Group(id.clone()),
                }),
            }),
        }
    }
    fn note_view_workspace(&self, v: &notes::SchematicView) -> Result<Workspace, String> {
        let mut next = self.journal.current.clone();
        next.collapsed = v.collapsed.clone();
        next.focus = v.focus.as_ref().map(|f| match f {
            notes::Focus::Component(id) => NodeSource::Component(id.clone()),
            notes::Focus::Group(id) => NodeSource::Group(id.clone()),
        });
        next.validate(&self.description)?;
        let projection = projection::project(
            &next.presentation(&self.description)?,
            &next.collapsed,
            next.focus.as_ref(),
        );
        v.state
            .validate(&projection.view)
            .map_err(|e| e.to_string())?;
        Ok(next)
    }
    fn validate_note_command(
        &self,
        command: &notes::Command,
        doc: &notes::Document,
    ) -> Result<(), String> {
        let view = match command {
            notes::Command::PutView { view } => Some(view),
            notes::Command::FollowView { id } => {
                Some(doc.views.get(id).ok_or("unknown saved view")?)
            }
            _ => None,
        };
        if let Some(v) = view.and_then(|v| v.schematic.as_ref()) {
            self.note_view_workspace(v)?;
        }
        Ok(())
    }
    pub(super) fn sync_annotations(&mut self) -> Result<(), String> {
        let mut navigation_error = None;
        let doc = self.note_document();
        if let Some(nav) = doc
            .navigation
            .as_ref()
            .filter(|n| n.revision != self.note_navigation)
        {
            self.note_navigation = nav.revision;
            if let Some(view) = doc.views.get(&nav.view) {
                let result = (|| -> Result<(), String> {
                    if let Some(v) = &view.schematic {
                        let next = self.note_view_workspace(v)?;
                        self.collapsed = next.collapsed;
                        self.focus = next.focus;
                        self.reproject();
                        self.diagram
                            .restore_state(v.state.clone())
                            .map_err(|e| e.to_string())?;
                        self.sync_view(false);
                    }
                    self.api_select(view.selection.clone())?;
                    Ok(())
                })();
                if let Err(error) = result {
                    self.error = Some(error.clone());
                    navigation_error = Some(error);
                }
            }
        }
        self.diagram.annotation_groups = doc
            .notes
            .values()
            .filter_map(|note| {
                let details = note.targets.resolve(&self.description).ok()?;
                let visible = self.projection.selection_highlights(&details);
                Some(sim_diagram::AnnotationRegion {
                    label: note.label.clone(),
                    color: note.color,
                    components: visible.components,
                })
            })
            .collect();
        let hover = if self.note_pointer_hover != SelectionTarget::None {
            &self.note_pointer_hover
        } else {
            &self.note_hover
        };
        self.diagram.annotation_hover = hover
            .resolve(&self.description)
            .ok()
            .map(|d| self.projection.selection_highlights(&d))
            .unwrap_or_default()
            .components;
        navigation_error.map_or(Ok(()), Err)
    }
    pub(super) fn annotation_api(
        &mut self,
        action: notes::Request,
        continuation: &mut Value,
    ) -> sim_api::Outcome {
        let result = (|| -> Result<Option<Value>, String> {
            if let Some(id) = continuation.as_u64() {
                return self
                    .annotations
                    .as_mut()
                    .ok_or("annotation store is not connected")?
                    .result(id)
                    .map(|r| r.map(|d| Some(json!(d))))
                    .unwrap_or(Ok(None));
            }
            let doc = self.note_document();
            let command = match action {
                notes::Request::Document => return Ok(Some(json!(doc))),
                notes::Request::Emphasize { target } => {
                    target
                        .validate(&self.description)
                        .map_err(|e| e.to_string())?;
                    self.note_hover = target;
                    return Ok(Some(json!({"emphasized":self.note_hover})));
                }
                notes::Request::SelectNote { id } => {
                    let note = doc.notes.get(&id).ok_or("unknown annotation")?;
                    self.api_select(note.targets.clone())?;
                    return Ok(Some(json!({"selection":note.targets})));
                }
                notes::Request::SaveView { id, label } => notes::Command::PutView {
                    view: self.saved_note_view(id, label),
                },
                notes::Request::RestoreView { id } => notes::Command::FollowView { id },
                notes::Request::FollowLink { note, index, reply } => {
                    let note = doc.notes.get(&note);
                    let link = match &reply {
                        None => note.and_then(|n| n.links.get(index)),
                        Some(r) => note
                            .and_then(|n| n.replies.iter().find(|c| &c.id == r))
                            .and_then(|c| c.links.get(index)),
                    }
                    .ok_or("unknown annotation link")?;
                    match &link.target {
                        notes::LinkTarget::Selection { target } => {
                            self.api_select(target.clone())?;
                            return Ok(Some(json!({"selection":target})));
                        }
                        notes::LinkTarget::View { id } => {
                            notes::Command::FollowView { id: id.clone() }
                        }
                    }
                }
                // Replies are written in the native viewer (sim-spatial), which
                // stamps their ids and times; this viewer shows them, resolves,
                // deletes a reply and edits a note's own text.
                notes::Request::Resolve { note, resolved } => {
                    notes::Command::Resolve { note, resolved }
                }
                notes::Request::DeleteComment { note, comment } => {
                    if comment == note {
                        return Err("delete the note to remove its text".into());
                    }
                    notes::Command::DeleteReply { note, reply: comment }
                }
                notes::Request::EditComment { note, comment, body } if comment == note => {
                    let mut n = doc.notes.get(&note).cloned().ok_or("unknown annotation")?;
                    n.text = body;
                    notes::Command::PutNote { note: n }
                }
                notes::Request::Reply { .. } | notes::Request::EditComment { .. } => {
                    return Err("replies are written and edited in the native viewer (sim-spatial); this viewer shows them".into());
                }
                notes::Request::Edit {
                    change,
                    expected_revision,
                } => {
                    self.validate_note_command(&change, &doc)?;
                    let id = self
                        .annotations
                        .as_mut()
                        .ok_or("annotation store not connected")?
                        .submit(change, expected_revision)?;
                    *continuation = json!(id);
                    return Ok(None);
                }
            };
            self.validate_note_command(&command, &doc)?;
            let id = self
                .annotations
                .as_mut()
                .ok_or("annotation store not connected")?
                .submit(command, None)?;
            *continuation = json!(id);
            Ok(None)
        })();
        match result {
            Ok(Some(value)) => sim_api::Outcome::Done(self.sync_annotations().map(|_| value)),
            Ok(None) => sim_api::Outcome::Pending,
            Err(error) => sim_api::Outcome::Done(Err(error)),
        }
    }
    pub(super) fn shared_notes_ui(&mut self, ui: &mut egui::Ui) {
        let doc = self.note_document();
        let selection = self.note_selection();
        ui.heading("Discussion");
        ui.horizontal(|ui| {
            for (label, enabled, change) in [
                (
                    "Undo discussion",
                    !doc.undo.is_empty(),
                    notes::Command::Undo,
                ),
                ("Redo", !doc.redo.is_empty(), notes::Command::Redo),
            ] {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    if let Some(store) = &mut self.annotations {
                        if let Err(e) = store.submit(change, Some(doc.revision)) {
                            self.error = Some(e);
                        }
                    }
                }
            }
        });
        if ui
            .add_enabled(
                selection != SelectionTarget::None
                    && self.note_editor.is_none()
                    && self.note_pending.is_none(),
                egui::Button::new("Annotate selection"),
            )
            .clicked()
        {
            let id = format!(
                "note-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            self.note_base = None;
            self.note_editor = Some(notes::Note {
                id,
                label: "New discussion".into(),
                text: String::new(),
                targets: selection.clone(),
                links: vec![notes::Link {
                    label: "Show these components".into(),
                    target: notes::LinkTarget::Selection { target: selection },
                }],
                color: [30, 155, 160],
                replies: vec![],
                resolved: false,
            });
        }
        self.note_pointer_hover = SelectionTarget::None;
        for note in doc.notes.values() {
            let color = egui::Color32::from_rgb(note.color[0], note.color[1], note.color[2]);
            egui::Frame::new()
                .fill(color.gamma_multiply(0.08))
                .stroke(egui::Stroke::new(1., color.gamma_multiply(0.55)))
                .corner_radius(8.)
                .inner_margin(10.)
                .show(ui, |ui| {
                    let title = ui.link(egui::RichText::new(&note.label).strong().color(color));
                    if title.hovered() {
                        self.note_pointer_hover = note.targets.clone();
                    }
                    if title.clicked() {
                        let _ = self.api_select(note.targets.clone());
                    }
                    if !note.text.is_empty() {
                        ui.label(&note.text);
                    }
                    for (index, link) in note.links.iter().enumerate() {
                        let response = ui.link(&link.label);
                        if response.hovered() {
                            self.note_pointer_hover = match &link.target {
                                notes::LinkTarget::Selection { target } => target.clone(),
                                notes::LinkTarget::View { id } => doc
                                    .views
                                    .get(id)
                                    .map(|v| v.selection.clone())
                                    .unwrap_or(SelectionTarget::None),
                            };
                        }
                        if response.clicked() {
                            let _ = self.annotation_api(
                                notes::Request::FollowLink {
                                    note: note.id.clone(),
                                    index,
                                    reply: None,
                                },
                                &mut Value::Null,
                            );
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.note_editor.is_none() && self.note_pending.is_none(),
                                egui::Button::new("Edit").small(),
                            )
                            .clicked()
                        {
                            self.note_base = Some(note.clone());
                            self.note_editor = Some(note.clone());
                        }
                        if ui.small_button("Remove").clicked() {
                            if let Some(store) = &mut self.annotations {
                                if let Err(e) = store.submit(
                                    notes::Command::DeleteNote {
                                        id: note.id.clone(),
                                    },
                                    Some(doc.revision),
                                ) {
                                    self.error = Some(e);
                                }
                            }
                        }
                    });
                });
            ui.add_space(6.);
        }
        if let Some(mut editor) = self.note_editor.take() {
            ui.separator();
            ui.label("Discussion label");
            ui.text_edit_singleline(&mut editor.label);
            ui.label("Notes");
            ui.text_edit_multiline(&mut editor.text);
            ui.horizontal(|ui| {
                ui.label("Group color");
                ui.color_edit_button_srgb(&mut editor.color);
            });
            if ui
                .add_enabled(
                    self.note_selection() != SelectionTarget::None,
                    egui::Button::new("Use current selection for this group"),
                )
                .clicked()
            {
                editor.targets = self.note_selection();
            }
            if ui.button("Link current selection").clicked() {
                let target = self.note_selection();
                if target != SelectionTarget::None {
                    editor.links.push(notes::Link {
                        label: "Referenced components".into(),
                        target: notes::LinkTarget::Selection { target },
                    });
                }
            }
            if ui.button("Save this angle / layout as a link").clicked() {
                let id = format!(
                    "view-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                );
                let view = self.saved_note_view(id.clone(), editor.label.clone());
                if let Some(store) = &mut self.annotations {
                    if store.submit(notes::Command::PutView { view }, None).is_ok() {
                        editor.links.push(notes::Link {
                            label: "Saved inspection view".into(),
                            target: notes::LinkTarget::View { id },
                        });
                    }
                }
            }
            let mut remove = None;
            for (index, link) in editor.links.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut link.label);
                    if ui.small_button("Remove link").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                editor.links.remove(index);
            }
            let mut keep = true;
            ui.horizontal(|ui| {
                if ui.button("Save discussion").clicked() {
                    if doc.notes.get(&editor.id)!=self.note_base.as_ref() {
                        self.error=Some("This discussion changed while you were editing. Your draft is retained; reopen the latest discussion to merge.".into());
                        return;
                    }
                    if let Some(store) = &mut self.annotations {
                        match store.submit(
                            notes::Command::PutNote {
                                note: editor.clone(),
                            },
                            Some(doc.revision),
                        ) {
                            Ok(id) => {
                                self.note_pending = Some((id, editor.clone()));
                                keep = false;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                if ui.button("Cancel edit").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.note_editor = Some(editor);
            }
        }
        if let Some((id, note)) = self.note_pending.take() {
            match self.annotations.as_mut().and_then(|s| s.result(id)) {
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    self.error = Some(e);
                    self.note_editor = Some(note);
                }
                None => self.note_pending = Some((id, note)),
            }
        }
        if let Some(error) = self.annotations.as_ref().and_then(|s| s.error()) {
            ui.colored_label(egui::Color32::DARK_RED, error);
        }
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_models_preserves_each_shared_annotation_path() {
        let first: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
        ))
        .unwrap();
        let mut second = first.clone();
        second.id = "second-model".into();
        let mut viewer = Viewer::new(vec![
            Example {
                label: "first".into(),
                description: first,
            },
            Example {
                label: "second".into(),
                description: second,
            },
        ])
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("annotation-path-{}-first.json", std::process::id()));
        viewer.workspace_path = std::env::temp_dir()
            .join("annotation-path.workspace.json")
            .display()
            .to_string();
        viewer.connect_annotations(path.clone());
        viewer.switch(1);
        let second_path = viewer.annotations.as_ref().unwrap().path.clone();
        assert_ne!(second_path, path);
        viewer.switch(0);
        assert_eq!(viewer.annotations.as_ref().unwrap().path, path);
        viewer.switch(1);
        assert_eq!(viewer.annotations.as_ref().unwrap().path, second_path);
    }
}
