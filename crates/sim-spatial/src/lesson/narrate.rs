//! Narrated explainers in the Learn screen: play a lesson's `explainer.md`
//! section by section, fire its cues on their words (scroll, highlight,
//! boxes and arrows, scene play/pause/seek, camera), hold the narration
//! while a scene plays (`wait-scene`) or loads, and (re)generate a section's
//! audio in the background through `sim-voice`.
//!
//! The narration clock is the audio's own playback position when a section
//! has audio; otherwise it runs on frame time with subtitles only.
use super::*;
use crate::builder::ui::{BORDER, FAINT, Kit, Look, RAISED, SUBTLE, TEXT, WARN};
use sim_lesson::narration::{self as nar, Cue, Explainer, Manifest, MarkState, Target, Timing, TimingKind};

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

/// Seek bar under the subtitles.
#[derive(Component)]
pub(crate) struct NarrationBar;
#[derive(Component)]
pub(crate) struct NarrationFill;
#[derive(Component)]
pub(crate) struct Subtitle;
/// The whole section's words (the transcript setting).
#[derive(Component)]
pub(crate) struct Transcript;
#[derive(Component)]
pub(crate) struct TranscriptRest;
#[derive(Component)]
pub(crate) struct SubtitleRest;
#[derive(Component)]
pub(crate) struct NarrationTime;
#[derive(Component)]
pub(crate) struct JobProgress;

/// Input: pressing or dragging the narration bar seeks within the section
/// (the same `Seek { time_s }` as REST `lesson_narration`).
pub(super) fn seek(bars: Query<(&Interaction, &bevy::ui::RelativeCursorPosition), With<NarrationBar>>, learn: Res<Learn>, mut out: MessageWriter<Act<super::actions::LessonCommand>>) {
    for (interaction, cursor) in &bars {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(p) = crate::view::cursor_fraction(cursor) else { continue };
        let Some(n) = learn.narration.as_ref() else { continue };
        let time_s = p.x.clamp(0., 1.) as f64 * n.duration();
        out.write(Act::ui(super::actions::LessonCommand::Ui(LessonAction::Narrate(NarrateAction::Seek { time_s }))));
    }
}

/// Height kept free under the reading column for the narration bar.
pub(super) fn reserved(l: &Learn) -> f32 {
    match &l.narration {
        None => 0.,
        Some(n) if n.playing || n.time > 0. || n.section > 0 => 132.,
        Some(_) => 76.,
    }
}

