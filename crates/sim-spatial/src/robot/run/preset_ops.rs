//! `RunController`'s preset-only handlers: motion requests, recordings, replay and the gait preview.
use bevy::math::DQuat;
use serde_json::{Value, json};
use std::sync::Arc;
use crate::robot::gait::{GaitAction, GaitPreview};
use crate::robot::motion::{self, Motion};
use crate::robot::preset::PresetRun;
use crate::robot::recording::{self, Listed, Saved};
use super::protocol::Command;
use super::{Drive, MotionRequest, Phase, ReplayPhase, ReplayState, RunController};

impl RunController {
    /// The built preset's typed inputs and motion config (None before a build).
    pub fn drive(&self) -> Option<&Arc<Drive>> {
        self.drive.as_ref()
    }
    /// Whether physical motion keys are read: a built preset with a motion config.
    pub fn motion_keys_active(&self) -> bool {
        self.drive.as_ref().is_some_and(|d| d.motion.is_some()) && !matches!(self.status.phase, Phase::Failed | Phase::Ended)
    }
    pub fn keys_physical(&self) -> bool {
        self.keys_physical
    }
    /// The held value of session input `index` in the latest accepted frame.
    fn held(&self, index: usize) -> Option<f64> {
        self.frame.as_ref().and_then(|f| f.inputs.get(index).copied())
    }

    /// Why a motion request cannot be sent now, else the built drive and its motion config.
    fn check_motion(&self) -> Result<(Arc<Drive>, &Motion), String> {
        self.recorded_refusal("a motion request")?;
        let Some(p) = &self.preset else {
            return Err("motion requests are for robot presets (their declared Rust controller); `--robot FILE` has servo-target jog".into());
        };
        let id = &p.preset.id;
        if let Some(why) = self.replay_block() {
            return Err(format!("preset `{id}`: motion request refused: {why}"));
        }
        match self.status.phase {
            Phase::Failed => return Err(format!("preset `{id}`: the run failed; Reset rebuilds the session before motion requests")),
            Phase::Ended => return Err(format!("preset `{id}`: {}; Reset rebuilds at t = 0 before motion requests", self.status.end.as_ref().and_then(|e| e["message"].as_str()).unwrap_or("the run ended"))),
            _ => {}
        }
        let drive = self.drive.as_ref().ok_or_else(|| format!("preset `{id}`: no built session yet; Run or Step builds {} before motion requests", p.kind()))?;
        let motion = drive.motion.as_ref().ok_or_else(|| format!("preset `{id}` has no motion config: its session's policy_contract has no step_reference and presets.json declares no motion_commands"))?;
        if let Some(h) = &drive.heartbeat {
            let x = self.held(h.index).unwrap_or(f64::NAN);
            if !(x < h.upper) {
                return Err(format!("preset `{id}`: motion packet sequence `{}` is exhausted at {x} (upper {}); reset the session", h.name, h.upper));
            }
        }
        Ok((drive.clone(), motion))
    }
    /// Validates `request` against the session's typed channel bounds, without sending it.
    fn motion_values(&self, request: &MotionRequest) -> Result<[f64; 3], String> {
        let (drive, motion) = self.check_motion()?;
        let current = self.requested.unwrap_or_else(|| std::array::from_fn(|i| self.held(motion.channels[i].index).unwrap_or(0.0)));
        match request {
            MotionRequest::Key(k) => motion.keys(&[*k]),
            MotionRequest::HeldKeys(keys) => motion.keys(keys),
            MotionRequest::Stop => motion.stop(),
            MotionRequest::Channels(map) if map.is_empty() => Err("robot_input needs at least one channel value, a key, or stop".into()),
            MotionRequest::Channels(map) => motion.values(current, &map.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>(), &drive.inputs),
        }
    }
    /// Why a motion request is refused now (`Ok` when it would be sent).
    pub fn check_motion_request(&self, request: &MotionRequest) -> Result<(), String> {
        self.motion_values(request).map(|_| ())
    }
    /// The one motion handler behind physical keys, `system_ui` motion:*, the
    /// inspector buttons and REST `robot_input`. Refusals name the channel and
    /// its bounds and are kept for `robot_state.motion.last_refusal`.
    pub fn motion(&mut self, request: MotionRequest) -> Result<(), String> {
        let values = match self.motion_values(&request) {
            Ok(v) => v,
            Err(e) => {
                self.motion_refusal = Some(e.clone());
                return Err(e);
            }
        };
        self.thread.send(Command::Motion { values }).map_err(|_| "the run thread has stopped".to_string())?;
        self.motion_refusal = None;
        self.requested = Some(values);
        (self.keys, self.keys_physical) = match request {
            MotionRequest::Key(k) => (vec![k], false),
            MotionRequest::HeldKeys(keys) => (motion::KEYS.into_iter().filter(|k| keys.contains(k)).collect(), true),
            MotionRequest::Stop | MotionRequest::Channels(_) => (Vec::new(), false),
        };
        Ok(())
    }
    /// `robot_state.motion`: source, channels with bounds, requested and held
    /// values, active keys, heartbeat, last refusal and the motion-request label.
    pub fn motion_json(&self) -> Value {
        if let Some(r) = &self.recorded {
            return json!({"label": motion::LABEL, "available": false, "unavailable_reason": r.refusal("a motion request")});
        }
        let Some(p) = &self.preset else { return Value::Null };
        let declared = json!({"motion_commands": p.preset.entry.get("motion_commands"), "motion_heartbeat": p.preset.entry.get("motion_heartbeat"), "motion_key_vectors": p.preset.entry.get("motion_key_vectors")});
        let available = self.check_motion().map(|_| ());
        let drive = self.drive.as_ref();
        let motion = drive.and_then(|d| d.motion.as_ref());
        let channels: Option<Vec<Value>> = motion.map(|m| {
            m.channels.iter().enumerate().map(|(i, c)| json!({"name": c.name, "index": c.index, "kind": c.kind, "unit": c.unit, "lower": c.lower, "upper": c.upper,
                "requested": self.requested.map(|r| r[i]), "held": self.held(c.index)})).collect()
        });
        let heartbeat = drive.and_then(|d| d.heartbeat.as_ref()).map(|h| json!({"channel": h.name, "index": h.index, "lower": h.lower, "upper": h.upper, "value": self.held(h.index), "rule": motion::HEARTBEAT_RULE}));
        json!({"label": motion::LABEL, "available": available.is_ok(), "unavailable_reason": available.err(),
            "source": motion.map(|m| m.source), "config": motion.map(Motion::json), "channels": channels,
            "requested": self.requested, "active_keys": self.keys.iter().map(|k| k.to_string()).collect::<Vec<_>>(), "keys_physical": self.keys_physical,
            "heartbeat": heartbeat, "last_refusal": self.motion_refusal, "last_apply_error": self.motion_error,
            "session_inputs": drive.map(|d| d.inputs.iter().map(|c| json!({"name": c.name, "lower": c.lower, "upper": c.upper, "initial": c.initial})).collect::<Vec<_>>()),
            "declared_in_presets_json": declared, "keys": motion::KEY_SEMANTICS, "stop_key": motion::STOP_KEY, "clamping": motion::CLAMP_RULE,
            "values_rule": "requested: the values last sent this generation (null until a request; Reset clears them); held: the session's input value in the latest accepted frame. Non-motion channels keep their held values."})
    }

