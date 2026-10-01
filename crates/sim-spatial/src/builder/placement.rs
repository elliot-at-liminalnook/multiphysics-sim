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

pub(super) fn draw_handles(b: Res<Builder>, selection: Res<Selection>, registry: Res<DocumentRegistry>, mut gizmos: Gizmos) {
    let Some(spec) = picked::names(&selection, &registry).first().and_then(|n| b.spec(n)) else {
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
    mut selection: ResMut<Selection>,
    mut registry: ResMut<DocumentRegistry>,
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
    // Dragging an unselected part selects it first (as a click would).
    let mut pick = Picked::new(&mut selection, &mut registry);
    if !pick.names().contains(&name) && pick.set([name.clone()]).is_err() {
        return;
    }
    let names: Vec<_> = pick.names().into_iter().collect();
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
    typing: crate::ui_kit::text::Typing,
    buttons: Res<ButtonInput<MouseButton>>,
    mut gizmos: Gizmos,
    mut selection: ResMut<Selection>,
    mut registry: ResMut<DocumentRegistry>,
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
    let selected = picked::names(&selection, &registry);
    if let Some(name) = selected.first().cloned() {
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
                                selected.iter().cloned().collect(),
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
    let saving = drag.committed_revision.is_none();
    if let Some(hold_preview) = poll_drop(&mut b, &scene, &mut drag) {
        // A palette drop just saved: the placed instance is selected.
        if let (true, Some(_), Some(name)) = (saving, drag.committed_revision, drag.work.new_name.clone()) {
            let mut pick = Picked::new(&mut selection, &mut registry);
            pick.sync(&b);
            let _ = pick.set([name]);
        }
        if hold_preview {
            b.drag = Some(drag);
        }
        return;
    }
    // A kit field with the keyboard has the drag's keys (Escape cancels the field, X/Y/Z are typed).
    let typing = typing.get();
    if (keys.just_pressed(KeyCode::Escape) && !typing)
        || drag.level != b.level
        || !drag.same_scene(&b.document)
    {
        b.status = "Display drag cancelled; source placement unchanged.".into();
        b.panel_dirty = true;
        return;
    }
    for (key, axis) in [(KeyCode::KeyX, 0), (KeyCode::KeyY, 1), (KeyCode::KeyZ, 2)] {
        if keys.just_pressed(key) && !typing {
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
                // `update` selects a palette drop's new instance.
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
mod tests;