/// The narration bar at the bottom of the reading column.
pub(super) fn bar(commands: &mut Commands, k: &Kit, l: &Learn) {
    use crate::builder::ui::{LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR};
    let Some(n) = &l.narration else { return };
    let started = n.playing || n.time > 0. || n.section > 0;
    commands
        .spawn((
            Node { border_radius: BorderRadius::all(Val::Px(9.)), position_type: PositionType::Absolute, left: Val::Px(LEFT_WIDTH + 24.), right: Val::Px(RIGHT_WIDTH + 24.), bottom: Val::Px(STATUSBAR + 10.), max_height: Val::Px(reserved(l) - 16.), overflow: Overflow::clip(), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::axes(Val::Px(14.), Val::Px(10.)), border: UiRect::all(Val::Px(1.)), ..default() },
            BackgroundColor(Color::srgba(0.07, 0.086, 0.106, 0.96)),
            BorderColor::all(BORDER),
            GlobalZIndex(20),
            super::ui::LearnPanel,
        ))
        .with_children(|b| {
            b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                let act = |a: NarrateAction| LessonAction::Narrate(a);
                r.spawn(k.button(if n.playing { "Pause" } else if started { "Resume" } else { "Listen" }, act(if n.playing { NarrateAction::Pause } else { NarrateAction::Play }), Look::Primary, true));
                r.spawn(k.button("‹", act(NarrateAction::Prev), Look::Ghost, true));
                r.spawn(k.button("›", act(NarrateAction::Next), Look::Ghost, n.section + 1 < n.explainer.sections.len()));
                r.spawn(k.button("Say that again", act(NarrateAction::Replay), Look::Ghost, started));
                let s = &n.explainer.sections[n.section];
                let kind = match n.timings.get(n.section).map(|t| t.kind) {
                    Some(TimingKind::Aligned) => "voice",
                    Some(TimingKind::Estimated) => "voice · estimated timing",
                    _ => "no audio yet · subtitles only",
                };
                // A section whose text changed since its voice was made plays silently: say so plainly.
                let silent = n.explainer.audio(&n.manifest, s).is_none() && n.explainer.sections.iter().any(|x| n.explainer.audio(&n.manifest, x).is_some());
                let kind = if silent { "no voice for this section's current text · subtitles only" } else { kind };
                r.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.text(format!("{} / {} · {}", n.section + 1, n.explainer.sections.len(), s.title), 13., TEXT, 2), k.text(kind, 10.5, if silent { crate::builder::ui::WARN } else { FAINT }, if silent { 1 } else { 0 })]));
                if silent && n.job.is_none() {
                    r.spawn(k.button("Make its voice", act(NarrateAction::Generate { section: Some(s.id.clone()) }), Look::Ghost, true));
                }
                if let Some(w) = &n.wait {
                    r.spawn(k.text(match w { Wait::SceneStops => "waiting for the scene", Wait::SceneReady(_) => "waiting for the scene to load", Wait::Quiz(_) => "your turn: answer the question" }, 11., crate::ACCENT, 1));
                }
                r.spawn((k.text("", 11.5, SUBTLE, 0), NarrationTime));
                r.spawn(k.button("Stop", act(NarrateAction::Stop), Look::Ghost, started));
            });
            if started {
                b.spawn((Node { flex_wrap: FlexWrap::Wrap, ..default() }, children![(Text::new(""), TextFont { font: k.f.medium.clone().into(), font_size: FontSize::Px(15.), ..default() }, TextColor(TEXT), TextLayout::linebreak(bevy::text::LineBreak::WordBoundary), Subtitle, children![(TextSpan::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(15.), ..default() }, TextColor(FAINT), SubtitleRest)])]));
                if l.settings.transcript {
                    b.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), max_height: Val::Px(110.), overflow: Overflow::scroll_y(), padding: UiRect::all(Val::Px(8.)), border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(BORDER))).with_children(|t| {
                        t.spawn((Text::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(13.), ..default() }, TextColor(TEXT), TextLayout::linebreak(bevy::text::LineBreak::WordBoundary), Transcript, children![(TextSpan::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(13.), ..default() }, TextColor(FAINT), TranscriptRest)]));
                    });
                }
                b.spawn((Button, NarrationBar, bevy::ui::RelativeCursorPosition::default(), Node { border_radius: BorderRadius::all(Val::Px(3.)), height: Val::Px(6.), border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(RAISED), BorderColor::all(BORDER)))
                    .with_children(|bar| {
                        bar.spawn((Node { border_radius: BorderRadius::all(Val::Px(3.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(crate::ACCENT), NarrationFill, Pickable::IGNORE));
                    });
            }
        });
}

/// The explainer's sections in the outline, with audio state and generation.
pub(super) fn outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    let Some(n) = &l.narration else { return };
    col.spawn(k.section("Explainer"));
    let plan = sim_voice::plan(&n.explainer);
    let missing: f64 = plan.iter().filter(|p| p.status != sim_voice::Status::Current).map(|p| p.estimated_usd).sum();
    for (i, (item, s)) in plan.iter().zip(&n.explainer.sections).enumerate() {
        let current = i == n.section && (n.playing || n.time > 0.);
        col.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.), flex_shrink: 0., ..default() }).with_children(|r| {
            r.spawn((Node { flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.button(&format!("{}{}", if current { "▸ " } else { "" }, s.title), LessonAction::Narrate(NarrateAction::Section { index: i }), Look::Ghost, true)]));
            let (label, color) = match item.status {
                sim_voice::Status::Current => ("voice", crate::builder::ui::OK),
                sim_voice::Status::Stale => ("changed", WARN),
                sim_voice::Status::Missing => ("silent", FAINT),
            };
            r.spawn(k.text(label, 10.5, color, 1));
            r.spawn(k.button(if item.status == sim_voice::Status::Current { "↻" } else { "Make" }, LessonAction::Narrate(NarrateAction::Generate { section: Some(s.id.clone()) }), Look::Ghost, n.job.is_none()));
        });
    }
    match &n.job {
        Some(j) => {
            col.spawn(k.text(&j.label, 11.5, TEXT, 1));
            col.spawn((k.text("", 11., SUBTLE, 0), JobProgress));
        }
        None if missing > 0. => {
            col.spawn(k.button(&format!("Make missing voice (~${missing:.2})"), LessonAction::Narrate(NarrateAction::Generate { section: None }), Look::Secondary, true));
            col.spawn(k.text("Gemini 3.8 Flash TTS via OpenRouter; timing aligned with whisper-1. Estimated cost; capped at $1 per run.", 10.5, FAINT, 0));
        }
        None => {}
    }
    if let Some(r) = &n.last_report {
        col.spawn(k.text(r, 10.5, SUBTLE, 0));
    }
}

