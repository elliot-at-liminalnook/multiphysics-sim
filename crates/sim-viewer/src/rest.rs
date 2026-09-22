//! Thin command adapter. Journal, projection and process worker remain authoritative.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sim_api::{Outcome, capability as c};
use sim_inspect::selection::SelectionTarget;
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Annotations {
        action: sim_inspect::annotations::Request,
    },
    Render {
        request: super::render::Request,
    },
    State,
    Description,
    Workspace,
    Measurements,
    RuntimeDescription,
    History,
    Drafts {
        annotation: Option<AnnotationEditor>,
        group: Option<GroupEditor>,
    },
    ApplyDrafts,
    Select {
        target: SelectionTarget,
    },
    Focus {
        source: NodeSource,
    },
    ToggleGroup {
        id: String,
    },
    Overview,
    Back,
    Model {
        index: usize,
    },
    OpenDescription {
        path: String,
    },
    Diagram {
        state: sim_inspect::DiagramState,
    },
    Arrange,
    CancelLayout,
    Fit,
    Domains {
        dimmed: BTreeSet<String>,
    },
    Panels {
        browser: bool,
        notes: bool,
        compact: bool,
        search: String,
    },
    Annotate {
        annotation: Annotation,
    },
    Group {
        id: Option<String>,
        label: String,
        members: BTreeSet<String>,
        color: [u8; 3],
    },
    DeleteGroup {
        id: String,
    },
    Undo,
    Redo,
    WorkspaceReplace {
        workspace: Workspace,
    },
    WorkspaceOpen {
        path: String,
    },
    WorkspaceSave {
        path: Option<String>,
    },
    SimulationLoad {
        path: String,
    },
    Simulation {
        action: sim_runtime::system_session::Command,
    },
    SimulationCancel,
    SimulationRestart,
    SimulationUnload,
    Graphs {
        observables: BTreeSet<String>,
        cursor: Option<f64>,
    },
    Experiments {
        action: super::experiments_ui::rest::Command,
    },
}
pub(super) fn server_with(port: u16, builder: bool) -> std::io::Result<sim_api::Server> {
    let mut all = capabilities();
    if builder {
        all.extend([
            c("system", json!({"label":"Place resistor","commands":[{"command":"add_instance","at":"","name":"r1","instance":{"kind":{"kind":"element","component_type":"electrical.resistor"},"parameters":{"resistance":{"value":100}}}}]}),
                "Apply sim-system commands atomically (shared validation and undo history with the physical viewer and CLI)"),
            c("system_state", json!({}), "System file, revision, level, selection, findings and compile status"),
            c("system_level", json!({"path":"regulator"}), "Drill into a subsystem instance path"),
            c("system_select", json!({"names":["q1"]}), "Select instances at the current level"),
            c("system_undo", json!({}), "Undo in the shared history"),
            c("system_redo", json!({}), "Redo in the shared history"),
        ]);
    }
    sim_api::Server::bind(port, "schematic-assembly", all)
}
fn capabilities() -> Vec<Value> {
        vec![
            c(
                "annotations",
                json!({"action":{"operation":"document"}}),
                "Shared multi-part discussion notes, typed links, saved views, edit/delete, follow_link, select_note and emphasize",
            ),
            c(
                "render",
                json!({"request":{"kind":"schematic","options":{"size":{"width":1280,"height":900}}}}),
                "Off-screen PNG: schematic, graphs or experiment; returns an immutable image URL and source metadata",
            ),
            c(
                "drafts",
                json!({"annotation":null,"group":null}),
                "Replace or explicitly clear open annotation/group editor drafts; inspect them in state first",
            ),
            c(
                "apply_drafts",
                json!({}),
                "Apply open editor drafts through the same validation and undo paths",
            ),
            c(
                "state",
                json!({}),
                "Current model, diagram, selection, layout, journal and worker status",
            ),
            c(
                "description",
                json!({}),
                "Shared typed source model, diagnostics and observables",
            ),
            c(
                "workspace",
                json!({}),
                "Full editable analysis document; never physical geometry",
            ),
            c(
                "runtime_description",
                json!({}),
                "Compiled typed observables including runtime-added states",
            ),
            c(
                "measurements",
                json!({}),
                "Runtime description, status and latest actual sample",
            ),
            c(
                "history",
                json!({}),
                "Bounded display history, not a full-rate recording",
            ),
            c(
                "select",
                json!({"target":{"kind":"components","ids":["source-id"]}}),
                "SelectionTarget: none, components, ports or nets; linked across viewers",
            ),
            c(
                "focus",
                json!({"source":{"Component":"source-id"}}),
                "Focus component or group (NodeSource)",
            ),
            c(
                "toggle_group",
                json!({"id":"group-id"}),
                "Expand/collapse a declared group",
            ),
            c("overview", json!({}), "System overview"),
            c("back", json!({}), "Previous focus and collapse state"),
            c(
                "model",
                json!({"index":0}),
                "Switch model, retaining every analysis workspace",
            ),
            c(
                "open_description",
                json!({"path":"description.json"}),
                "Validate and add a captured source description",
            ),
            c(
                "diagram",
                json!({"state":"use state.diagram"}),
                "Validated DiagramState: node positions, pins, camera and zoom; same undo journal",
            ),
            c(
                "arrange",
                json!({}),
                "Start background layout; state.layout_pending reports progress",
            ),
            c(
                "cancel_layout",
                json!({}),
                "Cancel background layout, retain last usable positions",
            ),
            c(
                "fit",
                json!({}),
                "Request viewport fit (native viewport only)",
            ),
            c(
                "domains",
                json!({"dimmed":[]}),
                "Dim typed domains without changing the model",
            ),
            c(
                "panels",
                json!({"browser":true,"notes":false,"compact":false,"search":""}),
                "Browser, diagnostics, density and search",
            ),
            c(
                "annotate",
                json!({"annotation":{"target":{"kind":"component","id":"source-id"},"label":null,"text":"note"}}),
                "Annotate/rename; empty text and null label removes annotation",
            ),
            c(
                "group",
                json!({"id":null,"label":"Subsystem","members":["source-id"],"color":[50,150,180]}),
                "Create or edit disjoint analysis group",
            ),
            c(
                "delete_group",
                json!({"id":"analysis/group/1"}),
                "Remove analysis group, retaining components",
            ),
            c("undo", json!({}), "Undo analysis or layout edit"),
            c("redo", json!({}), "Redo analysis or layout edit"),
            c(
                "workspace_replace",
                json!({"workspace":"use workspace resource"}),
                "Validate and commit a complete analysis edit with undo",
            ),
            c(
                "workspace_open",
                json!({"path":"review.workspace.json"}),
                "Open analysis sidecar; refuses to discard unsaved work",
            ),
            c(
                "workspace_save",
                json!({"path":null}),
                "Save or save as using existing optimistic file conflict checks",
            ),
            c(
                "simulation_load",
                json!({"path":"capture.live.json"}),
                "Load an exactly source-bound worker capture",
            ),
            c(
                "simulation",
                json!({"action":{"command":"step"}}),
                "Worker commands describe/start/pause/step/reset/cancel/begin_recording/take_recording; completes on actual reply",
            ),
            c(
                "simulation_cancel",
                json!({}),
                "Terminate worker and retain measured history",
            ),
            c(
                "simulation_unload",
                json!({}),
                "Stop worker, release capture and permit switching source model",
            ),
            c(
                "simulation_restart",
                json!({}),
                "Restart a stopped worker from captured configuration",
            ),
            c(
                "graphs",
                json!({"observables":[],"cursor":null}),
                "Replace up to eight graph channels and shared time cursor",
            ),
            c(
                "experiments",
                json!({"action":{"operation":"state"}}),
                "Experiment review, configuration, evaluation, refinement and immutable export; see /v1/experiment-capabilities",
            ),
        ]
}
impl Viewer {
    fn api_state(&self) -> Value {
        json!({"annotations_revision":self.note_document().revision,"annotation_emphasis":self.note_hover,"discussion_draft":self.note_editor,"annotation_draft":self.annotation_editor,"group_draft":self.group_editor,"description_id":self.description.id,"model":self.current,"models":self.examples.iter().map(|e|json!({"label":e.label,"id":e.description.id})).collect::<Vec<_>>(),
            "diagram":self.diagram.state,"selected":self.diagram.selected,"marked":self.diagram.marked_components,"collapsed":self.collapsed,"focus":self.focus,"dimmed_domains":self.diagram.dimmed_domains,
            "layout_pending":self.diagram.is_layout_pending(),"layout_error":self.diagram.layout_error,"layout_cancelled":self.diagram.layout_cancelled,
            "workspace_path":self.workspace_path,"unsaved":self.saved_workspace.as_ref()!=Some(&self.journal.current),"can_undo":self.journal.can_undo(),"can_redo":self.journal.can_redo(),
            "browser":self.show_browser,"notes":self.show_notes,"compact":self.compact,"search":self.search,"error":self.error,"link":self.link_status(),"live":self.live.as_ref().map(|l|l.api_state())})
    }
    pub(super) fn api_select(&mut self, target: SelectionTarget) -> Result<(), String> {
        let details = target
            .resolve(&self.description)
            .map_err(|e| e.to_string())?;
        let h = self.projection.selection_highlights(&details);
        self.diagram.marked_components = h.components.clone();
        self.diagram.selected = match &target {
            SelectionTarget::None => None,
            SelectionTarget::Components { .. } => h
                .components
                .iter()
                .next()
                .cloned()
                .map(Selection::Component),
            SelectionTarget::Ports { .. } => h.ports.iter().next().cloned().map(Selection::Port),
            SelectionTarget::Nets { .. } => h.nets.iter().next().cloned().map(Selection::Net),
        };
        self.diagram.linked_highlights = Some(h);
        self.select_source(target);
        // Stamp before exchange so projection proxies never broaden exact IDs.
        if let Some(link) = &mut self.link {
            link.api_stamp(&self.diagram);
        }
        Ok(())
    }
    fn api_execute(
        &mut self,
        command: &sim_api::Command,
        continuation: &mut Value,
        ctx: &egui::Context,
        cancelled: bool,
    ) -> Outcome {
        if command.command.starts_with("system") {
            return Outcome::Done(self.system_api(command));
        }
        let parsed = match sim_api::decode::<Command>(command) {
            Ok(c) => c,
            Err(e) => return Outcome::Done(Err(e)),
        };
        if let Command::Annotations { action } = parsed {
            return self.annotation_api(action, continuation);
        }
        if let Command::Render { request } = parsed {
            return self.render_image(request, continuation, cancelled);
        }
        if let Command::Simulation { action } = parsed {
            return match &mut self.live {
                Some(l) => {
                    if cancelled {
                        let result = l.api_cancel();
                        Outcome::Done(Err(result
                            .err()
                            .unwrap_or("cancelled: simulation worker stopped".into())))
                    } else {
                        l.api_command(action, continuation)
                    }
                }
                None => Outcome::Done(Err("no live capture loaded".into())),
            };
        }
        if let Command::Experiments { action } = parsed {
            return self.experiments.api_command(action, continuation, ctx);
        }
        let result = (|| -> sim_api::Result {
            match parsed {
                Command::Drafts { annotation, group } => {
                    self.annotation_editor = annotation;
                    self.group_editor = group;
                }
                Command::ApplyDrafts => {
                    self.error = None;
                    let annotation = self.annotation_editor.clone();
                    if let Some(group) = self.group_editor.clone() {
                        self.apply_analysis(AnalysisAction::Group(group));
                        if let Some(e) = &self.error {
                            return Err(e.clone());
                        }
                    }
                    if let Some(editor) = annotation {
                        self.apply_analysis(AnalysisAction::Annotation(Annotation {
                            target: editor.target.clone(),
                            label: Some(editor.label.clone()),
                            text: editor.text.clone(),
                        }));
                        if let Some(e) = &self.error {
                            self.annotation_editor = Some(editor);
                            return Err(e.clone());
                        }
                    }
                }
                Command::State => return Ok(self.api_state()),
                Command::Description => return Ok(json!(self.description)),
                Command::Workspace => {
                    self.sync_view(false);
                    return Ok(json!(self.journal.current));
                }
                Command::RuntimeDescription => {
                    return Ok(self
                        .live
                        .as_ref()
                        .map(|l| l.api_description())
                        .unwrap_or(Value::Null));
                }
                Command::Measurements => {
                    return Ok(self
                        .live
                        .as_ref()
                        .map(|l| l.api_state())
                        .unwrap_or(Value::Null));
                }
                Command::History => {
                    return Ok(self
                        .live
                        .as_ref()
                        .map(|l| l.api_history())
                        .unwrap_or(Value::Null));
                }
                Command::Select { target } => self.api_select(target)?,
                Command::Focus { source } => {
                    let mut next = self.journal.current.clone();
                    next.focus = Some(source.clone());
                    next.validate(&self.description)?;
                    self.apply(Action::Focus(source));
                }
                Command::ToggleGroup { id } => {
                    if !self.presentation.groups.contains_key(&id) {
                        return Err("unknown group".into());
                    }
                    self.apply(Action::Expand(id));
                }
                Command::Overview => self.apply(Action::Overview),
                Command::Back => self.apply(Action::Back),
                Command::Model { index } => {
                    if index >= self.examples.len() {
                        return Err("unknown model index".into());
                    }
                    if self.live.is_some() && index != self.current {
                        return Err("unload live capture before switching source model".into());
                    }
                    self.error = None;
                    self.switch(index);
                    if let Some(e) = &self.error {
                        return Err(e.clone());
                    }
                }
                Command::OpenDescription { path } => {
                    if self.live.is_some() {
                        return Err("cannot switch source with a live capture loaded".into());
                    }
                    if self.annotation_editor.is_some() || self.group_editor.is_some() {
                        return Err("apply or close the current editor first".into());
                    }
                    let d: SystemDescription =
                        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                    d.validate().map_err(|e| e.to_string())?;
                    self.examples.push(Example {
                        label: path,
                        description: d,
                    });
                    self.switch(self.examples.len() - 1);
                }
                Command::Diagram { state } => {
                    self.sync_view(false);
                    self.diagram
                        .restore_state(state)
                        .map_err(|e| e.to_string())?;
                    self.sync_view(true);
                }
                Command::Arrange => self.diagram.reset_layout(&self.projection.view),
                Command::CancelLayout => self.diagram.cancel_layout(),
                Command::Fit => self.diagram.fit(),
                Command::Domains { dimmed } => {
                    if dimmed.iter().any(|id| {
                        !self.description.ports.values().any(|p| {
                            sim_diagram::style::port_domain(&self.description, &p.schema).key == *id
                        })
                    }) {
                        return Err("unknown domain".into());
                    }
                    self.diagram.dimmed_domains = dimmed;
                    self.sync_view(false);
                }
                Command::Panels {
                    browser,
                    notes,
                    compact,
                    search,
                } => {
                    self.show_browser = browser;
                    self.show_notes = notes;
                    self.compact = compact;
                    self.search = search;
                }
                Command::Annotate { annotation } => {
                    self.api_analysis(AnalysisAction::Annotation(annotation))?
                }
                Command::Group {
                    id,
                    label,
                    members,
                    color,
                } => self.api_analysis(AnalysisAction::Group(GroupEditor {
                    id,
                    label,
                    members,
                    color,
                    search: String::new(),
                    note: String::new(),
                }))?,
                Command::DeleteGroup { id } => {
                    if !self.journal.current.groups.contains_key(&id) {
                        return Err("unknown analysis group".into());
                    }
                    self.api_analysis(AnalysisAction::DeleteGroup(id))?;
                }
                Command::Undo => self.api_analysis(AnalysisAction::Undo)?,
                Command::Redo => self.api_analysis(AnalysisAction::Redo)?,
                Command::WorkspaceReplace { workspace } => {
                    workspace.validate(&self.description)?;
                    for view in workspace.views.values() {
                        let projection = sim_diagram::projection::project(
                            &workspace.presentation(&self.description)?,
                            &workspace.collapsed,
                            workspace.focus.as_ref(),
                        );
                        if view.state.description_id == projection.view.id {
                            view.state
                                .validate(&projection.view)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    self.sync_view(false);
                    self.journal.commit(workspace, &self.description)?;
                    self.restore_analysis();
                }
                Command::WorkspaceOpen { path } => {
                    self.sync_view(false);
                    if self
                        .saved_workspace
                        .as_ref()
                        .map_or(self.journal.can_undo(), |s| s != &self.journal.current)
                    {
                        return Err("save current analysis before opening another sidecar".into());
                    }
                    self.load_workspace(path.into())?;
                }
                Command::WorkspaceSave { path } => {
                    if let Some(path) = path {
                        self.workspace_path = path;
                    }
                    self.api_analysis(AnalysisAction::Save)?;
                }
                Command::SimulationLoad { path } => {
                    if self.live.is_some() {
                        return Err("capture already loaded; cancel/restart it instead".into());
                    }
                    self.load_live(std::path::Path::new(&path))?;
                }
                Command::SimulationUnload => {
                    self.live = None;
                }
                Command::SimulationCancel => {
                    self.live.as_mut().ok_or("no live capture")?.api_cancel()?
                }
                Command::SimulationRestart => {
                    self.live.as_mut().ok_or("no live capture")?.api_restart()?
                }
                Command::Graphs {
                    observables,
                    cursor,
                } => self
                    .live
                    .as_mut()
                    .ok_or("no live capture")?
                    .api_graphs(observables, cursor)?,
                Command::Annotations { .. }
                | Command::Render { .. }
                | Command::Simulation { .. }
                | Command::Experiments { .. } => unreachable!(),
            }
            Ok(self.api_state())
        })();
        Outcome::Done(result)
    }
    fn api_analysis(&mut self, action: AnalysisAction) -> Result<(), String> {
        if self.annotation_editor.is_some() || self.group_editor.is_some() {
            return Err("apply or close the current editor before a REST analysis edit".into());
        }
        self.error = None;
        self.apply_analysis(action);
        self.error.clone().map_or(Ok(()), Err)
    }
    pub(super) fn api_tick(&mut self, ctx: &egui::Context) {
        let _ = self.sync_annotations();
        self.diagram.poll_layout();
        if let Some(l) = &mut self.live {
            l.api_tick();
        }
        self.experiments.api_poll(ctx);
        if let Some(mut api) = self.api.take() {
            api.poll(|command, continuation, cancelled| {
                self.api_execute(command, continuation, ctx, cancelled)
            });
            self.sync_selection();
            if api.snapshot_due() {
                api.publish("state", self.api_state());
                api.publish_changed("description", &self.description.id, || {
                    json!(self.description)
                });
                api.publish("workspace", json!(self.journal.current));
                api.publish("annotations", json!(self.note_document()));
                api.publish(
                    "measurements",
                    self.live
                        .as_ref()
                        .map(|l| l.api_state())
                        .unwrap_or(Value::Null),
                );
                api.publish_changed(
                    "experiment-capabilities",
                    "1",
                    super::experiments_ui::rest::capabilities,
                );
            }
            self.api = Some(api);
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewer() -> Viewer {
        Viewer::new(vec![Example {
            label: "Motor".into(),
            description: serde_json::from_str(include_str!(
                "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
            ))
            .unwrap(),
        }])
        .unwrap()
    }
    fn call(v: &mut Viewer, name: &str, args: Value) -> sim_api::Result {
        match v.api_execute(
            &sim_api::Command {
                command: name.into(),
                args,
            },
            &mut Value::Null,
            &egui::Context::default(),
            false,
        ) {
            Outcome::Done(r) => r,
            Outcome::Pending | Outcome::Image(_) => panic!("unexpected pending or image"),
        }
    }
    #[test]
    fn api_analysis_uses_real_journal_and_invalid_edits_do_not_change_source() {
        let mut v = viewer();
        let before = json!(v.description);
        let id = v.description.components.keys().next().unwrap().clone();
        call(&mut v,"annotate",json!({"annotation":{"target":{"kind":"component","id":id},"label":null,"text":"REST test"}})).unwrap();
        assert_eq!(v.journal.current.annotations.len(), 1);
        call(&mut v, "undo", json!({})).unwrap();
        assert!(v.journal.current.annotations.is_empty());
        call(&mut v, "redo", json!({})).unwrap();
        assert_eq!(v.journal.current.annotations.len(), 1);
        let workspace = json!(v.journal.current);
        assert!(
            call(
                &mut v,
                "group",
                json!({"id":null,"label":"Invalid","members":["missing"],"color":[0,0,0]})
            )
            .is_err()
        );
        assert_eq!(json!(v.journal.current), workspace);
        assert_eq!(json!(v.description), before);
        assert!(
            call(
                &mut v,
                "panels",
                json!({"browser":true,"notes":false,"compact":false,"search":"","typo":true})
            )
            .is_err()
        );
    }
    #[test]
    fn invalid_navigation_and_camera_are_rejected_without_panics() {
        let mut v = viewer();
        let before = json!(v.diagram.state);
        assert!(call(&mut v, "model", json!({"index":9999})).is_err());
        assert!(
            call(
                &mut v,
                "select",
                json!({"target":{"kind":"ports","ids":["missing"]}})
            )
            .is_err()
        );
        assert!(call(&mut v, "focus", json!({"source":{"Component":"missing"}})).is_err());
        let mut invalid = before.clone();
        invalid["zoom"] = json!(-1.);
        assert!(call(&mut v, "diagram", json!({"state":invalid})).is_err());
        assert_eq!(json!(v.diagram.state), before);
    }
    #[test]
    fn failed_draft_application_retains_the_annotation_after_group_commit() {
        let mut v = viewer();
        let id = v.description.components.keys().next().unwrap().clone();
        call(&mut v,"drafts",json!({"annotation":{"target":{"kind":"component","id":"missing"},"label":"Draft","text":"Preserve this"},"group":{"id":null,"label":"Group","members":[id],"color":[0,100,100],"search":"","note":""}})).unwrap();
        assert!(call(&mut v, "apply_drafts", json!({})).is_err());
        assert_eq!(v.annotation_editor.as_ref().unwrap().text, "Preserve this");
        assert_eq!(v.journal.current.groups.len(), 1);
    }
}
