//! Reversible display placement, using the same grid operation as REST.
use super::*;
use bevy::picking::pointer::PointerButton;
use sim_system::display::Grid;

pub(super) struct DragState {
    pub names: Vec<String>,
    pub start: Vec3,
    pub hit: Vec3,
    pub target: Vec3,
    pub revision: u64,
    pub level: String,
    pub item: Option<PaletteItem>,
    pub axis: Option<usize>,
    pub released: bool,
    pub validation: Option<(Vec3, Result<sim_system::display_overlap::Report, String>)>,
    paths: Vec<String>,
    frame: Transform,
    axis_grab: Option<(usize, f32)>,
    validator:
        super::placement_worker::Latest<Vec3, Result<sim_system::display_overlap::Report, String>>,
    submitted: Option<Vec3>,
    work: Arc<Work>,
    /// The commit (a save): completes even if the drag state is dropped.
    committing: Option<crate::jobs::Job<sim_system::store::Applied>>,
    committed_revision: Option<u64>,
}
struct Work {
    document: sim_system::SystemDocument,
    registry: BehaviorRegistry,
    level: String,
    names: Vec<String>,
    item: Option<PaletteItem>,
    new_name: Option<String>,
    path: PathBuf,
    /// Placement-relevant content at drag start; discussion edits leave it unchanged.
    scene_hash: String,
    template: std::sync::OnceLock<Result<Vec<SystemCommand>, String>>,
}
impl Work {
    fn commands(&self, target: Vec3) -> Result<Vec<SystemCommand>, String> {
        let Some(item) = &self.item else {
            return sim_system::display::moves(
                &self.document,
                &self.registry,
                &self.level,
                &self.names,
                target.to_array(),
                false,
            )
            .map_err(|e| e.to_string());
        };
        let mut commands = self
            .template
            .get_or_init(|| {
                let mut commands = vec![];
                if let Some(path) = &item.library_path {
                    commands.push(SystemCommand::AddDefinitions {
                        definitions: sim_system::library::import(std::path::Path::new(path))
                            .map_err(|e| e.to_string())?,
                    });
                }
                commands.push(SystemCommand::AddInstance {
                    at: self.level.clone(),
                    name: self.new_name.clone().unwrap(),
                    instance: sim_system::snap::starter(&self.registry, &item.kind, &item.label),
                });
                Ok(commands)
            })
            .clone()?;
        if let Some(SystemCommand::AddInstance { instance, .. }) = commands.last_mut() {
            instance.placement.position = target.to_array();
        }
        Ok(commands)
    }
    fn validate(&self, target: Vec3) -> Result<sim_system::display_overlap::Report, String> {
        sim_system::display_overlap::preview(
            &self.document,
            &self.registry,
            &self.commands(target)?,
        )
        .map_err(|e| e.to_string())
    }
    fn commit(&self, target: Vec3) -> Result<sim_system::store::Applied, String> {
        let store = SystemStore::new(self.path.clone());
        let commands = self.commands(target)?;
        let mut expected = self.document.revision;
        // A comment or agent reply saved meanwhile bumps the revision but not the
        // assembly; retry against it. Any other edit still rejects the drop.
        for _ in 0..8 {
            match store.apply(&self.registry, "Drag display parts", &commands, Some(expected)) {
                Err(sim_system::SystemError::Stale { .. }) => {
                    let current = store.load().map_err(|e| e.to_string())?;
                    if sim_system::display::scene_hash(&current) != self.scene_hash {
                        return Err(
                            "Another editor changed the assembly during the drag; source placement unchanged."
                                .into(),
                        );
                    }
                    expected = current.revision;
                }
                result => return result.map_err(|e| e.to_string()),
            }
        }
        Err("The system file kept changing; placement not saved.".into())
    }
}
impl DragState {
    pub(super) fn awaiting_scene(&self) -> bool {
        self.committed_revision.is_some()
    }
    /// A released drag that would not move anything; saving it would only add
    /// an empty undo step and a revision that never gets a new scene.
    fn unchanged(&self) -> bool {
        self.item.is_none() && self.target == self.start
    }
    /// True while the assembly matches the drag's snapshot. Discussion-only edits
    /// (comments, agent replies) advance the tracked revision instead of cancelling.
    fn same_scene(&mut self, document: &sim_system::SystemDocument) -> bool {
        if document.revision == self.revision {
            return true;
        }
        if sim_system::display::scene_hash(document) != self.work.scene_hash {
            return false;
        }
        self.revision = document.revision;
        true
    }
    fn new(
        b: &Builder,
        names: Vec<String>,
        start: Vec3,
        hit: Vec3,
        item: Option<PaletteItem>,
        axis: Option<usize>,
    ) -> Self {
        let new_name = item.as_ref().map(|item| {
            b.unique_name(match &item.kind {
                InstanceKind::Element { component_type } => {
                    component_type.rsplit('.').next().unwrap_or("part")
                }
                InstanceKind::Subsystem { definition } => {
                    definition.rsplit('.').next().unwrap_or("sub")
                }
            })
        });
        let work = Arc::new(Work {
            document: b.document.clone(),
            registry: b.registry.clone(),
            level: b.level.clone(),
            names: names.clone(),
            item: item.clone(),
            new_name,
            path: b.store.path.clone(),
            scene_hash: sim_system::display::scene_hash(&b.document),
            template: Default::default(),
        });
        let validation_work = work.clone();
        Self {
            paths: names.iter().map(|n| b.full_path(n)).collect(),
            frame: b.frame(),
            names,
            start,
            hit,
            target: start,
            revision: b.document.revision,
            level: b.level.clone(),
            item,
            axis,
            released: false,
            validation: None,
            axis_grab: None,
            validator: super::placement_worker::Latest::new(move |p| validation_work.validate(*p)),
            submitted: None,
            work,
            committing: None,
            committed_revision: None,
        }
    }
    fn project(&mut self, origin: Vec3, direction: Vec3, grid: &Grid, snap: bool) {
        let inverse = self.frame.to_matrix().inverse();
        let (_, _, n) = grid.plane.axes();
        let origin = inverse.transform_point3(origin);
        let dir = inverse.transform_vector3(direction);
        let mut hit = None;
        if let Some(axis) = self.axis {
            // Closest point on the axis to the mouse ray (works for vertical handles).
            let mut unit = Vec3::ZERO;
            unit[axis] = 1.;
            let w = origin - self.start;
            let dot = dir.dot(unit);
            let denom = 1. - dot * dot;
            if denom.abs() > 1e-5 {
                let coordinate = (w.dot(unit) - dot * w.dot(dir)) / denom;
                let initial = match self.axis_grab {
                    Some((a, x)) if a == axis => x,
                    _ => {
                        self.axis_grab = Some((axis, coordinate));
                        coordinate
                    }
                };
                hit = Some(self.start + unit * (coordinate - initial));
            }
        } else if dir[n].abs() > 1e-6 {
            let height = if self.item.is_some() {
                grid.origin_m[n]
            } else {
                self.hit[n]
            };
            let t = (height - origin[n]) / dir[n];
            if t >= 0. {
                hit = Some(origin + dir * t);
            }
        }
        if let Some(hit) = hit {
            let raw = if self.axis.is_some() || self.item.is_some() {
                hit
            } else {
                self.start + hit - self.hit
            };
            let snapped = grid.point(raw.to_array(), snap).unwrap_or(raw.to_array());
            self.target = Vec3::from_array(snapped);
            if let Some(axis) = self.axis {
                for i in 0..3 {
                    if i != axis {
                        self.target[i] = self.start[i];
                    }
                }
            } else if self.item.is_none() {
                self.target[n] = self.start[n];
            }
        }
    }
    fn current_validation(
        &self,
    ) -> Option<&(Vec3, Result<sim_system::display_overlap::Report, String>)> {
        self.validation.as_ref().filter(|(p, _)| *p == self.target)
    }
    fn delta(&self, path: &str) -> Vec3 {
        if self.paths.iter().any(|p| under(path, p)) {
            self.frame.rotation * (self.target - self.start)
        } else {
            Vec3::ZERO
        }
    }
}
fn under(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|s| s.starts_with('/'))
}
pub(super) fn preview_delta(b: &Builder, path: &str) -> Vec3 {
    b.drag.as_ref().map(|d| d.delta(path)).unwrap_or(Vec3::ZERO)
}
pub(super) fn part_transform(b: &Builder, scene: &SpatialScene, index: usize) -> Transform {
    let mut transform = animation::part_transform(scene, index);
    transform.translation += preview_delta(b, &scene.spatial.parts[index].component);
    transform
}
/// Both the rendered meshes and annotation anchors consume this exact preview,
/// in this frame, including the time spent saving/rebuilding after mouse release.
pub(super) fn apply_preview(
    b: Res<Builder>,
    scene: Res<SpatialScene>,
    mut parts: Query<(&Part, &mut Transform)>,
) {
    for (p, mut transform) in &mut parts {
        *transform = part_transform(&b, &scene, p.index);
    }
}

