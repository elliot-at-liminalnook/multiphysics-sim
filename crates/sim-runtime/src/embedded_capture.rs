//! Reader for recorded embedded execution captures (`*-execution.json`).
//!
//! The format is the run report [`crate::embedded`] writes: the report's
//! `frames[]` come from `EmbeddedSession::frame()` (`time_s`, `poses[]` of
//! `{name, position_m, rotation}` with published velocities), and the
//! top-level `source`, `completed`, `simulated_s`, `stepping_wall_s`, `step_s`
//! and `completed_steps` come from the run summary. The reader lives beside
//! that writer so every consumer (the native viewer's recorded presets, tools)
//! reads the one shape.
//!
//! Only the fields listed here are deserialized; contacts, motor, servo and
//! the other large per-frame fields are skipped. Metadata is never defaulted:
//! a field that is absent (or null) in the file is `None` and is reported as
//! absent. Nothing is recomputed: a capture is recorded physics, played back.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One link pose of a frame, as `EmbeddedSession::frame()` writes it: the link
/// frame at its com in the model frame (Z up), SI units; `rotation` row-major.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CapturePose {
    pub name: String,
    pub position_m: [f64; 3],
    pub rotation: [[f64; 3]; 3],
    /// World-frame linear velocity of the link frame, when published.
    #[serde(default)]
    pub velocity_m_s: Option<[f64; 3]>,
    /// World-frame angular velocity, when published.
    #[serde(default)]
    pub angular_velocity_rad_s: Option<[f64; 3]>,
}

/// One recorded frame: its simulation time and the link poses by name.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CaptureFrame {
    pub time_s: f64,
    pub poses: Vec<CapturePose>,
}

/// The capture's `source` block fields the viewer reports (the export's own
/// provenance, verbatim). Absent or null fields are `None`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct CaptureSource {
    #[serde(default)]
    pub fidelity: Option<String>,
    #[serde(default)]
    pub cad_sha256: Option<String>,
    #[serde(default)]
    pub cad_revision: Option<serde_json::Value>,
    #[serde(default)]
    pub file: Option<String>,
}

/// The run summary fields of a capture, as written. Absent or null fields are
/// `None`; `source` is `None` when the file has no source block.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct CaptureMeta {
    #[serde(default)]
    pub source: Option<CaptureSource>,
    #[serde(default)]
    pub completed: Option<bool>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
    #[serde(default)]
    pub simulated_s: Option<f64>,
    #[serde(default)]
    pub stepping_wall_s: Option<f64>,
    #[serde(default)]
    pub step_s: Option<f64>,
    #[serde(default)]
    pub completed_steps: Option<u64>,
    #[serde(default)]
    pub requested_steps: Option<u64>,
}
impl CaptureMeta {
    /// Recorded compute rate: simulated seconds per stepping wall second, when
    /// both are in the file and the wall time is positive.
    pub fn recorded_rate(&self) -> Option<f64> {
        match (self.simulated_s, self.stepping_wall_s) {
            (Some(s), Some(w)) if w > 0.0 => Some(s / w),
            _ => None,
        }
    }
}

/// The file's top level. Not `#[serde(flatten)]`: flattening would buffer
/// every unknown (large) field instead of skipping it.
#[derive(Deserialize)]
struct CaptureFile {
    #[serde(default)]
    frames: Option<Vec<CaptureFrame>>,
    #[serde(default)]
    source: Option<CaptureSource>,
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    simulated_s: Option<f64>,
    #[serde(default)]
    stepping_wall_s: Option<f64>,
    #[serde(default)]
    step_s: Option<f64>,
    #[serde(default)]
    completed_steps: Option<u64>,
    #[serde(default)]
    requested_steps: Option<u64>,
}

/// Rule for time order, reported with the capture.
pub const TIME_RULE: &str = "frames are sorted by time_s (stable); after sorting, time_s must be strictly increasing: equal times are refused as non-monotonic because the at-or-before lookup would be ambiguous";
/// The lookup rule (web/viewer/viewer.js `replayAt`).
pub const LOOKUP_RULE: &str = "the frame at or before t (the last frame with time_s <= t); t before the first frame gives frame 0 (web/viewer/viewer.js replayAt)";

