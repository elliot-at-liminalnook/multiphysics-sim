//! Narrated explainers in the Learn screen: play a lesson's `explainer.md`
//! section by section, fire its cues on their words (scroll, highlight,
//! boxes and arrows, scene play/pause/seek, camera), hold the narration
//! while a scene plays (`wait-scene`) or loads, and (re)generate a section's
//! audio in the background through `sim-voice`.
//!
//! The narration clock is the audio's own playback position when a section
//! has audio; otherwise it runs on frame time with subtitles only.
use super::*;
use crate::ui_kit::{ACCENT, BORDER, FAINT, Kit, LEFT_WIDTH, Look, OK, ON_ACCENT, RIGHT_WIDTH, STATUSBAR, SUBTLE, SliderLook, TEXT, TOPBAR, UiFonts, WARN};
use sim_lesson::narration::{self as nar, Cue, Explainer, Manifest, MarkState, Target, Timing, TimingKind};

mod bar;
mod overlay;

pub(super) use bar::{bar, live, outline, reserved, seek};
pub(super) use overlay::overlay;
pub(crate) use overlay::{ChartNode, FigureNode};

#[derive(Component, Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum NarrateAction {
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    Section { index: usize },
    Seek { time_s: f64 },
    /// Back to the start of the sentence being spoken (or the one before, just after it began).
    Replay,
    /// (Re)generate one section's audio, or every stale/missing one.
    Generate { section: Option<String> },
}

/// Why the narration is holding.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wait {
    /// Until the scene stops playing.
    SceneStops,
    /// Until the scene is loaded and recorded; then run these cues.
    SceneReady(Vec<Cue>),
    /// Until the reader answers this question.
    Quiz(String),
}

pub(crate) struct GenJob {
    pub label: String,
    /// Paid voice generation: runs to its end (nothing in `sim_voice::generate`
    /// checks a token); progress lines arrive as the job's progress message.
    work: crate::jobs::Job<sim_voice::Report>,
}
impl GenJob {
    /// The latest progress line ("Starting…" until the first one).
    pub fn progress(&self) -> String {
        let p = self.work.progress().message;
        if p.is_empty() { "Starting…".into() } else { p }
    }
}

pub struct Narration {
    pub explainer: Explainer,
    pub manifest: Manifest,
    pub timings: Vec<Timing>,
    pub times: Vec<Vec<f64>>,
    pub section: usize,
    pub time: f64,
    pub playing: bool,
    pub(crate) wait: Option<Wait>,
    next_cue: usize,
    pub marks: MarkState,
    pub(crate) job: Option<GenJob>,
    pub error: Option<String>,
    stamp: Option<std::time::SystemTime>,
    manifest_stamp: Option<std::time::SystemTime>,
    audio: BTreeMap<String, Arc<Vec<u8>>>,
    pub(crate) loaded: Option<String>,
    pub last_report: Option<String>,
}

impl Narration {
    /// View directives the narration has in effect now.
    pub(crate) fn view(&self) -> sim_script::presentation::ViewState {
        self.explainer.sections.get(self.section).zip(self.times.get(self.section)).map(|(s, t)| nar::view_at(s, t, self.time)).unwrap_or_default()
    }
}