/// Subtitles, clock, progress (no rebuild).
#[allow(clippy::type_complexity)]
pub(super) fn live(
    learn: Res<Learn>,
    mut said: Query<&mut Text, (With<Subtitle>, Without<NarrationTime>, Without<JobProgress>)>,
    mut rest: Query<&mut TextSpan, With<SubtitleRest>>,
    mut clock: Query<&mut Text, (With<NarrationTime>, Without<Subtitle>, Without<JobProgress>)>,
    mut progress: Query<&mut Text, (With<JobProgress>, Without<Subtitle>, Without<NarrationTime>)>,
    mut fill: Query<&mut Node, With<NarrationFill>>,
    mut transcript: Query<&mut Text, (With<Transcript>, Without<Subtitle>, Without<NarrationTime>, Without<JobProgress>)>,
    mut transcript_rest: Query<&mut TextSpan, (With<TranscriptRest>, Without<SubtitleRest>)>,
) {
    let Some(n) = &learn.narration else { return };
    if !transcript.is_empty() {
        let (said, rest) = n.transcript();
        let rest = if said.is_empty() { rest } else { format!(" {rest}") };
        for mut t in &mut transcript {
            if t.0 != said {
                t.0 = said.clone();
            }
        }
        for mut t in &mut transcript_rest {
            if t.0 != rest {
                t.0 = rest.clone();
            }
        }
    }
    let (a, b) = n.subtitle();
    for mut t in &mut said {
        if t.0 != a {
            t.0 = a.clone();
        }
    }
    let b = if a.is_empty() { b } else { format!(" {b}") };
    for mut t in &mut rest {
        if t.0 != b {
            t.0 = b.clone();
        }
    }
    let value = format!("{:.0}:{:02.0} / {:.0}:{:02.0}", (n.time / 60.).floor(), n.time % 60., (n.duration() / 60.).floor(), n.duration() % 60.);
    for mut t in &mut clock {
        if t.0 != value {
            t.0 = value.clone();
        }
    }
    if let Some(j) = &n.job {
        let p = j.progress();
        for mut t in &mut progress {
            if t.0 != p {
                t.0 = p.clone();
            }
        }
    }
    let w = Val::Percent(((n.time / n.duration().max(1e-9)).clamp(0., 1.) * 100.) as f32);
    for mut node in &mut fill {
        if node.width != w {
            node.width = w;
        }
    }
}

/// Screen overlays: text highlight, boxes and arrows, placed every frame
/// on their targets (blocks, charts, scene cards, projected parts).
#[derive(Component)]
pub(crate) struct Overlay;
/// A figure image, by ID, with its own coordinate size.
#[derive(Component)]
pub(crate) struct FigureNode(pub String, pub f32, pub f32);
/// A chart image, by observable key, with its time window.
#[derive(Component)]
pub(crate) struct ChartNode(pub String, pub f64, pub f64);

