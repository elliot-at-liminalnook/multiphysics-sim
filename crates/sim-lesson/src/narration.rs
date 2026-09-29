//! Narrated explainers: a plain-text script beside a lesson
//! (`<slug>/explainer.md`) that is spoken section by section while cues
//! placed between its words drive the lesson: highlight text, draw boxes and
//! arrows around passages, parts, plot regions and scenes, and run, pause
//! and seek simulations.
//!
//! ````text
//! ---
//! voice: Kore
//! style: Warm, unhurried physics teacher talking to a curious engineer.
//! ---
//! ## intro: Two equations
//! [[scroll top]] Every motor you have ever used <short pause> comes down to
//! TWO equations. [[highlight "Current through the winding makes torque"]]
//! Current makes torque ...
//!
//! ## spin-up: Watching it spin up
//! @style: a little brighter
//! [[scene load-step]] Let's watch one. [[play]] [[arrow part:rotor "the rotor"]]
//! The rotor races up ... [[box plot:rotor.shaft.speed@0..0.25 "spin-up"]]
//! [[wait-scene]] And there it settles.
//! ````
//!
//! Spoken text goes to the voice verbatim, including Gemini's inline tags
//! (`<short pause>`, `<sigh>`, …) and CAPITALS for emphasis; tags are hidden
//! from subtitles. A cue fires when the narration reaches the word after it.
//! Audio is generated per section (`sim-voice`) and keyed by
//! [`Section::hash`], so editing cues never needs new audio.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_MODEL: &str = "google/gemini-3.8-flash-tts";
pub const DEFAULT_VOICE: &str = "Kore";
pub const DEFAULT_ALIGN: &str = "openai/whisper-1";
/// Reading pace used when a section has no audio yet (words per minute).
pub const SILENT_WPM: f64 = 150.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_voice")]
    pub voice: String,
    /// Sustained delivery (Gemini `speech_metadata.style`); keep it short.
    #[serde(default)]
    pub style: String,
    /// Transcription model for word timing, or `none` to estimate.
    #[serde(default = "default_align")]
    pub align: String,
    /// Pace for sections without audio.
    #[serde(default)]
    pub pace_wpm: Option<f64>,
}
fn default_model() -> String {
    DEFAULT_MODEL.into()
}
fn default_voice() -> String {
    DEFAULT_VOICE.into()
}
fn default_align() -> String {
    DEFAULT_ALIGN.into()
}

/// What a cue points at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// The top of the lesson.
    Top,
    /// The prose block containing this quote.
    Text { quote: String },
    /// A part (instance path) in the live scene.
    Part { path: String },
    /// A chart of the live scene, optionally a time window of it.
    Plot { key: String, window: Option<[f64; 2]> },
    Scene { id: String },
    Compare { id: String },
    /// A heading (by slug) or embed (by id), e.g. a question.
    Block { id: String },
    /// A figure (by file stem), optionally a region in its own units.
    Figure { id: String, region: Option<[f32; 4]> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cue", rename_all = "snake_case")]
pub enum Cue {
    Scroll { target: Target },
    /// Tint text or glow a part until the next highlight (None clears).
    Highlight { target: Option<Target> },
    Box { target: Target, label: String, for_s: Option<f64> },
    Arrow { target: Target, label: String, for_s: Option<f64> },
    /// Remove boxes and arrows.
    Unmark,
    /// Remove highlights, boxes and arrows.
    Clear,
    /// Make a scene live and bring it into view.
    Scene { id: String },
    Play,
    Pause,
    Restart,
    Seek { time_s: f64 },
    Speed { factor: f64 },
    /// Play the scene and stop it at this time.
    PlayUntil { time_s: f64 },
    Camera { preset: String, focus: Option<String> },
    /// Hold the narration here until the scene stops playing.
    WaitScene,
    /// Bring a question into view and hold the narration until it is answered.
    Quiz { id: String },
    /// Camera and emphasis directive shared with scene scripts.
    View { view: sim_script::presentation::View },
}
impl Cue {
    /// Cues that act once when reached (the rest are state).
    pub fn is_event(&self) -> bool {
        matches!(self, Cue::Scene { .. } | Cue::Play | Cue::Pause | Cue::Restart | Cue::Seek { .. } | Cue::Speed { .. } | Cue::PlayUntil { .. } | Cue::Camera { .. } | Cue::WaitScene | Cue::Scroll { .. } | Cue::Quiz { .. } | Cue::View { view: sim_script::presentation::View::Zoom { .. } })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placed {
    /// Index of the word the cue precedes (== word count at the end).
    pub word: usize,
    pub line: usize,
    pub cue: Cue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub line: usize,
    pub style: Option<String>,
    pub voice: Option<String>,
    /// Exactly what the voice reads (inline tags kept, cues removed).
    pub spoken: String,
    /// Displayed words (tags removed), in order.
    pub words: Vec<String>,
    /// Inline pauses and vocal events: (word index they precede, seconds).
    pub pauses: Vec<(usize, f64)>,
    pub cues: Vec<Placed>,
    /// Audio cache key: model, voice, style and spoken text.
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct Explainer {
    pub path: PathBuf,
    pub meta: Meta,
    pub sections: Vec<Section>,
}

fn err(path: &Path, line: usize, message: impl Into<String>) -> crate::LessonError {
    crate::LessonError { path: path.into(), line, message: message.into() }
}

/// Seconds an inline tag adds when timing is estimated.
fn tag_seconds(tag: &str) -> f64 {
    match tag.trim() {
        "short pause" => 0.4,
        "long pause" => 1.0,
        "breath" | "exhales" => 0.35,
        _ => 0.5,
    }
}

fn number(s: &str, what: &str) -> Result<f64, String> {
    let v: f64 = s.trim_end_matches('s').parse().map_err(|_| format!("{what} must be a number, found `{s}`"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!("{what} must be finite and ≥ 0"));
    }
    Ok(v)
}

/// Split `box part:rotor "the rotor" for=3` into words, keeping quotes.
fn args(s: &str) -> Result<Vec<(String, bool)>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '"' {
            let mut q = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(ch) => q.push(ch),
                    None => return Err("unclosed quote".into()),
                }
            }
            out.push((q, true));
        } else {
            let mut w = String::from(c);
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                w.push(ch);
                chars.next();
            }
            out.push((w, false));
        }
    }
    Ok(out)
}

