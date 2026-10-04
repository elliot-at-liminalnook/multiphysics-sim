//! Record video (the browser's `video-export.js`: the canvas's drawn frames
//! to a file): robot mode's 3D viewport captured at most [`FPS`] times a
//! second while recording (Bevy screenshots cropped to the robot camera's
//! viewport) and encoded on a jobs-owned thread into an H.264 MP4
//! (`sim_render::video`) under `runs/robot-video/`. A capture of drawn
//! frames, not a physics benchmark and no substitute for a saved input
//! recording. The same `RobotAction::Video` as the header button and REST
//! `robot_video`; leaving robot mode finishes the file.
use super::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

/// Capture rate (frames per second), as the browser's `captureStream(30)`.
pub const FPS: f32 = 30.0;
pub const VIDEO_RULE: &str = "Record video captures robot mode's 3D viewport (no panels) at most 30 frames a second, each frame stamped with its wall time since recording began, and encodes them off the UI thread as H.264 in an MP4 (OpenH264 via sim_render::video) at runs/robot-video/<preset id or model stem>-<UTC stamp>.mp4 under the workspace root (never overwritten); stopping writes the file. Frames come as fast as the window draws (an unfocused window draws less often; the display cap applies). While the window is hidden (minimized, fully covered, on another Space, screen locked) macOS draws nothing into it: starting is refused, and frames are not captured while it stays hidden (the last visible frame is held; hidden_s reports how long). Drawn frames only: not a physics benchmark, not a substitute for Save recording.";
/// Starting while the window is hidden.
pub const HIDDEN: &str = "video refused: the window is not visible (minimized, fully covered, on another Space or the screen locked); macOS draws nothing into a hidden window, so the video would be blank. Show the window and start again.";

pub(crate) enum VideoCmd {
    Frame { data: Vec<u8>, width: u32, bgra: bool, crop: [u32; 4], time_s: f64 },
    /// Write the last frame and the index (a capture still in flight is ignored).
    Finish,
}
#[derive(Default)]
pub(crate) struct VideoStatus {
    frames: u64,
    done: Option<Result<u64, String>>,
}

struct Active {
    path: PathBuf,
    started: Instant,
    thread: crate::jobs::RunThread<VideoCmd, VideoStatus>,
    /// A screenshot was asked for and has not arrived.
    pending: bool,
    last_capture: Option<Instant>,
    /// Seconds the window was hidden while recording (no frame captured).
    hidden_s: f64,
}

/// The recorder (kept across robot documents in the window).
#[derive(Resource, Default)]
pub struct VideoRecorder {
    active: Option<Active>,
    /// Recordings being finished (the encoder writes the index), with their paths.
    finishing: Vec<(PathBuf, crate::jobs::RunThread<VideoCmd, VideoStatus>, f64)>,
    /// The last outcome: the file written, or why not.
    pub last: Option<Result<String, String>>,
    /// The window is hidden now (`rest::Occlusion`, mirrored by [`capture`]).
    hidden: bool,
}

