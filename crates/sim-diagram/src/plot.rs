//! Reusable time-series widget for any egui host. No worker, selection, model,
//! or recording dependency. Pass points from live history, saved data, or a tool.
pub use sim_inspect::plot::PlotPoint;

/// Caller-owned samples. `None` breaks the curve; values are never interpolated.
/// Share one `Option<f64>` cursor between instances to link their inspection time.
///
/// ```
/// # fn panel(ui: &mut egui::Ui) {
/// use sim_diagram::plot::TimeGraph;
/// use sim_inspect::plot::PlotPoint;
/// let data = [Some(PlotPoint { time: 0., value: 12., accepted_stage: false }), None];
/// let mut cursor = None;
/// TimeGraph::new(&data).unit("V").time_range([0., 1.]).show(ui, &mut cursor);
/// # }
/// ```
pub struct TimeGraph<'a> {
    samples: &'a [Option<PlotPoint>],
    unit: &'a str,
    time_range: Option<[f64; 2]>,
    height: f32,
    color: egui::Color32,
}
/// Geometry and nearest actual sample, usable by caller tooltips/linked panels.
pub struct TimeGraphResponse {
    pub response: egui::Response,
    pub plot_rect: egui::Rect,
    pub time_range: [f64; 2],
    pub value_range: [f64; 2],
    pub nearest: Option<PlotPoint>,
}
fn finite(p: &PlotPoint) -> bool {
    p.time.is_finite() && p.value.is_finite()
}
impl<'a> TimeGraph<'a> {
    pub fn new(samples: &'a [Option<PlotPoint>]) -> Self {
        Self {
            samples,
            unit: "",
            time_range: None,
            height: 140.,
            color: egui::Color32::from_rgb(22, 112, 170),
        }
    }
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }
    /// Invalid/degenerate ranges fall back to finite data bounds.
    pub fn time_range(mut self, range: [f64; 2]) -> Self {
        self.time_range = Some(range);
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        if height.is_finite() {
            self.height = height.max(80.);
        }
        self
    }
    pub fn color(mut self, color: egui::Color32) -> Self {
        self.color = color;
        self
    }
    pub fn show(self, ui: &mut egui::Ui, cursor: &mut Option<f64>) -> TimeGraphResponse {
        let series = self.samples;
        let points: Vec<_> = series.iter().flatten().filter(|p| finite(p)).collect();
        let min = points.iter().map(|p| p.time).reduce(f64::min).unwrap_or(0.);
        let max = points.iter().map(|p| p.time).reduce(f64::max).unwrap_or(1.);
        let [xmin, xmax] = self
            .time_range
            .filter(|r| r[0].is_finite() && r[1].is_finite() && r[1] > r[0])
            .unwrap_or([min, if max > min { max } else { min + 1. }]);
        if !self.unit.is_empty() {
            ui.small(format!("Value [{}] · time [s]", self.unit));
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(180.), self.height),
            egui::Sense::hover(),
        );
        let chart = egui::Rect::from_min_max(
            rect.min + egui::vec2(72., 8.),
            rect.max - egui::vec2(12., 25.),
        );
        let painter = ui.painter_at(rect);
        let color = self.color;

        let (mut ymin, mut ymax) = points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.value), hi.max(p.value))
            });
        if points.is_empty() {
            ymin = 0.;
            ymax = 1.;
        }
        let span = (ymax - ymin).abs();
        let pad = if span > 0. {
            span * 0.08
        } else {
            (ymax.abs() * 0.01).max(1e-9)
        };
        ymin -= pad;
        ymax += pad;
        let xy = |p: &PlotPoint| {
            egui::pos2(
                chart.left() + ((p.time - xmin) / (xmax - xmin)) as f32 * chart.width(),
                chart.bottom() - ((p.value - ymin) / (ymax - ymin)) as f32 * chart.height(),
            )
        };
        let text = |pos, align, s: String| {
            painter.text(
                pos,
                align,
                s,
                egui::FontId::monospace(10.),
                egui::Color32::DARK_GRAY,
            );
        };
        for i in 0..=2 {
            let y = chart.bottom() - chart.height() * i as f32 / 2.;
            painter.line_segment(
                [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
                (1., egui::Color32::LIGHT_GRAY),
            );
            text(
                egui::pos2(chart.left() - 4., y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.5e}", ymin + (ymax - ymin) * i as f64 / 2.),
            );
        }
        text(
            chart.left_bottom() + egui::vec2(0., 4.),
            egui::Align2::LEFT_TOP,
            format!("{xmin:.3} s"),
        );
        text(
            chart.right_bottom() + egui::vec2(0., 4.),
            egui::Align2::RIGHT_TOP,
            format!("{xmax:.3} s"),
        );
        let data_painter = ui.painter_at(chart);
        let mut previous = None;
        for point in series {
            if let Some(point) = point.filter(|p| finite(p)) {
                let pos = xy(&point);
                if let Some(prev) = previous {
                    data_painter.line_segment([prev, pos], (1.5, color));
                }
                data_painter.circle_filled(pos, 2., color);
                previous = Some(pos);
            } else {
                previous = None;
            }
        }
        if let Some(pos) = response.hover_pos().filter(|p| chart.contains(*p)) {
            *cursor = Some(xmin + ((pos.x - chart.left()) / chart.width()) as f64 * (xmax - xmin));
            ui.ctx().request_repaint();
        }
        if let Some(time) = cursor.filter(|t| *t >= xmin && *t <= xmax) {
            let x = chart.left() + ((time - xmin) / (xmax - xmin)) as f32 * chart.width();
            painter.line_segment(
                [egui::pos2(x, chart.top()), egui::pos2(x, chart.bottom())],
                (1., egui::Color32::GRAY),
            );
            if let Some(p) = points
                .iter()
                .min_by(|a, b| (a.time - time).abs().total_cmp(&(b.time - time).abs()))
            {
                text(
                    chart.center_top(),
                    egui::Align2::CENTER_TOP,
                    format!(
                        "sample {:.4} s: {:.5} · {}",
                        p.time,
                        p.value,
                        if p.accepted_stage {
                            "accepted stage"
                        } else {
                            "endpoint"
                        }
                    ),
                );
            }
        } else if points.is_empty() {
            text(
                chart.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for available samples".into(),
            );
        } else if let Some(p) = points.last() {
            text(
                chart.center_top(),
                egui::Align2::CENTER_TOP,
                format!(
                    "{:.5} · {}",
                    p.value,
                    if p.accepted_stage {
                        "accepted stage"
                    } else {
                        "endpoint"
                    }
                ),
            );
        }
        let nearest = cursor.and_then(|time| {
            points
                .iter()
                .min_by(|a, b| (a.time - time).abs().total_cmp(&(b.time - time).abs()))
                .map(|p| **p)
        });
        TimeGraphResponse {
            response,
            plot_rect: chart,
            time_range: [xmin, xmax],
            value_range: [ymin, ymax],
            nearest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_graph_hosts_share_cursor_without_a_runtime_or_model() {
        let ctx = egui::Context::default();
        let voltage = [
            Some(PlotPoint {
                time: 0.,
                value: 0.,
                accepted_stage: false,
            }),
            None,
            Some(PlotPoint {
                time: 1.,
                value: 12.,
                accepted_stage: true,
            }),
        ];
        let temperature = [Some(PlotPoint {
            time: 0.5,
            value: 294.,
            accepted_stage: false,
        })];
        let mut cursor = None;
        let mut chart = None;
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700., 600.),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            chart = Some(
                TimeGraph::new(&voltage)
                    .unit("V")
                    .time_range([0., 1.])
                    .show(ui, &mut cursor)
                    .plot_rect,
            );
        });
        output.textures_delta.clear();
        let pos = chart.unwrap().center();
        let mut output = ctx.run_ui(input(vec![egui::Event::PointerMoved(pos)]), |ui| {
            let first = TimeGraph::new(&voltage)
                .unit("V")
                .time_range([0., 1.])
                .show(ui, &mut cursor);
            let second = TimeGraph::new(&temperature)
                .unit("K")
                .height(160.)
                .time_range([0., 1.])
                .show(ui, &mut cursor);
            assert_eq!(first.time_range, second.time_range);
            assert_eq!(second.nearest.unwrap().time, 0.5);
            assert!(second.value_range[0] < 294. && second.value_range[1] > 294.);
        });
        output.textures_delta.clear();
        assert!((cursor.unwrap() - 0.5).abs() < 1e-5);
    }
    #[test]
    fn empty_and_unavailable_series_have_finite_axes() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            for data in [
                vec![],
                vec![None],
                vec![Some(PlotPoint {
                    time: f64::NAN,
                    value: f64::INFINITY,
                    accepted_stage: false,
                })],
            ] {
                let result = TimeGraph::new(&data)
                    .time_range([1., 1.])
                    .show(ui, &mut None);
                assert!(
                    result
                        .time_range
                        .iter()
                        .chain(&result.value_range)
                        .all(|x| x.is_finite())
                );
                assert!(result.nearest.is_none());
            }
        });
        output.textures_delta.clear();
    }
}