pub fn parse_target(word: &str, quoted: bool) -> Result<Target, String> {
    if quoted {
        if word.trim().is_empty() {
            return Err("empty quote".into());
        }
        return Ok(Target::Text { quote: word.into() });
    }
    if word == "top" {
        return Ok(Target::Top);
    }
    let (kind, rest) = word.split_once(':').ok_or_else(|| format!("target `{word}`: use \"quoted text\", top, part:, plot:, scene:, compare: or block:"))?;
    if rest.is_empty() {
        return Err(format!("target `{word}` is empty after `{kind}:`"));
    }
    Ok(match kind {
        "part" => Target::Part { path: rest.into() },
        "scene" => Target::Scene { id: rest.into() },
        "compare" => Target::Compare { id: rest.into() },
        "block" | "quiz" => Target::Block { id: rest.into() },
        "figure" => match rest.split_once('@') {
            None => Target::Figure { id: rest.into(), region: None },
            Some((id, r)) => {
                let v: Vec<f32> = r.split(',').map(|x| x.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| format!("figure region `{r}` must be x,y,w,h"))?;
                if v.len() != 4 || v[2] <= 0.0 || v[3] <= 0.0 || v.iter().any(|x| !x.is_finite()) {
                    return Err(format!("figure region `{r}` must be x,y,w,h with positive size"));
                }
                Target::Figure { id: id.into(), region: Some([v[0], v[1], v[2], v[3]]) }
            }
        },
        "plot" => match rest.split_once('@') {
            None => Target::Plot { key: rest.into(), window: None },
            Some((key, w)) => {
                let (a, b) = w.split_once("..").ok_or_else(|| format!("plot window `{w}` must be t0..t1"))?;
                let (a, b) = (number(a, "window start")?, number(b, "window end")?);
                if a >= b {
                    return Err("plot window must start before it ends".into());
                }
                Target::Plot { key: key.into(), window: Some([a, b]) }
            }
        },
        other => return Err(format!("unknown target kind `{other}:` (part, plot, figure, scene, compare, quiz, block)")),
    })
}

