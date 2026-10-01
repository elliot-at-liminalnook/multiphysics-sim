//! The live scene: the timebar, the card viewport, playback with its
//! presentation cues, and camera framing.
use super::*;

/// The timeline bar (a kit slider over the run's fraction): press or drag to scrub.
#[derive(Component)]
pub(crate) struct Timebar;
/// Input: pressing or dragging the timebar seeks the live scene (a
/// `SeekTo` every frame it is held; the press that starts a drag may count
/// a rewind). Nothing is sent while the scene is still recording.
pub(super) fn seek(bars: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<Timebar>>, learn: Res<Learn>, mut pressing: Local<bool>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    let mut pressed = false;
    for (value, held, interaction) in &bars {
        if !crate::ui_kit::slider_held(held, interaction) {
            continue;
        }
        pressed = true;
        let Some(a) = learn.scene.as_ref().filter(|a| a.run.is_some()) else { continue };
        let time = (value.0.clamp(0.0, 1.0) as f64) * a.duration();
        out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::SeekTo { time: Some(time), rewind: !*pressing })));
    }
    *pressing = pressed;
}

/// Marks the node the live scene draws into.
#[derive(Component)]
pub(crate) struct SceneViewport(pub String);

pub(super) fn viewport(learn: Res<Learn>, mut scene: ResMut<SpatialScene>, nodes: Query<(&SceneViewport, &ComputedNode, &UiGlobalTransform, Option<&bevy::ui::CalculatedClip>)>, window: Single<&Window>) {
    let want = if !learn.active {
        None
    } else {
        let active = learn.scene.as_ref().filter(|a| a.installed).map(|a| a.id.as_str());
        let found = active.and_then(|id| nodes.iter().find(|(v, ..)| v.0 == id));
        Some(match found {
            Some((_, node, transform, clip)) => {
                let center = transform.translation;
                let full = Rect::from_center_size(center, node.size());
                let window_rect = Rect::new(0., 0., window.physical_width() as f32, window.physical_height() as f32);
                let mut visible = full.intersect(window_rect);
                if let Some(c) = clip {
                    visible = visible.intersect(c.clip);
                }
                crate::LearnView { full, visible: if visible.is_empty() { Rect::default() } else { visible } }
            }
            None => crate::LearnView::default(),
        })
    };
    if scene.learn_view != want {
        scene.learn_view = want;
    }
}