    /// Why a save cannot be requested now (`Ok` when it would be sent).
    /// Refusals name the reason; the target itself is checked by `save_recording`.
    pub fn check_save(&self) -> Result<&Arc<PresetRun>, String> {
        self.recorded_refusal("save recording")?;
        let Some(p) = &self.preset else {
            return Err("recordings are for robot presets (the shared EmbeddedSession/EmbeddedEnvironment recording); `--robot FILE` runs PhysicalRobot, which keeps no recording".into());
        };
        let id = &p.preset.id;
        if let Some(why) = self.replay_block() {
            return Err(format!("preset `{id}`: save refused: {why}"));
        }
        match self.status.phase {
            Phase::Idle => return Err(format!("preset `{id}`: no built session yet; Run or Step builds {} before a recording can be saved", p.kind())),
            Phase::Building => return Err(format!("preset `{id}`: the session is building; save once it is built")),
            _ => {}
        }
        if let Some(t) = &self.saving {
            return Err(format!("preset `{id}`: a save is still being written ({}); wait for it", t.display()));
        }
        Ok(p)
    }
    /// The one save handler behind the Save recording button, `system_ui`
    /// recording:save and REST `robot_save_recording`. The target is resolved
    /// here without file-system access (recording::target); the run
    /// thread snapshots the shared recording and a writer thread writes the
    /// pair, reported in `recording_json` once done.
    pub fn save_recording(&mut self, path: Option<&str>, note: Option<&str>) -> Result<std::path::PathBuf, String> {
        let result = self.check_save().and_then(|p| {
            let unix_ms = recording::now_ms();
            recording::target(&p.root, &p.preset.id, path, unix_ms).map(|t| (t, unix_ms))
        });
        let (target, unix_ms) = match result {
            Ok(x) => x,
            Err(e) => {
                self.save_error = Some(e.clone());
                return Err(e);
            }
        };
        self.save_requested += 1;
        self.thread.send(Command::SaveRecording { seq: self.save_requested, target: target.clone(), note: note.map(str::to_string), unix_ms }).map_err(|_| "the run thread has stopped".to_string())?;
        self.saving = Some(target.clone());
        self.save_error = None;
        Ok(target)
    }
    /// `robot_state.recording`: availability, the pending target, the last pair written and the last error, with the rules.
    pub fn recording_json(&self) -> Value {
        let available = self.check_save().map(|_| ());
        json!({"available": available.is_ok(), "unavailable_reason": available.err(), "pending": self.saving, "last_saved": self.saved, "error": self.save_error,
            "saves_requested": self.save_requested, "saves_finished": self.save_done,
            "root": self.preset.as_ref().map(|p| &p.root), "location_rule": recording::LOCATION_RULE, "file_rule": recording::FILE_RULE,
            "replayable_rule": recording::REPLAYABLE_RULE,
            "kind_rule": "the browser's kind for the same preset (web/worker.js: a task → EnvironmentSimulation.recording() = EmbeddedEnvironment::episode_recording(), kind sampled_environment_recording; otherwise EmbeddedSimulation.recording() = EmbeddedSession::recording(), kind embedded_session)"})
    }
    pub fn saved(&self) -> Option<&Saved> {
        self.saved.as_ref()
    }
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }
    pub fn save_pending(&self) -> Option<&std::path::Path> {
        self.saving.as_deref()
    }

    /// Lists the saved recordings of the loaded preset off the UI thread (at
    /// open, after each finished save and on request; a newer listing replaces
    /// an older one); `recordings_json` once done.
    pub fn refresh_recordings(&mut self) {
        let Some(p) = self.preset.clone() else { return };
        self.listing.start(crate::jobs::Pool::Io, "the recording lister", move |_| recording::list(&p.root, &p.preset.id));
    }
    pub fn recordings(&self) -> &[Listed] {
        &self.recordings
    }
    /// `robot_state.recordings`: the saved recordings of the loaded preset (null for `--robot FILE`).
    pub fn recordings_json(&self) -> Value {
        let Some(p) = &self.preset else { return Value::Null };
        json!({"dir": p.root.join(recording::DIR).join(&p.preset.id), "files": self.recordings, "pending": self.listing.pending().is_some(), "error": self.list_error,
            "rule": "*.json (not *.meta.json) in runs/robot-presets/<preset-id>/ under the root, by file name (UTC stamp, oldest first); meta summarises the sidecar when it exists; listed off the UI thread at open, after each save and on robot_replay {action: \"list\"}"})
    }

    /// Why run controls, motion and Save are refused because of a replay (None when no replay holds them).
    pub(super) fn replay_block(&self) -> Option<String> {
        let r = &self.replay;
        match r.phase {
            ReplayPhase::Replaying => Some(format!("replay of {} in progress ({}); Cancel or Reset", r.file(), r.progress())),
            ReplayPhase::Cancelled if r.replaced => Some(format!("the run is a cancelled partial replay of {} ({}); Reset starts a fresh run, or replay a recording", r.file(), r.progress())),
            _ => None,
        }
    }
    /// Why a replay cannot be started now (`Ok` when it would be sent).
    pub fn check_replay(&self) -> Result<&Arc<PresetRun>, String> {
        self.recorded_refusal("replay")?;
        let Some(p) = &self.preset else {
            return Err("replay is for robot presets (the shared EmbeddedSession/EmbeddedEnvironment prepare_replay); `--robot FILE` runs PhysicalRobot, which has no recording or replay".into());
        };
        let id = &p.preset.id;
        if self.replay.phase == ReplayPhase::Replaying {
            return Err(format!("preset `{id}`: a replay of {} is in progress ({}); Cancel or Reset before another replay", self.replay.file(), self.replay.progress()));
        }
        if self.status.phase == Phase::Building {
            return Err(format!("preset `{id}`: the session is building; replay once it is built"));
        }
        if self.running {
            return Err(format!("preset `{id}`: the run is running; Pause before replaying (a replay replaces the current run)"));
        }
        if let Some(why) = self.gait.as_ref().and_then(GaitPreview::holds) {
            return Err(format!("preset `{id}`: {why}; Stop the gait preview before a replay"));
        }
        Ok(p)
    }
    /// The one replay handler behind the inspector Replay buttons, `system_ui`
    /// replay:<file> and REST `robot_replay`. The run thread reads the file,
    /// prepares it through the shared prepare_replay and advances it in chunks
    /// (recording::REPLAY_RULE); the verdict is in `replay_json`.
    pub fn replay(&mut self, file: Option<&str>, path: Option<&str>) -> Result<std::path::PathBuf, String> {
        let p = self.check_replay()?;
        let source = recording::replay_source(&p.root, &p.preset.id, file, path)?;
        // Frames of the replaced run are stale once the replay (or its refusal) is published.
        self.generation += 1;
        self.running = false;
        self.requested = None;
        self.keys.clear();
        self.keys_physical = false;
        self.motion_refusal = None;
        self.motion_error = None;
        self.replay = ReplayState::new(self.replay.seq + 1, self.generation, Some(source.clone()), ReplayPhase::Replaying);
        self.graphs.clear(self.generation);
        self.thread.send(Command::Replay { generation: self.generation, seq: self.replay.seq, path: source.clone() }).map_err(|_| "the run thread has stopped".to_string())?;
        Ok(source)
    }
    /// Cancel: the run thread stops between chunks (phase cancelled, never done).
    pub fn cancel_replay(&mut self) -> Result<(), String> {
        self.check_cancel()?;
        self.thread.send(Command::CancelReplay).map_err(|_| "the run thread has stopped".to_string())?;
        self.replay.cancel_requested = true;
        Ok(())
    }
    pub fn check_cancel(&self) -> Result<(), String> {
        if self.replay.phase != ReplayPhase::Replaying {
            return Err(format!("no replay in progress to cancel (replay phase {:?})", self.replay.phase).to_lowercase());
        }
        if self.replay.cancel_requested {
            return Err(format!("cancel of {} already requested; it stops between chunks", self.replay.file()));
        }
        Ok(())
    }
    pub fn replay_state(&self) -> &ReplayState {
        &self.replay
    }
    /// `robot_state.replay`: path, phase, completed/total, verdict, error and measured, with the rules.
    pub fn replay_json(&self) -> Value {
        if self.preset.is_none() {
            return Value::Null;
        }
        let available = self.check_replay().map(|_| ());
        let mut v = json!(self.replay);
        v["available"] = json!(available.is_ok());
        v["unavailable_reason"] = json!(available.err());
        v["replay_rule"] = json!(recording::REPLAY_RULE);
        v["verdict_rule"] = json!(recording::VERDICT_RULE);
        v["identity_rule"] = json!(recording::IDENTITY_RULE);
        v["pause_step_rule"] = json!("Pause and Step are refused during a replay (\"replay … in progress; Cancel or Reset\"): a replay re-executes the recorded schedule to its end or to Cancel, and pausing or stepping it would add a second, unrecorded control path; Cancel stops it between chunks and Reset returns to a fresh run");
        v
    }
    /// Tests only: the whole held action through the motion handler's setter.
    #[cfg(test)]
    pub(super) fn set_inputs(&self, values: Vec<f64>) {
        self.thread.send(Command::SetInputs(values)).unwrap();
    }

    /// Why a gait preview action is refused: no preset scene, a running physics
    /// run or a replay in progress (for open, play and seek), then the preview's own checks.
    pub fn check_gait(&self, action: &GaitAction) -> Result<(), String> {
        self.recorded_refusal("gait preview")?;
        let Some(g) = &self.gait else {
            return Err("the gait preview poses a robot preset's scene with the shared KinematicMirror; a `--robot FILE` robot has no scene, so open a preset (robot_preset)".into());
        };
        if matches!(action, GaitAction::Open { .. } | GaitAction::Play | GaitAction::Seek { .. }) {
            if self.running {
                return Err("a physics run is running; Pause it before a gait preview (the preview would hide the simulated pose)".into());
            }
            if self.replay.phase == ReplayPhase::Replaying {
                return Err(format!("a replay of {} is in progress ({}); Cancel or Reset it before a gait preview", self.replay.file(), self.replay.progress()));
            }
        }
        g.check(action)
    }
    /// The one gait-preview handler (robot_gait): inspector, `system_ui` and REST `robot_gait`.
    pub fn gait(&mut self, action: GaitAction) -> Result<(), String> {
        self.check_gait(&action)?;
        self.gait.as_mut().expect("checked").act(action)
    }
    /// `robot_state.gait_preview` (null for `--robot FILE`).
    pub fn gait_json(&self) -> Value {
        let block = self.check_gait(&GaitAction::Play).err().filter(|e| e.starts_with("a physics") || e.starts_with("a replay"));
        self.gait.as_ref().map_or(Value::Null, |g| g.json(block))
    }
    pub fn gait_preview(&self) -> Option<&GaitPreview> {
        self.gait.as_ref()
    }
    /// The link poses to draw: the gait preview's while a gait is loaded, else the latest accepted frame's.
    pub fn display_poses(&self) -> Option<&[Option<([f64; 3], DQuat)>]> {
        self.gait.as_ref().and_then(GaitPreview::poses).or_else(|| self.frame.as_ref().map(|f| f.poses.as_slice()))
    }
}