pub fn parse_cue(body: &str) -> Result<Cue, String> {
    let a = args(body)?;
    let Some((verb, _)) = a.first() else { return Err("empty cue".into()) };
    let rest = &a[1..];
    let one = |what: &str| -> Result<&(String, bool), String> { rest.first().ok_or_else(|| format!("{verb} needs {what}")) };
    let for_s = || -> Result<Option<f64>, String> { rest.iter().find_map(|(w, q)| (!q).then(|| w.strip_prefix("for=")).flatten()).map(|v| number(v, "for=")).transpose() };
    let label = || rest.iter().skip(1).find(|(_, q)| *q).map(|(w, _)| w.clone()).unwrap_or_default();
    let cue = match verb.as_str() {
        "scroll" => Cue::Scroll { target: parse_target(&one("a target")?.0, one("a target")?.1)? },
        "highlight" => match one("a target or off")? {
            (w, false) if w == "off" => Cue::Highlight { target: None },
            (w, q) => Cue::Highlight { target: Some(parse_target(w, *q)?) },
        },
        "box" | "arrow" => {
            let (w, q) = one("a target")?;
            let target = parse_target(w, *q)?;
            if matches!(target, Target::Top) {
                return Err(format!("{verb} cannot point at top"));
            }
            if verb == "box" {
                Cue::Box { target, label: label(), for_s: for_s()? }
            } else {
                Cue::Arrow { target, label: label(), for_s: for_s()? }
            }
        }
        "unmark" => Cue::Unmark,
        "clear" => Cue::Clear,
        "scene" => Cue::Scene { id: one("a scene id")?.0.clone() },
        "play" => Cue::Play,
        "pause" => Cue::Pause,
        "restart" => Cue::Restart,
        "seek" => Cue::Seek { time_s: number(&one("a time")?.0, "seek time")? },
        "play-until" => Cue::PlayUntil { time_s: number(&one("a time")?.0, "play-until time")? },
        "speed" => {
            let f = number(&one("a factor")?.0, "speed")?;
            if !(f > 0.0 && f <= 100.0) {
                return Err("speed must be in (0, 100]".into());
            }
            Cue::Speed { factor: f }
        }
        "camera" => {
            let preset = one("a preset")?.0.clone();
            sim_script::presentation::Preset::parse(&preset)?;
            Cue::Camera { preset, focus: rest.get(1).map(|w| w.0.clone()) }
        }
        "wait-scene" => Cue::WaitScene,
        "quiz" => Cue::Quiz { id: one("a question id")?.0.clone() },
        "zoom" | "orbit" | "spotlight" | "pin" | "unpin" | "inset" | "xray" | "explode" => {
            use sim_script::presentation::View;
            let off = rest.first().is_some_and(|(w, q)| !q && w == "off");
            let flag = || -> Result<bool, String> {
                match rest.first().map(|(w, _)| w.as_str()) {
                    Some("on") | None => Ok(true),
                    Some("off") => Ok(false),
                    Some(w) => Err(format!("{verb} takes on or off, not `{w}`")),
                }
            };
            // A bare path, or part:path.
            let path = |w: &str| w.strip_prefix("part:").unwrap_or(w).to_string();
            let view = match verb.as_str() {
                "zoom" => {
                    let words: Vec<&(String, bool)> = rest.iter().filter(|(w, q)| *q || !w.starts_with("over=")).collect();
                    let (focus, factor) = match words.as_slice() {
                        [] => (None, 1.),
                        [(w, _)] if w.parse::<f64>().is_ok() => (None, number(w, "zoom")?),
                        [(w, _)] => (Some(path(w)).filter(|p| p != "all"), 1.),
                        [(w, _), (f, _), ..] => (Some(path(w)).filter(|p| p != "all"), number(f, "zoom")?),
                    };
                    let seconds = rest.iter().find_map(|(w, q)| (!q).then(|| w.strip_prefix("over=")).flatten()).map(|v| number(v, "over=")).transpose()?.unwrap_or(1.2);
                    View::Zoom { focus, zoom: factor, seconds }
                }
                "orbit" => View::Orbit { rate: if off { 0. } else { rest.first().map(|(w, _)| number(w, "orbit rate")).transpose()?.unwrap_or(0.2) } },
                "spotlight" => View::Spotlight { paths: if off { vec![] } else { rest.iter().filter(|(_, q)| !q).map(|(w, _)| path(w)).collect() } },
                "pin" => View::Pin { path: path(&one("a part")?.0), label: label() },
                "unpin" => View::Unpin,
                "inset" => View::Inset { path: (!off).then(|| rest.first().map(|(w, _)| path(w))).flatten(), zoom: rest.get(1).map(|(w, _)| number(w, "inset zoom")).transpose()?.unwrap_or(3.) },
                "xray" => View::Xray { on: flag()? },
                _ => View::Explode { on: flag()? },
            };
            view.validate()?;
            Cue::View { view }
        }
        other => return Err(format!("unknown cue `{other}` (scroll, highlight, box, arrow, unmark, clear, scene, play, pause, restart, seek, play-until, speed, camera, wait-scene, quiz, zoom, orbit, spotlight, pin, unpin, inset, xray, explode)")),
    };
    Ok(cue)
}

/// Audio cache key.
pub fn section_hash(meta: &Meta, style: Option<&str>, voice: Option<&str>, spoken: &str) -> String {
    let key = serde_json::json!({"model": meta.model, "voice": voice.unwrap_or(&meta.voice), "style": [meta.style.as_str(), style.unwrap_or("")], "spoken": spoken});
    crate::hash(&key.to_string())
}

