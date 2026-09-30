//! Contact sheets of lesson animation (REST `lesson_frames`): many frames of
//! a scene (or of the narrated explainer) as one labelled grid image, so the
//! motion, emphasis and pacing can be checked at a glance and then examined
//! at finer spacing where something looks wrong.
//!
//! Two ways to pick frames:
//!
//! - `seek`: the scene is paused at each time and drawn. Times are on the
//!   **screen** clock (the pacing plan: reading holds and slow motion appear
//!   as runs of similar frames) or the **sim** clock (simulated seconds), and
//!   are either listed (`times`) or spread over `from`..`to` (`count`).
//! - `live`: the scene (or a narration section) plays for real and a frame
//!   is taken every `interval_s` of wall time: exactly what a reader sees,
//!   narration cues included. Narration is muted while it is captured.
//!
//! Each tile is labelled with its index, screen time and sim time; the
//! artifact's metadata lists the same per tile. Frames are cropped to the
//! scene's card (default), its 3D view or the whole window.
use super::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Seek,
    Live,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Clock {
    /// On-screen playback time (the pacing plan).
    #[default]
    Screen,
    /// Simulated seconds.
    Sim,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    /// The scene's card: 3D view, controls, charts.
    #[default]
    Card,
    /// The 3D view only.
    View,
    Window,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// Play the scene from `from` (live mode).
    #[default]
    Scene,
    /// Play narration section `section` (live mode; muted).
    Narration,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FramesRequest {
    /// Scene ID (default: the live scene).
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub clock: Clock,
    /// Seek mode: exact times (on `clock`).
    #[serde(default)]
    pub times: Vec<f64>,
    /// Seek mode: a span on `clock` (default: the whole run); live mode: where the scene starts.
    #[serde(default)]
    pub from: Option<f64>,
    #[serde(default)]
    pub to: Option<f64>,
    /// Frames (default 64; at most 256, a 16 × 16 sheet).
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub columns: Option<usize>,
    /// Live mode: seconds of wall time between frames (default 0.5).
    #[serde(default)]
    pub interval_s: Option<f64>,
    #[serde(default)]
    pub start: Start,
    /// Live narration: the section index.
    #[serde(default)]
    pub section: Option<usize>,
    #[serde(default)]
    pub region: Region,
    /// Tile width in pixels (default 320).
    #[serde(default)]
    pub tile_width: Option<u32>,
    /// Also write the sheet here (a .png path).
    #[serde(default)]
    pub path: Option<PathBuf>,
}

struct Tile {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    label: (String, String),
    meta: serde_json::Value,
}

enum Phase {
    /// First frames: scroll the card into view, let layout settle; then start live playback.
    Warmup(u32),
    /// Seek to the next time (seek mode) or wait for its moment (live mode).
    Place,
    /// Frames to wait after seeking, so the view is drawn at the new time.
    Settle(u32, (String, String), serde_json::Value),
    Capturing(Arc<Mutex<Option<Image>>>, (String, String), serde_json::Value),
}

pub(crate) struct FrameJob {
    req: FramesRequest,
    scene: String,
    /// Seek: target times. Live: capture offsets in wall seconds.
    times: Vec<f64>,
    index: usize,
    phase: Phase,
    tiles: Vec<Tile>,
    started: Option<f64>,
    /// Frames waited for a camera glide to finish before this tile.
    glide_wait: u32,
    pub error: Option<String>,
    pub done: bool,
}

impl Learn {
    /// Start a contact sheet (REST `lesson_frames`).
    pub(crate) fn start_frames(&mut self, req: FramesRequest) -> Result<(), String> {
        if self.frames.is_some() {
            return Err("a contact sheet is already being captured".into());
        }
        if let Some(id) = &req.scene {
            if self.scene.as_ref().is_none_or(|a| &a.id != id) {
                if let Some(t) = id.strip_prefix("task-") {
                    self.start_task(t)?;
                } else {
                    // Like gating, a prediction lock does not apply to a capture.
                    let lesson = self.lesson.clone().ok_or("no lesson is open")?;
                    let scene = lesson.scene(id).map(|s| sim_runtime::lesson::lesson_scene(&lesson, s)).ok_or_else(|| format!("no scene `{id}` in this lesson"))?;
                    self.activate_scene(scene);
                }
            }
        }
        let a = self.scene.as_ref().ok_or("no live scene: give `scene`")?;
        let count = req.count.unwrap_or(64).clamp(1, 256);
        let times = match req.mode {
            Mode::Seek if !req.times.is_empty() => req.times.iter().copied().take(256).collect(),
            Mode::Seek => {
                let (a0, b0) = (req.from.unwrap_or(0.), req.to.unwrap_or(f64::NAN));
                let span = (a0, b0);
                // The end is filled in once the run (and its plan) exist.
                (0..count).map(|i| if count == 1 { span.0 } else { i as f64 / (count - 1) as f64 }).collect()
            }
            Mode::Live => {
                let dt = req.interval_s.unwrap_or(0.5).clamp(0.05, 10.);
                (0..count).map(|i| i as f64 * dt).collect()
            }
        };
        if req.path.as_ref().is_some_and(|p| p.extension().is_none_or(|e| e != "png")) {
            return Err("path must end in .png".into());
        }
        if req.mode == Mode::Live && req.start == Start::Narration && self.narration.is_none() {
            return Err("this lesson has no narration".into());
        }
        self.scroll_to = Some(a.id.clone());
        self.frames = Some(FrameJob { scene: a.id.clone(), req, times, index: 0, phase: Phase::Warmup(20), tiles: Vec::new(), started: None, glide_wait: 0, error: None, done: false });
        self.dirty = true;
        Ok(())
    }
}

/// Crop `rect` (physical px) from a captured window image and scale it to `width`.
fn tile(image: &Image, rect: URect, width: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (iw, ih) = (image.width(), image.height());
    let data = image.data.as_ref()?;
    let bgra = matches!(image.texture_descriptor.format, bevy::render::render_resource::TextureFormat::Bgra8Unorm | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb);
    let r = URect::from_corners(rect.min.min(UVec2::new(iw, ih)), rect.max.min(UVec2::new(iw, ih)));
    let (cw, ch) = (r.width().max(1), r.height().max(1));
    let w = width.min(cw).max(16);
    let h = ((ch as f64 * w as f64 / cw as f64).round() as u32).max(8);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // Box average over the source pixels this one covers.
            let (x0, x1) = (r.min.x + x * cw / w, (r.min.x + (x + 1) * cw / w).max(r.min.x + x * cw / w + 1));
            let (y0, y1) = (r.min.y + y * ch / h, (r.min.y + (y + 1) * ch / h).max(r.min.y + y * ch / h + 1));
            let mut acc = [0u32; 3];
            let mut n = 0u32;
            for sy in (y0..y1.min(ih)).step_by(1) {
                for sx in (x0..x1.min(iw)).step_by(1) {
                    let i = ((sy * iw + sx) * 4) as usize;
                    let (r0, g0, b0) = if bgra { (data[i + 2], data[i + 1], data[i]) } else { (data[i], data[i + 1], data[i + 2]) };
                    acc[0] += r0 as u32;
                    acc[1] += g0 as u32;
                    acc[2] += b0 as u32;
                    n += 1;
                }
            }
            let o = ((y * w + x) * 4) as usize;
            let n = n.max(1);
            out[o] = (acc[0] / n) as u8;
            out[o + 1] = (acc[1] / n) as u8;
            out[o + 2] = (acc[2] / n) as u8;
            out[o + 3] = 255;
        }
    }
    Some((out, w, h))
}

