//! Presentation pacing: how fast a scripted scene plays on screen, so a
//! viewer can take in each caption, highlight and change. A [`PlaybackPlan`]
//! maps wall-clock seconds to simulated seconds. It never changes the
//! physics or the recorded run; it only decides how long each moment stays
//! on screen, and the viewer always shows the resulting playback speed.
//!
//! The rules follow the multimedia-learning evidence:
//! - Reading: captions are held for their reading time at subtitle pace
//!   (broadcast subtitle guidance targets about 160–180 words per minute;
//!   silent reading of expository text averages about 240, Brysbaert 2019,
//!   and reading while watching motion is slower), plus a short orientation
//!   beat to find the new text.
//! - Signaling (Mayer): a highlight or camera move must stay long enough for
//!   the eye to travel to it and fixate (a saccade takes about 0.2 s to
//!   start; locating and identifying a target takes longer), so each has a
//!   minimum dwell before the next change.
//! - Segmenting (Mayer & Chandler 2001): the scene holds at each new caption
//!   (an event boundary) so motion and reading do not compete, and
//!   authored `pause` cues hand control to the learner.
//! - Apprehension (Tversky, Morrison & Bétrancourt 2002): an animation must
//!   be slow enough to be perceived. Where a plotted quantity changes by half
//!   its range, the plan slows playback so that change takes at least
//!   `change_s` on screen, up to `maximum_auto_slowdown`; faster changes are
//!   reported as warnings so the author can slow the scene explicitly.
//!
//! The numeric thresholds are design choices within those findings, not
//! measured constants; they live in [`PacingRules`] so they can be tuned.
use crate::presentation::{Action, Timeline};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PacingRules {
    /// Caption reading rate while a scene is on screen, words per minute.
    pub reading_wpm: f64,
    /// Time to notice a new caption and move the eyes to it (s).
    pub caption_orientation_s: f64,
    /// Shortest time a caption stays before the scene moves on (s).
    pub minimum_caption_s: f64,
    /// Shortest time a highlight, box or arrow stays before the next (s).
    pub mark_dwell_s: f64,
    /// Time to re-orient after the camera moves (s).
    pub camera_settle_s: f64,
    /// Shortest on-screen duration of any stretch of motion between cues (s).
    pub minimum_segment_s: f64,
    /// A plotted change of half its range takes at least this long (s).
    pub change_s: f64,
    /// Automatic slow motion never goes below authored speed divided by this.
    pub maximum_auto_slowdown: f64,
    /// Hold for reading, looking and camera moves (off when the learner explores).
    pub holds: bool,
}
impl Default for PacingRules {
    fn default() -> Self {
        Self {
            reading_wpm: 170.,
            caption_orientation_s: 0.6,
            minimum_caption_s: 2.0,
            mark_dwell_s: 2.0,
            camera_settle_s: 1.2,
            minimum_segment_s: 1.5,
            change_s: 1.5,
            maximum_auto_slowdown: 8.,
            holds: true,
        }
    }
}
impl PacingRules {
    /// Free exploration: the learner drives, so no holds for reading or
    /// looking; fast changes are still slowed so they can be seen.
    pub fn exploring() -> Self {
        Self { holds: false, ..Self::default() }
    }
    /// Seconds to read a caption: orientation plus its words at the reading
    /// rate, and never less than the minimum.
    pub fn reading_s(&self, text: &str) -> f64 {
        let words = text.split_whitespace().count() as f64;
        (self.caption_orientation_s + words * 60. / self.reading_wpm).max(self.minimum_caption_s)
    }
}

/// Why playback is holding still.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// A caption appeared: time to read it.
    Read,
    /// A part was highlighted: time to find it.
    Look,
    /// The camera moved: time to re-orient.
    Camera,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Pace {
    Hold { reason: HoldReason, remaining_s: f64 },
    /// Simulated seconds per wall second.
    Run { speed: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct Piece {
    wall: [f64; 2],
    sim: [f64; 2],
    hold: Option<HoldReason>,
}

/// Wall-clock schedule of one scene. Build it once per recorded run.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlaybackPlan {
    pieces: Vec<Piece>,
    /// Wall times where an authored `pause` stops playback.
    pauses: Vec<f64>,
    /// Stretches too fast to follow even with automatic slow motion.
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Default)]
struct Demand {
    read: f64,
    look: f64,
    camera: f64,
    pause: bool,
}

