//! Scene sync (SimSync): the file watch, installing loaded models (physical
//! or planar), run frames onto the link meshes, the camera's bounds and
//! view area (the orbit itself is the shared `crate::camera`), selection
//! highlight and inspector scrolling.
use super::*;

/// UI thread, FILE mode: stats the opened file every `source::POLL`;
/// a changed stat writes the one Reload action (trigger watch), applied next frame.
pub(super) fn watch(mut view: ResMut<RobotView>, mut out: MessageWriter<crate::app::actions::Act<RobotAction>>) {
    let idle = view.load.is_none();
    let due = match view.source.as_mut() {
        Some(s) if idle => s.poll(std::time::Instant::now()),
        _ => false,
    };
    if due {
        // A refusal (a check already in flight) is retried by the next poll.
        out.write(crate::app::actions::Act::quiet(RobotAction::Reload { trigger: ReloadTrigger::Watch }));
    }
}

/// Takes the worker's result (a preset open, or a FILE open/reload from
/// `robot_source`); spawns meshes and the link list on a model to apply.
/// A failed or unchanged reload returns before anything is despawned.
pub(super) fn receive(
    mut commands: Commands,
    old: Query<Entity, Or<(With<LinkMesh>, With<LinkRow>)>>,
    mut view: ResMut<RobotView>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Materials>,
    root: Single<Entity, With<RobotRoot>>,
    list: Single<Entity, With<ListRoot>>,
    mut orbit: Single<&mut Orbit, With<RobotCamera>>,
    rules: Single<&crate::camera::OrbitRules, With<RobotCamera>>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
    fonts: Res<UiFonts>,
    mut selection: ResMut<Selection>,
    mut registry: ResMut<DocumentRegistry>,
) {
    let started = match view.status {
        Status::Loading(t) => t,
        _ => std::time::Instant::now(),
    };
    // `reload`: the trigger and the worker's seconds when a FILE reload replaces a displayed model.
    let (result, reload) = if let Some(load) = view.load.as_ref() {
        let Some(result) = load.poll() else {
            redraw.write(bevy::window::RequestRedraw);
            return;
        };
        view.load = None;
        (result, None)
    } else {
        let Some(source) = view.source.as_mut() else { return };
        let Some((trigger, mut checked)) = source.take() else {
            if source.busy().is_some() {
                redraw.write(bevy::window::RequestRedraw);
            }
            return;
        };
        let seconds = checked.seconds;
        let results = checked.results.take();
        let was_failing = source.failing.is_some();
        let settled = source.settle(trigger, checked, source::now_utc());
        if let Some(r) = results {
            // Read by the same worker; its status is always judged against the displayed model.
            view.stress.set(r);
        }
        let Some(source) = view.source.as_mut() else { return };
        let Some(loaded) = settled else {
            // Unchanged, or failed: the displayed model, meshes, run and selection stay.
            let failing = source.failing.clone();
            match (failing, view.model.is_some() || view.planar.is_some()) {
                (Some(e), false) => view.status = Status::Error(e),
                (Some(_), true) => view.notice = Some(format!("{} reload failed: showing the last good model (see header)", if trigger == ReloadTrigger::Watch { "watched" } else { "manual" })),
                (None, _) if trigger == ReloadTrigger::Manual => view.notice = Some("manual reload: file unchanged (same sha256); nothing replaced".into()),
                (None, _) if was_failing => view.notice = Some("the file on disk matches the displayed model again (same sha256); nothing replaced".into()),
                // A watch that finds identical bytes (a touch, an atomic same-content rewrite) stays quiet.
                (None, _) => {}
            }
            return;
        };
        // The first successful load (open, or a watch after a failed open) is not a reload.
        let reload = (view.model.is_some() || view.planar.is_some()).then_some((trigger, seconds));
        match loaded {
            FileModel::Physical(loaded) => (Ok((*loaded, None)), reload),
            FileModel::Planar(loaded) => {
                let k = Kit { f: &fonts };
                install_planar(&mut commands, &old, &mut view, *list, &k, *loaded, reload, started, (&mut *selection, &mut *registry));
                return;
            }
        }
    };
    // A reopened robot (REST robot_preset) or a reloaded file replaces the previous meshes and rows.
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let (mut loaded, preset) = match result {
        Ok(l) => l,
        Err(e) => {
            view.status = Status::Error(e);
            return;
        }
    };
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let to_display = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
    for (i, (link, geometry)) in loaded.model.links.iter().zip(&loaded.geometry).enumerate() {
        let com = Vec3::new(link.com[0] as f32, link.com[1] as f32, link.com[2] as f32);
        let Some(g) = geometry else { continue };
        for p in &g.positions {
            let w = to_display(com + Vec3::from_array(*p));
            lo = lo.min(w);
            hi = hi.max(w);
        }
        let count = g.positions.len() as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, g.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals.clone());
        mesh.insert_indices(Indices::U32((0..count).collect()));
        let entity = commands
            .spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.normal.clone()), Transform::from_translation(com), Visibility::default(), LinkMesh(i), Pickable::default()))
            .observe(actions::pick_link)
            .id();
        commands.entity(*root).add_child(entity);
    }
    view.bounds = lo.x.is_finite().then_some((lo, hi));
    if lo.x.is_finite() {
        // The bounds a fit frames; the focus moves only on an open (`home`).
        orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
        orbit.centre = (lo + hi) / 2.0;
        // A reload keeps the distance, within the new model's zoom limits.
        orbit.clamp_radius(&rules);
    } else if reload.is_none() {
        // No geometry to frame: an open keeps the focus where it was, as before.
        orbit.centre = orbit.focus;
    }
    // A reload keeps the user's camera; an open frames the bounds at
    // 3.2 × extent from the current heading (`camera::place`).
    orbit.home = reload.is_none();
    view.triangles = loaded.geometry.iter().map(|g| g.as_ref().map_or(0, |g| g.triangles())).collect();
    // New meshes are painted (or not) for the stress overlay by `stress_paint`.
    view.stress.revision += 1;
    let k = Kit { f: &fonts };
    let rows: Vec<Entity> = loaded
        .model
        .links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let name = if view.triangles[i] > 0 { l.name.clone() } else { format!("{}  (no collision geometry)", l.name) };
            // A one-line selectable row; `highlight` sets its `Tint` from the selection
            // (found again by name below; until then it indexes the previous model).
            commands
                .spawn((
                    Button, crate::ui_kit::activation::Ordinary,
                    RobotAction::SelectLink { index: i, name: l.name.clone() },
                    LinkRow(i),
                    Tint::selectable(false),
                    AccessibleLabel::new(name.as_str()),
                    Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), flex_shrink: 0.0, ..default() },
                    BackgroundColor(Color::NONE),
                    children![k.text(name.as_str(), size::ITEM, TEXT, 0)],
                ))
                .id()
        })
        .collect();
    commands.entity(*list).add_children(&rows);
    // A file's previous run context is discarded (its thread stops) and the
    // fresh one continues its generation; a preset opens a new view.
    // A reload that turns a planar (v2) file into a physical one: the planar run
    // is joined off the UI thread; its speed and contacts choice carry over.
    let planar = view.planar.take().map(|p| (p.run.speed_scale(), p.contacts, p.run.frame().is_some_and(|f| f.steps > 0) || p.run.phase() == planar::PlanarPhase::Running, p));
    let previous = view.run.take();
    // A FILE's controller binding, loaded by the reload worker (`Loaded::controlled`):
    // none runs the hold controller, a loaded one the drive session, a failed one a failed run naming it.
    let controlled = loaded.controlled.take();
    // A robot project's model runs the project's system (`Loaded::composed`).
    let composed = loaded.composed.take();
    let (mut run, mut run_reset) = match preset {
        Some(Opened::Preset(run)) => {
            let run = RunController::spawn_preset(std::sync::Arc::new(run));
            // Built at open, as the browser worker builds at load: its inputs and t = 0 frame show at once.
            run.prepare();
            (run, false)
        }
        Some(Opened::Recorded(run)) => (RunController::spawn_recorded(std::sync::Arc::new(run)), false),
        // Replacing a planar run: the physical run continues its generation (older frames stay stale).
        None => match (planar.as_ref(), previous) {
            (Some((_, _, _, p)), None) => (RunController::spawn_file(loaded.model.clone(), controlled, composed, p.run.generation() + 1), false),
            (_, previous) => RunController::replace_file(previous, loaded.model.clone(), controlled, composed),
        },
    };
    if let Some((speed, contacts, had_run, replaced)) = planar {
        crate::jobs::drop_off_thread(replaced, "the planar v2 run");
        run_reset |= had_run;
        if speed != run.speed_scale() {
            let _ = run.speed(SpeedRequest::Set { scale: speed });
        }
        let flags = run.overlays();
        if flags.contacts != contacts {
            let _ = run.set_overlays(OverlayFlags { contacts, ..flags });
        }
    }
    let generation = run.generation();
    view.run = Some(run);
    // A tested recipe's Load and run / Replay tested inputs (`leaderboard::AfterOpen`).
    if let (Some(after), Some(run)) = (view.after_open.take(), view.run.as_mut()) {
        let result = match after {
            leaderboard::AfterOpen::Run { initial } => {
                if let Some(values) = initial {
                    run.apply_action(values);
                }
                run.act(RunAction::Start)
            }
            leaderboard::AfterOpen::Replay { path } => run.replay(None, path.to_str()).map(|_| ()),
        };
        if let Err(e) = result {
            view.run_message = Some(e);
        }
    }
    // The selected link is kept by name across a reload (`picked::reloaded`).
    let kept = picked::reloaded(&mut selection, &mut registry, reload.is_some(), |n| loaded.model.links.iter().position(|l| l.name == n));
    view.model = Some(loaded.model);
    view.notes = loaded.notes;
    view.cad_link = Some(loaded.cad_link);
    view.pose_dirty = true;
    view.run_message = None;
    view.status = Status::Loaded { seconds: match reload {
        Some((_, s)) => s,
        None => started.elapsed().as_secs_f64(),
    } };
    if let Some((trigger, _)) = reload {
        let reason = if trigger == ReloadTrigger::Watch { "file changed on disk" } else { "manual reload" };
        let run_text = if run_reset { "run reset" } else { "no run to reset" };
        let note = match &kept {
            Some((n, true)) => format!("; selection kept: {n}"),
            Some((n, false)) => format!("; selection cleared: link `{n}` is not in the new file"),
            None => String::new(),
        };
        // Which controller the new run context uses (the binding is re-read on every reload).
        let controller = match view.run.as_ref() {
            Some(r) if r.controlled().is_some() => format!("; controller: {} (binding re-read)", run::CONTROLLER_LABEL),
            Some(r) if r.binding_error().is_some() => "; controller binding failed to load: the run is failed (see the header)".to_string(),
            _ => String::new(),
        };
        view.notice = Some(format!("reloaded: {reason}; {run_text}; generation {generation}{note}{controller}"));
        if let Some(s) = view.source.as_mut() {
            s.run_reset = Some(run_reset);
        }
    }
    view.ui_revision += 1;
    view.panels_ready = true;
}