impl VideoRecorder {
    pub fn recording(&self) -> bool {
        self.active.is_some()
    }
    /// Starts recording to `path` (its directory is created by the encoder thread).
    pub fn start(&mut self, path: PathBuf) -> Result<(), String> {
        if self.active.is_some() {
            return Err("already recording video; stop it first".into());
        }
        if self.hidden {
            return Err(HIDDEN.into());
        }
        let target = path.clone();
        let thread = crate::jobs::RunThread::spawn("robot-video", VideoStatus::default(), move |rx, out| encode(target, rx, out));
        self.active = Some(Active { path, started: Instant::now(), thread, pending: false, last_capture: None, hidden_s: 0.0 });
        self.last = None;
        Ok(())
    }
    /// Stops recording: the encoder finishes the file (closing its channel is its stop signal).
    pub fn stop(&mut self) -> Result<(), String> {
        let a = self.active.take().ok_or("not recording video")?;
        // The thread is kept until it reports the file written.
        a.thread.send(VideoCmd::Finish).map_err(|_| "the video encoder has stopped".to_string())?;
        self.finishing.push((a.path, a.thread, a.hidden_s));
        Ok(())
    }
    /// `robot_state.video`.
    pub fn json(&self) -> Value {
        let a = self.active.as_ref();
        let frames = a.map(|a| a.thread.shared().lock().unwrap_or_else(|p| p.into_inner()).frames);
        json!({"recording": a.is_some(), "path": a.map(|a| &a.path), "seconds": a.map(|a| a.started.elapsed().as_secs_f64()), "frames": frames,
            "hidden_s": a.map(|a| a.hidden_s), "window_hidden": self.hidden,
            "finishing": self.finishing.iter().map(|(p, _, _)| p).collect::<Vec<_>>(), "last": self.last.as_ref().map(|l| match l { Ok(m) => json!({"ok": m}), Err(e) => json!({"error": e}) }),
            "fps": FPS, "rule": VIDEO_RULE})
    }
}

/// The encoder thread: frames until the channel closes, then the file's index.
fn encode(path: PathBuf, rx: mpsc::Receiver<VideoCmd>, out: std::sync::Arc<std::sync::Mutex<VideoStatus>>) {
    let mut encoder: Option<sim_render::video::Mp4Encoder> = None;
    let mut error: Option<String> = None;
    loop {
        // A closed channel or Finish ends the recording.
        let Ok(VideoCmd::Frame { data, width, bgra, crop, time_s }) = rx.recv() else { break };
        if error.is_some() {
            continue;
        }
        let [x0, y0, w, h] = crop;
        let result = (|| -> Result<(), String> {
            if encoder.is_none() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                encoder = Some(sim_render::video::Mp4Encoder::create(&path, w as usize, h as usize, FPS)?);
            }
            let e = encoder.as_mut().expect("created above");
            let (ew, eh) = e.size();
            let stride = width as usize * 4;
            let mut rgba = vec![0u8; ew * eh * 4];
            for y in 0..eh {
                let src = (y0 as usize + y) * stride + x0 as usize * 4;
                let row = data.get(src..src + ew * 4).ok_or("a captured frame is smaller than the recorded viewport (the window was resized while recording)")?;
                let dst = &mut rgba[y * ew * 4..(y + 1) * ew * 4];
                dst.copy_from_slice(row);
                if bgra {
                    for px in dst.chunks_mut(4) {
                        px.swap(0, 2);
                    }
                }
            }
            e.push(&rgba, ew * 4, time_s)
        })();
        match result {
            Ok(()) => out.lock().unwrap_or_else(|p| p.into_inner()).frames += 1,
            Err(e) => error = Some(e),
        }
    }
    let done = match (error, encoder) {
        (Some(e), _) => Err(e),
        (None, None) => Err("no frame was captured".into()),
        (None, Some(e)) => e.finish(),
    };
    out.lock().unwrap_or_else(|p| p.into_inner()).done = Some(done);
}