impl Explainer {
    /// `explainer.md` beside a lesson.
    pub fn path_for(lesson: &crate::Lesson) -> PathBuf {
        lesson.dir().join("explainer.md")
    }
    pub fn load(path: &Path) -> Result<Self, crate::LessonError> {
        let source = std::fs::read_to_string(path).map_err(|e| err(path, 0, e.to_string()))?;
        Self::parse(path, &source)
    }
    pub fn parse(path: &Path, source: &str) -> Result<Self, crate::LessonError> {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        let mut meta: Meta = serde_norway::from_str("{}").unwrap();
        if lines.first().is_some_and(|l| l.trim_end() == "---") {
            let close = lines.iter().skip(1).position(|l| l.trim_end() == "---").map(|p| p + 1).ok_or_else(|| err(path, 1, "front matter is not closed with `---`"))?;
            let yaml = lines[1..close].join("\n");
            if !yaml.trim().is_empty() {
                meta = serde_norway::from_str(&yaml).map_err(|e| err(path, 1 + e.location().map(|l| l.line()).unwrap_or(1), format!("front matter: {e}")))?;
            }
            i = close + 1;
        }
        if meta.voice.trim().is_empty() || meta.model.trim().is_empty() {
            return Err(err(path, 1, "voice and model must not be empty"));
        }
        if meta.pace_wpm.is_some_and(|w| !(60.0..=400.0).contains(&w)) {
            return Err(err(path, 1, "pace_wpm must be 60–400"));
        }
        let mut sections: Vec<Section> = Vec::new();
        let mut current: Option<(Section, Vec<(usize, String)>)> = None;
        let finish = |cur: Option<(Section, Vec<(usize, String)>)>, sections: &mut Vec<Section>, meta: &Meta| -> Result<(), crate::LessonError> {
            if let Some((mut s, body)) = cur {
                scan(path, &mut s, &body)?;
                if s.words.is_empty() {
                    return Err(err(path, s.line, format!("section `{}` has nothing to say", s.id)));
                }
                s.hash = section_hash(meta, s.style.as_deref(), s.voice.as_deref(), &s.spoken);
                sections.push(s);
            }
            Ok(())
        };
        while i < lines.len() {
            let line = lines[i];
            let number = i + 1;
            if let Some(h) = line.strip_prefix("## ") {
                finish(current.take(), &mut sections, &meta)?;
                let (id, title) = match h.split_once(':') {
                    Some((id, t)) => (id.trim(), t.trim()),
                    None => (h.trim(), ""),
                };
                if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || id.len() > 64 {
                    return Err(err(path, number, format!("section id `{id}` must be letters, digits, - or _ (write `## id: Title`)")));
                }
                if sections.iter().any(|s| s.id == id) {
                    return Err(err(path, number, format!("section `{id}` appears twice")));
                }
                current = Some((Section { id: id.into(), title: if title.is_empty() { id.replace(['-', '_'], " ") } else { title.into() }, line: number, style: None, voice: None, spoken: String::new(), words: vec![], pauses: vec![], cues: vec![], hash: String::new() }, Vec::new()));
            } else if let Some((s, body)) = current.as_mut() {
                let t = line.trim();
                if let Some(v) = t.strip_prefix("@style:") {
                    s.style = Some(v.trim().to_string()).filter(|v| !v.is_empty());
                } else if let Some(v) = t.strip_prefix("@voice:") {
                    s.voice = Some(v.trim().to_string()).filter(|v| !v.is_empty());
                } else if t.starts_with('@') {
                    return Err(err(path, number, format!("unknown setting `{t}` (@style: or @voice:)")));
                } else if !t.starts_with("<!--") {
                    body.push((number, line.to_string()));
                }
            } else if !line.trim().is_empty() {
                return Err(err(path, number, "text before the first `## section` heading"));
            }
            i += 1;
        }
        finish(current.take(), &mut sections, &meta)?;
        if sections.is_empty() {
            return Err(err(path, 1, "an explainer needs at least one `## section`"));
        }
        Ok(Self { path: path.into(), meta, sections })
    }
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }
}

/// Split one section's body into spoken text, words, pauses and cues.
fn scan(path: &Path, s: &mut Section, body: &[(usize, String)]) -> Result<(), crate::LessonError> {
    let mut spoken = String::new();
    for (line, text) in body {
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let cue = rest.find("[[");
            let tag = rest.find('<');
            let next = match (cue, tag) {
                (Some(c), Some(t)) => Some(c.min(t)),
                (a, b) => a.or(b),
            };
            let Some(at) = next else {
                push_words(s, &mut spoken, rest);
                break;
            };
            push_words(s, &mut spoken, &rest[..at]);
            rest = &rest[at..];
            if rest.starts_with("[[") {
                let end = rest.find("]]").ok_or_else(|| err(path, *line, "cue `[[` is never closed with `]]`"))?;
                if rest[2..end].contains("[[") || rest[2..end].contains(']') {
                    return Err(err(path, *line, format!("cue `{}` is never closed with `]]`", rest[..end].split("[[").nth(1).map(|c| format!("[[{c}")).unwrap_or_default().trim())));
                }
                let cue = parse_cue(&rest[2..end]).map_err(|m| err(path, *line, format!("cue [[{}]]: {m}", &rest[2..end])))?;
                s.cues.push(Placed { word: s.words.len(), line: *line, cue });
                rest = &rest[end + 2..];
            } else {
                let end = rest.find('>').ok_or_else(|| err(path, *line, "`<` starts an inline tag that is never closed with `>`"))?;
                let tag = &rest[1..end];
                if tag.trim().is_empty() || tag.len() > 40 {
                    return Err(err(path, *line, format!("inline tag `<{tag}>` looks wrong (Gemini tags are like <short pause> or <sigh>)")));
                }
                s.pauses.push((s.words.len(), tag_seconds(tag)));
                if !spoken.is_empty() && !spoken.ends_with(' ') {
                    spoken.push(' ');
                }
                spoken.push_str(&rest[..=end]);
                rest = &rest[end + 1..];
            }
        }
        if !spoken.ends_with(' ') && !spoken.is_empty() {
            spoken.push(' ');
        }
    }
    s.spoken = spoken.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(())
}

fn push_words(s: &mut Section, spoken: &mut String, text: &str) {
    for w in text.split_whitespace() {
        if !spoken.is_empty() && !spoken.ends_with(' ') {
            spoken.push(' ');
        }
        spoken.push_str(w);
        s.words.push(w.to_string());
    }
}