#[allow(clippy::too_many_arguments)]
pub(super) fn overlay(
    mut commands: Commands,
    learn: Res<Learn>,
    scene: Res<SpatialScene>,
    fonts: Option<Res<crate::builder::ui::UiFonts>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<Orbit>>,
    blocks: Query<(&super::ui::BlockNode, &ComputedNode, &UiGlobalTransform)>,
    charts: Query<(&ChartNode, &ComputedNode, &UiGlobalTransform)>,
    figures: Query<(&FigureNode, &ComputedNode, &UiGlobalTransform)>,
    existing: Query<Entity, With<Overlay>>,
    mut signature: Local<String>,
) {
    let Some(fonts) = fonts else { return };
    let scale = window.scale_factor();
    let rect_of = |node: &ComputedNode, gt: &UiGlobalTransform| Rect::from_center_size(gt.translation / scale, node.size() / scale);
    let block_rect = |id: &str| blocks.iter().find(|(b, n, _)| b.0 == id && n.size().y > 0.).map(|(_, n, g)| rect_of(n, g));
    let page = Rect::new(crate::builder::ui::LEFT_WIDTH, crate::builder::ui::TOPBAR, window.width() - crate::builder::ui::RIGHT_WIDTH, window.height() - crate::builder::ui::STATUSBAR);
    let mut items: Vec<(u8, Rect, String)> = Vec::new(); // 0 highlight, 1 box, 2 arrow
    if let (true, Some(n)) = (learn.active, &learn.narration) {
        let m = &n.marks;
        if let Some(q) = &m.text {
            if let Some(r) = target_block(&learn, &Target::Text { quote: q.clone() }).and_then(|b| block_rect(&b)) {
                items.push((0, r, String::new()));
            }
        }
        if let Some(t) = &m.block {
            if let Some(r) = target_block(&learn, t).and_then(|b| block_rect(&b)) {
                items.push((0, r, String::new()));
            }
        }
        for mark in &m.marks {
            let rect = match &mark.target {
                Target::Part { path } => {
                    let view = scene.learn_view.filter(|v| v.visible.width() > 1.);
                    view.and_then(|v| {
                        let (center, radius) = scene.bounds_of(Some(path));
                        let (cam, gt) = *camera;
                        // Through NDC, which follows the card's partial view.
                        let size = v.visible.size() / scale;
                        let to_screen = |w: Vec3| cam.world_to_ndc(gt, w).map(|n| Vec2::new((n.x + 1.) * 0.5 * size.x, (1. - n.y) * 0.5 * size.y));
                        let c = to_screen(center)?;
                        let edge = to_screen(center + gt.right() * radius.max(0.004))?;
                        let r = (edge - c).length().max(14.);
                        let origin = v.visible.min / scale;
                        let p = origin + c;
                        let inside = Rect::from_corners(v.visible.min / scale, v.visible.max / scale);
                        inside.contains(p).then(|| Rect::from_center_half_size(p, Vec2::splat(r)))
                    })
                }
                Target::Plot { key, window: w } => charts.iter().find(|(c, n, _)| &c.0 == key && n.size().y > 0.).map(|(c, n, g)| {
                    let r = rect_of(n, g);
                    match w {
                        Some([a, b]) => {
                            let span = (c.2 - c.1).max(1e-12);
                            let x0 = r.min.x + r.width() * ((a - c.1) / span).clamp(0., 1.) as f32;
                            let x1 = r.min.x + r.width() * ((b - c.1) / span).clamp(0., 1.) as f32;
                            Rect::new(x0, r.min.y, x1.max(x0 + 4.), r.max.y)
                        }
                        None => r,
                    }
                }),
                Target::Figure { id, region } => figures.iter().find(|(f, n, _)| &f.0 == id && n.size().y > 0.).map(|(f, n, g)| {
                    let r = rect_of(n, g);
                    match region {
                        Some([x, y, w, h]) => {
                            let (sx, sy) = (r.width() / f.1.max(1.), r.height() / f.2.max(1.));
                            Rect::new(r.min.x + x * sx, r.min.y + y * sy, r.min.x + (x + w) * sx, r.min.y + (y + h) * sy)
                        }
                        None => r,
                    }
                }),
                other => target_block(&learn, other).and_then(|b| block_rect(&b)),
            };
            if let Some(r) = rect {
                items.push((if mark.arrow { 2 } else { 1 }, r, mark.label.clone()));
            }
        }
    }
    // Keep only what is on the page (not under the toolbar or panels).
    items.retain(|(_, r, _)| r.max.y > page.min.y && r.min.y < page.max.y);
    let sig = items.iter().map(|(k, r, l)| format!("{k}|{l}|{:.0},{:.0},{:.0},{:.0}", r.min.x, r.min.y, r.max.x, r.max.y)).collect::<Vec<_>>().join(";");
    if *signature == sig {
        return;
    }
    *signature = sig;
    for e in &existing {
        commands.entity(e).despawn();
    }
    let accent = crate::ACCENT;
    let label = |commands: &mut Commands, text: &str, at: Vec2| {
        if text.is_empty() {
            return;
        }
        commands.spawn((
            Overlay,
            Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, left: Val::Px(at.x), top: Val::Px(at.y), padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), ..default() },
            BackgroundColor(accent),
            GlobalZIndex(31),
            Pickable::IGNORE,
            children![(Text::new(text), TextFont { font: fonts.semibold.clone().into(), font_size: FontSize::Px(12.), ..default() }, TextColor(Color::srgb(0.03, 0.09, 0.09)))],
        ));
    };
    let line = |commands: &mut Commands, a: Vec2, b: Vec2, width: f32| {
        let d = b - a;
        commands.spawn((
            Overlay,
            Node { border_radius: BorderRadius::all(Val::Px(width * 0.5)), position_type: PositionType::Absolute, left: Val::Px((a.x + b.x - d.length()) * 0.5), top: Val::Px((a.y + b.y) * 0.5 - width * 0.5), width: Val::Px(d.length()), height: Val::Px(width), ..default() },
            UiTransform::from_rotation(Rot2::radians(d.y.atan2(d.x))),
            BackgroundColor(accent),
            GlobalZIndex(30),
            Pickable::IGNORE,
        ));
    };
    for (kind, r, text) in &items {
        match kind {
            0 => {
                let r = Rect::new(r.min.x - 6., r.min.y - 3., r.max.x + 6., r.max.y + 3.);
                commands.spawn((Overlay, Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, left: Val::Px(r.min.x), top: Val::Px(r.min.y), width: Val::Px(r.width()), height: Val::Px(r.height()), border: UiRect::left(Val::Px(3.)), ..default() }, BackgroundColor(accent.with_alpha(0.10)), BorderColor::all(accent), GlobalZIndex(29), Pickable::IGNORE));
            }
            1 => {
                let r = Rect::new(r.min.x - 5., r.min.y - 5., r.max.x + 5., r.max.y + 5.);
                commands.spawn((Overlay, Node { border_radius: BorderRadius::all(Val::Px(7.)), position_type: PositionType::Absolute, left: Val::Px(r.min.x), top: Val::Px(r.min.y), width: Val::Px(r.width()), height: Val::Px(r.height()), border: UiRect::all(Val::Px(2.5)), ..default() }, BorderColor::all(accent), GlobalZIndex(30), Pickable::IGNORE));
                label(&mut commands, text, Vec2::new(r.min.x, (r.min.y - 24.).max(page.min.y + 4.)));
            }
            _ => {
                // Tail up-left of the target, kept on the page; head on the target's edge.
                let target = r.center();
                let mut tail = Vec2::new(r.min.x - 70., r.min.y - 56.);
                tail = tail.clamp(page.min + Vec2::new(12., 30.), page.max - Vec2::new(12., 12.));
                if tail.distance(target) < 40. {
                    tail = target + Vec2::new(-80., -60.);
                }
                let dir = (target - tail).normalize_or(Vec2::X);
                // Where the ray from the tail enters the target rectangle (padded).
                let pad = Rect::new(r.min.x - 6., r.min.y - 6., r.max.x + 6., r.max.y + 6.);
                let mut head = target;
                for step in 0..400 {
                    let p = tail + dir * step as f32 * 2.;
                    if pad.contains(p) {
                        head = p;
                        break;
                    }
                }
                line(&mut commands, tail, head, 3.);
                let back = -dir * 14.;
                let side = Vec2::new(-dir.y, dir.x) * 7.;
                line(&mut commands, head, head + back + side, 3.);
                line(&mut commands, head, head + back - side, 3.);
                label(&mut commands, text, tail + Vec2::new(-8., -26.));
            }
        }
    }
    let _ = (SUBTLE, TEXT);
}