/// Installs a planar (v2) file from `receive` (`robot_source`'s loaded
/// outcome): the previous meshes and rows go, a previous physical run (or
/// planar run) is joined off the UI thread, the v3 state is cleared and a
/// planar run thread starts (it builds at once and waits paused at t = 0).
/// The selection is kept by body/link name; speed and contacts carry over.
#[allow(clippy::too_many_arguments)]
fn install_planar(
    commands: &mut Commands,
    old: &Query<Entity, Or<(With<LinkMesh>, With<LinkRow>)>>,
    view: &mut RobotView,
    list: Entity,
    k: &Kit<'_>,
    loaded: planar::PlanarLoaded,
    reload: Option<(ReloadTrigger, f64)>,
    started: std::time::Instant,
    (selection, registry): (&mut Selection, &mut DocumentRegistry),
) {
    for entity in old {
        commands.entity(entity).despawn();
    }
    let (mut speed, mut contacts, mut generation, mut run_reset, mut joint) = (1.0, true, 0, false, (0, None));
    // A planar file that was running keeps running after a reload (the CAD
    // scene's edit, save, watch loop): the new run is started once built.
    let mut resume = false;
    if let Some(run) = view.run.take() {
        (speed, contacts, run_reset) = (run.speed_scale(), run.overlays().contacts, run.has_run_state());
        generation = run.generation() + 1;
        crate::jobs::drop_off_thread(run, "the robot run (replaced by a planar v2 file)");
    }
    if let Some(p) = view.planar.take() {
        (speed, contacts, joint) = (p.run.speed_scale(), p.contacts, (p.selected_joint, p.selected_joint_name().map(str::to_string)));
        run_reset = p.run.frame().is_some_and(|f| f.steps > 0) || p.run.phase() == planar::PlanarPhase::Running;
        resume = p.run.phase() == planar::PlanarPhase::Running;
        generation = p.run.generation() + 1;
        crate::jobs::drop_off_thread(p, "the planar v2 run");
    }
    // The v3 state: nothing of it describes a planar file.
    view.model = None;
    view.triangles.clear();
    view.notes = FileNotes::default();
    view.cad_link = None;
    view.mirror = None;
    view.graphs_visible = false;
    if view.stress.enabled {
        view.stress.enabled = false;
        view.stress.revision += 1;
    }
    let rows: Vec<Entity> = loaded
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let name = if b.ground { format!("{}  (ground: fixed root)", b.name) } else { b.name.clone() };
            commands
                .spawn((
                    Button, crate::ui_kit::activation::Ordinary,
                    RobotAction::SelectLink { index: i, name: b.name.clone() },
                    LinkRow(i),
                    Tint::selectable(false),
                    AccessibleLabel::new(name.as_str()),
                    Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), flex_shrink: 0.0, ..default() },
                    BackgroundColor(Color::NONE),
                    children![k.text(name.as_str(), size::ITEM, TEXT, 0)],
                ))
                .id()
        })
        .collect();
    commands.entity(list).add_children(&rows);
    // The selected link or body is kept by name across a reload (`picked::reloaded`).
    let kept = picked::reloaded(selection, registry, reload.is_some(), |n| loaded.model.bodies.iter().position(|b| b.name == n));
    let mut planar = PlanarView::new(loaded, generation, speed, contacts, reload.is_none());
    // Carried by name: the new file's joint order may differ (resolved in planar_sync once built).
    (planar.selected_joint, planar.pending_joint) = joint;
    // Queued behind the build on the run thread, so it starts once built (a failed build ignores it).
    if resume {
        let _ = planar.run.act(RunAction::Start);
    }
    view.planar = Some(planar);
    view.run_message = None;
    view.pose_dirty = false;
    view.status = Status::Loaded { seconds: reload.map_or_else(|| started.elapsed().as_secs_f64(), |(_, s)| s) };
    if let Some((trigger, _)) = reload {
        let reason = if trigger == ReloadTrigger::Watch { "file changed on disk" } else { "manual reload" };
        let run = match (run_reset, resume) {
            (_, true) => "run reset; it runs again from t = 0 once the new build is ready",
            (true, false) => "run reset",
            (false, false) => "no run to reset",
        };
        let note = match &kept {
            Some((n, true)) => format!("; selection kept: {n}"),
            Some((n, false)) => format!("; selection cleared: `{n}` is not a body of the new file"),
            None => String::new(),
        };
        view.notice = Some(format!("reloaded ({}): {reason}; {run}; generation {generation}{note}", planar::FORMAT_NAME));
        if let Some(s) = view.source.as_mut() {
            s.run_reset = Some(run_reset);
        }
    }
    view.ui_revision += 1;
    view.panels_ready = true;
}