/// Lowercase letters and digits only, for matching transcripts.
pub fn norm(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingKind {
    /// Word times from a transcript of the audio.
    Aligned,
    /// Spread over the audio's length by word length and pauses.
    Estimated,
    /// No audio: reading pace.
    Silent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timing {
    pub kind: TimingKind,
    pub duration_s: f64,
    /// Start time of each word.
    pub starts: Vec<f64>,
}

fn weight(word: &str) -> f64 {
    let mut w = 0.6 + norm(word).chars().count() as f64 * 0.075;
    if word.ends_with([',', ';', ':']) || word.ends_with('—') {
        w += 0.2;
    }
    if word.ends_with(['.', '?', '!']) {
        w += 0.45;
    }
    w
}

/// Word times spread over `duration` (or, without audio, at `wpm`).
pub fn estimate(s: &Section, duration: Option<f64>, wpm: f64) -> Timing {
    let fixed: f64 = s.pauses.iter().map(|p| p.1).sum();
    let weights: Vec<f64> = s.words.iter().map(|w| weight(w)).collect();
    let total: f64 = weights.iter().sum::<f64>().max(1e-9);
    let (kind, duration) = match duration {
        Some(d) => (TimingKind::Estimated, d),
        None => (TimingKind::Silent, s.words.len() as f64 * 60.0 / wpm * 1.0 + fixed),
    };
    // Scale speech to fill what the pauses leave (pauses shrink if they would not fit).
    let pause_scale = if fixed > 0.8 * duration { 0.8 * duration / fixed } else { 1.0 };
    let per = (duration - fixed * pause_scale).max(0.0) / total;
    let mut t = 0.0;
    let mut starts = Vec::with_capacity(s.words.len());
    for (i, w) in weights.iter().enumerate() {
        t += s.pauses.iter().filter(|p| p.0 == i).map(|p| p.1 * pause_scale).sum::<f64>();
        starts.push(t);
        t += w * per;
    }
    Timing { kind, duration_s: duration, starts }
}

/// Word times from a transcript (word, start, end), matched to the script
/// by a global alignment of normalized words; unmatched words are placed
/// between their matched neighbours.
pub fn align(s: &Section, transcript: &[(String, f64, f64)], duration: f64) -> Timing {
    let a: Vec<String> = s.words.iter().map(|w| norm(w)).collect();
    let b: Vec<String> = transcript.iter().map(|w| norm(&w.0)).collect();
    let (n, m) = (a.len(), b.len());
    if m == 0 {
        return estimate(s, Some(duration), SILENT_WPM);
    }
    // Needleman–Wunsch: match +2 (prefix-equal words +1), gap −1.
    let score = |x: &str, y: &str| -> i32 {
        if x == y {
            2
        } else if !x.is_empty() && !y.is_empty() && (x.starts_with(y) || y.starts_with(x)) {
            1
        } else {
            -1
        }
    };
    let mut dp = vec![vec![0i32; m + 1]; n + 1];
    for i in 0..=n {
        dp[i][0] = -(i as i32);
    }
    for j in 0..=m {
        dp[0][j] = -(j as i32);
    }
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = (dp[i - 1][j - 1] + score(&a[i - 1], &b[j - 1])).max(dp[i - 1][j] - 1).max(dp[i][j - 1] - 1);
        }
    }
    let mut matched: Vec<Option<f64>> = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let s = score(&a[i - 1], &b[j - 1]);
        if dp[i][j] == dp[i - 1][j - 1] + s {
            if s > 0 {
                matched[i - 1] = Some(transcript[j - 1].1);
            }
            i -= 1;
            j -= 1;
        } else if dp[i][j] == dp[i - 1][j] - 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    // Fill gaps by interpolating between known times (monotone).
    let mut starts = vec![0.0; n];
    let known: Vec<(usize, f64)> = matched.iter().enumerate().filter_map(|(i, t)| t.map(|t| (i, t))).collect();
    if known.is_empty() {
        return estimate(s, Some(duration), SILENT_WPM);
    }
    for k in 0..n {
        let before = known.iter().rev().find(|(i, _)| *i <= k);
        let after = known.iter().find(|(i, _)| *i >= k);
        starts[k] = match (before, after) {
            (Some(&(i0, t0)), Some(&(i1, t1))) if i1 > i0 => t0 + (t1 - t0) * (k - i0) as f64 / (i1 - i0) as f64,
            (Some(&(_, t0)), _) if before.unwrap().0 == k => t0,
            (Some(&(i0, t0)), None) => (t0 + (duration - t0) * (k - i0) as f64 / (n - i0) as f64).min(duration),
            (None, Some(&(i1, t1))) => t1 * k as f64 / i1.max(1) as f64,
            (Some(&(_, t0)), Some(_)) => t0,
            (None, None) => 0.0,
        };
    }
    for k in 1..n {
        if starts[k] < starts[k - 1] {
            starts[k] = starts[k - 1];
        }
    }
    Timing { kind: TimingKind::Aligned, duration_s: duration, starts }
}

/// When each cue fires.
pub fn cue_times(s: &Section, t: &Timing) -> Vec<f64> {
    s.cues.iter().map(|c| t.starts.get(c.word).copied().unwrap_or(t.duration_s)).collect()
}

/// A box or arrow on screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mark {
    pub arrow: bool,
    pub target: Target,
    pub label: String,
}

/// Highlights and marks in effect at time `t` of a section.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MarkState {
    pub text: Option<String>,
    pub part: Option<String>,
    pub block: Option<Target>,
    pub marks: Vec<Mark>,
}

