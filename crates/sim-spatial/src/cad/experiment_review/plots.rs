//! Shared chart rasterizer, on jobs. Match baseline by stable identity and
//! explicit unit; never align unrelated signals by their display names.
use super::*;
#[derive(Resource, Default)]
struct Plot {
    key: String,
    job: Option<Job<Image>>,
}
pub(crate) fn build(a: &mut App) {
    a.init_resource::<Plot>()
        .add_systems(Update, tick.in_set(crate::app::ViewerSet::JobResults));
}
fn points(series: &Value) -> Vec<[f64; 2]> {
    let Some(t) = series["t"].as_array() else {
        return vec![];
    };
    let Some(y) = series["values"].as_array() else {
        return vec![];
    };
    t.iter()
        .zip(y)
        .filter_map(|(t, y)| Some([t.as_f64()?, y.as_f64()?]))
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .collect()
}
fn tick(mut s: ResMut<ReviewState>, mut plot: ResMut<Plot>, mut images: ResMut<Assets<Image>>) {
    let selected_series = &s.sample["signals"][&s.signal];
    let key = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        s.sequence,
        s.signal,
        s.baseline.as_deref().unwrap_or(""),
        selected_series["identity"],
        selected_series["unit"],
        selected_series["values"].as_array().map_or(0, Vec::len),
        s.captured
            .as_ref()
            .map_or(String::new(), |c| c.comparison.to_string())
    );
    if plot.key != key {
        plot.key = key.clone();
        let series = s.sample["signals"][&s.signal].clone();
        let selected = points(&series);
        let baseline = s
            .captured
            .as_ref()
            .and_then(|c| c.baseline["review_signals"].as_object())
            .and_then(|catalogue| {
                catalogue
                    .values()
                    .find(|v| v["identity"] == series["identity"] && v["unit"] == series["unit"])
            })
            .map(points)
            .unwrap_or_default();
        plot.job = Some(Job::spawn(
            Pool::Compute,
            s.sequence,
            "captured signal plot",
            move |_| {
                let (px, _, _) = crate::chart::rasterize_span(
                    &[(&selected, [140, 170, 255]), (&baseline, [237, 184, 92])],
                    None,
                );
                let mut image = crate::chart::blank_image();
                image.data = Some(px);
                Ok(image)
            },
        ));
    }
    if let Some(answer) = plot.job.as_ref().and_then(|j| j.poll()) {
        plot.job = None;
        match answer {
            Ok(image) => {
                if let Some(handle) = &s.chart {
                    images
                        .insert(handle.id(), image)
                        .map_err(|e| s.error = Some(e.to_string()))
                        .ok();
                } else {
                    s.chart = Some(images.add(image));
                }
            }
            Err(e) => s.error = Some(e),
        }
        s.touch();
    }
}
