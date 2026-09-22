use crate::{Rendered, Size, canvas::*};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub size: Size,
    pub observables: Vec<String>,
    pub time_range: Option<[f64; 2]>,
    pub cursor: Option<f64>,
}
#[derive(Clone)]
pub struct Series {
    pub label: String,
    pub color: [u8; 3],
    pub points: Vec<Option<[f64; 2]>>,
}
#[derive(Clone)]
pub struct Panel {
    pub title: String,
    pub unit: String,
    pub series: Vec<Series>,
}
pub fn render(
    title: &str,
    subtitle: &str,
    panels: &[Panel],
    options: &Options,
    mut metadata: serde_json::Value,
) -> Result<Rendered, String> {
    let size = options.size.validate()?;
    if panels.is_empty() || panels.len() > 8 {
        return Err("select between one and eight graph channels".into());
    }
    if (size.height as usize) < 100 + panels.len() * 110 {
        return Err("increase image height to at least 100 + 110 pixels per graph".into());
    }
    if options.cursor.is_some_and(|v| !v.is_finite())
        || options
            .time_range
            .is_some_and(|r| !r[0].is_finite() || !r[1].is_finite() || r[0] >= r[1])
    {
        return Err("invalid graph time range or cursor".into());
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for panel in panels {
        for series in &panel.series {
            for p in series.points.iter().flatten() {
                if !p[0].is_finite() || !p[1].is_finite() {
                    return Err("nonfinite graph point".into());
                }
                lo = lo.min(p[0]);
                hi = hi.max(p[0]);
            }
        }
    }
    let [xmin, xmax] = options.time_range.unwrap_or_else(|| {
        if lo.is_finite() {
            [lo, if hi > lo { hi } else { lo + 1. }]
        } else {
            [0., 1.]
        }
    });
    let mut c = Canvas::new(size.width, size.height);
    c.header(title, subtitle);
    let h = (size.height as f32 - 112.) / panels.len() as f32;
    let left = 104.;
    let right = size.width as f32 - 34.;
    let mut reports = Vec::new();
    for (index, panel) in panels.iter().enumerate() {
        let y = 94. + index as f32 * h;
        let top = y + 48.;
        let bottom = y + h - 31.;
        let values: Vec<_> = panel
            .series
            .iter()
            .flat_map(|s| s.points.iter().flatten())
            .filter(|p| p[0] >= xmin && p[0] <= xmax)
            .collect();
        let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &values {
            ymin = ymin.min(p[1]);
            ymax = ymax.max(p[1]);
        }
        if !ymin.is_finite() {
            ymin = 0.;
            ymax = 1.;
        }
        let pad = if ymax > ymin {
            ((ymax - ymin) * 0.08).max(1e-9)
        } else {
            (ymax.abs() * 0.01).max(1e-9)
        };
        ymin -= pad;
        ymax += pad;
        c.text(
            28.,
            y,
            18.,
            &if panel.title.ends_with(&format!("[{}]", panel.unit)) {
                panel.title.clone()
            } else {
                format!("{}  [{}]", panel.title, panel.unit)
            },
            INK,
            size.width as f32 - 56.,
        );
        let xy = |p: [f64; 2]| {
            [
                (left as f64 + (p[0] - xmin) / (xmax - xmin) * (right - left) as f64) as f32,
                (bottom as f64 - (p[1] - ymin) / (ymax - ymin) * (bottom - top) as f64) as f32,
            ]
        };
        for tick in 0..=4 {
            let f = tick as f64 / 4.;
            let xx = left + (right - left) * f as f32;
            let yy = bottom - (bottom - top) * f as f32;
            c.line([xx, top], [xx, bottom], 1., GRID);
            c.line([left, yy], [right, yy], 1., GRID);
            c.text(
                8.,
                yy - 7.,
                12.,
                &number(ymin + (ymax - ymin) * f),
                MUTED,
                90.,
            );
            c.text(
                xx - 19.,
                bottom + 5.,
                12.,
                &number(xmin + (xmax - xmin) * f),
                MUTED,
                84.,
            );
        }
        for (series_index, series) in panel.series.iter().enumerate() {
            let mut previous = None;
            for p in &series.points {
                match p {
                    Some(p) if p[0] >= xmin && p[0] <= xmax => {
                        let point = xy(*p);
                        if let Some(last) = previous {
                            c.line(last, point, 1.8, series.color);
                        }
                        if values.len() < 60 {
                            c.dot(point[0], point[1], 2.4, series.color);
                        }
                        previous = Some(point);
                    }
                    _ => previous = None,
                }
            }
            let lx =
                left + series_index as f32 * ((right - left) / panel.series.len().max(1) as f32);
            c.line([lx, y + 31.], [lx + 18., y + 31.], 2.5, series.color);
            c.text(
                lx + 23.,
                y + 23.,
                12.,
                &series.label,
                series.color,
                ((right - left) / panel.series.len().max(1) as f32) - 30.,
            );
        }
        if values.is_empty() {
            c.text(
                left + 20.,
                top + 10.,
                18.,
                "No available samples in this time range",
                MUTED,
                right - left - 30.,
            );
        }
        if let Some(t) = options.cursor.filter(|t| *t >= xmin && *t <= xmax) {
            let x = xy([t, ymin])[0];
            c.line([x, top], [x, bottom], 1.2, [175, 82, 39]);
        }
        reports.push(json!({"title":panel.title,"unit":panel.unit,"available_points":values.len(),"y_range":[ymin,ymax]}));
    }
    c.text(
        28.,
        size.height as f32 - 18.,
        12.,
        "Time [s] · actual sample timestamps · unavailable values break the curves",
        MUTED,
        size.width as f32 - 56.,
    );
    metadata["kind"] = json!("graphs");
    metadata["size"] = json!(size);
    metadata["time_range"] = json!([xmin, xmax]);
    metadata["panels"] = json!(reports);
    Ok(Rendered {
        png: c.png()?,
        metadata,
    })
}