fn stamp(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Narration {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let explainer = Explainer::load(path).map_err(|e| e.to_string())?;
        let mut n = Self { manifest: Manifest::default(), timings: vec![], times: vec![], section: 0, time: 0., playing: false, wait: None, next_cue: 0, marks: MarkState::default(), job: None, error: None, stamp: stamp(path), manifest_stamp: None, audio: BTreeMap::new(), loaded: None, last_report: None, explainer };
        n.refresh();
        Ok(n)
    }
    /// Reload the manifest and recompute every section's word and cue times.
    fn refresh(&mut self) {
        let path = self.explainer.narration_dir().join("narration.json");
        self.manifest_stamp = stamp(&path);
        self.manifest = self.explainer.manifest();
        self.timings = self.explainer.sections.iter().map(|s| self.explainer.timing(&self.manifest, s)).collect();
        self.times = self.explainer.sections.iter().zip(&self.timings).map(|(s, t)| nar::cue_times(s, t)).collect();
        self.audio.clear();
        self.loaded = None;
        self.section = self.section.min(self.explainer.sections.len().saturating_sub(1));
    }
    pub fn duration(&self) -> f64 {
        self.timings.get(self.section).map(|t| t.duration_s).unwrap_or(0.)
    }
    pub fn total(&self) -> f64 {
        self.timings.iter().map(|t| t.duration_s).sum()
    }
    fn key(&self) -> Option<String> {
        let s = self.explainer.sections.get(self.section)?;
        self.explainer.audio(&self.manifest, s).map(|e| format!("{}:{}", s.id, e.hash))
    }
    fn bytes(&mut self) -> Option<Arc<Vec<u8>>> {
        let s = self.explainer.sections.get(self.section)?;
        let entry = self.explainer.audio(&self.manifest, s)?;
        let path = self.explainer.narration_dir().join(&entry.file);
        if let Some(b) = self.audio.get(&entry.file) {
            return Some(b.clone());
        }
        let b = Arc::new(std::fs::read(path).ok()?);
        self.audio.insert(entry.file.clone(), b.clone());
        Some(b)
    }
    /// Move to (section, time) without replaying past events.
    fn jump(&mut self, section: usize, time: f64) {
        self.section = section.min(self.explainer.sections.len().saturating_sub(1));
        self.time = time.clamp(0., self.duration());
        self.next_cue = self.times.get(self.section).map(|t| t.partition_point(|c| *c < self.time - 1e-9)).unwrap_or(0);
        self.wait = None;
        self.loaded = None;
    }
    /// When the sentence being spoken began (the previous one's, within its first second).
    pub fn sentence_start(&self) -> f64 {
        let (Some(s), Some(t)) = (self.explainer.sections.get(self.section), self.timings.get(self.section)) else { return 0. };
        let spoken = nar::words_at(t, self.time);
        let ends = |w: &String| w.ends_with(['.', '?', '!']);
        let starts: Vec<usize> = (0..=spoken.min(s.words.len())).filter(|&i| i == 0 || ends(&s.words[i - 1])).collect();
        let at = |i: usize| t.starts.get(i).copied().unwrap_or(0.);
        match starts.as_slice() {
            [.., prev, last] if self.time - at(*last) < 1.0 => at(*prev),
            [.., last] => at(*last),
            [] => 0.,
        }
    }
    /// The whole section's words, split at the current word (for the transcript).
    pub fn transcript(&self) -> (String, String) {
        let (Some(s), Some(t)) = (self.explainer.sections.get(self.section), self.timings.get(self.section)) else { return (String::new(), String::new()) };
        let spoken = nar::words_at(t, self.time).min(s.words.len());
        (s.words[..spoken].join(" "), s.words[spoken..].join(" "))
    }
    /// Subtitle: the sentence being spoken, split at the current word.
    pub fn subtitle(&self) -> (String, String) {
        let (Some(s), Some(t)) = (self.explainer.sections.get(self.section), self.timings.get(self.section)) else { return (String::new(), String::new()) };
        let spoken = nar::words_at(t, self.time);
        let ends = |w: &String| w.ends_with(['.', '?', '!']);
        let start = (0..spoken).rev().find(|&i| i > 0 && ends(&s.words[i - 1])).unwrap_or(0);
        let start = if spoken > 0 && spoken == s.words.len() { (0..spoken - 1).rev().find(|&i| i > 0 && ends(&s.words[i - 1])).unwrap_or(0) } else { start };
        let end = (spoken.max(1)..=s.words.len()).find(|&i| i == s.words.len() || ends(&s.words[i - 1])).unwrap_or(s.words.len());
        let said = s.words[start..spoken.min(end)].join(" ");
        let rest = s.words[spoken.min(end)..end].join(" ");
        (said, rest)
    }
}

/// The block a target lives in (for scrolling, highlighting and marks).
pub(crate) fn target_block(l: &Learn, t: &Target) -> Option<String> {
    let lesson = l.lesson.as_ref()?;
    match t {
        Target::Top => lesson.blocks.first().map(|b| b.id.clone()),
        Target::Text { quote } => {
            let index = l.index.as_ref()?;
            let text = &index.text.text;
            let at = text.find(&sim_annotate::text::normalize(quote))?;
            index.block_at(text[..at].chars().count()).map(String::from)
        }
        Target::Scene { id } | Target::Compare { id } => Some(id.clone()),
        Target::Block { id } => lesson.headings().find(|(_, _, _, slug)| slug == id).map(|(b, ..)| b.id.clone()).or_else(|| lesson.block(id).map(|b| b.id.clone())),
        Target::Part { .. } | Target::Plot { .. } => l.scene.as_ref().map(|a| a.id.clone()),
        Target::Figure { id, .. } => Some(format!("figure:{id}")),
    }
}

