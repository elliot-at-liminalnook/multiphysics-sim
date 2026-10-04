//! A live run's frame history and its review timeline: the applied frames of
//! the current generation (the last [`HISTORY_S`] of sim time, at most
//! [`HISTORY_MAX`] frames), so a paused run, a finished replay or an ended
//! episode can be scrubbed back through (the links, readouts and chart
//! cursor show the frame at that time). Review only: nothing is re-simulated,
//! and Run, Step, Reset or a replay return to the live frame (a run resumes
//! from its live state, never from a reviewed one).
use serde_json::{Value, json};
use super::{Frame, RunController};

/// Sim-time window kept (s).
pub const HISTORY_S: f64 = 30.0;
/// Cap on frames kept.
pub const HISTORY_MAX: usize = 4000;
pub const HISTORY_RULE: &str = "review only: the run's applied frames of the current generation (the last HISTORY_S s of sim time, at most HISTORY_MAX frames) are kept as published; a review time shows the latest kept frame at or before it (links, readouts, chart cursor). Reviewing needs a run that is not running (seeking a running run pauses it first); Run, Step, Reset, a replay or a new generation return to the live frame, and a run always continues from its live state. Nothing is re-simulated or written.";

impl RunController {
    /// Keeps an applied frame (called by `poll` for each fresh frame of the current generation).
    pub(super) fn record_history(&mut self, f: &Frame) {
        if self.history.front().is_some_and(|h| h.generation != f.generation) || self.history.back().is_some_and(|h| h.time > f.time) {
            self.history.clear();
            self.history_view = None;
        }
        if self.history.back().is_some_and(|h| h.time == f.time) {
            self.history.pop_back();
        }
        self.history.push_back(f.clone());
        while self.history.len() > HISTORY_MAX || self.history.front().is_some_and(|h| h.time < f.time - HISTORY_S) {
            self.history.pop_front();
        }
    }
    /// Drops the history (Reset, a replay start: a new generation).
    pub(super) fn clear_history(&mut self) {
        self.history.clear();
        self.history_view = None;
    }
    /// Leaves review (Run, Step): the live frame shows again.
    pub(super) fn end_review(&mut self) {
        self.history_view = None;
    }
    /// The kept time span [first, last], when any frame is kept.
    pub fn history_span(&self) -> Option<(f64, f64)> {
        Some((self.history.front()?.time, self.history.back()?.time))
    }
    /// Why reviewing time `t` (None: back to live) is refused: a recorded
    /// preset (its own timeline), no kept frame, a time outside the span.
    pub fn check_history(&self, t: Option<f64>) -> Result<(), String> {
        self.recorded_refusal("the run history timeline (a recorded preset has its own Recorded timeline)")?;
        let Some(t) = t else { return Ok(()) };
        let (first, last) = self.history_span().ok_or("no run history yet: Run or Step first")?;
        if !t.is_finite() || t < first - 1e-9 || t > last + 1e-9 {
            return Err(format!("history time t = {t} s is outside the kept span [{first:.3}, {last:.3}] s (refused, not clamped)"));
        }
        Ok(())
    }
    /// Reviews time `t` (None: live). A running run is paused first (as the
    /// browser's timeline pauses), so the reviewed frame stays put.
    pub fn review(&mut self, t: Option<f64>) -> Result<(), String> {
        self.check_history(t)?;
        if t.is_some() && self.check(super::RunAction::Pause).is_ok() {
            self.act(super::RunAction::Pause)?;
        }
        self.history_view = t;
        Ok(())
    }
    /// The frame under review, when reviewing.
    pub fn reviewed(&self) -> Option<&Frame> {
        let t = self.history_view?;
        let i = self.history.partition_point(|f| f.time <= t + 1e-12);
        self.history.get(i.saturating_sub(1))
    }
    /// The frame to show: the reviewed one, else the latest.
    pub fn shown_frame(&self) -> Option<&Frame> {
        self.reviewed().or(self.frame.as_ref())
    }
    pub fn review_time(&self) -> Option<f64> {
        self.history_view
    }
    /// `robot_state.history`: span, frames kept, the review time and the rule.
    pub fn history_json(&self) -> Value {
        let available = self.check_history(None).map(|()| self.history_span().is_some());
        json!({"available": available.as_ref().is_ok_and(|x| *x), "unavailable_reason": available.err(), "span": self.history_span().map(|(a, b)| [a, b]), "frames": self.history.len(),
            "reviewing": self.history_view.is_some(), "review_time": self.history_view, "reviewed_frame_time": self.reviewed().map(|f| f.time), "window_s": HISTORY_S, "max_frames": HISTORY_MAX, "rule": HISTORY_RULE})
    }
}
