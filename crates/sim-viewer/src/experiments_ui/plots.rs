use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use sim_runtime::{
    experiment_comparison::{Limits, hx_archive::Trial},
    experiment_study::TrialResult,
};
const MEASURED: Color32 = Color32::from_rgb(20, 115, 156);
const EMPIRICAL: Color32 = Color32::from_rgb(175, 127, 38);
const BASELINE: Color32 = Color32::from_rgb(123, 78, 181);
const CANDIDATE: Color32 = Color32::from_rgb(22, 138, 82);
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlotState {
    cursor: Option<f64>,
    zoom: f64,
    center: f64,
    measured: bool,
    empirical: bool,
    baseline: bool,
    candidate: bool,
}
impl Default for PlotState {
    fn default() -> Self {
        Self {
            cursor: None,
            zoom: 1.,
            center: 0.5,
            measured: true,
            empirical: true,
            baseline: true,
            candidate: true,
        }
    }
}
impl PlotState {
    pub(super) fn validate(&self) -> Result<(), String> {
        if !self.zoom.is_finite()
            || self.zoom <= 0.
            || !self.center.is_finite()
            || self.cursor.is_some_and(|v| !v.is_finite())
        {
            return Err("invalid plot zoom or time".into());
        }
        Ok(())
    }
    pub fn reset_time(&mut self) {
        self.cursor = None;
        self.zoom = 1.;
        self.center = 0.5;
    }
}
struct Series {
    color: Color32,
    points: Vec<(f64, f64)>,
    dots: bool,
}
pub fn show(
    ui: &mut egui::Ui,
    state: &mut PlotState,
    t: &Trial,
    r: Option<&TrialResult>,
    limits: &Limits,
) {
    ui.horizontal_wrapped(|ui| {
        for (value, label, color) in [
            (&mut state.measured, "Measured", MEASURED),
            (&mut state.empirical, "Reference", EMPIRICAL),
            (&mut state.baseline, "Baseline", BASELINE),
            (&mut state.candidate, "Candidate", CANDIDATE),
        ] {
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(color);
                ui.checkbox(value, label);
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label("Time zoom");
        ui.add(
            egui::Slider::new(&mut state.zoom, 1. ..=20.)
                .logarithmic(true)
                .show_value(false),
        );
        ui.label("Position");
        ui.add(egui::Slider::new(&mut state.center, 0. ..=1.).show_value(false));
        if ui.button("Fit").clicked() {
            state.reset_time();
        }
    });
    let end = t
        .measured
        .samples
        .last()
        .unwrap()
        .completion_s
        .max(t.duration_s)
        * 1.02;
    let start = (state.center * end - end / (2. * state.zoom)).clamp(0., end - end / state.zoom);
    let finish = start + end / state.zoom;
    let command = Series {
        color: Color32::from_rgb(63, 80, 108),
        points: vec![
            (0., t.drive),
            (t.duration_s, t.drive),
            (t.duration_s, 0.),
            (end, 0.),
        ],
        dots: false,
    };
    draw(
        ui,
        state,
        "Reconstructed command (duty fraction)",
        &[command],
        t,
        start,
        finish,
        75.,
        false,
        None,
    );
    let mut signals = vec![];
    let mut residuals = vec![];
    if state.measured {
        signals.push(Series {
            color: MEASURED,
            points: t
                .measured
                .samples
                .iter()
                .map(|s| (s.time_s, s.value))
                .collect(),
            dots: true,
        });
    }
    for (enabled, color, trace) in [
        (state.empirical, EMPIRICAL, Some(&t.predicted)),
        (
            state.baseline,
            BASELINE,
            r.and_then(|r| r.baseline.as_ref()).map(|p| &p.trace),
        ),
        (
            state.candidate,
            CANDIDATE,
            r.and_then(|r| r.candidate.as_ref()).map(|p| &p.trace),
        ),
    ] {
        if enabled {
            if let Some(trace) = trace {
                signals.push(Series {
                    color,
                    points: trace.samples.iter().map(|s| (s.time_s, s.value)).collect(),
                    dots: false,
                });
                residuals.push(Series {
                    color,
                    points: trace
                        .samples
                        .iter()
                        .zip(&t.measured.samples)
                        .map(|(p, m)| (p.time_s, p.value - m.value))
                        .collect(),
                    dots: false,
                });
            }
        }
    }
    draw(
        ui,
        state,
        "Encoder displacement (rad)",
        &signals,
        t,
        start,
        finish,
        185.,
        true,
        None,
    );
    draw(
        ui,
        state,
        "Residual: prediction − measurement (rad)",
        &residuals,
        t,
        start,
        finish,
        125.,
        false,
        Some(limits.final_abs_error),
    );
    ui.small("x: seconds since command midpoint. Grey bands mark gaps >2.5× median sample spacing; dots are observations. Horizontal whiskers are host acquisition windows, not sensor sample age.");
    ui.small("Residual guide lines show the final-error limit; only the final sample is gated by that limit. RMSE is a separate whole-trial gate.");
    if let Some(cursor) = state.cursor {
        if let Some((i, m)) = t
            .measured
            .samples
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.time_s - cursor)
                    .abs()
                    .total_cmp(&(b.time_s - cursor).abs())
            })
        {
            ui.small(format!(
                "t {:.6} s · measured {:.6} rad · host {:.6}…{:.6} s",
                m.time_s, m.value, m.request_s, m.completion_s
            ));
            for (name, p) in [
                ("Baseline", r.and_then(|r| r.baseline.as_ref())),
                ("Candidate", r.and_then(|r| r.candidate.as_ref())),
            ] {
                if let Some(p) = p {
                    ui.small(format!(
                        "{name}: {:.6} rad · residual {:+.6} rad",
                        p.trace.samples[i].value,
                        p.trace.samples[i].value - m.value
                    ));
                }
            }
        }
    }
    egui::Grid::new("trial-metrics")
        .striped(true)
        .show(ui, |ui| {
            for label in [
                "Source",
                "RMSE rad",
                "Max |error| rad",
                "Final |error| rad",
                "Pair outcome / archived outcome",
            ] {
                ui.strong(label);
            }
            ui.end_row();
            for (name, m) in [
                ("Reference", Some(&t.comparison)),
                (
                    "Baseline",
                    r.and_then(|r| r.baseline.as_ref()).map(|p| &p.metrics),
                ),
                (
                    "Candidate",
                    r.and_then(|r| r.candidate.as_ref()).map(|p| &p.metrics),
                ),
            ] {
                ui.label(name);
                if let Some(m) = m {
                    ui.label(format!("{:.5}", m.rmse));
                    ui.label(format!("{:.5}", m.maximum_abs_error));
                    ui.label(format!("{:.5}", m.final_error.abs()));
                    // These physical metrics are inspectable even when the pair
                    // is incomplete; its outcome always comes from the runtime.
                    let outcome=if name=="Reference" {if m.passes {sim_runtime::experiment_study::TrialOutcome::Pass}else{sim_runtime::experiment_study::TrialOutcome::Fail}}else{r.map(|r|r.outcome()).unwrap_or(sim_runtime::experiment_study::TrialOutcome::Unscored)};
                    ui.colored_label(
                        match outcome {sim_runtime::experiment_study::TrialOutcome::Pass=>Color32::DARK_GREEN,sim_runtime::experiment_study::TrialOutcome::Fail=>Color32::DARK_RED,sim_runtime::experiment_study::TrialOutcome::Unscored=>Color32::GRAY},
                        format!("{} {}",if name=="Reference" {"archived"}else{"pair"},outcome.label()),
                    );
                } else {
                    ui.label("—");
                    ui.label("—");
                    ui.label("—");
                    ui.label(format!("pair {}",sim_runtime::experiment_study::TrialOutcome::Unscored.label()));
                }
                ui.end_row();
            }
        });
    ui.small(format!("Physical limits: RMSE ≤ {:.6} rad, final |error| ≤ {:.6} rad. Reference uses original {:.6} / {:.6} rad.",limits.rmse,limits.final_abs_error,t.limits.rmse,t.limits.final_abs_error));
    if let Some(r) = r {
        for error in &r.errors {
            ui.colored_label(Color32::DARK_RED, error);
        }
    }
}
fn draw(
    ui: &mut egui::Ui,
    state: &mut PlotState,
    title: &str,
    series: &[Series],
    trial: &Trial,
    xmin: f64,
    xmax: f64,
    height: f32,
    windows: bool,
    limit: Option<f64>,
) {
    ui.label(title);
    let (response, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(200.), height),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    let area = rect.shrink2(Vec2::new(45., 16.));
    let mut low = 0f64;
    let mut high = 0f64;
    for s in series {
        for &(_, y) in &s.points {
            low = low.min(y);
            high = high.max(y);
        }
    }
    if let Some(l) = limit {
        low = low.min(-l);
        high = high.max(l);
    }
    let pad = ((high - low) * 0.08).max(0.001);
    low -= pad;
    high += pad;
    let point = |x: f64, y: f64| {
        Pos2::new(
            area.left() + ((x - xmin) / (xmax - xmin)) as f32 * area.width(),
            area.bottom() - ((y - low) / (high - low)) as f32 * area.height(),
        )
    };
    painter.rect_filled(rect, 4., Color32::from_rgb(246, 248, 250));
    for i in 0..=2 {
        let y = low + (high - low) * i as f64 / 2.;
        painter.line_segment(
            [point(xmin, y), point(xmax, y)],
            Stroke::new(1., Color32::from_gray(221)),
        );
        painter.text(
            Pos2::new(area.left() - 3., point(xmin, y).y),
            egui::Align2::RIGHT_CENTER,
            format!("{y:.3}"),
            egui::FontId::proportional(10.),
            Color32::DARK_GRAY,
        );
    }
    for i in 0..=4 {
        let x = xmin + (xmax - xmin) * i as f64 / 4.;
        painter.text(
            Pos2::new(point(x, low).x, area.bottom() + 3.),
            egui::Align2::CENTER_TOP,
            format!("{x:.3}"),
            egui::FontId::proportional(10.),
            Color32::DARK_GRAY,
        );
    }
    let painter = painter.with_clip_rect(area);
    let mut gaps = trial
        .measured
        .samples
        .windows(2)
        .map(|w| w[1].time_s - w[0].time_s)
        .collect::<Vec<_>>();
    gaps.sort_by(f64::total_cmp);
    let spacing = gaps.get(gaps.len() / 2).copied().unwrap_or(f64::INFINITY);
    for w in trial.measured.samples.windows(2) {
        if w[1].time_s - w[0].time_s > spacing * 2.5 {
            painter.rect_filled(
                egui::Rect::from_min_max(point(w[0].time_s, high), point(w[1].time_s, low)),
                0.,
                Color32::from_gray(226),
            );
        }
    }
    if let Some(l) = limit {
        for y in [-l, l] {
            painter.line_segment(
                [point(xmin, y), point(xmax, y)],
                Stroke::new(1., Color32::from_rgb(195, 130, 130)),
            );
        }
    }
    for s in series {
        if s.dots {
            for &(t, y) in &s.points {
                painter.circle_filled(point(t, y), 2.3, s.color);
            }
        } else {
            painter.add(egui::Shape::line(
                s.points.iter().map(|&(x, y)| point(x, y)).collect(),
                Stroke::new(1.6, s.color),
            ));
        }
    }
    if windows && state.measured {
        for m in &trial.measured.samples {
            painter.line_segment(
                [point(m.request_s, m.value), point(m.completion_s, m.value)],
                Stroke::new(1., MEASURED.gamma_multiply(0.5)),
            );
        }
    }
    if let Some(pos) = response.hover_pos() {
        let time =
            xmin + ((pos.x - area.left()) / area.width()).clamp(0., 1.) as f64 * (xmax - xmin);
        if state.cursor != Some(time) {
            state.cursor = Some(time);
            ui.ctx().request_repaint();
        }
    }
    if let Some(time) = state.cursor {
        painter.line_segment(
            [point(time, low), point(time, high)],
            Stroke::new(1., Color32::GRAY),
        );
    }
}