/// A 5 × 7 bitmap font for tile labels.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        ':' => [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0],
        '-' => [0, 0, 0, 0x1F, 0, 0, 0],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        's' => [0, 0, 0x0E, 0x10, 0x0E, 0x01, 0x1E],
        'i' => [0x04, 0, 0x0C, 0x04, 0x04, 0x04, 0x0E],
        'm' => [0, 0, 0x1A, 0x15, 0x15, 0x11, 0x11],
        'n' => [0, 0, 0x16, 0x19, 0x11, 0x11, 0x11],
        'a' => [0, 0, 0x0E, 0x01, 0x0F, 0x11, 0x0F],
        'r' => [0, 0, 0x16, 0x19, 0x10, 0x10, 0x10],
        'c' => [0, 0, 0x0E, 0x10, 0x10, 0x11, 0x0E],
        'e' => [0, 0, 0x0E, 0x11, 0x1F, 0x10, 0x0E],
        'w' => [0, 0, 0x11, 0x11, 0x15, 0x15, 0x0A],
        'p' => [0, 0, 0x1E, 0x11, 0x1E, 0x10, 0x10],
        'l' => [0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'y' => [0, 0, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        'h' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11],
        'o' => [0, 0, 0x0E, 0x11, 0x11, 0x11, 0x0E],
        'd' => [0x01, 0x01, 0x0D, 0x13, 0x11, 0x11, 0x0F],
        _ => [0; 7],
    }
}
fn draw_text(buf: &mut [u8], width: u32, x: u32, y: u32, text: &str, scale: u32, color: [u8; 3]) {
    for (k, c) in text.chars().enumerate() {
        let g = glyph(c);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5u32 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + (k as u32 * 6 + col) * scale + dx;
                        let py = y + row as u32 * scale + dy;
                        let i = ((py * width + px) * 4) as usize;
                        if i + 3 < buf.len() && px < width {
                            buf[i..i + 3].copy_from_slice(&color);
                        }
                    }
                }
            }
        }
    }
}