/// SimSync, a planar (v2) file: takes the planar run thread's latest frame
/// (never one of an older generation), keeps the window redrawing while
/// frames are expected, and frames the camera on the first built frame of an
/// open (front view of the working plane) or a reload (extent only).
pub(super) fn planar_sync(
    mut view: ResMut<RobotView>,
    mut orbit: Single<&mut Orbit, With<RobotCamera>>,
    rules: Single<&crate::camera::OrbitRules, With<RobotCamera>>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    // Checked through a shared borrow first: a physical view is not marked changed.
    if view.planar.is_none() {
        return;
    }
    let Some(p) = view.planar.as_mut() else { return };
    p.run.poll();
    if p.run.active() {
        redraw.write(bevy::window::RequestRedraw);
    }
    // Every frame: also clamps a carried index when the old run had no built frame to name it.
    p.resolve_pending_joint();
    let Some(move_focus) = p.frame_camera else { return };
    let Some((lo, hi)) = p.run.frame().filter(|f| f.built).and_then(planar::bounds) else { return };
    p.frame_camera = None;
    orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
    orbit.centre = (lo + hi) / 2.0;
    orbit.clamp_radius(&rules);
    if move_focus {
        // Looking along −Z: x right, y up, as the plane is drawn; `home`
        // moves the focus to the centre at 3.2 × extent (`camera::place`).
        orbit.interrupt();
        orbit.trackball = None;
        orbit.yaw = 0.0;
        orbit.pitch = 0.12;
        orbit.home = true;
    }
}

