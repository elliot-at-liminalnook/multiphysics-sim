//! Read-only schematic pane for build mode: the current level of the
//! compiled system laid out by the shared `sim_diagram` layer (hierarchy
//! projection, initial placement, orthogonal routing, domain styles) on a
//! worker thread, and drawn with Bevy UI nodes beside the 3D view.
//!
//! Presentation only: the layout is never written to the document or the
//! file. The pane keeps no selection of its own; each box activates
//! `BuildAction::SchematicSelect`, which runs the Outline's selection path,
//! and the highlight is projected from the shared selection (`picked`).
//! Layouts are keyed by (description id, revision, level). A newer key
//! cancels the pending job, and a layout for an old key is only ever shown
//! dimmed under a "Stale" label with its boxes disabled.
use super::*;
use crate::ui_kit::{BAR, BORDER, Kit, LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR, SUBTLE, SWITCHER_STRIP, TEXT, TOPBAR, Tint, WARN, size};
use sim_diagram::{Layout, layout as diagram_layout, projection, style};
use std::sync::atomic::AtomicBool;

/// What a layout was computed for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Key {
    pub description_id: String,
    pub revision: u64,
    pub level: String,
}

/// A finished layout of one level.
pub(crate) struct Laid {
    pub key: Key,
    pub projection: projection::Projection,
    pub layout: Layout,
    /// Diagram node id -> instance name at `key.level`.
    pub instances: BTreeMap<String, String>,
    /// Worker time for projection, placement and routing.
    pub layout_ms: f64,
}

/// A layout in progress; dropping it cancels the worker (`lay_out` checks the token).
struct Job {
    key: Key,
    work: crate::jobs::Job<Option<Laid>>,
}

#[derive(Default)]
pub(crate) struct Schematic {
    pub visible: bool,
    /// Latest compiled description and the document revision it was compiled from.
    source: Option<(Arc<SystemDescription>, u64)>,
    laid: Option<Laid>,
    job: Option<Job>,
    error: Option<String>,
    /// Key whose layout failed (not retried until the key changes).
    failed: Option<Key>,
    /// Jobs dropped because a newer key superseded them.
    pub(crate) superseded: usize,
    /// UI-thread cost of the last pane rebuild (ms).
    pub(crate) build_ms: f64,
    /// Pane size in logical pixels (set from the window each frame).
    pub(crate) pane: [f32; 2],
    /// Digest of what the pane shows, to rebuild only on change.
    shown: u64,
}

/// The instance at `level` containing a flattened component or group path.
pub(crate) fn instance_at(level: &str, path: &str) -> Option<String> {
    let rest = if level.is_empty() { path } else { path.strip_prefix(&format!("{level}/"))? };
    rest.split('/').next().filter(|s| !s.is_empty()).map(str::to_string)
}

/// The part of the flattened description inside `level` (every component
/// under that subsystem, and the nets among them). Presentation only.
fn level_description(d: &SystemDescription, level: &str) -> SystemDescription {
    if level.is_empty() {
        return d.clone();
    }
    let mut s = d.clone();
    s.components.retain(|id, _| projection::in_group(d, id, level));
    let components = &s.components;
    s.ports.retain(|_, p| components.contains_key(&p.component));
    let ports = &s.ports;
    for net in s.nets.values_mut() {
        net.ports.retain(|p| ports.contains_key(p));
    }
    s.nets.retain(|_, n| n.ports.len() >= 2);
    let inside = |id: &str| {
        let mut g = Some(id);
        while let Some(x) = g {
            if x == level {
                return true;
            }
            g = d.groups.get(x).and_then(|g| g.parent.as_deref());
        }
        false
    };
    s.groups.retain(|id, _| inside(id));
    if let Some(g) = s.groups.get_mut(level) {
        g.parent = None;
    }
    s.observables.clear();
    s.diagnostics.clear();
    s
}

