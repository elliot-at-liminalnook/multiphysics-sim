//! Reusable schematic widget over inspection data. No physics dependency.
pub mod analysis;
pub mod layout;
pub mod plot;
pub mod projection;
pub mod style;
mod worker;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use layout::{Side, WIDTH};
pub use layout::{initial_state, route};
use sim_inspect::{DiagramState, Point, PortKind, SystemDescription};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Selection {
    Component(String),
    Port(String),
    Net(String),
}
#[derive(Debug, Clone, PartialEq)]
pub struct NetRoute {
    pub junction: Point,
    pub junctions: Vec<Point>,
    pub branches: Vec<Vec<Point>>,
}
#[derive(Debug, Clone)]
pub struct Layout {
    pub ports: BTreeMap<String, Point>,
    pub nets: BTreeMap<String, NetRoute>,
    pub nodes: BTreeMap<String, layout::NodeBox>,
    pub sides: BTreeMap<String, Side>,
    pub bounds: [Point; 2],
    pub unrouted: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CardDecoration {
    pub note: String,
    pub group_label: Option<String>,
    pub color: Option<[u8; 3]>,
}

pub struct AnnotationRegion {pub label:String,pub color:[u8;3],pub components:BTreeSet<String>}

/// A reference image drawn behind the schematic, in diagram coordinates.
/// Presentation only; never part of the description or layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    pub texture: egui::TextureId,
    /// Center, in diagram units.
    pub center: Point,
    pub size: [f32; 2],
    pub opacity: f32,
}

pub struct Diagram {
    pub backdrops: Vec<Backdrop>,
    pub annotation_groups:Vec<AnnotationRegion>,
    pub annotation_hover:BTreeSet<String>,
    /// Presentation bundles, with their number of distinct underlying nets.
    pub bundles: BTreeMap<String, usize>,
    pub marked_components: BTreeSet<String>,
    pub decorations: BTreeMap<String, CardDecoration>,
    pub state: DiagramState,
    pub dimmed_domains: BTreeSet<String>,
    pub selected: Option<Selection>,
    /// Projected linked selection; presentation-only and never serialized into
    /// the physical description or analysis annotations.
    pub linked_highlights: Option<sim_inspect::selection::SelectionDetails>,
    layout: Layout,
    fit_requested: bool,
    job: Option<worker::LayoutJob>,
    pub layout_cancelled: bool,
    pub layout_error: Option<String>,
    source: std::sync::Arc<SystemDescription>,
}

impl Diagram {
    pub fn new(description: &SystemDescription) -> Self {
        let source = std::sync::Arc::new(description.clone());
        let state = DiagramState::new(description);
        let layout = Layout {
            ports: BTreeMap::new(),
            nets: BTreeMap::new(),
            nodes: BTreeMap::new(),
            sides: BTreeMap::new(),
            bounds: [Point { x: 0., y: 0. }, Point { x: WIDTH, y: 160. }],
            unrouted: vec![],
        };
        let job = Some(worker::LayoutJob::start(
            source.clone(),
            state.clone(),
            true,
        ));
        Self {
            backdrops: Vec::new(), annotation_groups:vec![],annotation_hover:BTreeSet::new(),
            bundles: BTreeMap::new(),
            marked_components: BTreeSet::new(),
            decorations: BTreeMap::new(),
            dimmed_domains: BTreeSet::new(),
            state,
            layout,
            selected: None,
            linked_highlights: None,
            fit_requested: true,
            job,
            layout_cancelled: false,
            layout_error: None,
            source,
        }
    }
    pub fn is_layout_pending(&self) -> bool {
        self.job.is_some()
    }
    pub fn cancel_layout(&mut self) {
        self.job = None;
        if !self.layout.nodes.is_empty() {
            self.state.positions = self
                .layout
                .nodes
                .iter()
                .map(|(id, n)| (id.clone(), n.position))
                .collect();
        }
        self.layout_cancelled = true;
    }
    pub fn poll_layout(&mut self) {
        let result = self.job.as_ref().map(|job| job.result());
        match result {
            Some(Ok(Some((revision, positions, layout)))) => {
                if revision == self.state.revision {
                    self.state.positions = positions;
                    self.layout = layout;
                }
                self.job = None;
            }
            Some(Err(error)) => {
                self.layout_error = Some(error);
                self.job = None;
            }
            _ => {}
        }
    }
    fn request_layout(&mut self, arrange: bool) {
        self.job = None;
        self.layout_cancelled = false;
        self.layout_error = None;
        self.job = Some(worker::LayoutJob::start(
            self.source.clone(),
            self.state.clone(),
            arrange,
        ));
    }