pub(super) fn draw_handles(b: Res<Builder>, mut gizmos: Gizmos) {
    let Some(spec) = b.selected.iter().next().and_then(|n| b.spec(n)) else {
        return;
    };
    let center = b
        .drag
        .as_ref()
        .map(|d| d.target)
        .unwrap_or(Vec3::from_array(spec.placement.position));
    let frame = b
        .drag
        .as_ref()
        .map(|d| d.frame)
        .unwrap_or_else(|| b.frame());
    let length = b.grid().spacing_m * 4.;
    for axis in 0..3 {
        let mut end = center;
        end[axis] += length;
        let color = [
            Color::srgb(1., 0.35, 0.32),
            Color::srgb(0.4, 0.95, 0.5),
            Color::srgb(0.35, 0.6, 1.),
        ][axis];
        gizmos.line(
            frame.transform_point(center),
            frame.transform_point(end),
            color,
        );
        gizmos.sphere(
            Isometry3d::from_translation(frame.transform_point(end)),
            length * 0.07,
            color,
        );
    }
}
impl Builder {
    pub fn grid(&self) -> Grid {
        self.definition_id()
            .and_then(|id| self.document.definitions.get(&id))
            .map(|d| d.grid.clone())
            .unwrap_or_default()
    }
    pub fn display_move(
        &mut self,
        names: Vec<String>,
        position_m: [f32; 3],
        snap: bool,
        preview: bool,
        expected_revision: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        if expected_revision.is_some_and(|r| r != self.document.revision) {
            return Err("stale display placement; reload system_state".into());
        }
        let commands = sim_system::display::moves(
            &self.document,
            &self.registry,
            &self.level,
            &names,
            position_m,
            snap,
        )
        .map_err(|e| e.to_string())?;
        let overlap =
            sim_system::display_overlap::preview(&self.document, &self.registry, &commands)
                .map_err(|e| e.to_string())?;
        if !preview {
            overlap.require_allowed().map_err(|e| e.to_string())?;
            self.apply("Move display parts", commands.clone())?;
        }
        Ok(
            serde_json::json!({"semantics":sim_system::display::SEMANTICS,"frame":"enclosing_definition","unit":"m","level":self.level,"revision":self.document.revision,"preview":preview,"commands":commands,"overlap":overlap}),
        )
    }
    pub fn set_grid(&mut self, grid: Grid) -> Result<(), String> {
        self.apply(
            "Display grid",
            vec![SystemCommand::SetDisplayGrid {
                at: self.level.clone(),
                grid,
            }],
        )
        .map(|_| ())
    }
    fn frame(&self) -> Transform {
        let f = self
            .subsystems
            .get(&self.level)
            .copied()
            .unwrap_or(sim_system::flatten::WorldPlacement::IDENTITY);
        Transform::from_translation(Vec3::from_array(f.position))
            .with_rotation(Quat::from_array(f.rotation_xyzw))
    }
}
pub(crate) fn start_part(
    e: On<Pointer<DragStart>>,
    parts: Query<&Part>,
    scene: Res<SpatialScene>,
    builder: Option<ResMut<Builder>>,
    mode: Option<Res<State<crate::ViewerMode>>>,
) {
    // Only Build places parts: not under the lesson screen, and not in the
    // modes the builder waits through (inspect's parts are not the builder's).
    if mode.is_some_and(|m| *m.get() != crate::ViewerMode::Build) {
        return;
    }
    let Some(mut b) = builder else { return };
    // Alt-drag on a running system pushes on the part instead of moving it.
    if b.grab.is_some() {
        return;
    }
    if e.button != PointerButton::Primary
        || b.mode != Mode::Select
        || b.input.is_some()
        || b.drag.is_some()
    {
        return;
    }
    let Ok(p) = parts.get(e.entity) else { return };
    let Some(name) = b.instance_for_component(&scene.spatial.parts[p.index].component) else {
        return;
    };
    if !b.selected.contains(&name) {
        b.selected = BTreeSet::from([name.clone()]);
    }
    let names: Vec<_> = b.selected.iter().cloned().collect();
    let Some(spec) = names.first().and_then(|n| b.spec(n)) else {
        return;
    };
    let start = Vec3::from_array(spec.placement.position);
    let hit = e
        .hit
        .position
        .map(|p| b.frame().to_matrix().inverse().transform_point3(p))
        .unwrap_or(start);
    b.drag = Some(DragState::new(&b, names, start, hit, None, None));
    b.panel_dirty = true;
}
pub(super) fn start_palette(
    e: On<Pointer<DragStart>>,
    actions: Query<&BuildAction>,
    mut b: ResMut<Builder>,
) {
    if e.button != PointerButton::Primary {
        return;
    }
    let Ok(BuildAction::Preview(index)) = actions.get(e.entity) else {
        return;
    };
    let Some(item) = b.filtered().get(*index).map(|p| (*p).clone()) else {
        return;
    };
    let start = Vec3::from_array(b.grid().origin_m);
    b.drag = Some(DragState::new(&b, vec![], start, start, Some(item), None));
}

