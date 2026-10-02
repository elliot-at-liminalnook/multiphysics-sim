//! One screenshot in flight, two frames queued. Reference samples complete
//! before screenshot dispatch; no camera/geometry derivation is duplicated.
use super::*;
pub(crate) use crate::jobs::video::Frame as Pixels;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::{
    path::PathBuf,
    sync::mpsc::{SyncSender, TrySendError},
};
pub(crate) struct Export {
    gate: crate::jobs::video::PublicationGate,
    sequence: u64,
    stamp: Stamp,
    program: Value,
    total: u64,
    next: u64,
    fps: u32,
    width: u32,
    height: u32,
    pub(crate) capture_pending: bool,
    sender: Option<SyncSender<Pixels>>,
    encode: Job<PathBuf>,
    sample: Option<Job<PoseSample>>,
    continuation: Option<sim_runtime::cad_client::motion::PoseContinuation>,
    requested_time: Option<f64>,
    prepared: bool,
    capture_started: Option<std::time::Instant>,
    cancelled: bool,
    saved_sample: Option<PoseSample>,
    saved_cursor: f64,
    saved_active: bool,
    pub(crate) camera: Option<crate::camera::Orbit>,
    screenshot: Option<Entity>,
}
impl Export {
    pub(crate) fn cancel(&mut self) {
        self.cancelled = crate::jobs::video::cancel(&self.gate);
        if self.cancelled {
            self.encode.cancel();
        }
        if let Some(j) = &self.sample {
            j.cancel();
        }
        self.sender = None;
    }
    pub(crate) fn state(&self) -> Value {
        json!({"frames":self.next,"total":self.total,"cancel_requested":self.cancelled,"progress":self.encode.progress().message,"fraction":self.encode.progress().fraction,"size":[self.width,self.height],"fps":self.fps,"publication":format!("{:?}",*self.gate.lock().unwrap_or_else(|e|e.into_inner()))})
    }
}
pub(crate) fn build(a: &mut App) {
    a.add_systems(Update, capture.in_set(crate::app::ViewerSet::Present));
}
pub(crate) fn preview_ready(d: &CadDocument, s: &MotionState) -> Result<(), String> {
    let stamp = s.identity.as_ref().ok_or("Enter reference pose first")?;
    guard(d, stamp)?;
    if s.sampling.is_some() || s.queued_sample.is_some() {
        return Err("Pause and wait for the pending preview pose before exporting; seek intent is preserved".into());
    }
    let sample = s
        .sample
        .as_ref()
        .ok_or("Wait for a published reference pose before exporting")?;
    if !sampling::matches(sample, stamp, s.cursor) {
        return Err("Wait for the current preview cursor to publish before exporting".into());
    }
    Ok(())
}
pub(crate) fn start(d: &CadDocument, s: &mut MotionState) -> Result<(), String> {
    preview_ready(d, s)?;
    if s.program_job.is_some() || s.loading.is_some() {
        return Err("Wait for reference metadata/program validation before exporting".into());
    }
    if !s.active {
        return Err("Enter active reference preview before exporting".into());
    }
    if s.export.is_some() {
        return Err("An export is still running; cancel and wait for terminal receipt".into());
    }
    let stamp = s.identity.clone().ok_or("Enter reference pose first")?;
    guard(d, &stamp)?;
    let program: Value =
        serde_json::from_str(&s.editor).map_err(|e| format!("motion.program: {e}"))?;
    let duration = program["duration"]
        .as_f64()
        .ok_or("Program duration missing")?;
    if !duration.is_finite() || !(0.05..=30.).contains(&duration) {
        return Err("Native export duration is bounded to 0.05–30 seconds".into());
    }
    let destination = PathBuf::from(&s.path);
    if !destination.is_absolute() || destination.extension().is_none_or(|e| e != "mp4") {
        return Err("Choose an absolute .mp4 destination".into());
    }
    let fps = if s.fps > 0 { s.fps } else { 24 };
    let width = if s.width > 0 { s.width } else { 1280 };
    let height = if s.height > 0 { s.height } else { 720 };
    let total = (duration * fps as f64).ceil() as u64;
    let (tx, rx) = std::sync::mpsc::sync_channel::<Pixels>(2);
    let gate = crate::jobs::video::gate();
    let worker_gate = gate.clone();
    let encode = Job::spawn(
        Pool::Dedicated,
        d.generation,
        "native kinematic video",
        move |ctx| {
            crate::jobs::video::encode(ctx, rx, destination, fps, total, width, height, worker_gate)
        },
    );
    s.playing = false;
    s.cancel_requested = false;
    let continuation = sampling::prior(s.sample.as_ref(), &stamp);
    s.export = Some(Export {
        gate,
        sequence: s.sequence,
        stamp,
        program,
        total,
        next: 0,
        fps,
        width,
        height,
        capture_pending: false,
        sender: Some(tx),
        encode,
        sample: None,
        continuation,
        requested_time: None,
        prepared: false,
        capture_started: None,
        cancelled: false,
        saved_sample: s.sample.clone(),
        saved_cursor: s.cursor,
        saved_active: s.active,
        camera: None,
        screenshot: None,
    });
    Ok(())
}
pub(crate) fn deliver(s: &mut MotionState, p: Pixels) -> Result<(), String> {
    let e = s
        .export
        .as_mut()
        .ok_or("Capture belongs to a completed export")?;
    if p.sequence != e.sequence || p.index != e.next || e.cancelled {
        return Err("Stale export capture refused".into());
    }
    e.screenshot = None;
    e.capture_pending = false;
    e.capture_started = None;
    match e
        .sender
        .as_ref()
        .ok_or("Export delivery cancelled")?
        .try_send(p)
    {
        Ok(()) => {
            e.next += 1;
            e.prepared = false;
            Ok(())
        }
        Err(TrySendError::Full(_)) => {
            e.cancel();
            Err("Export frame queue full; cancelled without publication".into())
        }
        Err(_) => {
            e.cancel();
            Err("Export frame delivery closed".into())
        }
    }
}
#[derive(Component)]
struct OwnedScreenshot(u64);
fn capture(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    mut s: ResMut<MotionState>,
    camera: Query<&Camera, With<Camera3d>>,
    screenshots: Query<(Entity, &OwnedScreenshot)>,
) {
    for (entity, owned) in &screenshots {
        if s.export
            .as_ref()
            .is_none_or(|e| e.cancelled || e.sequence != owned.0 || e.screenshot != Some(entity))
        {
            commands.entity(entity).despawn();
        }
    }
    let Some(d) = doc else { return };
    let s = &mut *s;
    let Some(e) = s.export.as_mut() else { return };
    if guard(&d, &e.stamp).is_err() || s.sequence != e.sequence {
        e.cancel();
        return;
    }
    if e.capture_started
        .is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(15))
    {
        s.error = Some("Screenshot delivery timed out; destination preserved".into());
        e.cancel();
        return;
    }
    if e.cancelled || e.capture_pending || e.next >= e.total {
        return;
    }
    if let Some(answer) = e.sample.as_ref().and_then(|j| j.poll()) {
        e.sample = None;
        match answer {
            Ok(sample) => {
                if e.requested_time
                    .is_none_or(|time| !sampling::matches(&sample, &e.stamp, time))
                {
                    s.error = Some(
                        "Export reference sample identity/time mismatch; continuation unchanged"
                            .into(),
                    );
                    e.cancel();
                    return;
                }
                e.continuation = Some(sample.continuation());
                s.sample = Some(sample);
                e.prepared = true;
                return;
            }
            Err(error) => {
                s.error = Some(error);
                e.cancel();
                return;
            }
        }
    }
    if !e.prepared {
        if e.sample.is_none() {
            let Some(c) = d.client.clone() else {
                e.cancel();
                return;
            };
            let r = PoseRequest {
                document_id: e.stamp.document.clone().unwrap_or_default(),
                expected_revision: e.stamp.revision,
                positions: s.positions.clone(),
                program: Some(e.program.clone()),
                time: e.next as f64 / e.fps as f64,
                prior: e.continuation.clone(),
            };
            e.requested_time = Some(r.time);
            e.sample = Some(Job::spawn(
                Pool::Dedicated,
                d.generation,
                "export reference sample",
                move |_| c.sample_pose(&r).map_err(|e| e.to_string()),
            ));
        }
        return;
    }
    e.capture_pending = true;
    e.capture_started = Some(std::time::Instant::now());
    let Some(rect) = camera
        .iter()
        .next()
        .and_then(Camera::physical_viewport_rect)
    else {
        s.error = Some("CAD camera physical viewport unavailable".into());
        e.cancel();
        return;
    };
    let crop = [rect.min.x, rect.min.y, rect.width(), rect.height()];
    let sequence = e.sequence;
    let index = e.next;
    let entity = commands
        .spawn((
            Screenshot::primary_window(),
            OwnedScreenshot(sequence),
            DespawnOnExit(crate::app::ModeScope::Cad),
        ))
        .observe(
            move |capture: On<ScreenshotCaptured>,
                  mut out: MessageWriter<crate::app::actions::Act<CadAction>>| {
                let image = &capture.image;
                let size = image.texture_descriptor.size;
                let bgra = matches!(
                    image.texture_descriptor.format,
                    bevy::render::render_resource::TextureFormat::Bgra8Unorm
                        | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb
                );
                let supported = bgra
                    || matches!(
                        image.texture_descriptor.format,
                        bevy::render::render_resource::TextureFormat::Rgba8Unorm
                            | bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
                    );
                let data = if supported {
                    image.data.clone().unwrap_or_default()
                } else {
                    Vec::new()
                };
                out.write(crate::app::actions::Act::ui(
                    MotionArgs {
                        pixels: Some(Pixels {
                            sequence,
                            index,
                            width: size.width,
                            height: size.height,
                            bgra,
                            crop,
                            data,
                        }),
                        ..MotionArgs::of(MotionOp::CapturedFrame)
                    }
                    .action(),
                ));
            },
        )
        .id();
    e.screenshot = Some(entity);
}
pub(crate) fn poll(s: &mut MotionState, doc: Option<&CadDocument>) {
    if let Some(result) = s.export.as_ref().and_then(|e| e.encode.poll()) {
        let e = s.export.take().unwrap();
        let current = doc.is_some_and(|d| guard(d, &e.stamp).is_ok());
        if current {
            s.sample = e.saved_sample.clone();
            s.cursor = e.saved_cursor;
        }
        s.active = current && e.saved_active && !s.cancel_requested;
        s.history.push(
            json!({"export_sequence":e.sequence,"result":result,"cancel_requested":e.cancelled}),
        );
        s.touch();
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        if crate::jobs::video::cancel(&self.gate) {
            self.encode.cancel();
        }
    }
}