impl Learn {
    /// Load `explainer.md` beside the open lesson, if there is one.
    pub(crate) fn load_narration(&mut self) {
        self.narration = None;
        let Some(lesson) = &self.lesson else { return };
        let path = Explainer::path_for(lesson);
        if !path.is_file() {
            return;
        }
        match Narration::load(&path) {
            Ok(n) => self.narration = Some(n),
            Err(e) => self.status = format!("Explainer: {e}"),
        }
    }

    pub(crate) fn narrate(&mut self, action: NarrateAction) -> Result<(), String> {
        self.dirty = true;
        let n = self.narration.as_mut().ok_or("this lesson has no explainer.md")?;
        match action {
            NarrateAction::Play => {
                if n.time >= n.duration() - 1e-6 && n.section + 1 >= n.explainer.sections.len() {
                    n.jump(0, 0.);
                }
                n.playing = true;
                if n.loaded.is_some() {
                    self.player().play();
                }
            }
            NarrateAction::Pause => {
                n.playing = false;
                self.player().pause();
            }
            NarrateAction::Stop => {
                n.playing = false;
                n.jump(0, 0.);
                self.player().stop();
                self.narration_part = None;
            }
            NarrateAction::Next => {
                // Leaving a section early is a skip (for authors' reports).
                let skipped = (n.time < n.duration() - 0.5).then(|| n.explainer.sections[n.section].id.clone());
                let next = (n.section + 1).min(n.explainer.sections.len() - 1);
                n.jump(next, 0.);
                if let (Some(id), Some(slug)) = (skipped, self.lesson.as_ref().map(|l| l.slug.clone())) {
                    self.progress.block(&slug, &format!("narration:{id}")).narration_skips += 1;
                }
            }
            NarrateAction::Replay => {
                let (s, t) = (n.section, n.sentence_start());
                n.jump(s, t);
                n.playing = true;
            }
            NarrateAction::Prev => {
                let prev = if n.time > 2.0 { n.section } else { n.section.saturating_sub(1) };
                n.jump(prev, 0.);
            }
            NarrateAction::Section { index } => {
                if index >= n.explainer.sections.len() {
                    return Err(format!("no section {index}"));
                }
                n.jump(index, 0.);
                n.playing = true;
            }
            NarrateAction::Seek { time_s } => {
                let s = n.section;
                n.jump(s, time_s);
            }
            NarrateAction::Generate { section } => {
                if n.job.is_some() {
                    return Err("a narration job is already running".into());
                }
                let explainer = n.explainer.clone();
                let sections: Vec<String> = section.iter().cloned().collect();
                let label = match &section {
                    Some(s) => format!("Generating narration for `{s}`"),
                    None => "Generating missing narration".into(),
                };
                // Network requests for minutes: a dedicated thread.
                let work = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "narration generation", move |ctx| {
                    sim_voice::api_key().and_then(|k| sim_voice::Voice::new(&k)).and_then(|voice| {
                        sim_voice::generate(&explainer, &voice, &sim_voice::Request { sections: &sections, force: false, budget_usd: sim_voice::DEFAULT_BUDGET_USD }, &|m| ctx.message(m))
                    })
                });
                n.job = Some(GenJob { label, work });
            }
        }
        Ok(())
    }

    pub(crate) fn player(&mut self) -> &sim_voice::player::Player {
        self.player.get_or_insert_with(sim_voice::player::Player::new)
    }

    /// Run one narration cue.
    fn fire(&mut self, cue: &Cue, scene: &mut SpatialScene, orbit: &mut Orbit) {
        let ready = self.scene.as_ref().is_some_and(|a| a.installed && a.run.is_some());
        let needs_scene = matches!(cue, Cue::Play | Cue::Pause | Cue::Restart | Cue::Seek { .. } | Cue::PlayUntil { .. } | Cue::Speed { .. } | Cue::Camera { .. });
        if needs_scene && !ready && self.scene.is_some() {
            // Hold the narration until the scene can obey.
            if let Some(n) = self.narration.as_mut() {
                match &mut n.wait {
                    Some(Wait::SceneReady(pending)) => pending.push(cue.clone()),
                    _ => n.wait = Some(Wait::SceneReady(vec![cue.clone()])),
                }
            }
            return;
        }
        match cue {
            Cue::Scroll { target } => self.scroll_to = target_block(self, target),
            Cue::Scene { id } => {
                if self.scene.as_ref().is_none_or(|a| &a.id != id) {
                    self.activate(id);
                }
                self.scroll_to = Some(id.clone());
            }
            Cue::Play => self.act(LessonAction::Play, scene),
            Cue::Pause => self.act(LessonAction::Pause, scene),
            Cue::Restart => self.act(LessonAction::Restart, scene),
            Cue::Seek { time_s } => {
                if let Some(a) = self.scene.as_mut() {
                    a.seek(*time_s);
                }
            }
            Cue::Speed { factor } => {
                if let Some(a) = self.scene.as_mut() {
                    a.user_speed = *factor;
                }
            }
            Cue::PlayUntil { time_s } => {
                if let Some(a) = self.scene.as_mut() {
                    if a.time >= *time_s {
                        a.seek(0.);
                    }
                    a.stop_at = Some(*time_s);
                    a.playing = true;
                }
            }
            Cue::Camera { preset, focus } => {
                let spec = CameraSpec { preset: sim_script::presentation::Preset::parse(preset).ok(), focus: focus.clone(), ..Default::default() };
                frame(scene, orbit, &spec);
            }
            Cue::WaitScene => {
                if self.scene.as_ref().is_some_and(|a| a.playing) {
                    if let Some(n) = self.narration.as_mut() {
                        n.wait = Some(Wait::SceneStops);
                    }
                }
            }
            Cue::Quiz { id } => {
                self.scroll_to = Some(id.clone());
                if !self.quiz_done(id) {
                    if let Some(n) = self.narration.as_mut() {
                        n.wait = Some(Wait::Quiz(id.clone()));
                    }
                }
            }
            Cue::View { view: sim_script::presentation::View::Zoom { focus, zoom, seconds } } => {
                let aspect = super::view_aspect(&scene).unwrap_or(1.6);
                let (yaw, pitch) = orbit.heading();
                let pose = crate::view::frame_pose(scene, focus.as_deref(), *zoom as f32, yaw, pitch, aspect);
                orbit.glide_to(pose, *seconds as f32);
            }
            // Other view directives are state: `view_at` folds them each frame.
            Cue::View { .. } => {}
            Cue::Highlight { .. } | Cue::Box { .. } | Cue::Arrow { .. } | Cue::Unmark | Cue::Clear => {}
        }
        self.dirty = true;
    }
}

