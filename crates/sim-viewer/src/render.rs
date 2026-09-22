//! Snapshot the existing projection and observations; rasterize off the UI thread.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Schematic {
        #[serde(default)]
        options: sim_render::diagram::Options,
    },
    Graphs {
        #[serde(default)]
        options: sim_render::graphs::Options,
    },
    Experiment {
        #[serde(default)]
        options: sim_render::graphs::Options,
    },
}
impl Viewer {
    pub(super) fn render_image(
        &mut self,
        request: Request,
        continuation: &mut Value,
        cancelled: bool,
    ) -> sim_api::Outcome {
        if self.image_task.is_none() {
            let task = (|| -> Result<sim_api::ImageTask, String> {
                match request {
                    Request::Schematic { options } => {
                        options.size.validate()?;
                        if self.diagram.is_layout_pending() {
                            return Err("layout is still being arranged; retry render after layout_pending is false".into());
                        }
                        let snapshot = self.capture_diagram();
                        Ok(sim_api::ImageTask::spawn(move || {
                            sim_render::diagram::render(&snapshot, &options).map(|r| {
                                sim_api::Artifact {
                                    png: r.png,
                                    metadata: r.metadata,
                                }
                            })
                        }))
                    }
                    Request::Graphs { options } => {
                        options.size.validate()?;
                        let (panels, metadata) = self
                            .live
                            .as_ref()
                            .ok_or("no live capture loaded")?
                            .capture_graphs(&options)?;
                        Ok(sim_api::ImageTask::spawn(move || {
                            sim_render::graphs::render("Simulation measurements","Bounded display history · each channel retains its own units and sample times",&panels,&options,metadata).map(|r|sim_api::Artifact{png:r.png,metadata:r.metadata})
                        }))
                    }
                    Request::Experiment { options } => {
                        options.size.validate()?;
                        let (panels, metadata) = self.experiments.capture_graphs()?;
                        Ok(sim_api::ImageTask::spawn(move || {
                            sim_render::graphs::render("Measured and simulated response","Captured experiment · original measurements, reference, baseline and candidate",&panels,&options,metadata).map(|r|sim_api::Artifact{png:r.png,metadata:r.metadata})
                        }))
                    }
                }
            })();
            match task {
                Ok(task) => {
                    self.image_task = Some(task);
                    *continuation = json!(true);
                }
                Err(e) => return sim_api::Outcome::Done(Err(e)),
            }
        }
        let result = self.image_task.as_mut().unwrap().poll(cancelled);
        if !matches!(result, sim_api::Outcome::Pending) {
            self.image_task = None;
        }
        result
    }
    fn capture_diagram(&self) -> sim_render::diagram::Snapshot {
        use sim_render::diagram::{Card, Edge, Port, Snapshot};
        let d = &self.projection.view;
        let layout = self.diagram.layout();
        let color = |p: &sim_inspect::PortDescription| {
            let c = sim_diagram::style::port_domain(d, &p.schema).color;
            [c.r(), c.g(), c.b()]
        };
        let cards = layout
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                let component = d.components.get(id)?;
                Some(Card {
                    id: id.clone(),
                    label: component.label.clone(),
                    detail: component.component_type.clone(),
                    position: [node.position.x, node.position.y],
                    size: [sim_diagram::layout::WIDTH, node.height],
                    ports: d
                        .ports
                        .values()
                        .filter(|p| &p.component == id)
                        .filter_map(|p| {
                            layout.ports.get(&p.id).map(|point| Port {
                                position: [point.x, point.y],
                                label: p.name.clone(),
                                color: color(p),
                            })
                        })
                        .collect(),
                    selected: self.diagram.annotation_hover.contains(id)
                        || self.diagram.marked_components.contains(id)
                        || self.diagram.selected == Some(Selection::Component(id.clone())),
                })
            })
            .collect();
        let edges = layout
            .nets
            .iter()
            .flat_map(|(id, route)| {
                let selected = self.diagram.selected == Some(Selection::Net(id.clone()));
                let color = d
                    .nets
                    .get(id)
                    .and_then(|n| n.ports.first())
                    .and_then(|id| d.ports.get(id))
                    .map(color)
                    .unwrap_or([80, 110, 130]);
                route.branches.iter().map(move |branch| Edge {
                    id: id.clone(),
                    points: branch.iter().map(|p| [p.x, p.y]).collect(),
                    color,
                    selected,
                })
            })
            .collect();
        Snapshot {
            cards,
            regions: self
                .diagram
                .annotation_groups
                .iter()
                .map(|g| sim_render::Region {
                    label: g.label.clone(),
                    color: g.color,
                    components: g.components.clone(),
                })
                .collect(),
            edges,
            camera: [self.diagram.state.camera.x, self.diagram.state.camera.y],
            zoom: self.diagram.state.zoom,
            metadata: json!({"annotations":self.note_document(),"source_description_id":self.description.id,"projection_id":d.id,"diagram_revision":self.diagram.state.revision,"collapsed":self.collapsed,"focus":self.focus,"selection":self.diagram.selected}),
        }
    }
}