/// A recorded capture: its path, time-sorted frames and run metadata.
#[derive(Clone, Debug)]
pub struct RecordedCapture {
    pub path: PathBuf,
    pub frames: Vec<CaptureFrame>,
    pub meta: CaptureMeta,
}
impl RecordedCapture {
    /// Reads and parses `path`. Errors name the path: a missing or unreadable
    /// file, invalid JSON or a malformed frame, no frames, or equal time_s.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(path, &text)
    }
    /// Parses capture bytes read from `path` (named in every error).
    pub fn parse(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let name = path.display();
        let file: CaptureFile = serde_json::from_slice(bytes).map_err(|e| format!("{name}: {e}"))?;
        let frames = file.frames.ok_or_else(|| format!("{name}: no `frames` array; a recorded capture needs frames"))?;
        if frames.is_empty() {
            return Err(format!("{name}: `frames` is empty; a recorded capture needs at least one frame"));
        }
        // Stable sort by time, keeping each frame's index in the file for errors.
        let mut indexed: Vec<(usize, CaptureFrame)> = frames.into_iter().enumerate().collect();
        indexed.sort_by(|a, b| a.1.time_s.total_cmp(&b.1.time_s));
        for w in indexed.windows(2) {
            if !(w[1].1.time_s > w[0].1.time_s) {
                return Err(format!("{name}: frames[{}] and frames[{}] both have time_s {}; time_s must be strictly increasing (non-monotonic time)", w[0].0, w[1].0, w[1].1.time_s));
            }
        }
        let meta = CaptureMeta { source: file.source, completed: file.completed, error: file.error, simulated_s: file.simulated_s, stepping_wall_s: file.stepping_wall_s,
            step_s: file.step_s, completed_steps: file.completed_steps, requested_steps: file.requested_steps };
        Ok(Self { path: path.to_path_buf(), frames: indexed.into_iter().map(|(_, f)| f).collect(), meta })
    }
    /// Index of the frame shown at time `t` ([`LOOKUP_RULE`]).
    pub fn index_at(&self, t: f64) -> usize {
        frame_at(&self.frames, t)
    }
    /// Recorded span: last frame time minus first frame time.
    pub fn duration_s(&self) -> f64 {
        self.frames.last().map_or(0.0, |l| l.time_s) - self.frames.first().map_or(0.0, |f| f.time_s)
    }
}

/// [`LOOKUP_RULE`] over time-sorted frames (0 for an empty slice).
pub fn frame_at(frames: &[CaptureFrame], t: f64) -> usize {
    frames.partition_point(|f| f.time_s <= t).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: f64, x: f64) -> serde_json::Value {
        serde_json::json!({"time_s": t, "contacts": [1, 2, 3], "poses": [{"name": "body", "position_m": [x, 0.0, 0.0], "rotation": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "velocity_m_s": [0.0, 0.0, 0.0]}]})
    }

    #[test]
    fn capture_sorts_looks_up_and_refuses_by_path() {
        let dir = std::env::temp_dir().join(format!("embedded-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, v: serde_json::Value| {
            let p = dir.join(name);
            std::fs::write(&p, serde_json::to_vec(&v).unwrap()).unwrap();
            p
        };
        // Written out of order; step_s absent (reported absent, not defaulted).
        let good = write("good.json", serde_json::json!({"frames": [frame(0.2, 2.0), frame(0.0, 0.0), frame(0.1, 1.0)],
            "source": {"fidelity": "test fidelity", "cad_sha256": "abc"}, "completed": true, "simulated_s": 0.2, "stepping_wall_s": 0.4, "motor_components": [0]}));
        let c = RecordedCapture::read(&good).unwrap();
        assert_eq!(c.frames.iter().map(|f| f.time_s).collect::<Vec<_>>(), vec![0.0, 0.1, 0.2]);
        assert_eq!(c.frames[2].poses[0].position_m[0], 2.0);
        assert_eq!(c.frames[0].poses[0].angular_velocity_rad_s, None);
        let s = c.meta.source.as_ref().unwrap();
        assert_eq!((s.fidelity.as_deref(), s.cad_sha256.as_deref(), s.file.as_deref()), (Some("test fidelity"), Some("abc"), None));
        assert_eq!((c.meta.completed, c.meta.step_s), (Some(true), None));
        assert_eq!(c.meta.recorded_rate(), Some(0.5));
        assert!((c.duration_s() - 0.2).abs() < 1e-12);
        // At or before t; before the first frame gives frame 0.
        for (t, i) in [(-1.0, 0), (0.0, 0), (0.05, 0), (0.1, 1), (0.15, 1), (0.2, 2), (9.0, 2)] {
            assert_eq!(c.index_at(t), i, "t = {t}");
        }
        let err = |name: &str, v: serde_json::Value| RecordedCapture::read(&write(name, v)).unwrap_err();
        let e = err("empty.json", serde_json::json!({"frames": []}));
        assert!(e.contains("empty.json") && e.contains("empty"), "{e}");
        let e = err("equal.json", serde_json::json!({"frames": [frame(0.0, 0.0), frame(0.1, 1.0), frame(0.1, 2.0)]}));
        assert!(e.contains("equal.json") && e.contains("frames[1] and frames[2]") && e.contains("non-monotonic"), "{e}");
        let e = RecordedCapture::read(&dir.join("missing.json")).unwrap_err();
        assert!(e.contains("missing.json"), "{e}");
        std::fs::write(dir.join("bad.json"), b"{not json").unwrap();
        let e = RecordedCapture::read(&dir.join("bad.json")).unwrap_err();
        assert!(e.contains("bad.json"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