/// Advance the narration, fire cues, follow waits, poll generation jobs.
pub(super) fn tick(time: Res<Time>, mut learn: ResMut<Learn>, mut scene: ResMut<SpatialScene>, mut orbit: Single<&mut Orbit>) {
    let l = &mut *learn;
    // Explainer and manifest edits reload (manifest: after generation elsewhere).
    if let Some(n) = l.narration.as_mut() {
        if stamp(&n.explainer.path) != n.stamp {
            let (section, t) = (n.section, n.time);
            match Narration::load(&n.explainer.path.clone()) {
                Ok(mut fresh) => {
                    fresh.jump(section, t);
                    fresh.playing = n.playing;
                    *n = fresh;
                    l.status = "Reloaded explainer.md".into();
                }
                Err(e) => {
                    n.stamp = stamp(&n.explainer.path);
                    l.status = format!("Explainer: {e}");
                }
            }
            l.dirty = true;
        } else if stamp(&n.explainer.narration_dir().join("narration.json")) != n.manifest_stamp {
            let (section, t) = (n.section, n.time);
            n.refresh();
            n.jump(section, t);
            l.dirty = true;
        }
        let finished = n.job.as_ref().and_then(|j| j.work.poll());
        if let Some(result) = finished {
            n.last_report = Some(match &result {
                Ok(r) => format!("Generated {} section(s), {:.1} s of audio · estimated ${:.4}{}{}", r.generated.len(), r.audio_seconds, r.estimated_usd, r.measured_usd.map(|m| format!(" · measured ${m:.4}")).unwrap_or_default(), if r.warnings.is_empty() { String::new() } else { format!(" · {}", r.warnings.join("; ")) }),
                Err(e) => format!("Narration: {e}"),
            });
            l.status = n.last_report.clone().unwrap_or_default();
            n.job = None;
            let (section, t) = (n.section, n.time);
            n.refresh();
            n.jump(section, t);
            l.dirty = true;
        }
    }
    if !l.active {
        if let Some(n) = l.narration.as_mut().filter(|n| n.playing) {
            n.playing = false;
            if let Some(p) = &l.player {
                p.pause();
            }
        }
        return;
    }
    let Some(n) = l.narration.as_mut() else { return };
    // Waits.
    let scene_ready = l.scene.as_ref().is_some_and(|a| a.installed && a.run.is_some());
    let scene_playing = l.scene.as_ref().is_some_and(|a| a.playing);
    let mut release: Vec<Cue> = Vec::new();
    let quiz_done = match &n.wait {
        Some(Wait::Quiz(id)) => {
            let id = id.clone();
            let _ = n;
            l.quiz_done(&id)
        }
        _ => false,
    };
    let n = l.narration.as_mut().unwrap();
    if quiz_done {
        n.wait = None;
    }
    match &n.wait {
        Some(Wait::SceneStops) if !scene_playing => n.wait = None,
        Some(Wait::SceneReady(pending)) if scene_ready => {
            release = pending.clone();
            n.wait = None;
        }
        _ => {}
    }
    let holding = n.wait.is_some();
    // The playback rate is part of what is loaded: changing it reloads the audio.
    let rate = l.settings.narration_speed.clamp(0.5, 2.0) as f64;
    let n = l.narration.as_mut().unwrap();
    let key = n.key().map(|k| format!("{k}@{rate:.2}"));
    // Audio follows the play state; the clock follows the audio.
    let (playing, has_audio) = (n.playing && !holding, key.is_some());
    let player_state = l.player.as_ref().map(|p| p.state());
    let n = l.narration.as_mut().unwrap();
    let mut load: Option<(String, Arc<Vec<u8>>, f64, bool)> = None;
    let mut pause_audio = false;
    let mut play_audio = false;
    if has_audio && n.playing {
        if n.loaded != key {
            if let Some(bytes) = n.bytes() {
                load = Some((key.clone().unwrap(), bytes, n.time, playing));
                n.loaded = key.clone();
            }
        } else if let Some(st) = &player_state {
            if st.error.is_none() && st.loaded == key {
                if playing && !st.playing && !st.finished {
                    play_audio = true;
                }
                if !playing && st.playing {
                    pause_audio = true;
                }
            }
        }
    }
    let audio_clock = player_state.as_ref().filter(|st| st.error.is_none() && st.loaded.is_some() && st.loaded == key && n.loaded == key && load.is_none());
    if playing {
        let dt = time.delta_secs_f64();
        n.time = match audio_clock {
            Some(st) if st.finished => n.duration(),
            Some(st) => st.position.max(n.time.min(st.position + 0.25)).min(n.duration()),
            None => (n.time + dt * rate).min(n.duration()),
        };
    }
    // Cues due now.
    let section = n.section;
    let mut due: Vec<Cue> = Vec::new();
    if !holding {
        if let (Some(s), Some(times)) = (n.explainer.sections.get(section), n.times.get(section)) {
            while n.next_cue < s.cues.len() && times[n.next_cue] <= n.time + 1e-9 && n.wait.is_none() {
                let cue = s.cues[n.next_cue].cue.clone();
                n.next_cue += 1;
                let stops = matches!(cue, Cue::WaitScene);
                due.push(cue);
                if stops {
                    break;
                }
            }
        }
    }
    // Section end: advance or stop.
    let mut advanced = false;
    if playing && n.time >= n.duration() - 1e-9 && n.next_cue >= n.explainer.sections.get(section).map(|s| s.cues.len()).unwrap_or(0) {
        if n.section + 1 < n.explainer.sections.len() {
            let next = n.section + 1;
            n.jump(next, 0.);
            advanced = true;
        } else {
            n.playing = false;
        }
    }
    n.marks = n.explainer.sections.get(n.section).zip(n.times.get(n.section)).map(|(s, t)| nar::marks_at(s, t, n.time)).unwrap_or_default();
    let part = n.marks.part.clone();
    if let Some(p) = l.player.as_ref().or(if load.is_some() { None } else { None }) {
        if pause_audio {
            p.pause();
        }
        if play_audio {
            p.play();
        }
    }
    if let Some((k, bytes, at, play)) = load {
        l.player().load_at_rate(&k, bytes, at, play, rate);
    }
    if holding {
        if let Some(p) = &l.player {
            if p.state().playing {
                p.pause();
            }
        }
    }
    for cue in release.iter().chain(due.iter()) {
        l.fire(cue, &mut scene, &mut orbit);
    }
    if l.narration_part != part {
        l.narration_part = part;
    }
    if advanced {
        l.dirty = true;
    }
}