/// Boxes and arrows stay at least the shared look time
/// ([`sim_script::pacing::PacingRules::mark_dwell_s`]): an `unmark`, `clear`
/// or short `for` that comes sooner lets a young mark finish its dwell.
pub fn marks_at(s: &Section, times: &[f64], t: f64) -> MarkState {
    let dwell = sim_script::pacing::PacingRules::default().mark_dwell_s;
    let mut st = MarkState::default();
    // Mark, when it appeared, when it goes.
    let mut marks: Vec<(Mark, f64, Option<f64>)> = Vec::new();
    let expire = |marks: &mut Vec<(Mark, f64, Option<f64>)>, at: f64| {
        marks.retain(|(_, shown, _)| at - shown < dwell);
        for (_, shown, until) in marks.iter_mut() {
            *until = Some(until.unwrap_or(f64::INFINITY).min(*shown + dwell));
        }
    };
    for (c, at) in s.cues.iter().zip(times) {
        if *at > t + 1e-9 {
            break;
        }
        match &c.cue {
            Cue::Highlight { target } => {
                st.text = None;
                st.part = None;
                st.block = None;
                match target {
                    Some(Target::Text { quote }) => st.text = Some(quote.clone()),
                    Some(Target::Part { path }) => st.part = Some(path.clone()),
                    Some(other) => st.block = Some(other.clone()),
                    None => {}
                }
            }
            Cue::Box { target, label, for_s } | Cue::Arrow { target, label, for_s } => marks.push((Mark { arrow: matches!(c.cue, Cue::Arrow { .. }), target: target.clone(), label: label.clone() }, *at, for_s.map(|d| at + d.max(dwell)))),
            Cue::Unmark => expire(&mut marks, *at),
            Cue::Clear => {
                expire(&mut marks, *at);
                let kept = std::mem::take(&mut marks);
                st = MarkState::default();
                marks = kept;
            }
            _ => {}
        }
    }
    st.marks = marks.into_iter().filter(|(_, _, until)| until.is_none_or(|u| t < u)).map(|(m, _, _)| m).collect();
    st
}

/// View directives in effect at time `t` of a section (zoom: the latest glide).
pub fn view_at(s: &Section, times: &[f64], t: f64) -> sim_script::presentation::ViewState {
    let mut v = sim_script::presentation::ViewState::default();
    for (c, at) in s.cues.iter().zip(times) {
        if *at > t + 1e-9 {
            break;
        }
        if let Cue::View { view } = &c.cue {
            v.apply(*at, view);
        }
    }
    v
}

/// Words spoken by time `t` (for subtitles).
pub fn words_at(timing: &Timing, t: f64) -> usize {
    timing.starts.partition_point(|s| *s <= t + 1e-9)
}

/// Check cue targets against the lesson: quotes exist in its text, scene,
/// compare and block IDs exist, and scene commands follow a `scene` cue.
/// (Part paths and plot keys are checked against systems by `sim-lesson check`.)
pub fn check(explainer: &Explainer, lesson: &crate::Lesson) -> Vec<crate::LessonError> {
    let index = lesson.text_index();
    let text = sim_annotate::text::normalize(&index.text.text);
    let mut out = Vec::new();
    let mut scene: Option<String> = lesson.scenes().next().map(|(_, s)| s.id.clone());
    let figures: Vec<String> = lesson.figures().into_iter().map(|f| f.id).collect();
    let blocks: Vec<String> = lesson.headings().map(|(_, _, _, slug)| slug.to_string()).chain(lesson.blocks.iter().filter_map(|b| b.embed_id().map(String::from))).collect();
    for s in &explainer.sections {
        for c in &s.cues {
            let mut targets = Vec::new();
            match &c.cue {
                Cue::Scroll { target } | Cue::Box { target, .. } | Cue::Arrow { target, .. } => targets.push(target),
                Cue::Highlight { target: Some(t) } => targets.push(t),
                Cue::Scene { id } => {
                    if lesson.scene(id).is_none() {
                        out.push(err(&explainer.path, c.line, format!("no scene `{id}` in the lesson")));
                    }
                    scene = Some(id.clone());
                }
                Cue::Play | Cue::Pause | Cue::Restart | Cue::Seek { .. } | Cue::PlayUntil { .. } | Cue::WaitScene if scene.is_none() => out.push(err(&explainer.path, c.line, "the lesson has no scene to control")),
                Cue::Quiz { id } if lesson.quiz(id).is_none() => out.push(err(&explainer.path, c.line, format!("no sim-quiz `{id}` in the lesson"))),
                _ => {}
            }
            for t in targets {
                let problem = match t {
                    Target::Text { quote } if !text.contains(&sim_annotate::text::normalize(quote)) => Some(format!("quote \"{quote}\" is not in the lesson text")),
                    Target::Scene { id } if lesson.scene(id).is_none() => Some(format!("no scene `{id}`")),
                    Target::Compare { id } if !lesson.compares().any(|(_, c)| &c.id == id) => Some(format!("no comparison `{id}`")),
                    Target::Block { id } if !blocks.contains(id) => Some(format!("no heading or embed `{id}` ({})", blocks.join(", "))),
                    Target::Figure { id, .. } if !figures.contains(id) => Some(format!("no figure `{id}` ({})", figures.join(", "))),
                    _ => None,
                };
                if let Some(p) = problem {
                    out.push(err(&explainer.path, c.line, p));
                }
            }
        }
    }
    out
}

