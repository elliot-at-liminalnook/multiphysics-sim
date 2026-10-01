//! Scene sliders and chart hover: inputs that preview locally and send
//! lesson actions when let go or clicked.
use super::*;

/// A slider's track (a kit slider over the parameter's fraction of its
/// range; the reader drags along it).
#[derive(Component)]
pub(crate) struct SliderTrack(pub String);
#[derive(Component)]
pub(crate) struct SliderFill(pub String);
#[derive(Component)]
pub(crate) struct SliderValue(pub String);

impl ActiveScene {
    /// A slider's value now: while dragging, the drag; else the reader's
    /// override; else the compiled system's value.
    pub(crate) fn slider_value(&self, scene: &SpatialScene, parameter: &str) -> Option<f64> {
        if let Some((p, v)) = &self.slider_drag {
            if p == parameter {
                return Some(*v);
            }
        }
        if let Some(v) = self.overrides.get(parameter) {
            return Some(*v);
        }
        if let Some(v) = self.scene.set.get(parameter) {
            return Some(*v);
        }
        let (at, name, key) = sim_script::presentation::split_parameter(parameter).ok()?;
        let path = if at.is_empty() { name } else { format!("{at}/{name}") };
        scene.description.components.get(&path)?.parameters.get(&key).map(|p| p.value)
    }
}

/// Input: dragging a slider moves its previewed value (`slider_drag`, kept
/// here); letting go sends a `Slider` action, which sets it and re-records.
pub(super) fn sliders(tracks: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction, &SliderTrack)>, mut learn: ResMut<Learn>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    if !learn.active {
        return;
    }
    let Some(a) = learn.scene.as_ref() else { return };
    let mut dragging = None;
    for (value, pressed, interaction, track) in &tracks {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == track.0) else { continue };
        // The kit slider's value is the pointer's fraction of the track, 0..=1.
        dragging = Some((track.0.clone(), spec.snap(spec.min + value.0.clamp(0., 1.) as f64 * (spec.max - spec.min))));
    }
    match (dragging, a.slider_drag.clone()) {
        (Some(d), previous) => {
            if previous.as_ref() != Some(&d) {
                learn.bypass_change_detection().scene.as_mut().unwrap().slider_drag = Some(d);
            }
        }
        (None, Some((parameter, value))) => {
            out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::Slider { parameter, value })));
        }
        (None, None) => {}
    }
}

/// Slider fills and values follow the drag and the reader's overrides.
pub(super) fn slider_live(learn: Res<Learn>, scene: Res<SpatialScene>, mut fills: Query<(&mut Node, &SliderFill)>, mut values: Query<(&mut Text, &SliderValue)>) {
    let Some(a) = learn.scene.as_ref() else { return };
    for (mut node, fill) in &mut fills {
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == fill.0) else { continue };
        let v = a.slider_value(&scene, &fill.0).unwrap_or(spec.min);
        let w = Val::Percent((((v - spec.min) / (spec.max - spec.min).max(1e-12)).clamp(0., 1.) * 100.) as f32);
        if node.width != w {
            node.width = w;
        }
    }
    for (mut text, value) in &mut values {
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == value.0) else { continue };
        let shown = a.slider_value(&scene, &value.0).map(|v| format!("{} {}", crate::builder::ui::num(v), spec.unit)).unwrap_or_default();
        if text.0 != shown {
            text.0 = shown;
        }
    }
}

/// A time chart that previews its moment on hover and lights with its part.
#[derive(Component)]
pub(crate) struct ChartHover(pub String, pub f64, pub f64);

/// Hovering a chart shows that moment in the scene (and the part it
/// measures); clicking keeps it. Hovering a part lights up its charts.
/// The hover preview is local (restored when the pointer leaves); a click
/// keeps the moment through the lesson handler (`KeepMoment`), once per
/// press, with the moment held when the press ends (a drag keeps where it ends).
#[allow(clippy::too_many_arguments)]
pub(super) fn chart_hover(mut charts: Query<(&Interaction, &bevy::ui::RelativeCursorPosition, &ChartHover, &mut BorderColor)>, mut learn: ResMut<Learn>, pointed: Res<crate::view::PartHover>, mut preview: Local<Option<f64>>, mut lit: Local<Option<String>>, mut held: Local<Option<f64>>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    if !learn.active {
        *held = None;
        return;
    }
    let mut hovered = None;
    let mut clicked = false;
    for (interaction, cursor, chart, _) in &charts {
        if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            if let Some(p) = crate::ui_kit::surface_point(cursor) {
                hovered = Some((chart.0.clone(), chart.1 + p.x.clamp(0., 1.) as f64 * (chart.2 - chart.1)));
                clicked |= *interaction == Interaction::Pressed;
            }
        }
    }
    let part_of = |key: &str| key.split('.').next().unwrap_or(key).to_string();
    // The press ended: keep its last moment through the handler (once per press).
    if !clicked {
        if let Some(t) = held.take() {
            out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::KeepMoment(t))));
        }
    }
    match (&hovered, learn.scene.as_ref().is_some_and(|a| a.run.is_some() && !a.playing)) {
        (Some((key, t)), true) => {
            let a = learn.bypass_change_detection().scene.as_mut().unwrap();
            if preview.is_none() {
                *preview = Some(a.time);
            }
            if (a.time - t).abs() > 1e-9 {
                a.seek(*t);
            }
            if clicked {
                // Kept while held: nothing to restore on leaving.
                *preview = None;
                *held = Some(*t);
            }
            let part = part_of(key);
            if learn.hover_part.as_deref() != Some(part.as_str()) {
                learn.hover_part = Some(part.clone());
                *lit = Some(part);
            }
        }
        _ => {
            if let Some(back) = preview.take() {
                if let Some(a) = learn.bypass_change_detection().scene.as_mut() {
                    a.seek(back);
                }
            }
            if lit.take().is_some() && hovered.is_none() {
                learn.hover_part = None;
            }
        }
    }
    // Charts of the hovered part (a link, a chart, the narration) get an outline.
    let part = learn.hover_part.clone().or_else(|| pointed.0.clone()).or_else(|| learn.narration_part.clone());
    for (_, _, chart, mut border) in &mut charts {
        let on = part.as_deref().is_some_and(|p| part_of(&chart.0) == p || chart.0.starts_with(&format!("{p}/")));
        let color = if on { crate::ui_kit::ACCENT } else { Color::NONE };
        if border.top != color {
            *border = BorderColor::all(color);
        }
    }
}
