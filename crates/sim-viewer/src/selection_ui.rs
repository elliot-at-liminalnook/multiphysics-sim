//! Selection is transient presentation state; it never reprojects or edits analysis.
use super::*;
use sim_inspect::selection::{
    SelectionTarget,
    native::{Peer, SelectionClient},
};

pub(super) struct LinkedSelection {
    pub(super) client: SelectionClient,
    pub target: SelectionTarget,
    stamp: (Option<Selection>, BTreeSet<String>),
    view: String,
}
impl Viewer {
    pub(super) fn inspected_nets(&self, visible_id: &str) -> Vec<String> {
        self.link
            .as_ref()
            .filter(|link| link.client.description_id() == self.description.id)
            .and_then(|link| match &link.target {
                SelectionTarget::Nets { ids } => Some(ids.iter().cloned().collect()),
                _ => None,
            })
            .unwrap_or_else(|| {
                self.projection
                    .connections
                    .get(visible_id)
                    .cloned()
                    .unwrap_or_default()
            })
    }

    pub(super) fn select_source(&mut self, target: SelectionTarget) {
        if let Some(link) = &mut self.link {
            if link.client.description_id() != self.description.id {
                return;
            }
            match link.client.exchange(target) {
                Ok(target) => {
                    link.target = target;
                    self.diagram.linked_highlights = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    pub(super) fn connect_selection(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        let target = self.projection.source_selection(
            self.diagram.selected.as_ref(),
            &self.diagram.marked_components,
        );
        self.link = Some(LinkedSelection {
            client: SelectionClient::connect(
                std::sync::Arc::new(self.description.clone()),
                path,
                Peer::Schematic,
                target.clone(),
            )
            .map_err(|e| e.to_string())?,
            target,
            stamp: (
                self.diagram.selected.clone(),
                self.diagram.marked_components.clone(),
            ),
            view: self.projection.view.id.clone(),
        });
        Ok(())
    }
    pub(super) fn sync_selection(&mut self) {
        let Some(link) = &mut self.link else { return };
        if link.client.description_id() != self.description.id {
            self.diagram.linked_highlights = None;
            link.view.clear();
            return;
        }
        let stamp = (
            self.diagram.selected.clone(),
            self.diagram.marked_components.clone(),
        );
        let changed_view = link.view != self.projection.view.id;
        let local_changed = !changed_view && stamp != link.stamp;
        let local = if local_changed {
            self.projection.source_selection(stamp.0.as_ref(), &stamp.1)
        } else {
            link.target.clone()
        };
        match link.client.exchange(local) {
            Ok(target) => {
                if (!local_changed && target != link.target) || changed_view {
                    let details = target
                        .resolve(&self.description)
                        .expect("validated selection");
                    let h = self.projection.selection_highlights(&details);
                    // A projection proxy is only for navigation. Never publish it back
                    // as a broader group/bundle unless the user makes a new selection.
                    self.diagram.selected = match &target {
                        SelectionTarget::None => None,
                        SelectionTarget::Components { .. } => h
                            .components
                            .iter()
                            .find(|id| self.projection.view.components.contains_key(*id))
                            .cloned()
                            .map(Selection::Component),
                        SelectionTarget::Ports { .. } => h
                            .ports
                            .iter()
                            .find(|id| self.projection.view.ports.contains_key(*id))
                            .cloned()
                            .map(Selection::Port),
                        SelectionTarget::Nets { .. } => h
                            .nets
                            .iter()
                            .find(|id| self.projection.view.nets.contains_key(*id))
                            .cloned()
                            .map(Selection::Net),
                    };
                    self.diagram.marked_components.clear();
                    eprintln!("schematic selection: {target:?}");
                }
                let changed = target != link.target
                    || changed_view
                    || self.diagram.linked_highlights.is_none();
                link.target = target;
                if changed {
                    self.diagram.linked_highlights = Some(
                        self.projection.selection_highlights(
                            &link
                                .target
                                .resolve(&self.description)
                                .expect("validated selection"),
                        ),
                    );
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        link.stamp = (
            self.diagram.selected.clone(),
            self.diagram.marked_components.clone(),
        );
        link.view = self.projection.view.id.clone();
    }
    pub(super) fn link_status(&self) -> String {
        match &self.link {
            Some(link) if link.client.description_id() != self.description.id => {
                "Link paused: another model is open".into()
            }
            Some(link) => link.client.status("assembly"),
            None => "Standalone schematic".into(),
        }
    }
    /// Show exact source identities even when the diagram displays a collapsed proxy.
    pub(super) fn linked_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(link) = &self.link else { return };
        if link.client.description_id() != self.description.id {
            return;
        }
        let target = link.target.clone();
        if target == SelectionTarget::None {
            return;
        }
        ui.strong("Shared selection");
        let (kind, ids) = match &target {
            SelectionTarget::Components { ids } => ("Component", ids),
            SelectionTarget::Ports { ids } => ("Port", ids),
            SelectionTarget::Nets { ids } => ("Connection", ids),
            SelectionTarget::None => return,
        };
        for id in ids {
            ui.small(format!("{kind}: {id}"));
        }
        let details = target
            .resolve(&self.description)
            .expect("validated selection");
        for id in &details.ports {
            let p = &self.description.ports[id];
            ui.small(format!(
                "{}.{}",
                self.description.components[&p.component].label, p.name
            ));
        }
        let h = self.projection.selection_highlights(&details);
        if !h
            .components
            .iter()
            .any(|id| self.projection.view.components.contains_key(id))
        {
            ui.label("Outside the current diagram view.");
            if ui.button("Reveal in diagram").clicked() {
                if let Some(id) = details.components.first() {
                    self.requested = Some(Action::Focus(NodeSource::Component(id.clone())));
                }
            }
        }
        if ui.small_button("Clear shared selection").clicked() {
            self.diagram.selected = None;
            self.diagram.marked_components.clear();
            // An invisible selection has no diagram stamp change.
            if let Some(link) = &mut self.link {
                match link.client.exchange(SelectionTarget::None) {
                    Ok(target) => {
                        link.target = target;
                        self.diagram.linked_highlights = None;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn remote_net_inside_a_bundle_keeps_exact_inspection_and_annotation_scope() {
        let d: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/full-robot.description.json"
        ))
        .unwrap();
        let mut viewer = Viewer::new(vec![Example {
            label: "Bundle test".into(),
            description: d.clone(),
        }])
        .unwrap();
        let (proxy, nets) = viewer
            .projection
            .connections
            .iter()
            .find(|(_, nets)| nets.len() > 1)
            .unwrap();
        let proxy = proxy.clone();
        let real = nets[0].clone();
        let target = SelectionTarget::net(&real);
        let dir = sim_inspect::selection::native::create_session(&d, target.clone()).unwrap();
        viewer.connect_selection(dir).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while viewer.link.as_ref().unwrap().target != target {
            viewer.sync_selection();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(viewer.diagram.selected, Some(Selection::Net(proxy.clone())));
        assert_eq!(viewer.inspected_nets(&proxy), vec![real]);
        viewer.sync_selection();
        assert_eq!(
            viewer.link.as_ref().unwrap().target,
            target,
            "proxy must not expand or echo a remote selection"
        );
    }
    #[test]
    fn remote_selection_preserves_analysis_view_and_local_multi_selection() {
        let d: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
        ))
        .unwrap();
        let mut viewer = Viewer::new(vec![Example {
            label: "Linked test".into(),
            description: d.clone(),
        }])
        .unwrap();
        super::super::tests::settle(&mut viewer);
        let motor = "example/motor-thermal/motor".to_string();
        let supply = "example/motor-thermal/supply".to_string();
        let annotation = Annotation {
            target: Target::Component(motor.clone()),
            label: None,
            text: "Unsaved engineering note".into(),
        };
        viewer
            .journal
            .current
            .annotations
            .insert(annotation.target.key(), annotation);
        viewer.diagram.state.zoom = 1.3;
        viewer.diagram.state.pinned.insert(motor.clone());
        let state = viewer.diagram.state.clone();
        let workspace = viewer.journal.current.clone();
        let physical = serde_json::to_vec(&viewer.description).unwrap();
        let dir =
            sim_inspect::selection::native::create_session(&d, SelectionTarget::None).unwrap();
        let mut assembly = SelectionClient::connect(
            std::sync::Arc::new(d),
            dir.clone(),
            Peer::Assembly,
            SelectionTarget::None,
        )
        .unwrap();
        viewer.connect_selection(dir.clone()).unwrap();
        let target = SelectionTarget::component(&motor);
        assembly.exchange(target.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while viewer.link.as_ref().unwrap().target != target {
            viewer.sync_selection();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            viewer.diagram.selected,
            Some(Selection::Component(motor.clone()))
        );
        assert_eq!(viewer.diagram.state, state);
        assert_eq!(viewer.journal.current, workspace);
        assert_eq!(serde_json::to_vec(&viewer.description).unwrap(), physical);
        viewer.diagram.marked_components = BTreeSet::from([motor.clone(), supply.clone()]);
        viewer.diagram.selected = Some(Selection::Component(supply.clone()));
        viewer.sync_selection();
        assert_eq!(viewer.diagram.marked_components.len(), 2);
        assert_eq!(
            viewer.link.as_ref().unwrap().target,
            SelectionTarget::Components {
                ids: BTreeSet::from([motor, supply])
            }
        );
        drop(viewer);
        drop(assembly);
        // Worker shutdown is nonblocking; leave its tiny private directory to avoid
        // racing a last atomic write. Normal sessions likewise survive a peer close.
    }
}

impl LinkedSelection {
    pub(super) fn api_stamp(&mut self, diagram: &Diagram) {
        self.stamp = (diagram.selected.clone(), diagram.marked_components.clone());
    }
}
