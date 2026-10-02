//! Bounded native capture delivery and encoder ownership. No feature spawns a
//! process. The destination is replaced only after successful encoding, flush
//! and the final cancellation check; cleanup removes this job's directory only.
use super::{ChildProcess, Ctx};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::Duration,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Publication {
    Open,
    Cancelled,
    Published,
}
pub type PublicationGate = std::sync::Arc<std::sync::Mutex<Publication>>;
pub fn gate() -> PublicationGate {
    std::sync::Arc::new(std::sync::Mutex::new(Publication::Open))
}
pub fn cancel(gate: &PublicationGate) -> bool {
    let mut state = gate.lock().unwrap_or_else(|e| e.into_inner());
    if *state == Publication::Published {
        false
    } else {
        *state = Publication::Cancelled;
        true
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub sequence: u64,
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: bool,
    pub crop: [u32; 4],
    pub data: Vec<u8>,
}
pub fn encode(
    ctx: &Ctx,
    frames: Receiver<Frame>,
    destination: PathBuf,
    fps: u32,
    total: u64,
    width: u32,
    height: u32,
    gate: PublicationGate,
) -> Result<PathBuf, String> {
    if !destination.is_absolute() {
        return Err("Video destination must be absolute".into());
    }
    let parent = destination
        .parent()
        .ok_or("Video destination has no parent")?;
    let unique = format!(
        ".native-motion-{}-{}",
        std::process::id(),
        sim_annotate::uid("export")
    );
    let temporary = parent.join(unique);
    std::fs::create_dir(&temporary)
        .map_err(|e| format!("Create export temporary directory: {e}"))?;
    let result = (|| {
        let mut written_bytes = 0u64;
        for expected in 0..total {
            let frame = loop {
                if ctx.cancelled() {
                    return Err("Video export cancelled; destination preserved".into());
                }
                match frames.recv_timeout(Duration::from_millis(100)) {
                    Ok(frame) => break frame,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err("Frame producer closed; destination preserved".into());
                    }
                }
            };
            if frame.index != expected {
                return Err("Frame order mismatch; destination preserved".into());
            }
            if frame.data.len() > 64 * 1024 * 1024 {
                return Err("Screenshot exceeds bounded 64 MiB frame size".into());
            }
            let mut raw = frame.data;
            if frame.bgra {
                for p in raw.chunks_exact_mut(4) {
                    p.swap(0, 2);
                }
            }
            let rgba = image::RgbaImage::from_raw(frame.width, frame.height, raw)
                .ok_or("Screenshot dimensions/data mismatch")?;
            let [x, y, w, h] = frame.crop;
            if w == 0
                || h == 0
                || x.checked_add(w).is_none_or(|v| v > frame.width)
                || y.checked_add(h).is_none_or(|v| v > frame.height)
            {
                return Err("Stamped CAD viewport crop is invalid".into());
            }
            let cropped = image::imageops::crop_imm(&rgba, x, y, w, h).to_image();
            let resized = image::DynamicImage::ImageRgba8(cropped).resize(
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            let mut canvas =
                image::RgbaImage::from_pixel(width, height, image::Rgba([32, 36, 41, 255]));
            image::imageops::overlay(
                &mut canvas,
                &resized.to_rgba8(),
                ((width - resized.width()) / 2) as i64,
                ((height - resized.height()) / 2) as i64,
            );
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(canvas)
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            let png = png.into_inner();
            written_bytes += png.len() as u64;
            if written_bytes > 2 * 1024 * 1024 * 1024 {
                return Err(
                    "Export exceeds bounded 2 GiB temporary-frame budget; destination preserved"
                        .into(),
                );
            }
            std::fs::write(temporary.join(format!("{expected:08}.png")), png)
                .map_err(|e| format!("Write export frame: {e}"))?;
            ctx.steps(expected + 1, total);
            ctx.message("Capturing kinematic frames");
        }
        if ctx.cancelled() {
            return Err("Video export cancelled; destination preserved".into());
        }
        let output = temporary.join("encoded.mp4");
        let mut command = std::process::Command::new("ffmpeg");
        command
            .args([
                "-nostdin",
                "-y",
                "-loglevel",
                "error",
                "-framerate",
                &fps.to_string(),
                "-i",
            ])
            .arg(temporary.join("%08d.png"))
            .args([
                "-frames:v",
                &total.to_string(),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&output);
        let mut process = ChildProcess::spawn("native motion encoder", command)?;
        let began = std::time::Instant::now();
        loop {
            if began.elapsed() > Duration::from_secs(120) {
                process.stop();
                return Err("Encoder exceeded 120 seconds; destination preserved".into());
            }
            if ctx.cancelled() {
                process.stop();
                return Err("Video export cancelled during encoding; destination preserved".into());
            }
            if let Some(status) = process.exited() {
                if !status.contains("exit status: 0") && !status.contains("exit code: 0") {
                    return Err(format!("{status}; destination preserved"));
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        publish(ctx, &output, &destination, &gate)?;
        Ok(destination.clone())
    })();
    // Never remove the user's output or any other export's temporary directory.
    if let Err(e) = std::fs::remove_dir_all(&temporary) {
        if result.is_ok() {
            ctx.message(format!("Published video; temporary cleanup warning: {e}"));
        }
    }
    result
}
fn publish(
    ctx: &Ctx,
    temporary: &Path,
    destination: &Path,
    gate: &PublicationGate,
) -> Result<(), String> {
    let file = std::fs::File::open(temporary).map_err(|e| format!("Encoder output absent: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Flush encoded video: {e}"))?;
    let mut publication = gate.lock().unwrap_or_else(|e| e.into_inner());
    if ctx.cancelled() || *publication == Publication::Cancelled {
        return Err("Video export cancelled before publication; destination preserved".into());
    }
    std::fs::rename(temporary, destination)
        .map_err(|e| format!("Atomic video publication failed; destination preserved: {e}"))?;
    *publication = Publication::Published;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_linearized_with_publication() {
        let pending = gate();
        assert!(cancel(&pending));
        assert_eq!(*pending.lock().unwrap(), Publication::Cancelled);
        let published = gate();
        *published.lock().unwrap() = Publication::Published;
        assert!(!cancel(&published));
        assert_eq!(*published.lock().unwrap(), Publication::Published);
    }
    #[test]
    fn missing_temporary_preserves_existing_destination() {
        let root = std::env::temp_dir().join(sim_annotate::uid("publication-fixture"));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("existing.mp4");
        std::fs::write(&destination, b"existing user recording").unwrap();
        let missing = root.join("missing.mp4");
        let cancelled = gate();
        cancel(&cancelled);
        let dest = destination.clone();
        let job = super::super::Job::spawn(
            super::super::Pool::Io,
            0,
            "publication refusal fixture",
            move |ctx| {
                assert!(publish(ctx, &missing, &dest, &cancelled).is_err());
                Ok(())
            },
        );
        loop {
            if let Some(result) = job.poll() {
                result.unwrap();
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"existing user recording"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