/// Lay out one level with the shared layer: subsystems directly at `level`
/// collapse to one node each, then `initial_state_cancellable` places and
/// `route_cancellable` routes. None when cancelled.
pub(crate) fn lay_out(description: &SystemDescription, key: Key, cancel: &AtomicBool) -> Option<Laid> {
    let started = std::time::Instant::now();
    let composition = sim_system::composition::Composition::new(description.clone()).ok()?;
    let scoped = level_description(&composition.description, &key.level);
    let parent = (!key.level.is_empty()).then_some(key.level.as_str());
    let collapsed: BTreeSet<String> = scoped.groups.values().filter(|g| g.parent.as_deref() == parent).map(|g| g.id.clone()).collect();
    let presented = sim_diagram::composition::present(&scoped, &collapsed, None, cancel)?;
    let projection = presented.projection;
    let layout = presented.layout;
    let instances = projection
        .nodes
        .iter()
        .filter_map(|(id, source)| {
            let path = match source {
                projection::NodeSource::Component(p) | projection::NodeSource::Group(p) => p,
            };
            instance_at(&key.level, path).map(|i| (id.clone(), i))
        })
        .collect();
    Some(Laid { key, projection, layout, instances, layout_ms: started.elapsed().as_secs_f64() * 1e3 })
}

impl Schematic {
    /// A compile of `revision` finished: the next layouts use this description.
    pub(crate) fn set_source(&mut self, description: &SystemDescription, revision: u64) {
        self.source = Some((Arc::new(description.clone()), revision));
    }

    /// An edit moved the document `from` → `to` without changing the scene
    /// hash, so no compile follows: the compiled description is still the
    /// one for `to`. Only a source compiled at `from` is carried forward.
    pub(crate) fn advance_revision(&mut self, from: u64, to: u64) {
        if let Some((_, compiled)) = &mut self.source {
            if *compiled == from {
                *compiled = to;
            }
        }
    }

    /// The key the pane should show now: the compiled description's id at
    /// the document's revision and level. None before the first compile.
    pub(crate) fn current(&self, revision: u64, level: &str) -> Option<Key> {
        self.source.as_ref().map(|(d, _)| Key { description_id: d.id.clone(), revision, level: level.to_string() })
    }

    pub(crate) fn laid(&self) -> Option<&Laid> {
        self.laid.as_ref()
    }

    pub(crate) fn pending(&self) -> bool {
        self.job.is_some()
    }

    /// A layout is shown whose key is not the current one (an edit, a
    /// recompile or a level change since it was computed).
    pub(crate) fn stale(&self, revision: u64, level: &str) -> bool {
        self.laid.as_ref().is_some_and(|l| Some(&l.key) != self.current(revision, level).as_ref())
    }

    /// Poll the worker, cancel a job for an old key and start one for the
    /// current key. Cheap; called every frame. Never blocks.
    pub(crate) fn tick(&mut self, revision: u64, level: &str) {
        if let Some(job) = self.job.as_mut() {
            match job.work.poll() {
                Some(Ok(Some(laid))) => {
                    self.laid = Some(laid);
                    self.error = None;
                    self.job = None;
                }
                Some(Ok(None)) => self.job = None,
                Some(Err(e)) => {
                    self.error = Some(e);
                    self.failed = Some(job.key.clone());
                    self.job = None;
                }
                None => {}
            }
        }
        let current = self.current(revision, level);
        // A newer revision or level supersedes the pending job (dropping it cancels the worker).
        if self.job.as_ref().is_some_and(|j| Some(&j.key) != current.as_ref()) {
            self.job = None;
            self.superseded += 1;
        }
        if !self.visible {
            return;
        }
        // Lay out only a description compiled from this revision; until the
        // compile lands the shown layout stays marked stale.
        let Some((description, compiled)) = self.source.clone() else { return };
        let Some(key) = current.filter(|_| compiled == revision) else { return };
        if self.laid.as_ref().is_some_and(|l| l.key == key) || self.job.is_some() || self.failed.as_ref() == Some(&key) {
            return;
        }
        self.start(description, key);
    }