/// Generated audio for an explainer: `narration/narration.json` beside it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub sections: std::collections::BTreeMap<String, Entry>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// [`Section::hash`] the audio was made from.
    pub hash: String,
    /// WAV file, relative to the narration folder.
    pub file: String,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub timing: TimingKind,
    /// Transcript words with start and end times (aligned timing).
    #[serde(default)]
    pub words: Vec<(String, f64, f64)>,
    pub model: String,
    pub voice: String,
    #[serde(default)]
    pub align_model: Option<String>,
    /// Seconds since the Unix epoch.
    pub created_at: String,
    pub chars: usize,
    /// Why timing is estimated, if it is.
    #[serde(default)]
    pub note: String,
}
pub const MANIFEST_VERSION: u32 = 1;

impl Explainer {
    pub fn narration_dir(&self) -> PathBuf {
        self.path.parent().unwrap_or(Path::new(".")).join("narration")
    }
    pub fn manifest(&self) -> Manifest {
        std::fs::read(self.narration_dir().join("narration.json")).ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok()).filter(|m| m.version == MANIFEST_VERSION).unwrap_or(Manifest { version: MANIFEST_VERSION, sections: Default::default() })
    }
    /// The section's audio entry, if it matches the current text and voice.
    pub fn audio<'m>(&self, manifest: &'m Manifest, section: &Section) -> Option<&'m Entry> {
        manifest.sections.get(&section.id).filter(|e| e.hash == section.hash && self.narration_dir().join(&e.file).is_file())
    }
    /// Word timing for a section: aligned transcript, else estimated over
    /// its audio, else silent at the reading pace.
    pub fn timing(&self, manifest: &Manifest, section: &Section) -> Timing {
        match self.audio(manifest, section) {
            Some(e) if e.timing == TimingKind::Aligned && !e.words.is_empty() => align(section, &e.words, e.duration_s),
            Some(e) => estimate(section, Some(e.duration_s), SILENT_WPM),
            None => estimate(section, None, self.meta.pace_wpm.unwrap_or(SILENT_WPM)),
        }
    }
}
impl Manifest {
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        sim_annotate::store::write_atomic(&dir.join("narration.json"), &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SRC: &str = "---\nvoice: Kore\nstyle: warm teacher\n---\n## intro: Two equations\n[[scroll top]] Every motor <short pause> comes down to TWO equations.\n[[highlight \"two equations\"]] Current makes torque.\n\n## spin: Spin-up\n@style: brighter\n[[scene s1]] Watch. [[play]] [[arrow part:rotor \"the rotor\"]] It races up, [[box plot:rotor.shaft.speed@0..0.25 \"spin-up\" for=2]] then settles. [[wait-scene]]\n";
    #[test]
    fn parses_sections_cues_tags_and_hashes() {
        let e = Explainer::parse(Path::new("x/explainer.md"), SRC).unwrap();
        assert_eq!(e.sections.len(), 2);
        let a = &e.sections[0];
        assert_eq!(a.title, "Two equations");
        assert_eq!(a.spoken, "Every motor <short pause> comes down to TWO equations. Current makes torque.");
        assert_eq!(a.words.len(), 10);
        assert_eq!(a.pauses, vec![(2, 0.4)]);
        assert_eq!(a.cues[1].word, 7);
        let b = &e.sections[1];
        assert_eq!(b.style.as_deref(), Some("brighter"));
        assert!(matches!(&b.cues[3].cue, Cue::Box { target: Target::Plot { window: Some([0.0, 0.25]), .. }, label, for_s: Some(2.0) } if label == "spin-up"));
        // Cue edits keep the audio key; word edits change it.
        let moved = Explainer::parse(Path::new("x/explainer.md"), &SRC.replace("[[play]] ", "")).unwrap();
        assert_eq!(moved.sections[1].hash, b.hash);
        let reworded = Explainer::parse(Path::new("x/explainer.md"), &SRC.replace("Watch.", "Look.")).unwrap();
        assert_ne!(reworded.sections[1].hash, b.hash);
        assert_eq!(reworded.sections[0].hash, a.hash);
    }
    #[test]
    fn errors_name_lines() {
        let e = Explainer::parse(Path::new("x/explainer.md"), &SRC.replace("[[play]]", "[[jump]]")).unwrap_err();
        assert_eq!(e.line, 11);
        assert!(e.message.contains("unknown cue `jump`"));
        let e = Explainer::parse(Path::new("x/explainer.md"), &SRC.replace("[[scene s1]]", "[[scene s1]")).unwrap_err();
        assert_eq!(e.line, 11);
        assert!(Explainer::parse(Path::new("x/explainer.md"), "Hello\n## a\nhi").is_err());
    }
    #[test]
    fn alignment_follows_transcript_and_fills_gaps() {
        let e = Explainer::parse(Path::new("x/explainer.md"), SRC).unwrap();
        let s = &e.sections[0];
        // "TWO" transcribed as "2" and "torque" missing: neighbours bound them.
        let tr: Vec<(String, f64, f64)> = [("Every", 0.0), ("motor", 0.8), ("comes", 2.9), ("down", 3.6), ("to", 4.0), ("2", 4.6), ("equations.", 5.1), ("Current", 6.2), ("makes", 6.7)].iter().map(|(w, t)| (w.to_string(), *t, *t + 0.4)).collect();
        let t = align(s, &tr, 8.0);
        assert_eq!(t.kind, TimingKind::Aligned);
        assert_eq!(t.starts[2], 2.9);
        assert!(t.starts[5] > 4.0 && t.starts[5] < 5.1, "{:?}", t.starts);
        assert!(t.starts[9] >= 6.7 && t.starts[9] < 8.0);
        let times = cue_times(s, &t);
        assert_eq!(times[1], 6.2);
        let est = estimate(s, Some(8.0), SILENT_WPM);
        assert!(est.starts.windows(2).all(|w| w[0] <= w[1]) && *est.starts.last().unwrap() < 8.0);
        let silent = estimate(s, None, 150.0);
        assert_eq!(silent.kind, TimingKind::Silent);
    }
    #[test]
    fn marks_follow_time_expire_and_clear() {
        let e = Explainer::parse(Path::new("x/explainer.md"), SRC).unwrap();
        let s = &e.sections[1];
        let times = vec![0.0, 1.0, 1.0, 2.0, 5.0];
        let m = marks_at(s, &times, 2.5);
        assert_eq!(m.marks.len(), 2);
        assert!(m.marks[0].arrow);
        assert_eq!(marks_at(s, &times, 4.5).marks.len(), 1, "the box lasts 2 s");
        assert_eq!(marks_at(s, &times, 0.5).marks.len(), 0);
    }
    #[test]
    fn marks_stay_long_enough_to_find() {
        let src = "## a: A\n[[box part:rotor \"rotor\"]] One [[unmark]] two [[box part:load \"load\" for=0.5]] three [[clear]] four.\n";
        let e = Explainer::parse(Path::new("x/explainer.md"), src).unwrap();
        let s = &e.sections[0];
        // Box at 0, unmark at 0.5, second box at 1.0 (for 0.5 s), clear at 1.2.
        let times = vec![0.0, 0.5, 1.0, 1.2];
        let dwell = sim_script::pacing::PacingRules::default().mark_dwell_s;
        assert_eq!(marks_at(s, &times, 0.9).marks.len(), 1, "an early unmark lets the rotor box finish its dwell");
        assert_eq!(marks_at(s, &times, 1.5).marks.len(), 2);
        assert_eq!(marks_at(s, &times, dwell + 0.01).marks.len(), 1, "the rotor box goes after its dwell");
        assert_eq!(marks_at(s, &times, 1.0 + dwell + 0.01).marks.len(), 0, "a short for= is lengthened to the dwell");
    }
    #[test]
    fn figure_regions_and_quiz_holds_parse() {
        assert_eq!(parse_cue("box figure:torque-speed@60,52,40,40 \"stall\"").unwrap(), Cue::Box { target: Target::Figure { id: "torque-speed".into(), region: Some([60., 52., 40., 40.]) }, label: "stall".into(), for_s: None });
        assert_eq!(parse_cue("arrow figure:buck \"x\"").unwrap(), Cue::Arrow { target: Target::Figure { id: "buck".into(), region: None }, label: "x".into(), for_s: None });
        assert_eq!(parse_cue("scroll quiz:q1").unwrap(), Cue::Scroll { target: Target::Block { id: "q1".into() } });
        assert_eq!(parse_cue("quiz q1").unwrap(), Cue::Quiz { id: "q1".into() });
        assert!(parse_cue("box figure:a@1,2,0,4").is_err());
        assert!(parse_cue("box figure:a@1,2").is_err());
    }

    #[test]
    fn view_cues_parse_and_fold() {
        use sim_script::presentation::View;
        assert_eq!(parse_cue("zoom motor 2 over=1.5").unwrap(), Cue::View { view: View::Zoom { focus: Some("motor".into()), zoom: 2., seconds: 1.5 } });
        assert_eq!(parse_cue("zoom all").unwrap(), Cue::View { view: View::Zoom { focus: None, zoom: 1., seconds: 1.2 } });
        assert_eq!(parse_cue("orbit off").unwrap(), Cue::View { view: View::Orbit { rate: 0. } });
        assert_eq!(parse_cue("pin part:gearbox/worm \"the worm\"").unwrap(), Cue::View { view: View::Pin { path: "gearbox/worm".into(), label: "the worm".into() } });
        assert_eq!(parse_cue("inset gearbox 2.5").unwrap(), Cue::View { view: View::Inset { path: Some("gearbox".into()), zoom: 2.5 } });
        assert_eq!(parse_cue("xray on").unwrap(), Cue::View { view: View::Xray { on: true } });
        assert!(parse_cue("explode maybe").is_err());
        assert!(parse_cue("orbit 9").is_err());
        let e = Explainer::parse(Path::new("x/explainer.md"), "## a: A\n[[spotlight motor]] One [[pin part:rotor \"r\"]] two [[spotlight off]] three.\n").unwrap();
        let s = &e.sections[0];
        let times = vec![0.0, 1.0, 2.0];
        assert_eq!(view_at(s, &times, 1.5).spotlight, ["motor"]);
        assert_eq!(view_at(s, &times, 1.5).pins.len(), 1);
        assert!(view_at(s, &times, 2.5).spotlight.is_empty());
    }
}