/// Advance playback, show the recorded frame and apply presentation cues.
pub(super) fn playback(time: Res<Time>, mut learn: ResMut<Learn>, mut scene: ResMut<SpatialScene>, mut orbit: Single<&mut Orbit>) {
    // A scene's display magnification applies only while it is on screen.
    let magnify = if learn.active { learn.scene.as_ref().map_or(1., |a| a.scene.magnify as f32) } else { 1. };
    if scene.motion_scale != magnify {
        scene.motion_scale = magnify;
    }
    if !learn.active {
        if scene.companion.is_some() {
            scene.companion = None;
        }
        return;
    }
    let hover = learn.hover_part.clone().or_else(|| learn.narration_part.clone());
    let picked = learn.picked.clone();
    let narrated = learn.narration.as_ref().filter(|n| n.playing).map(|n| n.view()).filter(|v| !v.is_default());
    // A met challenge is kept in progress, with the values that met it.
    if let Some((id, values)) = learn.scene.as_mut().and_then(|a| a.challenge_met.take().map(|v| (a.id.clone(), v))) {
        if let Some(slug) = learn.slug().map(str::to_string) {
            learn.progress.record(&slug, &format!("{id}:challenge"), true, &values, sim_lesson::progress::Mode::Learn, false, sim_lesson::progress::now());
            let path = learn.progress_path.clone();
            let _ = learn.progress.save(&path);
            learn.status = "Challenge met: saved with your values.".into();
        }
    }
    let Some(a) = learn.scene.as_mut() else { return };
    if !a.installed {
        return;
    }
    // Frame the camera once the scene is in place.
    // Not before the card has a real size: framing a half-laid-out view
    // (a sliver's aspect) puts the camera far too far away.
    let aspect = view_aspect(&scene);
    if a.framed && aspect.is_some_and(|x| (x / a.framed_aspect.max(1e-3) - 1.).abs() > 0.15) {
        a.framed = false;
    }
    if let Some(aspect) = aspect.filter(|_| !a.framed) {
        let spec = a.scene.camera.clone().unwrap_or_default();
        frame(&scene, &mut orbit, &spec);
        a.framed = true;
        a.framed_aspect = aspect;
        // Cues seen before the card was placed apply again, over this framing.
        a.camera_cue = None;
        a.zoom_cue = None;
    }
    if a.show_pending {
        a.show_pending = false;
        if let Some(layers) = &a.scene.show {
            scene.state.overlays = layers.iter().copied().collect();
        }
    }
    let Some(run) = a.run.clone() else { return };
    let duration = a.duration();
    if a.playing {
        // The plan's clock: holds and slow motion from the shared pacing rules.
        let before = a.wall;
        let end = a.plan.duration();
        let mut next = a.wall + time.delta_secs_f64() * a.user_speed;
        if let Some(p) = a.plan.pause_between(before, next) {
            next = p;
            a.playing = false;
        }
        if next >= end {
            next = end;
            a.playing = false;
        }
        if let Some(stop) = a.stop_at.filter(|s| a.plan.sim_at(next) >= *s) {
            next = a.plan.wall_at(stop.min(duration)).max(before);
            a.playing = false;
            a.stop_at = None;
        }
        a.wall = next;
        a.time = if next >= end { duration } else { a.plan.sim_at(next) };
    }
    let state = a.timeline.state_at(a.time);
    if let Some((at, camera)) = state.camera.as_ref().filter(|_| a.framed) {
        if a.camera_cue != Some(*at) {
            a.camera_cue = Some(*at);
            frame(&scene, &mut orbit, camera);
        }
    }
    // The companion run, posed at the same moment.
    let companion = match (&a.scene.companion, &a.companion) {
        (Some(spec), Some(run)) => Some(crate::view::CompanionView { label: spec.label.clone(), ghost: spec.mode == sim_lesson::CompanionMode::Ghost, frame: run.frame_interpolated(a.time.min(run.frames.last().map(|f| f.time).unwrap_or(0.))) }),
        _ => None,
    };
    if companion.as_ref().map(|c| (&c.label, c.ghost, c.frame.as_ref().map(|f| f.time))) != scene.companion.as_ref().map(|c| (&c.label, c.ghost, c.frame.as_ref().map(|f| f.time))) {
        scene.companion = companion;
    }
    // View directives: the narration's while it speaks, else the scene's.
    let mut view = narrated.unwrap_or_else(|| state.view.clone());
    // A script's highlight is emphasis you can see: the part gets an arrow
    // with its name, and the rest of the scene dims a little.
    let soft: Vec<String> = if hover.is_none() && picked.is_none() { state.highlight.clone() } else { Vec::new() };
    for p in &soft {
        if !view.pins.iter().any(|(q, _)| q == p) {
            let label = scene.description.components.get(p).map(|c| c.label.clone()).unwrap_or_else(|| p.rsplit('/').next().unwrap_or(p).to_string());
            view.pins.push((p.clone(), label));
        }
    }
    if scene.soft_focus != soft {
        scene.soft_focus = soft;
    }
    if let Some((at, sim_script::presentation::View::Zoom { focus, zoom, seconds })) = state.view.zoom.as_ref().filter(|_| a.framed) {
        if a.zoom_cue != Some(*at) {
            a.zoom_cue = Some(*at);
            let aspect = view_aspect(&scene).unwrap_or(1.6);
            let (yaw, pitch) = orbit.heading();
            let pose = crate::view::frame_pose(&scene, focus.as_deref(), *zoom as f32, yaw, pitch, aspect);
            orbit.glide_to(pose, *seconds as f32);
        }
    }
    if (view.orbit as f32 - orbit.spin).abs() > 1e-6 && (view.orbit != scene.directives.orbit) {
        orbit.spin = view.orbit as f32;
    }
    if scene.directives != view {
        scene.directives = view;
    }
    // Zoom to a clicked part; back to the scene's framing when cleared.
    if picked != a.zoomed_part {
        a.zoomed_part = picked.clone();
        match &picked {
            Some(p) => {
                let aspect = view_aspect(&scene).unwrap_or(1.6);
                let (yaw, pitch) = orbit.heading();
                let pose = crate::view::frame_pose(&scene, Some(p), 1.0, yaw, pitch, aspect);
                orbit.glide_to(pose, crate::view::GLIDE_S);
            }
            None => frame(&scene, &mut orbit, &a.scene.camera.clone().unwrap_or_default()),
        }
    }
    if let Some(frame) = run.frame_interpolated(a.time) {
        let changed = scene.live.snapshot.as_ref().is_none_or(|s| s.frame.as_ref().is_none_or(|f| f.time != frame.time || f.sequence != frame.sequence));
        if changed {
            let snapshot = sim_inspect::live::LiveSnapshot { version: 1, source_description_id: scene.description.id.clone(), description: None, status: None, frame: Some(frame), error: run.error.clone() };
            scene.live.snapshot = Some(Arc::new(snapshot));
        }
    }
    // Highlight: hovered link, else picked part, else the script's highlight.
    let wanted: Vec<String> = match (hover, picked) {
        (Some(h), _) => vec![h],
        (None, Some(p)) => vec![p],
        (None, None) => state.highlight.clone(),
    };
    if wanted != a.highlight {
        a.highlight = wanted.clone();
        let target = crate::builder::discussion::selection(&scene, &wanted);
        let _ = scene.set_selection(target);
    }
}

/// The live scene view's aspect ratio, once its card is laid out.
pub(crate) fn view_aspect(scene: &SpatialScene) -> Option<f32> {
    scene.learn_view.map(|v| v.full).filter(|r| r.width() >= 80. && r.height() >= 60.).map(|r| r.width() / r.height())
}

/// Frame a camera spec: an eased glide from wherever the view is now.
pub(super) fn frame(scene: &SpatialScene, orbit: &mut Orbit, spec: &CameraSpec) {
    let (yaw, pitch) = spec.preset.map(|p| p.angles()).unwrap_or((0.35, 0.60));
    let aspect = view_aspect(&scene).unwrap_or(1.6);
    let pose = crate::view::frame_pose(scene, spec.focus.as_deref(), spec.zoom.unwrap_or(1.0), spec.yaw.unwrap_or(yaw), spec.pitch.unwrap_or(pitch), aspect);
    orbit.glide_to(pose, crate::view::GLIDE_S);
}

/// Times worth jumping to in a scene: captions, parameter changes and pauses.
pub(crate) fn event_times(a: &ActiveScene) -> Vec<f64> {
    use sim_script::presentation::Action;
    let d = a.duration();
    let mut times: Vec<f64> = a.timeline.cues.iter().filter(|c| matches!(c.action, Action::Caption { .. } | Action::Set { .. } | Action::Pause) && c.at_s > 1e-9 && c.at_s < d).map(|c| c.at_s).collect();
    times.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    times
}