    fn start(&mut self, description: Arc<SystemDescription>, key: Key) {
        let job_key = key.clone();
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Compute, key.revision, "the layout worker", move |ctx| Ok(lay_out(&description, job_key, ctx.cancel_flag())));
        self.error = None;
        self.job = Some(Job { key, work });
    }

    /// Diagram nodes highlighted for the one Builder selection, projected
    /// through `Projection::selection_highlights`.
    pub(crate) fn highlighted(&self, selected: &BTreeSet<String>) -> BTreeSet<String> {
        let Some(l) = &self.laid else { return BTreeSet::new() };
        let components = l
            .projection
            .node_members
            .values()
            .flatten()
            .filter(|c| instance_at(&l.key.level, c).is_some_and(|i| selected.contains(&i)))
            .cloned()
            .collect();
        let details = sim_inspect::selection::SelectionDetails { components, ports: BTreeSet::new(), nets: BTreeSet::new() };
        l.projection.selection_highlights(&details).components
    }

    pub(crate) fn json(&self, revision: u64, level: &str, selected: &BTreeSet<String>) -> serde_json::Value {
        let laid = self.laid();
        let highlighted = self.highlighted(selected);
        serde_json::json!({
            "visible": self.visible,
            "laid_out": laid.map(|l| &l.key),
            "current": self.current(revision, level),
            "pending": self.pending(),
            "stale": self.stale(revision, level),
            "node_count": laid.map(|l| l.layout.nodes.len()),
            "net_count": laid.map(|l| l.layout.nets.len()),
            "unrouted": laid.map(|l| l.layout.unrouted.len()),
            "layout_ms": laid.map(|l| l.layout_ms),
            "ui_build_ms": self.build_ms,
            "superseded_jobs": self.superseded,
            "error": self.error,
            "nodes": laid.map(|l| l.instances.iter().map(|(id, i)| serde_json::json!({"node": id, "instance": i, "label": l.projection.view.components.get(id).map(|c| c.label.clone()), "highlighted": highlighted.contains(id)})).collect::<Vec<_>>()),
        })
    }
}

/// Pane width for a window: half the space between the side panels.
fn pane_width(window: &Window) -> f32 {
    ((window.width() - LEFT_WIDTH - RIGHT_WIDTH) * 0.5).clamp(320., 760.)
}

/// Poll/start layouts, keep the 3D viewport clear of the pane, and mark the
/// panel for rebuild only when what the pane shows changed.
pub(super) fn update(mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, window: Option<Single<&Window>>) {
    let b = &mut *builder;
    b.schematic.tick(b.document.revision, &b.level);
    let width = match (&window, b.schematic.visible) {
        (Some(w), true) => pane_width(w).round(),
        _ => 0.,
    };
    let height = window.as_ref().map(|w| (w.height() - TOPBAR - STATUSBAR - SWITCHER_STRIP - scene.builder_dock).max(120.).round()).unwrap_or(0.);
    if scene.builder_side != width {
        scene.builder_side = width;
        b.panel_dirty = true;
    }
    b.schematic.pane = [width, height];
    let digest = {
        use std::hash::{Hash, Hasher};
        let s = &b.schematic;
        let mut h = std::hash::DefaultHasher::new();
        (s.visible, s.laid.as_ref().map(|l| &l.key), s.job.as_ref().map(|j| &j.key), s.stale(b.document.revision, &b.level), &s.error, width.to_bits(), height.to_bits()).hash(&mut h);
        h.finish()
    };
    if digest != b.schematic.shown {
        b.schematic.shown = digest;
        b.panel_dirty = true;
    }
}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (&self.description_id, self.revision, &self.level).hash(state);
    }
}

const HEAD: f32 = 34.;
const PAD: f32 = 10.;
const CANVAS: Color = Color::srgb(0.945, 0.957, 0.976);
const INK: Color = Color::srgb(0.129, 0.184, 0.263);
const MUTED: Color = Color::srgb(0.400, 0.463, 0.541);
const EDGE: Color = Color::srgb(0.780, 0.820, 0.878);
const PICK: Color = Color::srgb(0.133, 0.388, 0.804);

/// A `sim_diagram::style` colour as a Bevy colour.
macro_rules! rgb {
    ($c:expr) => {{
        let c = $c;
        Color::srgb_u8(c.r(), c.g(), c.b())
    }};
}