/// The grid image (PNG) and its per-tile metadata.
fn sheet(job: &FrameJob) -> Result<(Vec<u8>, serde_json::Value), String> {
    let n = job.tiles.len();
    if n == 0 {
        return Err("no frames were captured".into());
    }
    let columns = job.req.columns.unwrap_or_else(|| (n as f64).sqrt().ceil() as usize).clamp(1, 32);
    let rows = n.div_ceil(columns);
    let tw = job.tiles.iter().map(|t| t.w).max().unwrap_or(1);
    let th = job.tiles.iter().map(|t| t.h).max().unwrap_or(1);
    let (gap, label_h) = (4u32, 22u32);
    let (w, h) = (columns as u32 * (tw + gap) + gap, rows as u32 * (th + label_h + gap) + gap);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for p in buf.chunks_exact_mut(4) {
        p.copy_from_slice(&[12, 14, 17, 255]);
    }
    let mut meta = Vec::new();
    for (i, t) in job.tiles.iter().enumerate() {
        let (c, r) = ((i % columns) as u32, (i / columns) as u32);
        let (x0, y0) = (gap + c * (tw + gap), gap + r * (th + label_h + gap));
        draw_text(&mut buf, w, x0 + 2, y0 + 2, &t.label.0, 1, [230, 232, 236]);
        draw_text(&mut buf, w, x0 + 2, y0 + 12, &t.label.1, 1, [150, 200, 190]);
        for y in 0..t.h {
            let src = (y * t.w * 4) as usize;
            let dst = (((y0 + label_h + y) * w + x0) * 4) as usize;
            buf[dst..dst + (t.w * 4) as usize].copy_from_slice(&t.rgba[src..src + (t.w * 4) as usize]);
        }
        let mut m = t.meta.clone();
        m["index"] = serde_json::json!(i);
        m["row"] = serde_json::json!(r);
        m["column"] = serde_json::json!(c);
        meta.push(m);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&buf, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    Ok((png, serde_json::json!({"scene": job.scene, "mode": job.req.mode, "clock": job.req.clock, "region": job.req.region, "columns": columns, "rows": rows, "tile": [tw, th], "tiles": meta})))
}
use image::ImageEncoder;

/// Advance the capture by one frame.
pub(super) fn step(mut commands: Commands, time: Res<Time>, mut learn: ResMut<Learn>, scene: Res<SpatialScene>, window: Single<&Window>, blocks: Query<(&super::ui::BlockNode, &ComputedNode, &UiGlobalTransform)>, orbit: Single<&crate::Orbit>) {
    let Some(mut job) = learn.frames.take() else { return };
    if job.done || job.error.is_some() {
        learn.frames = Some(job);
        return;
    }
    let now = time.elapsed_secs_f64();
    let result: Result<(), String> = (|| {
        let a = learn.scene.as_ref().filter(|a| a.id == job.scene).ok_or("the scene changed during capture")?;
        if a.run.is_none() {
            // Still recording: wait.
            if matches!(job.phase, Phase::Warmup(_)) {
                job.phase = Phase::Warmup(4);
            }
            return Ok(());
        }
        let shoot = |commands: &mut Commands| {
            let slot = Arc::new(Mutex::new(None));
            let s = slot.clone();
            commands.spawn(Screenshot::primary_window()).observe(move |trigger: On<ScreenshotCaptured>| {
                if let Ok(mut g) = s.lock() {
                    *g = Some(trigger.event().image.clone());
                }
            });
            slot
        };
        let describe = |learn: &Learn, index: usize, screen: f64| {
            let a = learn.scene.as_ref().unwrap();
            let caption = a.timeline.state_at(a.time).caption;
            // The pace the reader would see here while it plays.
            let pace = super::ui::pace_label(a.pace(), a.user_speed, true);
            let narration = learn.narration.as_ref().map(|n| (n.section, n.time));
            ((format!("#{index} {screen:.2}s"), format!("sim {:.4}s", a.time)), serde_json::json!({"screen_s": screen, "sim_s": a.time, "caption": caption, "pace": pace, "narration": narration}))
        };
        match &mut job.phase {
            Phase::Warmup(n) => {
                if *n > 0 {
                    *n -= 1;
                    return Ok(());
                }
                if job.req.mode == Mode::Live {
                    job.started = Some(now);
                    match job.req.start {
                        Start::Scene => {
                            let a = learn.scene.as_mut().unwrap();
                            a.seek(job.req.from.unwrap_or(0.));
                            a.playing = true;
                        }
                        Start::Narration => {
                            learn.player_volume(0.);
                            learn.narrate(narrate::NarrateAction::Section { index: job.req.section.unwrap_or(0) })?;
                        }
                    }
                }
                job.phase = Phase::Place;
                Ok(())
            }
            Phase::Place => {
                match job.req.mode {
                    Mode::Seek => {
                        let a = learn.scene.as_mut().unwrap();
                        let t = job.times[job.index];
                        // Spread times are fractions of the span; listed ones are absolute.
                        let t = if job.req.times.is_empty() {
                            let span_end = job.req.to.unwrap_or(match job.req.clock { Clock::Screen => a.plan.duration(), Clock::Sim => a.duration() });
                            let start = job.req.from.unwrap_or(0.);
                            start + t * (span_end - start)
                        } else {
                            t
                        };
                        match job.req.clock {
                            Clock::Screen => a.seek_wall(t),
                            Clock::Sim => a.seek(t),
                        }
                        a.playing = false;
                        let screen = a.wall_time();
                        let (label, meta) = describe(&learn, job.index, screen);
                        job.phase = Phase::Settle(3, label, meta);
                    }
                    Mode::Live => {
                        let start = job.started.unwrap_or(now);
                        if now < start + job.times[job.index] {
                            return Ok(());
                        }
                        let (label, meta) = describe(&learn, job.index, now - start);
                        job.phase = Phase::Capturing(shoot(&mut commands), label, meta);
                    }
                }
                Ok(())
            }
            Phase::Settle(n, label, meta) => {
                if *n > 0 {
                    *n -= 1;
                    return Ok(());
                }
                // A seek shows where the camera is going, not a glide half done
                // (bounded, in case something keeps it moving).
                if job.req.mode == Mode::Seek && orbit.glide.is_some() && job.glide_wait < 240 {
                    job.glide_wait += 1;
                    return Ok(());
                }
                job.glide_wait = 0;
                let (label, meta) = (label.clone(), meta.clone());
                job.phase = Phase::Capturing(shoot(&mut commands), label, meta);
                Ok(())
            }
            Phase::Capturing(slot, label, meta) => {
                let Some(image) = slot.lock().ok().and_then(|mut g| g.take()) else { return Ok(()) };
                let scale = window.scale_factor();
                let (ww, wh) = (window.physical_width(), window.physical_height());
                let rect = match job.req.region {
                    Region::Window => URect::new(0, 0, ww, wh),
                    Region::View => scene.learn_view.map(|v| URect::from_corners(v.visible.min.max(Vec2::ZERO).as_uvec2(), v.visible.max.as_uvec2())).unwrap_or(URect::new(0, 0, ww, wh)),
                    Region::Card => blocks.iter().find(|(b, ..)| b.0 == job.scene).map(|(_, node, gt)| {
                        let c = gt.translation;
                        let half = node.size() * 0.5;
                        URect::from_corners((c - half).max(Vec2::ZERO).as_uvec2(), (c + half).as_uvec2())
                    }).filter(|r| r.width() > 16 && r.height() > 16).ok_or("the scene's card is not on screen")?,
                };
                let _ = scale;
                let (rgba, w, h) = tile(&image, rect, job.req.tile_width.unwrap_or(320).clamp(64, 1024)).ok_or("could not read the captured frame")?;
                job.tiles.push(Tile { rgba, w, h, label: label.clone(), meta: meta.clone() });
                job.index += 1;
                if job.index >= job.times.len() {
                    job.done = true;
                } else {
                    job.phase = Phase::Place;
                }
                Ok(())
            }
        }
    })();
    if let Err(e) = result {
        job.error = Some(e);
    }
    if job.done || job.error.is_some() {
        // Leave things as they were: scene paused, narration stopped and audible again.
        if job.req.mode == Mode::Live {
            if job.req.start == Start::Narration {
                let _ = learn.narrate(narrate::NarrateAction::Stop);
            }
            learn.player_volume(1.);
            if let Some(a) = learn.scene.as_mut() {
                a.playing = false;
            }
        }
    }
    learn.frames = Some(job);
}

/// Finish a capture for REST: the sheet as an image artifact (and a file).
pub(crate) fn finish(learn: &mut Learn) -> Option<sim_api::Outcome> {
    let job = learn.frames.as_ref()?;
    if let Some(e) = &job.error {
        let e = e.clone();
        learn.frames = None;
        return Some(sim_api::Outcome::Done(Err(e)));
    }
    if !job.done {
        return None;
    }
    let job = learn.frames.take()?;
    Some(match sheet(&job) {
        Err(e) => sim_api::Outcome::Done(Err(e)),
        Ok((png, mut meta)) => {
            if let Some(p) = &job.req.path {
                if let Err(e) = std::fs::write(p, &png) {
                    return Some(sim_api::Outcome::Done(Err(format!("{}: {e}", p.display()))));
                }
                meta["path"] = serde_json::json!(p);
            }
            sim_api::Outcome::Image(sim_api::Artifact { png, metadata: meta })
        }
    })
}