const WINDOWS: usize = 1200;
/// Neighbouring windows' speeds differ by at most this factor, so automatic
/// slow motion eases in and out instead of lurching.
const SPEED_STEP: f64 = 1.25;
/// Screen seconds over which the speed may change by `SPEED_STEP`.
const EASE_S: f64 = 0.2;

impl PlaybackPlan {
    /// `series` are the plotted observables as (times, values), used to find
    /// changes too fast to follow at the authored speed.
    pub fn new(timeline: &Timeline, duration_s: f64, series: &[(&[f64], &[f64])], rules: &PacingRules) -> Result<Self, String> {
        if !(duration_s.is_finite() && duration_s > 0.) {
            return Err("scene duration must be positive".into());
        }
        // Attention boundaries and what each one asks of the viewer.
        let mut bounds: Vec<(f64, Demand)> = vec![(0., Demand::default()), (duration_s, Demand::default())];
        let mut speed_times = vec![];
        for c in &timeline.cues {
            if c.at_s >= duration_s {
                continue;
            }
            let mut d = Demand::default();
            match &c.action {
                Action::Caption { text } if !text.trim().is_empty() => d.read = rules.reading_s(text),
                Action::Highlight { paths } if !paths.is_empty() => d.look = rules.mark_dwell_s,
                Action::Camera { .. } => d.camera = rules.camera_settle_s,
                Action::Pause => d.pause = true,
                Action::Set { .. } => {}
                Action::Speed { .. } => {
                    speed_times.push(c.at_s);
                    continue;
                }
                _ => continue,
            }
            bounds.push((c.at_s, d));
        }
        bounds.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, Demand)> = vec![];
        for (t, d) in bounds {
            match merged.last_mut() {
                Some((u, m)) if (t - *u).abs() < 1e-12 => {
                    m.read = m.read.max(d.read);
                    m.look = m.look.max(d.look);
                    m.camera = m.camera.max(d.camera);
                    m.pause |= d.pause;
                }
                _ => merged.push((t, d)),
            }
        }
        // Automatic slow-motion limit per window from the plotted changes.
        let width = duration_s / WINDOWS as f64;
        let mut limit = vec![f64::INFINITY; WINDOWS];
        for (times, values) in series {
            let (lo, hi) = values.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
            let half = 0.5 * (hi - lo);
            if !(half > 0.) || times.len() != values.len() {
                continue;
            }
            // From each sample, how long until the quantity has moved by half
            // its range? Shown in less than `change_s` it is too fast: that
            // stretch slows so it takes `change_s`. Ripple smaller than half
            // the range never counts, however fast it wiggles.
            let n = times.len();
            let stride = (n / 6000).max(1);
            for i in (0..n).step_by(stride) {
                let (t0, v0) = (times[i], values[i]);
                if !(t0.is_finite() && v0.is_finite()) {
                    continue;
                }
                // Changes slower than this need no slow motion at the authored speed.
                let horizon = t0 + rules.change_s * timeline.state_at(t0).speed;
                let mut j = i + 1;
                while j < n && times[j] <= horizon {
                    if (values[j] - v0).abs() >= half {
                        // The move itself: from the last sample still near the
                        // start value (a tenth of the swing), not the quiet lead-in.
                        let k = (i..j).rev().find(|&k| (values[k] - v0).abs() <= 0.1 * half).unwrap_or(i);
                        let start = times[k];
                        let speed = (times[j] - start) / rules.change_s;
                        let (w0, w1) = (((start / width).floor().max(0.) as usize).min(WINDOWS - 1), ((times[j] / width).ceil() as usize).clamp(1, WINDOWS));
                        for l in &mut limit[w0..w1.max(w0 + 1)] {
                            *l = l.min(speed.max(1e-12));
                        }
                        break;
                    }
                    j += 1;
                }
            }
        }
        // What the changes themselves demand, before easing spreads it out
        // (warnings are about the content, not the ease-in around it).
        let raw = limit.clone();
        // No slower than the cap below the authored speed; applied before
        // easing so the approach to the cap is as smooth as any other.
        for (w, l) in limit.iter_mut().enumerate() {
            let floor = timeline.state_at((w as f64 + 0.5) * width).speed / rules.maximum_auto_slowdown;
            *l = l.max(floor);
        }
        // Ease in and out over screen time, not simulated time: a window
        // shown for `d` seconds lets the speed change by SPEED_STEP per
        // EASE_S of it, so deep slow motion recovers in a second or so
        // rather than over many slow windows.
        let factor = |l: f64| if l.is_finite() && l > 0. { SPEED_STEP.powf((width / l) / EASE_S).max(SPEED_STEP) } else { f64::INFINITY };
        for w in 1..WINDOWS {
            limit[w] = limit[w].min(limit[w - 1] * factor(limit[w - 1]));
        }
        for w in (0..WINDOWS - 1).rev() {
            limit[w] = limit[w].min(limit[w + 1] * factor(limit[w + 1]));
        }
        // Sub-intervals: windows split at every boundary and speed change.
        let mut edges: Vec<f64> = (0..=WINDOWS).map(|i| i as f64 * width).chain(merged.iter().map(|b| b.0)).chain(speed_times).collect();
        edges.retain(|t| (0. ..=duration_s).contains(t));
        edges.sort_by(f64::total_cmp);
        edges.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let mut capped = vec![false; WINDOWS];
        let mut runs: Vec<([f64; 2], f64)> = edges
            .windows(2)
            .map(|e| {
                let mid = 0.5 * (e[0] + e[1]);
                let authored = timeline.state_at(e[0]).speed;
                let w = ((mid / width) as usize).min(WINDOWS - 1);
                let floor = authored / rules.maximum_auto_slowdown;
                if raw[w] < floor {
                    capped[w] = true;
                }
                ([e[0], e[1]], authored.min(limit[w].max(floor)))
            })
            .collect();
        // Every stretch between boundaries lasts at least the minimum.
        let mut pieces = vec![];
        let mut pauses = vec![];
        let mut wall = 0.;
        let mut first = 0;
        for (i, (t, d)) in merged.iter().enumerate() {
            let end = merged.get(i + 1).map_or(duration_s, |b| b.0);
            let last = runs.iter().position(|r| r.0[0] >= end - 1e-12).unwrap_or(runs.len());
            let segment = &mut runs[first..last];
            let natural: f64 = segment.iter().map(|(s, v)| (s[1] - s[0]) / v).sum();
            if natural > 0. && natural < rules.minimum_segment_s {
                let factor = natural / rules.minimum_segment_s;
                segment.iter_mut().for_each(|r| r.1 *= factor);
            }
            let motion = natural.max(if natural > 0. { rules.minimum_segment_s } else { 0. });
            if d.pause {
                pauses.push(wall);
            } else if rules.holds {
                // Captions always get the orientation beat before motion.
                let read = if d.read > 0. { (d.read - motion).max(rules.caption_orientation_s) } else { 0. };
                let (hold, reason) = [(read, HoldReason::Read), (d.look - motion, HoldReason::Look), (d.camera - motion, HoldReason::Camera)]
                    .into_iter()
                    .fold((0., HoldReason::Read), |best, c| if c.0 > best.0 { c } else { best });
                if hold > 1e-9 {
                    pieces.push(Piece { wall: [wall, wall + hold], sim: [*t, *t], hold: Some(reason) });
                    wall += hold;
                }
            }
            for (s, v) in &runs[first..last] {
                let dt = (s[1] - s[0]) / v;
                pieces.push(Piece { wall: [wall, wall + dt], sim: *s, hold: None });
                wall += dt;
            }
            first = last;
        }
        let mut warnings = vec![];
        let mut w = 0;
        while w < WINDOWS {
            if capped[w] {
                let start = w;
                while w < WINDOWS && capped[w] {
                    w += 1;
                }
                warnings.push(format!(
                    "{:.4}–{:.4} s: a plotted quantity changes faster than can be followed even at {}× automatic slow motion; lower speed() here",
                    start as f64 * width,
                    w as f64 * width,
                    rules.maximum_auto_slowdown
                ));
            }
            w += 1;
        }
        Ok(Self { pieces, pauses, warnings })
    }

    /// Total wall-clock length (s), holds included, pauses excluded.
    pub fn duration(&self) -> f64 {
        self.pieces.last().map_or(0., |p| p.wall[1])
    }

    fn piece(&self, wall: f64) -> Option<&Piece> {
        let i = self.pieces.partition_point(|p| p.wall[1] < wall);
        self.pieces.get(i.min(self.pieces.len().saturating_sub(1)))
    }

    /// Simulated time shown at wall time `wall`.
    pub fn sim_at(&self, wall: f64) -> f64 {
        let Some(p) = self.piece(wall.max(0.)) else { return 0. };
        if p.hold.is_some() || p.wall[1] <= p.wall[0] {
            return p.sim[0];
        }
        let f = ((wall - p.wall[0]) / (p.wall[1] - p.wall[0])).clamp(0., 1.);
        p.sim[0] + f * (p.sim[1] - p.sim[0])
    }

    /// The earliest wall time showing simulated time `sim` (a boundary's
    /// hold starts again, so seeking to a caption gives time to read it).
    pub fn wall_at(&self, sim: f64) -> f64 {
        for p in &self.pieces {
            if sim <= p.sim[1] + 1e-12 {
                if p.hold.is_some() || p.sim[1] <= p.sim[0] {
                    return p.wall[0];
                }
                let f = ((sim - p.sim[0]) / (p.sim[1] - p.sim[0])).clamp(0., 1.);
                return p.wall[0] + f * (p.wall[1] - p.wall[0]);
            }
        }
        self.duration()
    }

    /// What playback is doing at wall time `wall`.
    pub fn pace_at(&self, wall: f64) -> Pace {
        match self.piece(wall) {
            Some(Piece { hold: Some(reason), wall: w, .. }) => Pace::Hold { reason: *reason, remaining_s: (w[1] - wall).max(0.) },
            Some(p) if p.wall[1] > p.wall[0] => Pace::Run { speed: (p.sim[1] - p.sim[0]) / (p.wall[1] - p.wall[0]) },
            _ => Pace::Run { speed: 1. },
        }
    }

    /// The first authored pause strictly after `from` and at or before `to`
    /// (wall seconds).
    pub fn pause_between(&self, from: f64, to: f64) -> Option<f64> {
        self.pauses.iter().copied().find(|p| *p > from + 1e-9 && *p <= to + 1e-9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::Cue;
    fn cue(at_s: f64, action: Action) -> Cue {
        Cue { at_s, action }
    }
    fn caption(text: &str) -> Action {
        Action::Caption { text: text.into() }
    }

    #[test]
    fn captions_hold_for_reading_then_motion_keeps_its_speed() {
        let rules = PacingRules::default();
        let text = "Back-EMF has grown; current falls to what the load needs";
        let t = Timeline::new(vec![
            cue(0., Action::Speed { factor: 0.25 }),
            cue(0., caption("Switch on")),
            cue(0.12, caption(text)),
            cue(0.2, caption("Load doubled")),
        ])
        .unwrap();
        let plan = PlaybackPlan::new(&t, 2.0, &[], &rules).unwrap();
        let hold = |sim: f64| plan.wall_at(sim + 1e-9) - plan.wall_at(sim);
        // The 0.12 s caption is followed by only 0.08 s of motion (shown for
        // the 1.5 s minimum), so the scene holds for the rest of its reading.
        let read = rules.reading_s(text);
        assert!(read > 3.);
        assert!(matches!(plan.pace_at(plan.wall_at(0.12) + 0.01), Pace::Hold { reason: HoldReason::Read, .. }));
        assert!((hold(0.12) - (read - rules.minimum_segment_s)).abs() < 1e-6, "held {}", hold(0.12));
        // A caption followed by long motion still gets its orientation beat.
        assert!((hold(0.2) - rules.caption_orientation_s).abs() < 1e-6);
        // After the holds, motion runs at the authored speed.
        assert!(matches!(plan.pace_at(plan.wall_at(1.0)), Pace::Run { speed } if (speed - 0.25).abs() < 1e-12));
        assert!((plan.sim_at(plan.duration()) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn short_stretches_last_long_enough_to_see() {
        let rules = PacingRules::default();
        // 60 ms of simulated time with nothing marked: at least the minimum.
        let plan = PlaybackPlan::new(&Timeline::default(), 0.06, &[], &rules).unwrap();
        assert!((plan.duration() - rules.minimum_segment_s).abs() < 1e-9);
        assert!(matches!(plan.pace_at(0.5), Pace::Run { speed } if (speed - 0.04).abs() < 1e-9));
    }

    #[test]
    fn fast_plotted_changes_slow_down_smoothly_and_warn_past_the_cap() {
        let rules = PacingRules::default();
        // A quantity that rises over 0.5 s at t = 1 in a 4 s scene.
        let times: Vec<f64> = (0..=4000).map(|i| i as f64 * 1e-3).collect();
        let rise: Vec<f64> = times.iter().map(|t| ((t - 1.) / 0.5).clamp(0., 1.)).collect();
        let plan = PlaybackPlan::new(&Timeline::default(), 4., &[(&times, &rise)], &rules).unwrap();
        let (a, b) = (plan.wall_at(1.0), plan.wall_at(1.25));
        assert!(b - a >= rules.change_s * 0.9, "half the rise takes {} s on screen", b - a);
        assert!(matches!(plan.pace_at(plan.wall_at(3.5)), Pace::Run { speed } if (speed - 1.).abs() < 1e-12), "quiet stretches keep real time");
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        // A step in one sample cannot be slowed enough: reported.
        let step: Vec<f64> = times.iter().map(|t| if *t < 1. { 0. } else { 1. }).collect();
        let plan = PlaybackPlan::new(&Timeline::default(), 4., &[(&times, &step)], &rules).unwrap();
        assert_eq!(plan.warnings.len(), 1, "{:?}", plan.warnings);
        // Speeds between neighbouring windows change gradually.
        let mut last: Option<f64> = None;
        for i in 0..WINDOWS {
            if let Pace::Run { speed } = plan.pace_at(plan.wall_at((i as f64 + 0.5) * 4. / WINDOWS as f64)) {
                if let Some(l) = last {
                    let r: f64 = speed / l;
                    // At most SPEED_STEP per EASE_S of screen time (one window is shown for width/speed).
                    let allowed = SPEED_STEP.powf((4. / WINDOWS as f64 / l.min(speed)) / EASE_S).max(SPEED_STEP);
                    assert!(r <= allowed + 1e-9 && r >= 1. / allowed - 1e-9, "{l} -> {speed}");
                }
                last = Some(speed);
            }
        }
    }

    #[test]
    fn pauses_stop_before_their_reading_and_mapping_is_monotone() {
        let rules = PacingRules::default();
        let t = Timeline::new(vec![cue(0.5, caption("Power off")), cue(0.5, Action::Pause), cue(1.0, Action::Highlight { paths: vec!["drum".into()] })]).unwrap();
        let plan = PlaybackPlan::new(&t, 2., &[], &rules).unwrap();
        let p = plan.pause_between(0., plan.duration()).unwrap();
        assert!((plan.sim_at(p) - 0.5).abs() < 1e-12);
        assert!(plan.pause_between(p, plan.duration()).is_none());
        assert!(matches!(plan.pace_at(plan.wall_at(1.0) + 0.1), Pace::Hold { reason: HoldReason::Look, .. }));
        let mut last = 0.;
        for i in 0..=1000 {
            let s = plan.sim_at(i as f64 * plan.duration() / 1000.);
            assert!(s >= last - 1e-12);
            last = s;
        }
        for s in [0., 0.3, 0.77, 1.5, 2.] {
            assert!((plan.sim_at(plan.wall_at(s)) - s).abs() < 1e-9);
        }
    }

    #[test]
    fn exploring_drops_holds_but_keeps_slow_motion() {
        let t = Timeline::new(vec![cue(0.5, caption("A long caption that would normally hold the scene for reading"))]).unwrap();
        let guided = PlaybackPlan::new(&t, 2., &[], &PacingRules::default()).unwrap();
        let free = PlaybackPlan::new(&t, 2., &[], &PacingRules::exploring()).unwrap();
        assert!(guided.duration() > free.duration() + 2.);
        assert!((free.duration() - 3.).abs() < 1e-9, "no holds when exploring, only the minimum stretch of motion");
        let times: Vec<f64> = (0..=2000).map(|i| i as f64 * 1e-3).collect();
        let step: Vec<f64> = times.iter().map(|t| ((t - 1.) / 0.3).clamp(0., 1.)).collect();
        let free = PlaybackPlan::new(&Timeline::default(), 2., &[(&times, &step)], &PacingRules::exploring()).unwrap();
        assert!(free.duration() > 3., "fast changes still slow down");
    }
}