/// Present (after drawing): mirrors whether the window is hidden; while
/// recording a visible window, asks for one screenshot at a time at most FPS
/// times a second, cropped to the robot camera's viewport.
pub(super) fn capture(mut commands: Commands, mut rec: ResMut<VideoRecorder>, occlusion: Option<Res<crate::rest::Occlusion>>, camera: Query<&Camera, With<RobotCamera>>, time: Res<Time<Real>>) {
    let hidden = occlusion.is_some_and(|o| o.0);
    if rec.hidden != hidden {
        rec.hidden = hidden;
    }
    // Read first: a `ResMut` deref would mark the recorder changed every frame.
    if rec.active.is_none() {
        return;
    }
    let Some(a) = rec.active.as_mut() else { return };
    if hidden {
        a.hidden_s += time.delta_secs_f64();
        return;
    }
    if a.pending || a.last_capture.is_some_and(|t| t.elapsed().as_secs_f32() < 1.0 / FPS) {
        return;
    }
    let Some(rect) = camera.iter().next().and_then(Camera::physical_viewport_rect) else { return };
    let crop = [rect.min.x, rect.min.y, rect.width(), rect.height()];
    let Some(sender) = a.thread.sender() else { return };
    let started = a.started;
    a.pending = true;
    a.last_capture = Some(Instant::now());
    commands.spawn((Screenshot::primary_window(), DespawnOnExit(ModeScope::Robot))).observe(move |capture: On<ScreenshotCaptured>, mut rec: ResMut<VideoRecorder>| {
        use bevy::render::render_resource::TextureFormat as F;
        let image = &capture.image;
        let format = image.texture_descriptor.format;
        let bgra = matches!(format, F::Bgra8Unorm | F::Bgra8UnormSrgb);
        if let Some(a) = rec.active.as_mut() {
            a.pending = false;
        }
        if !(bgra || matches!(format, F::Rgba8Unorm | F::Rgba8UnormSrgb)) {
            rec.last = Some(Err(format!("video capture: unsupported window format {format:?}")));
            return;
        }
        let width = image.texture_descriptor.size.width;
        let data = image.data.clone().unwrap_or_default();
        let _ = sender.send(VideoCmd::Frame { data, width, bgra, crop, time_s: started.elapsed().as_secs_f64() });
    });
}

/// JobResults: finished recordings report the file written (or why not).
pub(super) fn receive(mut rec: ResMut<VideoRecorder>) {
    if rec.finishing.is_empty() {
        return;
    }
    let mut keep = Vec::new();
    let mut last = None;
    for (path, thread, hidden_s) in std::mem::take(&mut rec.finishing) {
        let done = thread.shared().lock().unwrap_or_else(|p| p.into_inner()).done.take();
        let hidden = if hidden_s > 0.0 { format!("; the window was hidden for {hidden_s:.1} s, held at its last visible frame") } else { String::new() };
        match done {
            None => keep.push((path, thread, hidden_s)),
            Some(Ok(frames)) => last = Some(Ok(format!("video saved: {} ({frames} frames{hidden})", path.display()))),
            Some(Err(e)) => last = Some(Err(format!("video not saved ({}): {e}", path.display()))),
        }
    }
    rec.finishing = keep;
    if last.is_some() {
        rec.last = last;
    }
}

/// Leaving robot mode finishes a recording in progress.
pub(super) fn leave(mut rec: ResMut<VideoRecorder>) {
    if rec.recording() {
        let _ = rec.stop();
    }
}

/// The file a new recording goes to: runs/robot-video/<preset id or model stem>-<stamp>.mp4.
pub(super) fn target(view: &RobotView) -> Result<PathBuf, String> {
    let root = view.root.clone().map_err(|e| format!("videos go under the workspace root: {e}"))?;
    let name = match view.preset.as_ref().or(view.drive_preset.as_ref()) {
        Some(p) => p.id.clone(),
        None => view.path.file_name().map(|f| f.to_string_lossy().trim_end_matches(".json").trim_end_matches(".simrobot").to_string()).unwrap_or_else(|| "robot".into()),
    };
    Ok(root.join("runs/robot-video").join(format!("{name}-{}.mp4", crate::robot::recording::stamp(crate::robot::recording::now_ms()))))
}

/// The header button's label.
#[derive(Component)]
pub(super) struct VideoButton;
pub(super) fn button_label(rec: Res<VideoRecorder>, buttons: Query<&Children, With<VideoButton>>, mut texts: Query<&mut Text>) {
    let want = if rec.recording() { "Stop video & save" } else { "Record video" };
    for children in &buttons {
        for c in children.iter() {
            if let Ok(mut t) = texts.get_mut(c) {
                if t.0 != want {
                    t.0 = want.to_string();
                }
            }
        }
    }
}