/// Draw the pane (called from the panel rebuild, so it redraws only when
/// the layout, the selection, the key or the window changed).
pub(super) fn pane(commands: &mut Commands, k: &Kit, b: &Builder, selected: &BTreeSet<String>) {
    let s = &b.schematic;
    if !s.visible || s.pane[0] <= 0. {
        return;
    }
    let [w, h] = s.pane;
    let (revision, level) = (b.document.revision, b.level.as_str());
    let stale = s.stale(revision, level);
    let place = if level.is_empty() { b.document.title.clone() } else { level.to_string() };
    let status = match (&s.laid, s.pending(), stale, &s.error) {
        (_, _, _, Some(e)) => (format!("Layout failed: {e}"), WARN),
        (None, _, _, None) if b.compile_error.is_some() => ("Waiting for a compiling system".into(), WARN),
        (None, _, _, None) => ("Laying out…".into(), WARN),
        (Some(l), pending, true, None) => (format!("Stale: showing revision {}{} · {}", l.key.revision, if l.key.level != level { format!(" of {}", if l.key.level.is_empty() { "the top level" } else { &l.key.level }) } else { String::new() }, if pending { "laying out the current one…" } else if b.compile_error.is_some() { "the current revision does not compile" } else if b.job.is_some() || b.scene_dirty { "waiting for the compile…" } else { "laying out the current one…" }), WARN),
        (Some(l), _, false, None) => (format!("Revision {} · {} nodes · {} unrouted · {:.0} ms", l.key.revision, l.layout.nodes.len(), l.layout.unrouted.len(), l.layout_ms), SUBTLE),
    };
    let highlighted = s.highlighted(selected);
    // A pane beside the inspector (right of the viewport, under the toolbar):
    // no kit dock sits inset from the window edge, so it is placed here.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(RIGHT_WIDTH),
                top: Val::Px(TOPBAR),
                width: Val::Px(w),
                height: Val::Px(h),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(BORDER),
            BuilderPanel,
        ))
        .with_children(|pane| {
            pane.spawn(Node { height: Val::Px(HEAD), padding: UiRect::axes(Val::Px(PAD), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), flex_shrink: 0., overflow: Overflow::clip(), ..default() })
                .with_children(|head| {
                    head.spawn(k.text(format!("Schematic · {place}"), size::SMALL, TEXT, 2));
                    head.spawn(k.text(status.0.clone(), size::DETAIL, status.1, 0));
                });
            pane.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_grow: 1., margin: UiRect { left: Val::Px(6.), right: Val::Px(6.), top: Val::Px(0.), bottom: Val::Px(6.) }, overflow: Overflow::clip(), ..default() }, BackgroundColor(CANVAS)))
                .with_children(|canvas| {
                    let Some(l) = &s.laid else { return };
                    draw(canvas, k, b, l, &highlighted, [w - 12., h - HEAD - 6.], stale);
                    if stale {
                        canvas.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), top: Val::Px(0.), bottom: Val::Px(0.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.07, 0.086, 0.106, 0.62))))
                            .with_children(|o| {
                                o.spawn(k.text("Stale layout (not the current system)", size::ITEM, WARN, 2));
                            });
                    }
                });
        });
}