/// Takes the run thread's latest frame (stale generations are discarded in
/// `RunController::poll`) and poses the link meshes from it; with no frame of
/// the current generation the static assembly pose is shown.
pub(super) fn apply_frames(mut view: ResMut<RobotView>, mut links: Query<(&LinkMesh, &mut Transform)>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    // The pinned picks follow the view into whichever run it holds.
    if view.run.as_ref().is_some_and(|r| r.picks() != view.picks.as_slice()) {
        let picks = view.picks.clone();
        if let Some(run) = view.run.as_mut() {
            run.set_picks(picks);
        }
    }
    let Some(run) = view.run.as_mut() else { return };
    let changed = run.poll();
    let active = run.active();
    if active {
        redraw.write(bevy::window::RequestRedraw);
    }
    if !changed && !view.pose_dirty {
        return;
    }
    view.pose_dirty = false;
    // The leg mirror's pose, else a loaded gait preview's, else the run's latest accepted frame.
    let poses = match view.mirror.as_ref() {
        Some(m) => Some(m.poses.as_slice()),
        None => view.run.as_ref().and_then(|r| r.display_poses()),
    };
    let Some(model) = view.model.as_ref() else { return };
    for (link, mut transform) in &mut links {
        let (p, q) = match poses.and_then(|f| f.get(link.0)).and_then(|p| p.as_ref()) {
            Some((p, q)) => (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)),
            None => {
                let c = model.links[link.0].com;
                (Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32), Quat::IDENTITY)
            }
        };
        if transform.translation != p || transform.rotation != q {
            transform.translation = p;
            transform.rotation = q;
        }
    }
}