pub(super) fn update(
    mut b: ResMut<Builder>,
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<Orbit>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut gizmos: Gizmos,
) {
    let frame = b.frame();
    let grid = b.grid();
    let point = |p: Vec3| frame.transform_point(p);
    let (a, c, _) = grid.plane.axes();
    if grid.visible {
        let center = Vec3::from_array(grid.origin_m);
        for k in -20..=20 {
            let color = if k == 0 {
                Color::srgba(0.35, 0.65, 0.68, 0.7)
            } else if k % 5 == 0 {
                Color::srgba(0.33, 0.40, 0.46, 0.65)
            } else {
                Color::srgba(0.27, 0.32, 0.38, 0.4)
            };
            for (u, v) in [(a, c), (c, a)] {
                let mut p = center;
                let mut q = center;
                p[u] += k as f32 * grid.spacing_m;
                q[u] = p[u];
                p[v] -= 20. * grid.spacing_m;
                q[v] += 20. * grid.spacing_m;
                gizmos.line(point(p), point(q), color);
            }
        }
    }
    let cursor = window.cursor_position();
    let in_scene = cursor.is_some_and(|p| {
        p.x > scene.left()
            && p.x < window.width() - scene.right()
            && p.y > scene.top()
            && p.y < window.height() - scene.bottom()
    });
    // Axis handles can be grabbed at their endpoint; keyboard X/Y/Z also constrains a drag.
    if let Some(name) = b.selected.iter().next().cloned() {
        if let Some(spec) = b.spec(&name) {
            let center = b
                .drag
                .as_ref()
                .map(|d| d.target)
                .unwrap_or(Vec3::from_array(spec.placement.position));
            let length = grid.spacing_m * 4.;
            for axis in 0..3 {
                let mut end = center;
                end[axis] += length;
                if b.drag.is_none() && in_scene && buttons.just_pressed(MouseButton::Left) {
                    if let (Some(cursor), Ok(screen)) =
                        (cursor, camera.0.world_to_viewport(camera.1, point(end)))
                    {
                        if cursor.distance(screen) < 12. {
                            b.drag = Some(DragState::new(
                                &b,
                                b.selected.iter().cloned().collect(),
                                center,
                                center,
                                None,
                                Some(axis),
                            ));
                        }
                    }
                }
            }
        }
    }
    let Some(mut drag) = b.drag.take() else {
        return;
    };
    // Keep the exact release preview until the authoritative transaction and
    // replacement scene arrive. No file locks, validation or compilation here.
    if let Some(hold_preview) = poll_drop(&mut b, &scene, &mut drag) {
        if hold_preview {
            b.drag = Some(drag);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape)
        || drag.level != b.level
        || !drag.same_scene(&b.document)
    {
        b.status = "Display drag cancelled; source placement unchanged.".into();
        b.panel_dirty = true;
        return;
    }
    for (key, axis) in [(KeyCode::KeyX, 0), (KeyCode::KeyY, 1), (KeyCode::KeyZ, 2)] {
        if keys.just_pressed(key) {
            drag.axis = Some(axis);
        }
    }
    if in_scene {
        if let Some(ray) = cursor.and_then(|p| camera.0.viewport_to_world(camera.1, p).ok()) {
            drag.project(
                ray.origin,
                *ray.direction,
                &grid,
                grid.snap && !keys.pressed(KeyCode::AltLeft) && !keys.pressed(KeyCode::AltRight),
            );
        }
    }
    if drag.submitted != Some(drag.target) {
        drag.validator.submit(drag.target);
        drag.submitted = Some(drag.target);
    }
    if let Some(result) = drag.validator.take() {
        // A result for an older cursor position must never color this preview
        // green, reject the current position, or authorize a drop.
        if result.0 == drag.target {
            drag.validation = Some(result);
        }
    }
    let current = drag.current_validation();
    let allowed = current.is_some_and(|(_, r)| r.as_ref().is_ok_and(|r| r.allowed));
    b.status = match current {
        Some((_, Ok(r))) if r.allowed => {
            "Display placement clear — release to place; Escape cancels.".into()
        }
        Some((_, Ok(_))) => "Overlap — choose a clear position; Escape cancels.".into(),
        Some((_, Err(e))) => e.clone(),
        None => "Moving parts · checking placement…".into(),
    };
    if let Some((_, Ok(report))) = current {
        for conflict in &report.conflicts {
            for bounds in [&conflict.first, &conflict.second] {
                let min = Vec3::from_array(bounds.min_m);
                let max = Vec3::from_array(bounds.max_m);
                gizmos.cube(
                    Transform::from_translation((min + max) * 0.5).with_scale(max - min),
                    Color::srgb(1., 0.2, 0.18),
                );
            }
        }
    }
    if drag.item.is_some() {
        gizmos.cube(
            Transform::from_translation(point(drag.target))
                .with_scale(Vec3::splat(grid.spacing_m * 2.)),
            if allowed {
                Color::srgba(0.4, 1., 0.85, 0.6)
            } else {
                Color::srgba(1., 0.2, 0.18, 0.8)
            },
        );
    }
    if drag.released || !buttons.pressed(MouseButton::Left) {
        if !in_scene {
            b.status = "Display drag cancelled outside the viewport.".into();
            b.panel_dirty = true;
            return;
        }
        if drag.unchanged() {
            b.status = "Placement unchanged.".into();
            b.panel_dirty = true;
            return;
        }
        // Commit checks the latest source revision and full overlap policy
        // again in the worker. A pending or stale preview is never trusted.
        drag.released = true;
        drag.validator.take();
        let work = drag.work.clone();
        let target = drag.target;
        drag.committing = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Io, 0, "Placement worker", move |_| work.commit(target)).complete_on_drop());
        b.status = "Checking and saving placement…".into();
        b.panel_dirty = true;
    }
    b.drag = Some(drag);
}