fn draw(canvas: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, l: &Laid, highlighted: &BTreeSet<String>, size: [f32; 2], stale: bool) {
    let [min, max] = l.layout.bounds;
    let margin = 24.;
    let scale = ((size[0] - 2. * margin) / (max.x - min.x).max(1.)).min((size[1] - 2. * margin) / (max.y - min.y).max(1.)).clamp(0.05, 1.);
    let ox = ((size[0] - (max.x - min.x) * scale) * 0.5).max(margin) - min.x * scale;
    let oy = ((size[1] - (max.y - min.y) * scale) * 0.5).max(margin) - min.y * scale;
    let at = |p: sim_inspect::Point| Vec2::new((ox + p.x * scale).round(), (oy + p.y * scale).round());
    let view = &l.projection.view;
    let instances = b.definition().map(|d| d.instances.keys().cloned().collect::<BTreeSet<_>>()).unwrap_or_default();
    // Nets first (under the boxes): orthogonal segments as thin nodes.
    let incident: BTreeSet<&String> = l
        .layout
        .nets
        .keys()
        .filter(|id| view.nets[*id].ports.iter().any(|p| view.ports.get(p).is_some_and(|p| highlighted.contains(&p.component))))
        .collect();
    for (id, net) in &l.layout.nets {
        let color = rgb!(style::net_domain(view, id).color);
        let thick = if incident.contains(id) { 3. } else { 1.5 };
        let color = if highlighted.is_empty() || incident.contains(id) { color } else { color.with_alpha(0.45) };
        crate::graph_presentation::route(canvas,net,at,color,thick);
    }
    let row = diagram_layout::ROW * scale;
    for (id, node) in &l.layout.nodes {
        let p = at(node.position);
        let (bw, bh) = ((diagram_layout::WIDTH * scale).round(), (node.height * scale).round());
        let component = view.components.get(id);
        let label = component.map(|c| c.label.clone()).unwrap_or_else(|| id.clone());
        let kind = component.map(|c| c.component_type.clone()).unwrap_or_default();
        let instance = l.instances.get(id).cloned();
        let on = highlighted.contains(id);
        let enabled = !stale && instance.as_ref().is_some_and(|i| instances.contains(i));
        let (fill, hover) = if on { (Color::srgb(0.878, 0.922, 0.992), Color::srgb(0.835, 0.894, 0.988)) } else { (Color::WHITE, Color::srgb(0.965, 0.973, 0.988)) };
        let mut e = canvas.spawn((
            Node { border_radius: BorderRadius::all(Val::Px(5.)), position_type: PositionType::Absolute, left: Val::Px(p.x), top: Val::Px(p.y), width: Val::Px(bw), height: Val::Px(bh), flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(7.), Val::Px(4.)), border: UiRect::all(Val::Px(if on { 2. } else { 1. })), overflow: Overflow::clip(), ..default() },
            BackgroundColor(fill),
            BorderColor::all(if on { PICK } else { EDGE }),
        ));
        if let Some(name) = instance {
            e.insert((Button, bevy::ui::prelude::AccessibleLabel::new(name.clone()), BuildAction::SchematicSelect(name), ui_api::Enabled(enabled), Tint::new(fill, hover)));
        }
        e.with_children(|card| {
            card.spawn(k.text(label, size::CAPTION, INK, 2));
            if diagram_layout::HEADER * scale >= 34. {
                card.spawn(k.text(kind, 10., MUTED, 0));
            }
        });
    }
    // Ports on top of the box edges, coloured by domain (`style::port_domain`).
    for (id, point) in &l.layout.ports {
        let Some(port) = view.ports.get(id) else { continue };
        let color = rgb!(style::port_domain(view, &port.schema).color);
        let c = at(*point);
        let physical = matches!(port.schema, sim_inspect::PortKind::Physical { .. });
        canvas.spawn((Node { border_radius: BorderRadius::all(Val::Px(if physical { 4. } else { 1. })), position_type: PositionType::Absolute, left: Val::Px(c.x - 4.), top: Val::Px(c.y - 4.), width: Val::Px(8.), height: Val::Px(8.), ..default() }, BackgroundColor(color)));
        if row >= 11. {
            let left = l.layout.sides.get(id).is_none_or(|s| *s == diagram_layout::Side::Left);
            let width = (diagram_layout::WIDTH * scale * 0.5 - 12.).max(0.);
            let mut n = Node { position_type: PositionType::Absolute, top: Val::Px(c.y - 7.), width: Val::Px(width), height: Val::Px(14.), overflow: Overflow::clip(), ..default() };
            if left {
                n.left = Val::Px(c.x + 8.);
            } else {
                n.left = Val::Px(c.x - 8. - width);
                n.justify_content = JustifyContent::FlexEnd;
            }
            canvas.spawn(n).with_children(|t| {
                t.spawn(k.text(port.name.clone(), 9.5, MUTED, 0));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_external_signal_source_is_presentation_context_not_invalid_source() {
        let mut d:SystemDescription=serde_json::from_str(include_str!("../../../../examples/systems-viewer/full-robot.description.json")).unwrap();
        let template=d.components.values().next().unwrap().clone();
        d.components.clear();d.ports.clear();d.nets.clear();d.groups.clear();
        d.groups.insert("inside".into(),sim_inspect::GroupDescription{id:"inside".into(),label:"Inside".into(),parent:None});
        for (id,group,output) in [("source",None,true),("inside/a",Some("inside"),false),("inside/b",Some("inside"),false)] {
            let mut c=template.clone();c.id=id.into();c.group=group.map(str::to_string);d.components.insert(id.into(),c);
            let pid=format!("{id}/signal");d.ports.insert(pid.clone(),sim_inspect::PortDescription{id:pid,component:id.into(),name:"signal".into(),schema:if output {sim_inspect::PortKind::SignalOutput{signal_type:sim_core::definitions::SignalType::Any}}else{sim_inspect::PortKind::SignalInput{signal_type:sim_core::definitions::SignalType::Any}},composite_parent:None});
        }
        d.nets.insert("signal-node".into(),sim_inspect::NetDescription{id:"signal-node".into(),ports:d.ports.keys().cloned().collect()});
        sim_system::composition::Composition::new(d.clone()).unwrap();
        assert_eq!(level_description(&d,"inside").nets["signal-node"].ports.len(),2);
        let laid=lay_out(&d,Key{description_id:d.id.clone(),revision:23,level:"inside".into()},&AtomicBool::new(false)).unwrap();
        assert_eq!(laid.projection.view.nets["signal-node"].ports.len(),2);
    }

    /// The pane's layout comes from the shared sim_diagram path on a worker,
    /// one node per instance at the level; a schematic activation runs the
    /// same dispatch as a click and sets the shared selection; an edit marks
    /// the shown layout stale until the new revision is compiled and laid
    /// out; a level change re-lays out; the file is never written by it.
    #[test]
    fn schematic_lays_out_levels_shares_selection_and_goes_stale_on_edit() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-schematic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
        let compile = |b: &Builder| system_builder::compile(&b.document, &registry, system_builder::config_for(&b.document)).unwrap();
        let compiled = compile(&b);
        let spatial = compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, "Review"));
        let mut scene = SpatialScene::for_builder(compiled.description.clone(), spatial).unwrap();
        let mut orbit = Orbit { focus: Vec3::ZERO, radius: 0.5, yaw: 0.5, pitch: 0.5, home: false, ..Default::default() };
        let (mut selection, mut documents) = super::super::test_support::test_selection(&b);
        let mut pick = Picked::new(&mut selection, &mut documents);
        let tick = |b: &mut Builder| {
            let level = b.level.clone();
            b.schematic.tick(b.document.revision, &level);
        };
        let settle = |b: &mut Builder| {
            let started = std::time::Instant::now();
            tick(b);
            while b.schematic.pending() {
                assert!(started.elapsed().as_secs() < 60, "layout did not finish");
                std::thread::sleep(std::time::Duration::from_millis(5));
                tick(b);
            }
        };
        let at_level = |b: &Builder| b.definition().unwrap().instances.keys().cloned().collect::<BTreeSet<_>>();

        // Hidden, or before a compile: nothing is laid out.
        tick(&mut b);
        b.schematic.visible = true;
        tick(&mut b);
        assert!(!b.schematic.pending() && b.schematic.laid().is_none());
        let rev1 = b.document.revision;
        b.schematic.set_source(&compiled.description, rev1);
        tick(&mut b);
        assert!(b.schematic.pending(), "the layout runs on a worker");
        settle(&mut b);
        let laid = b.schematic.laid().expect("laid out");
        assert_eq!(laid.key, Key { description_id: compiled.description.id.clone(), revision: rev1, level: String::new() });
        assert_eq!(laid.layout.nodes.len(), at_level(&b).len(), "one node per instance at the level");
        assert_eq!(laid.instances.values().cloned().collect::<BTreeSet<_>>(), at_level(&b));
        let state = pick.state(&b)["schematic"].clone();
        assert_eq!((state["stale"].as_bool(), state["pending"].as_bool(), state["node_count"].as_u64()), (Some(false), Some(false), Some(at_level(&b).len() as u64)));
        assert!(laid.layout.nets.len() > 0 && state["unrouted"].as_u64().is_some());

        // Schematic activation = the shared selection path; the highlight follows the shared selection.
        for name in ["motor", "gearbox"] {
            dispatch(&mut b, &mut scene, &mut orbit, &mut pick, BuildAction::SchematicSelect(name.into()));
            assert_eq!(pick.names(), BTreeSet::from([name.to_string()]));
            let state = pick.state(&b);
            assert_eq!(state["selected"], serde_json::json!([name]));
            let lit: Vec<_> = state["schematic"]["nodes"].as_array().unwrap().iter().filter(|n| n["highlighted"] == true).map(|n| n["instance"].as_str().unwrap().to_string()).collect();
            assert_eq!(lit, vec![name.to_string()]);
        }
        // A 3D click sets the same selection the schematic highlights.
        click_part(&mut b, &mut pick, "drum", false);
        assert_eq!(b.schematic.highlighted(&pick.names()).iter().map(|n| b.schematic.laid().unwrap().instances[n].clone()).collect::<Vec<_>>(), vec!["drum".to_string()]);
        assert_eq!(std::fs::read(&path).unwrap(), original, "laying out and selecting never write the file");

        // An edit: stale (old layout, not current) until compiled and laid out again.
        b.apply("Set parameter", vec![SystemCommand::SetParameter { at: String::new(), name: "load".into(), parameter: "mass".into(), binding: Some(sim_system::ParameterBinding::value(2.5)) }]).unwrap();
        let rev2 = b.document.revision;
        assert!(rev2 > rev1);
        tick(&mut b);
        let state = pick.state(&b)["schematic"].clone();
        assert_eq!((state["stale"].as_bool(), state["laid_out"]["revision"].as_u64(), state["current"]["revision"].as_u64()), (Some(true), Some(rev1), Some(rev2)));
        assert!(!b.schematic.pending(), "waits for the compile of the new revision");
        let compiled2 = compile(&b);
        b.schematic.set_source(&compiled2.description, rev2);
        tick(&mut b);
        assert!(b.schematic.pending() && b.schematic.stale(rev2, ""));
        settle(&mut b);
        assert!(!b.schematic.stale(rev2, ""));
        assert_eq!(b.schematic.laid().unwrap().key, Key { description_id: compiled2.description.id.clone(), revision: rev2, level: String::new() });
        assert!(!b.document.definitions.values().any(|d| serde_json::to_string(d).unwrap().contains("diagram")), "no layout in the document");

        // A scene-neutral edit (same value) bumps the revision but compiles nothing:
        // stale at once, then current at the new revision through the keyed worker.
        b.scene_dirty = false; // the rev2 compile has landed (set_source above stands in for rebuild_scene)
        b.apply("Same value", vec![SystemCommand::SetParameter { at: String::new(), name: "load".into(), parameter: "mass".into(), binding: Some(sim_system::ParameterBinding::value(2.5)) }]).unwrap();
        let rev3 = b.document.revision;
        assert!(rev3 > rev2 && !b.scene_dirty && b.job.is_none(), "no compile is queued for a scene-neutral edit");
        assert!(b.schematic.stale(rev3, ""), "the rev2 layout is not presented as current");
        tick(&mut b);
        assert!(b.schematic.pending(), "a layout for the new revision starts without a compile");
        settle(&mut b);
        assert!(!b.schematic.stale(rev3, "") && !b.schematic.pending());
        assert_eq!(b.schematic.laid().unwrap().key, Key { description_id: compiled2.description.id.clone(), revision: rev3, level: String::new() });
        // With a compile queued the source is not carried forward: the compile sets it.
        b.scene_dirty = true;
        b.apply("Same value again", vec![SystemCommand::SetParameter { at: String::new(), name: "load".into(), parameter: "mass".into(), binding: Some(sim_system::ParameterBinding::value(2.5)) }]).unwrap();
        let rev4 = b.document.revision;
        tick(&mut b);
        assert!(b.schematic.stale(rev4, "") && !b.schematic.pending(), "waits for the queued compile");
        b.schematic.set_source(&compiled2.description, rev4);
        settle(&mut b);
        assert!(!b.schematic.stale(rev4, ""));

        // Drilling into a subsystem lays out that level: its instances only.
        b.set_level("gearbox").unwrap();
        tick(&mut b);
        assert!(b.schematic.stale(rev2, "gearbox"));
        settle(&mut b);
        let laid = b.schematic.laid().unwrap();
        assert_eq!(laid.key.level, "gearbox");
        assert_eq!(laid.instances.values().cloned().collect::<BTreeSet<_>>(), at_level(&b));
        assert_eq!(laid.layout.nodes.len(), at_level(&b).len());
        std::fs::remove_dir_all(&dir).ok();
    }
}