/// SimSync, before `CameraSet::Viewport`: the 3D view draws between the
/// list, the inspector and the header, above the graph dock when it is
/// shown and the switcher strip (the shared camera falls back to the whole
/// window when they leave no room).
pub(super) fn view_area(view: Res<RobotView>, mut area: Single<&mut ViewArea, With<RobotCamera>>) {
    area.set_if_neq(wanted_area(view.graphs_visible));
}

/// The view area for the graph dock shown or not: the docks end above the
/// switcher strip (`ui_kit::SWITCHER_STRIP`), so the viewport does too.
pub(super) fn wanted_area(graphs_visible: bool) -> ViewArea {
    ViewArea::Docks { left: LEFT, right: RIGHT, top: TOP, bottom: crate::ui_kit::SWITCHER_STRIP + if graphs_visible { DOCK } else { 0.0 } }
}

/// The one selection, shown in 3D and in the list (a selectable `Tint`);
/// the current section's tab and the run buttons' enabled state (their
/// `Look`s are painted by `ui_kit::repaint_buttons`).
pub(super) fn highlight(
    view: Res<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    materials: Res<Materials>,
    mut meshes: Query<(&LinkMesh, &mut MeshMaterial3d<StandardMaterial>)>,
    mut rows: Query<(&LinkRow, &mut Tint)>,
    mut tabs: Query<(&TabButton, &mut Look)>,
    mut runs: Query<(&RunButton, &mut Enabled)>,
) {
    for (button, enabled) in &mut runs {
        // Robot mode's one `check` (as a click is judged): the run's own
        // check plus the mirroring refusal (`hardware::mirror::refuse_run`),
        // so Run, Step and Reset are dimmed while the mirror is shown.
        let ok = check(&view, &RobotAction::Run { action: button.0 }).is_ok();
        enable(enabled, ok);
    }
    for (tab, mut look) in &mut tabs {
        look.set_if_neq(Look::Tab(view.section == tab.0));
    }
    let selected = picked::link(&selection, &registry);
    for (link, mut material) in &mut meshes {
        let mirrored = view.mirror.as_ref().is_some_and(|m| m.tinted.contains(&link.0));
        let want = match (selected == Some(link.0), view.stress.painting()) {
            (true, false) => &materials.selected,
            (false, _) if mirrored => &materials.mirrored,
            (false, false) => &materials.normal,
            (true, true) => &materials.stress_selected,
            (false, true) => &materials.stress,
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
    for (row, mut tint) in &mut rows {
        tint.set_if_neq(Tint::selectable(selected == Some(row.0)));
    }
}

/// A kit button's `Enabled` flag (dims it and drops its hover), written only on a change.
pub(super) fn enable(mut flag: Mut<Enabled>, on: bool) {
    if flag.0 != on {
        flag.0 = on;
    }
}

/// Wheel over the inspector, or a requested offset (reset on selection and
/// section changes); reports the laid-out offset and its maximum back to REST.
/// While the Leg calibration panel covers the inspector, the wheel is the panel's (`hardware::panel`).
pub(super) fn scroll(
    mut view: ResMut<RobotView>,
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window>,
    panel: Single<(&mut ScrollPosition, &ComputedNode), With<InspectorScroll>>,
    hardware: Option<Res<hardware::Hardware>>,
) {
    let (mut position, node) = panel.into_inner();
    let delta = wheel_delta(&mut wheel, 24.0);
    let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
    let covered = hardware.is_some_and(|h| h.open);
    if delta != 0.0 && !covered && window.cursor_position().is_some_and(|p| p.x >= window.width() - RIGHT && p.y > TOP && p.y < window.height() - crate::ui_kit::SWITCHER_STRIP) {
        view.scroll_to = Some((position.y - delta).clamp(0.0, max));
    }
    if let Some(y) = view.scroll_to.take() {
        position.y = y.clamp(0.0, max);
    }
    if view.scroll != position.y || view.scroll_max != max {
        view.scroll = position.y;
        view.scroll_max = max;
    }
}