    pub fn restore_state(
        &mut self,
        state: DiagramState,
    ) -> Result<(), sim_inspect::InspectionError> {
        state.validate(&self.source)?;
        self.state = state;
        self.fit_requested = false;
        self.request_layout(false);
        Ok(())
    }
    pub fn fit(&mut self) {
        self.fit_requested = true;
    }
    pub fn reset_layout(&mut self, description: &SystemDescription) {
        if self.source.id != description.id {
            *self = Self::new(description);
            return;
        }
        self.state.revision += 1;
        self.request_layout(true);
        self.fit();
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn show(&mut self, ui: &mut egui::Ui, description: &SystemDescription) {
        self.poll_layout();
        if self.is_layout_pending() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        let (response, painter) = ui.allocate_painter(
            ui.available_size().max(Vec2::splat(1.)),
            Sense::click_and_drag(),
        );
        let viewport = response.rect;
        painter.rect_filled(viewport, 0, Color32::from_rgb(241, 244, 249));
        for b in &self.backdrops {
            let (zoom, camera) = (self.state.zoom, self.state.camera);
            let center = viewport.min + Vec2::new(camera.x + b.center.x * zoom, camera.y + b.center.y * zoom);
            let rect = Rect::from_center_size(center, Vec2::new(b.size[0], b.size[1]) * zoom);
            painter.image(b.texture, rect, Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)), Color32::WHITE.gamma_multiply(b.opacity.clamp(0., 1.)));
        }
        if self.layout.nodes.is_empty() {
            painter.text(
                viewport.center(),
                Align2::CENTER_CENTER,
                if self.layout_error.is_some() {
                    "Layout failed — choose Arrange to retry"
                } else if self.layout_cancelled {
                    "Layout cancelled — choose Arrange to retry"
                } else {
                    "Arranging the system…"
                },
                FontId::proportional(16.),
                Color32::DARK_GRAY,
            );
            return;
        }
        if self.fit_requested && !self.is_layout_pending() {
            let [min, max] = self.layout.bounds;
            self.state.zoom = ((viewport.width() - 80.) / (max.x - min.x + 40.))
                .min((viewport.height() - 120.) / (max.y - min.y + 40.))
                .clamp(0.15, 1.3);
            self.state.camera = Point {
                x: (viewport.width() - (max.x - min.x) * self.state.zoom) * 0.5
                    - min.x * self.state.zoom,
                y: (viewport.height() - (max.y - min.y) * self.state.zoom) * 0.4
                    - min.y * self.state.zoom,
            };
            self.fit_requested = false;
        }
        if response.hovered() {
            let zoom = ui.input(|i| i.zoom_delta());
            let wheel = ui.input(|i| i.smooth_scroll_delta().y);
            let factor = if zoom != 1. {
                zoom
            } else {
                (wheel * 0.002).exp()
            };
            if factor != 1. {
                let anchor = ui
                    .input(|i| i.pointer.hover_pos())
                    .unwrap_or(viewport.center())
                    - viewport.min;
                let old = self.state.zoom;
                self.state.zoom = (old * factor).clamp(0.12, 3.);
                let f = self.state.zoom / old;
                self.state.camera.x = anchor.x - (anchor.x - self.state.camera.x) * f;
                self.state.camera.y = anchor.y - (anchor.y - self.state.camera.y) * f;
            }
        }
        let zoom = self.state.zoom;
        let camera = self.state.camera;
        let screen =
            |p: Point| viewport.min + Vec2::new(camera.x + p.x * zoom, camera.y + p.y * zoom);
        for group in &self.annotation_groups {
            let mut bounds=Rect::NOTHING;
            for id in &group.components {if let Some(node)=self.layout.nodes.get(id){bounds=bounds.union(Rect::from_min_size(screen(node.position),Vec2::new(WIDTH,node.height)*zoom));}}
            if bounds.is_finite(){let bounds=bounds.expand(14.*zoom);let color=Color32::from_rgb(group.color[0],group.color[1],group.color[2]);painter.rect_filled(bounds,10.,color.gamma_multiply(0.06));painter.rect_stroke(bounds,10.,Stroke::new(1.7,color.gamma_multiply(0.7)),StrokeKind::Outside);painter.text(bounds.left_top()+Vec2::new(8.,-8.),Align2::LEFT_BOTTOM,&group.label,FontId::proportional((15.*zoom).max(11.)),color);}
        }
        let dark = Color32::from_rgb(33, 47, 67);
        let muted = Color32::from_rgb(102, 118, 138);
        let accent = Color32::from_rgb(34, 99, 205);
        let selected_component = match &self.selected {
            Some(Selection::Component(id)) => Some(id.as_str()),
            Some(Selection::Port(id)) => description.ports.get(id).map(|p| p.component.as_str()),
            _ => None,
        };
        let trace_nets = self
            .linked_highlights
            .as_ref()
            .map(|h| h.nets.clone())
            .unwrap_or_else(|| style::incident_nets(description, self.selected.as_ref()));
        let trace_nodes: BTreeSet<_> = trace_nets
            .iter()
            .flat_map(|id| &description.nets[id].ports)
            .filter_map(|id| description.ports.get(id))
            .map(|p| p.component.as_str())
            .collect();
        let mut next_selection = None;
        let mut pointer_on_item = false;
        // Paint emphasized paths last, with a background halo at crossings.
        let mut ordered: Vec<_> = self.layout.nets.iter().collect();
        ordered.sort_by_key(|(id, _)| trace_nets.contains(*id));
        let mut nearest_net: Option<(f32, String)> = None;
        for (id, net) in ordered {
            let domain = style::net_domain(description, id);
            let active = trace_nets.contains(id);
            let dimmed =
                self.dimmed_domains.contains(&domain.key) || (self.selected.is_some() && !active);
            let color = if dimmed {
                domain.color.gamma_multiply(0.14)
            } else {
                domain.color
            };
            let bundle_count = self.bundles.get(id).copied().unwrap_or(1);
            for branch in &net.branches {
                for pair in branch.windows(2) {
                    let segment = [screen(pair[0]), screen(pair[1])];
                    let stroke = Stroke::new(if active { 2.8 } else { 1.8 }, color);
                    if active && !dimmed {
                        painter.line_segment(
                            segment,
                            Stroke::new(6.5, Color32::from_rgb(241, 244, 249)),
                        );
                    }
                    if bundle_count > 1 {
                        dashed(&painter, segment, stroke);
                    } else {
                        painter.line_segment(segment, stroke);
                    }
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        if distance(pointer, segment) < 6. && viewport.contains(pointer) {
                            pointer_on_item = true;
                            let distance = distance(pointer, segment);
                            if nearest_net.as_ref().is_none_or(|(old, _)| distance < *old) {
                                nearest_net = Some((distance, id.clone()));
                            }
                        }
                    }
                }
            }
            if bundle_count > 1 && active {
                painter.text(
                    screen(net.junction) + Vec2::new(8., 10.),
                    Align2::LEFT_TOP,
                    format!("×{bundle_count}"),
                    FontId::proportional(11.),
                    color,
                );
            }
            if bundle_count == 1 {
                for junction in &net.junctions {
                    painter.circle_filled(screen(*junction), 3.5, color);
                }
            }
        }
        if response.clicked() {
            if let Some((_, id)) = nearest_net {
                next_selection = Some(Selection::Net(id));
            }
        }
        let mut moved = false;
        for component in description.components.values() {
            let Some(position) = self.state.positions.get(&component.id).copied() else {
                continue;
            };
            let rect = Rect::from_min_size(
                screen(position),
                Vec2::new(WIDTH, self.layout.nodes[&component.id].height) * zoom,
            );
            if !viewport.intersects(rect.expand(8.)) {
                continue;
            }
            let node = ui.interact(
                rect,
                response.id.with(&component.id),
                Sense::click_and_drag(),
            );
            if node.hovered() {
                pointer_on_item = true;
            }
            if node.clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    if !self.marked_components.remove(&component.id) {
                        self.marked_components.insert(component.id.clone());
                    }
                } else {
                    self.marked_components.clear();
                }
                next_selection = Some(Selection::Component(component.id.clone()));
            }
            if node.dragged() {
                let delta = node.drag_delta() / zoom;
                let ids: Vec<_> = if self.marked_components.contains(&component.id) {
                    self.marked_components.iter().cloned().collect()
                } else {
                    vec![component.id.clone()]
                };
                for id in ids {
                    if let Some(p) = self.state.positions.get_mut(&id) {
                        p.x += delta.x;
                        p.y += delta.y;
                        self.state.pinned.insert(id);
                    }
                }
                self.state.revision += 1;
                moved = true;
            }
            let active = self.annotation_hover.contains(&component.id) || Some(component.id.as_str()) == selected_component
                || self.marked_components.contains(&component.id)
                || self
                    .linked_highlights
                    .as_ref()
                    .is_some_and(|h| h.components.contains(&component.id));
            let related =
                self.selected.is_none() || trace_nodes.contains(component.id.as_str()) || active;
            painter.rect_filled(
                rect,
                7.,
                if related {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(239, 242, 246)
                },
            );
            painter.rect_stroke(
                rect,
                7.,
                Stroke::new(
                    if active { 2. } else { 1. },
                    if active {
                        accent
                    } else {
                        Color32::from_rgb(199, 209, 224)
                    },
                ),
                StrokeKind::Inside,
            );
            let decoration = self.decorations.get(&component.id);
            if let Some(rgb) = decoration.and_then(|d| d.color) {
                painter.rect_filled(
                    Rect::from_min_size(
                        rect.min + Vec2::new(4., 4.),
                        Vec2::new(rect.width() - 8., 3.),
                    ),
                    2.,
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                );
            }
            let title_font = (18. * zoom).max(11.);
            let title = shortened(
                &component.label,
                ((WIDTH - 32.) * zoom / (title_font * 0.55)).floor().max(4.) as usize,
            );
            painter.text(
                rect.min + Vec2::new(16., 22.) * zoom,
                Align2::LEFT_CENTER,
                title,
                FontId::proportional((18. * zoom).max(11.)),
                dark,
            );
            painter.text(
                rect.min + Vec2::new(16., 48.) * zoom,
                Align2::LEFT_CENTER,
                shortened(
                    &component.component_type,
                    ((WIDTH - 32.) * zoom / ((11. * zoom).max(8.) * 0.62))
                        .floor()
                        .max(4.) as usize,
                ),
                FontId::monospace((11. * zoom).max(8.)),
                muted,
            );
            if let Some(note) = decoration.filter(|d| !d.note.is_empty()) {
                painter.text(
                    rect.min + Vec2::new(16., 65.) * zoom,
                    Align2::LEFT_CENTER,
                    shortened(&format!("Note: {}", note.note.replace('\n', " ")), 34),
                    FontId::proportional((12. * zoom).max(8.)),
                    Color32::from_rgb(131, 86, 17),
                );
            } else if let Some((name, parameter)) = component
                .parameters
                .iter()
                .find(|(name, _)| !name.starts_with("initial."))
            {
                painter.text(
                    rect.min + Vec2::new(16., 65.) * zoom,
                    Align2::LEFT_CENTER,
                    shortened(
                        &format!(
                            "{name}  {} {}",
                            style::number(parameter.value),
                            parameter.unit.as_deref().unwrap_or("")
                        ),
                        31,
                    ),
                    FontId::proportional(12. * zoom),
                    dark,
                );
            }
            node.on_hover_text(format!(
                "{}\n{}\n{}\n{}\nDrag to move and pin. Shift-click to select several components.",
                component.label,
                component.component_type,
                decoration
                    .and_then(|d| d.group_label.as_deref())
                    .unwrap_or(""),
                decoration.map(|d| d.note.as_str()).unwrap_or("")
            ));
        }
        for (id, position) in &self.layout.ports {
            let center = screen(*position);
            if !viewport.contains(center) {
                continue;
            }
            let port = &description.ports[id];
            let physical = matches!(port.schema, PortKind::Physical { .. });
            let domain = style::port_domain(description, &port.schema);
            let color = if self.dimmed_domains.contains(&domain.key) {
                domain.color.gamma_multiply(0.2)
            } else {
                domain.color
            };
            if physical {
                painter.circle_filled(center, 4.5, color);
            } else {
                painter.rect_filled(Rect::from_center_size(center, Vec2::splat(8.)), 1., color);
            }
            if self
                .linked_highlights
                .as_ref()
                .is_some_and(|h| h.ports.contains(id))
            {
                painter.circle_stroke(
                    center,
                    7.5,
                    Stroke::new(2., Color32::from_rgb(206, 140, 35)),
                );
            }
            let side = self.layout.sides[id];
            if zoom > 0.35 {
                painter.text(
                    center + Vec2::new(-side.sign() * 10. * zoom, 0.),
                    if side == Side::Left {
                        Align2::LEFT_CENTER
                    } else {
                        Align2::RIGHT_CENTER
                    },
                    shortened(
                        &port.name,
                        (((WIDTH / 2. - 20.) * zoom) / ((12. * zoom).max(9.) * 0.62))
                            .floor()
                            .max(3.) as usize,
                    ),
                    FontId::monospace((12. * zoom).max(9.)),
                    muted,
                );
            }
            let response = ui.interact(
                Rect::from_center_size(center, Vec2::splat(16.)),
                response.id.with(id),
                Sense::click(),
            );
            if response.hovered() {
                pointer_on_item = true;
            }
            if response.clicked() {
                next_selection = Some(Selection::Port(id.clone()));
            }
            response.on_hover_text(format!(
                "{} · {}\n{}\nClick to trace this terminal and inspect quantities",
                description.components[&port.component].label, port.name, domain.label
            ));
            if matches!(port.schema, PortKind::SignalOutput { .. }) {
                painter.arrow(
                    center,
                    Vec2::new(side.sign() * 16., 0.),
                    Stroke::new(1.5, color),
                );
            }
        }
        if response.clicked() && !pointer_on_item {
            self.selected = None;
            self.marked_components.clear();
        }
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
            self.marked_components.clear();
        }
        if let Some(selection) = next_selection {
            self.selected = Some(selection);
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || (response.dragged() && !pointer_on_item && !moved)
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.state.camera.x += delta.x;
            self.state.camera.y += delta.y;
        }
        if moved {
            self.request_layout(false);
        }
    }
}

fn shortened(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.into()
    } else {
        format!("{}…", value.chars().take(limit - 1).collect::<String>())
    }
}
fn distance(point: Pos2, segment: [Pos2; 2]) -> f32 {
    let v = segment[1] - segment[0];
    if v.length_sq() == 0. {
        return point.distance(segment[0]);
    }
    point.distance(segment[0] + v * ((point - segment[0]).dot(v) / v.length_sq()).clamp(0., 1.))
}

fn dashed(painter: &egui::Painter, segment: [Pos2; 2], stroke: Stroke) {
    let delta = segment[1] - segment[0];
    let length = delta.length();
    if length == 0. {
        return;
    }
    let direction = delta / length;
    let mut start = 0.;
    while start < length {
        painter.line_segment(
            [
                segment[0] + direction * start,
                segment[0] + direction * (start + 7.).min(length),
            ],
            stroke,
        );
        start += 12.;
    }
}