fn poll_drop(b: &mut Builder, scene: &SpatialScene, drag: &mut DragState) -> Option<bool> {
    if let Some(job) = &drag.committing {
        match job.poll() {
            Some(Ok(applied)) => {
                drag.committing = None;
                drag.committed_revision = Some(applied.revision);
                b.reload();
                if let Some(name) = &drag.work.new_name {
                    b.selected = BTreeSet::from([name.clone()]);
                }
                b.status = "Placement saved.".into();
            }
            Some(Err(e)) => {
                b.reload();
                b.status = e.clone();
                b.action_error = Some(e);
                b.panel_dirty = true;
                return Some(false);
            }
            None => {}
        }
        return Some(true);
    }
    if let Some(revision) = drag.committed_revision {
        // Stop holding the preview once the new scene is in, or when no rebuild
        // is coming (the rebuild failed, or the saved edit changed no display).
        let rebuilding = b.scene_dirty || b.job.is_some();
        if scene.description.model_revision >= revision || !rebuilding {
            b.panel_dirty = true;
            return Some(false);
        }
        return Some(true);
    }
    None
}

/// A global observer: it also sees drags in modes without a builder.
pub(super) fn end_drag(e: On<Pointer<DragEnd>>, b: Option<ResMut<Builder>>) {
    let Some(mut b) = b else { return };
    if e.button == PointerButton::Primary {
        if let Some(d) = b.drag.as_mut() {
            d.released = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_system::{Definition, SystemDocument};
    fn fixture(test: impl FnOnce(Builder, SpatialScene)) {
        // A counter as well: parallel fixtures can share a clock tick.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "drag-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = SystemDocument::new("Drag fixture");
        let mut group = Definition::new("Assembly");
        group.instances.insert(
            "left".into(),
            InstanceSpec::element("electrical.resistor").with("resistance", 10.),
        );
        group.instances.insert(
            "right".into(),
            InstanceSpec::element("electrical.resistor")
                .with("resistance", 20.)
                .at([0.08, 0., 0.]),
        );
        doc.definitions.insert("assembly_def".into(), group);
        let root = doc.definitions.get_mut(&doc.root).unwrap();
        root.instances.insert(
            "a".into(),
            InstanceSpec::element("electrical.resistor")
                .with("resistance", 100.)
                .at([0., 0.02, 0.]),
        );
        root.instances.insert(
            "b".into(),
            InstanceSpec::element("electrical.resistor")
                .with("resistance", 200.)
                .at([0.5, 0.02, 0.]),
        );
        root.instances.insert(
            "assembly".into(),
            InstanceSpec::subsystem("assembly_def").at([0.2, 0.02, 0.]),
        );
        sim_system::display::assign_ids(&mut doc);
        let path = dir.join("test.system.json");
        SystemStore::create(&path, &doc).unwrap();
        let mut b = Builder::open(
            path,
            dir.join("library/systems"),
            sim_runtime::system_registry(),
        )
        .unwrap();
        let scene = scene(&mut b);
        test(b, scene);
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn scene(b: &mut Builder) -> SpatialScene {
        let flat = sim_system::flatten(&b.document, &b.registry).unwrap();
        let d = sim_inspect::model::describe(
            &flat.model,
            &b.registry,
            &flat.source_hash,
            flat.revision,
            &flat.identities,
        )
        .unwrap()
        .description;
        let spatial = flat.spatial(&d.id, &b.document.title);
        b.subsystems = flat.subsystems;
        SpatialScene::for_builder(d, spatial).unwrap()
    }
    fn drag(b: &Builder, names: &[&str]) -> DragState {
        let start = Vec3::from_array(b.spec(names[0]).unwrap().placement.position);
        DragState::new(
            b,
            names.iter().map(|s| s.to_string()).collect(),
            start,
            start,
            None,
            None,
        )
    }
    #[test]
    fn plane_drag_keeps_grab_offset_and_normal_coordinate_on_all_grid_planes() {
        fixture(|b, _| {
            for plane in [
                sim_system::display::Plane::Xy,
                sim_system::display::Plane::Xz,
                sim_system::display::Plane::Yz,
            ] {
                let mut d = drag(&b, &["a"]);
                let g = Grid {
                    plane,
                    snap: false,
                    ..Default::default()
                };
                let (a, c, n) = plane.axes();
                d.hit = d.start + Vec3::splat(0.003);
                let mut origin = d.hit;
                origin[a] += 0.013;
                origin[c] -= 0.017;
                origin[n] += 1.;
                let mut direction = Vec3::ZERO;
                direction[n] = -1.;
                d.project(origin, direction, &g, false);
                assert!((d.target[a] - d.start[a] - 0.013).abs() < 1e-6);
                assert!((d.target[c] - d.start[c] + 0.017).abs() < 1e-6);
                assert_eq!(d.target[n], d.start[n]);
            }
        });
    }
    #[test]
    fn snapping_and_alt_bypass_use_identical_grid_math() {
        fixture(|b, _| {
            let mut d = drag(&b, &["a"]);
            let g = Grid::default();
            d.project(Vec3::new(0.013, 1., 0.027), Vec3::NEG_Y, &g, true);
            assert!(d.target.abs_diff_eq(Vec3::new(0.01, 0.02, 0.03), 1e-6));
            d.project(Vec3::new(0.013, 1., 0.027), Vec3::NEG_Y, &g, false);
            assert!(d.target.abs_diff_eq(Vec3::new(0.013, 0.02, 0.027), 1e-6));
        });
    }
    #[test]
    fn axis_handles_do_not_jump_on_grab_and_lock_the_other_coordinates() {
        fixture(|b, _| {
            for axis in 0..3 {
                let mut d = drag(&b, &["a"]);
                d.axis = Some(axis);
                let normal = (axis + 1) % 3;
                let mut direction = Vec3::ZERO;
                direction[normal] = -1.;
                let mut origin = d.start;
                origin[axis] += 0.04;
                origin[normal] += 1.;
                d.project(origin, direction, &Grid::default(), false);
                assert!(d.target.abs_diff_eq(d.start, 1e-6));
                origin[axis] += 0.023;
                d.project(origin, direction, &Grid::default(), false);
                assert!((d.target[axis] - d.start[axis] - 0.023).abs() < 1e-6);
                for k in 0..3 {
                    if k != axis {
                        assert_eq!(d.target[k], d.start[k]);
                    }
                }
            }
        });
    }
    #[test]
    fn parallel_or_behind_camera_rays_preserve_last_finite_position() {
        fixture(|b, _| {
            let mut d = drag(&b, &["a"]);
            let expected = d.target;
            d.project(Vec3::Y, Vec3::X, &Grid::default(), false);
            d.project(Vec3::Y, Vec3::Y, &Grid::default(), false);
            d.axis = Some(0);
            d.project(Vec3::Y, Vec3::X, &Grid::default(), false);
            assert_eq!(d.target, expected);
            assert!(d.target.is_finite());
        });
    }
    #[test]
    fn nested_rotated_frame_projects_in_local_space_and_moves_in_world_space() {
        fixture(|mut b, _| {
            b.level = "assembly".into();
            let mut d = drag(&b, &["left"]);
            d.frame.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            let local_origin = Vec3::new(0.03, 1., 0.02);
            d.project(
                d.frame.transform_point(local_origin),
                d.frame.rotation * Vec3::NEG_Y,
                &Grid::default(),
                false,
            );
            assert!(d.target.abs_diff_eq(Vec3::new(0.03, 0., 0.02), 1e-6));
            assert!(
                d.delta("assembly/left")
                    .abs_diff_eq(Vec3::new(0., 0.03, 0.02), 1e-6)
            );
            assert_eq!(d.delta("assembly/right"), Vec3::ZERO);
        });
    }
    #[test]
    fn group_and_multiple_selection_move_rigidly_without_matching_similar_prefixes() {
        fixture(|b, _| {
            let mut d = drag(&b, &["a", "assembly"]);
            d.target += Vec3::X * 0.031;
            for path in ["a", "assembly", "assembly/left", "assembly/right"] {
                assert_eq!(d.delta(path), Vec3::X * 0.031);
            }
            for path in ["ab", "assembly2/left", "b"] {
                assert_eq!(d.delta(path), Vec3::ZERO);
            }
            let commands = d.work.commands(d.target).unwrap();
            let positions: Vec<_> = commands
                .into_iter()
                .map(|c| {
                    if let SystemCommand::MoveInstance { placement, .. } = c {
                        placement.position
                    } else {
                        panic!()
                    }
                })
                .collect();
            assert!((positions[1][0] - positions[0][0] - 0.2).abs() < 1e-6);
        });
    }
    #[test]
    fn bevy_mesh_and_annotation_anchor_follow_every_frame_without_accumulation() {
        fixture(|mut b, s| {
            let original = b.document.clone();
            let spatial = serde_json::to_value(&s.spatial).unwrap();
            let index = s
                .spatial
                .parts
                .iter()
                .position(|p| p.component == "a")
                .unwrap();
            let base = animation::part_transform(&s, index);
            let pin = Vec3::new(0.003, 0.004, 0.005);
            b.drag = Some(drag(&b, &["a"]));
            let mut app = App::new();
            app.insert_resource(b)
                .insert_resource(s)
                .add_systems(Update, apply_preview);
            let entity = app
                .world_mut()
                .spawn((Part { index }, Transform::default()))
                .id();
            for n in 0..240 {
                let delta = Vec3::new(n as f32 * 0.0001, 0., (n as f32 * 0.1).sin() * 0.01);
                let mut b = app.world_mut().resource_mut::<Builder>();
                let d = b.drag.as_mut().unwrap();
                d.target = d.start + delta;
                app.update();
                let mesh = *app.world().get::<Transform>(entity).unwrap();
                let b = app.world().resource::<Builder>();
                let s = app.world().resource::<SpatialScene>();
                let anchor = super::super::markers::transform(b, s, "a").unwrap();
                assert!(mesh.translation.abs_diff_eq(base.translation + delta, 1e-6));
                assert_eq!(mesh.transform_point(pin), anchor.transform_point(pin));
            }
            let b = app.world().resource::<Builder>();
            assert_eq!(b.document, original);
            assert!(b.store.history().undo.is_empty());
            assert_eq!(
                serde_json::to_value(&app.world().resource::<SpatialScene>().spatial).unwrap(),
                spatial
            );
            app.world_mut().resource_mut::<Builder>().drag = None;
            app.update();
            assert_eq!(*app.world().get::<Transform>(entity).unwrap(), base);
        });
    }
    #[test]
    fn group_annotations_and_descendant_pins_share_the_preview_delta() {
        fixture(|mut b, s| {
            let base = super::super::markers::transform(&b, &s, "assembly").unwrap();
            let child = super::super::markers::transform(&b, &s, "assembly/left").unwrap();
            let mut d = drag(&b, &["assembly"]);
            d.target += Vec3::Z * 0.11;
            b.drag = Some(d);
            for (path, old) in [("assembly", base), ("assembly/left", child)] {
                assert!(
                    (super::super::markers::transform(&b, &s, path)
                        .unwrap()
                        .translation
                        - old.translation)
                        .abs_diff_eq(Vec3::Z * 0.11, 1e-6)
                );
            }
        });
    }
    #[test]
    fn release_holds_preview_until_scene_replacement_then_has_no_double_delta() {
        fixture(|mut b, s| {
            let mut d = drag(&b, &["a"]);
            d.target += Vec3::Z * 0.1;
            d.released = true;
            let expected = Vec3::from_array(
                s.spatial
                    .parts
                    .iter()
                    .find(|p| p.component == "a")
                    .unwrap()
                    .position,
            ) + Vec3::Z * 0.1;
            let (release, gate) = mpsc::channel::<()>();
            let (work, target) = (d.work.clone(), d.target);
            d.committing = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "test commit", move |_| {
                let _ = gate.recv();
                work.commit(target)
            }));
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
            assert!(!d.awaiting_scene());
            release.send(()).unwrap();
            let started = std::time::Instant::now();
            while !d.awaiting_scene() && started.elapsed() < std::time::Duration::from_secs(10) {
                assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(d.awaiting_scene());
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
            b.drag = Some(d);
            let before = super::super::markers::transform(&b, &s, "a").unwrap();
            assert!(before.translation.abs_diff_eq(expected, 1e-6));
            b.reload();
            let new = scene(&mut b);
            let mut d = b.drag.take().unwrap();
            assert_eq!(poll_drop(&mut b, &new, &mut d), Some(false));
            assert!(
                super::super::markers::transform(&b, &new, "a")
                    .unwrap()
                    .translation
                    .abs_diff_eq(expected, 1e-6)
            );
            assert_eq!(b.store.history().undo.len(), 1);
        });
    }
    #[test]
    fn zero_move_release_saves_nothing_and_a_drop_without_rebuild_releases_the_preview() {
        fixture(|mut b, s| {
            let mut d = drag(&b, &["a"]);
            assert!(d.unchanged(), "a click that snaps back must not commit");
            d.target += Vec3::X * 0.01;
            assert!(!d.unchanged());
            // A saved revision that schedules no new scene (or whose rebuild
            // failed) must not hold the drag forever.
            d.committed_revision = Some(b.document.revision + 1);
            b.scene_dirty = true;
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
            b.scene_dirty = false;
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(false));
        });
    }
    #[test]
    fn comments_during_a_drag_neither_cancel_it_nor_reject_the_drop() {
        fixture(|mut b, _| {
            let mut d = drag(&b, &["a"]);
            let thread = sim_system::display::Thread {
                id: "note".into(),
                title: "Resistor".into(),
                resolved: false,
                targets: vec![sim_system::display::bind(&b.document, "a").unwrap()],
                comments: vec![],
                pin_m: None,
                view: None,
            };
            b.apply("note", vec![SystemCommand::PutThread { thread }])
                .unwrap();
            assert!(d.same_scene(&b.document));
            assert_eq!(d.revision, b.document.revision);
            d.work.commit(Vec3::new(0., 0.02, 0.1)).unwrap();
            b.reload();
            assert_eq!(b.spec("a").unwrap().placement.position, [0., 0.02, 0.1]);
            assert!(b.document.discussions.threads.contains_key("note"));
            // An assembly edit still cancels the drag and rejects its drop.
            let mut d = drag(&b, &["a"]);
            b.display_move(vec!["b".into()], [0.6, 0.02, 0.], false, false, None)
                .unwrap();
            assert!(!d.same_scene(&b.document));
            let saved = std::fs::read(&b.store.path).unwrap();
            assert!(d.work.commit(Vec3::new(0., 0.02, 0.2)).is_err());
            assert_eq!(std::fs::read(&b.store.path).unwrap(), saved);
        });
    }
    #[test]
    fn authoritative_drop_rejects_overlap_and_stale_edits_without_overwriting_sources() {
        fixture(|b, _| {
            let d = drag(&b, &["a"]);
            let before = std::fs::read(&b.store.path).unwrap();
            assert!(!d.work.validate(Vec3::new(0.5, 0.02, 0.)).unwrap().allowed);
            assert!(d.work.commit(Vec3::new(0.5, 0.02, 0.)).is_err());
            assert_eq!(std::fs::read(&b.store.path).unwrap(), before);
            d.work.commit(Vec3::new(0., 0.02, 0.1)).unwrap();
            let saved = std::fs::read(&b.store.path).unwrap();
            assert!(d.work.commit(Vec3::new(0., 0.02, 0.2)).is_err());
            assert_eq!(std::fs::read(&b.store.path).unwrap(), saved);
            assert_eq!(b.store.history().undo.len(), 1);
            b.store.undo().unwrap();
            assert_eq!(b.store.load().unwrap().definitions, b.document.definitions);
        });
    }
    #[test]
    fn drag_preview_matches_rest_preview_policy() {
        fixture(|mut b, _| {
            let d = drag(&b, &["a"]);
            for target in [Vec3::new(0., 0.02, 0.1), Vec3::new(0.5, 0.02, 0.)] {
                let report = d.work.validate(target).unwrap();
                let rest = b
                    .display_move(vec!["a".into()], target.to_array(), false, true, None)
                    .unwrap();
                assert_eq!(serde_json::to_value(report).unwrap(), rest["overlap"]);
            }
        });
    }
    #[test]
    fn thousand_part_bevy_preview_stays_inside_60hz_cpu_budget_while_validation_is_blocked() {
        fixture(|mut b, mut s| {
            use std::time::{Duration, Instant};
            let template = s.spatial.parts[0].clone();
            s.spatial.parts = (0..1024)
                .map(|i| {
                    let mut p = template.clone();
                    p.component = format!("assembly/part-{i}");
                    p.id = format!("part/{i}");
                    p
                })
                .collect();
            let mut d = drag(&b, &["assembly"]);
            let (entered, enter) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            d.validator = super::super::placement_worker::Latest::new(move |_: &Vec3| {
                entered.send(()).unwrap();
                gate.recv().unwrap();
                Err("controlled slow validator".into())
            });
            d.validator.submit(d.start);
            enter.recv_timeout(Duration::from_secs(2)).unwrap();
            b.drag = Some(d);
            let mut app = App::new();
            app.insert_resource(b)
                .insert_resource(s)
                .add_systems(Update, apply_preview);
            for index in 0..1024 {
                app.world_mut()
                    .spawn((Part { index }, Transform::default()));
            }
            let mut timings = vec![];
            for n in 0..250 {
                let begin = Instant::now();
                let mut b = app.world_mut().resource_mut::<Builder>();
                let d = b.drag.as_mut().unwrap();
                d.project(
                    Vec3::new(n as f32 * 0.001, 1., 0.1),
                    Vec3::NEG_Y,
                    &Grid::default(),
                    false,
                );
                app.update();
                let b = app.world().resource::<Builder>();
                let s = app.world().resource::<SpatialScene>();
                // Include annotation-anchor work in the hot-path measurement.
                for i in 0..100 {
                    assert!(
                        super::super::markers::transform(b, s, &format!("assembly/part-{i}"))
                            .is_some()
                    );
                }
                if n >= 10 {
                    timings.push(begin.elapsed().as_secs_f64() * 1000.);
                }
            }
            release.send(()).unwrap();
            timings.sort_by(f64::total_cmp);
            let p99 = timings[timings.len() * 99 / 100];
            println!(
                "drag CPU benchmark: 1024 Bevy mesh transforms + 100 annotation anchors; median {:.3} ms, p99 {:.3} ms; budget 16.667 ms (GPU excluded)",
                timings[timings.len() / 2],
                p99
            );
            assert!(
                p99 < 1000. / 60.,
                "drag preview p99 exceeded 60 Hz CPU budget: {p99:.3} ms"
            );
        });
    }
    #[test]
    fn stale_clear_result_cannot_approve_a_new_overlapping_position() {
        fixture(|b, _| {
            let mut d = drag(&b, &["a"]);
            d.target = Vec3::new(0., 0.02, 0.1);
            d.validation = Some((d.target, d.work.validate(d.target)));
            assert!(d.current_validation().unwrap().1.as_ref().unwrap().allowed);
            d.target = Vec3::new(0.5, 0.02, 0.);
            assert!(d.current_validation().is_none());
            assert!(d.work.commit(d.target).is_err());
        });
    }
    #[test]
    fn failed_drop_clears_pending_preview_without_changing_the_document() {
        fixture(|mut b, s| {
            let before = b.document.clone();
            let mut d = drag(&b, &["a"]);
            d.committing = Some(crate::jobs::Job::finished(0, d.work.commit(Vec3::new(0.5, 0.02, 0.))));
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(false));
            assert_eq!(b.document, before);
            assert!(b.action_error.as_ref().unwrap().contains("overlap"));
        });
    }
    #[test]
    fn actual_bevy_drag_start_and_end_observers_keep_selected_group_identity() {
        fixture(|mut b, s| {
            use bevy::picking::{
                backend::HitData,
                pointer::{Location, PointerId},
            };
            use bevy::camera::{ManualTextureViewHandle, NormalizedRenderTarget};
            b.selected = BTreeSet::from(["a".into(), "assembly".into()]);
            let index = s
                .spatial
                .parts
                .iter()
                .position(|p| p.component == "assembly/left")
                .unwrap();
            let mut app = App::new();
            app.insert_resource(b)
                .insert_resource(s)
                .add_observer(start_part)
                .add_observer(end_drag);
            let entity = app.world_mut().spawn(Part { index }).id();
            let location = Location {
                target: NormalizedRenderTarget::TextureView(ManualTextureViewHandle(0)),
                position: Vec2::ZERO,
            };
            app.world_mut().trigger(
                Pointer::new(
                    PointerId::Mouse,
                    location.clone(),
                    DragStart {
                        button: PointerButton::Primary,
                        hit: HitData::new(
                            Entity::PLACEHOLDER,
                            1.,
                            Some(Vec3::new(0.2, 0.02, 0.)),
                            None,
                        ),
                    },
                    entity,
                ),
            );
            assert_eq!(
                app.world()
                    .resource::<Builder>()
                    .drag
                    .as_ref()
                    .unwrap()
                    .names,
                vec!["a", "assembly"]
            );
            app.world_mut().trigger(
                Pointer::new(
                    PointerId::Mouse,
                    location,
                    DragEnd {
                        button: PointerButton::Primary,
                        distance: Vec2::X * 10.,
                    },
                    entity,
                ),
            );
            assert!(
                app.world()
                    .resource::<Builder>()
                    .drag
                    .as_ref()
                    .unwrap()
                    .released
            );
        });
    }
}
