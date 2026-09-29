//! A learner's progress: answers, predictions, reflections and the spaced
//! review schedule. Per machine and per learner, not lesson content:
//! `runs/lessons/progress.json` (override with `SIM_LESSON_PROGRESS`).
//!
//! Review uses Leitner boxes: a question answered correctly moves up a box
//! and returns after 1, 3, 7, 21 then 60 days; a miss in review drops it to
//! the first box. A revealed answer counts as passed for pacing (the lesson
//! unlocks) but is scheduled for review tomorrow.
//!
//! Support changes the schedule: an answer that needed hints, or that the
//! reader marked as a guess, does not move the question up a box. A miss the
//! reader was sure of is flagged; those are the errors most worth revisiting.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const INTERVAL_DAYS: [u64; 5] = [1, 3, 7, 21, 60];
const DAY: u64 = 86_400;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuizRecord {
    pub attempts: u32,
    pub correct: bool,
    /// The reader asked to see the answer.
    #[serde(default)]
    pub revealed: bool,
    #[serde(default)]
    pub last_answer: String,
    /// Predict questions: the committed prediction (number or option text).
    #[serde(default)]
    pub prediction: Option<String>,
    pub first_at: u64,
    pub last_at: u64,
    /// Leitner box (0 = due soon).
    #[serde(default)]
    pub box_: u8,
    /// Next review time (seconds since the Unix epoch); 0 = not scheduled.
    #[serde(default)]
    pub due: u64,
    #[serde(default)]
    pub reviews: u32,
    /// Hints revealed on the current attempt (reset when it is answered).
    #[serde(default)]
    pub hints_used: u32,
    /// Every attempt, oldest first (at most `HISTORY`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Attempt>,
    /// Misconceptions (remedy IDs) the reader's picks pointed at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remedies: Vec<String>,
    /// Varied questions: the numbers of the current attempt, and its seed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, f64>,
    #[serde(default)]
    pub seed: u64,
    /// A wrong answer the reader was sure of (cleared by a sure, right answer in review).
    #[serde(default)]
    pub confident_miss: bool,
}

/// How sure the reader was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Guess,
    FairlySure,
    Sure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub at: u64,
    pub correct: bool,
    #[serde(default)]
    pub review: bool,
    #[serde(default)]
    pub confidence: Option<Confidence>,
    #[serde(default)]
    pub hints: u32,
    #[serde(default)]
    pub answer: String,
}
pub const HISTORY: usize = 20;

/// Time and interaction on one block (for authors' reports).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockStats {
    /// Seconds the block was on screen.
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub visits: u32,
    /// Scenes: seeks backwards.
    #[serde(default)]
    pub rewinds: u32,
    /// Narration sections skipped before they finished.
    #[serde(default)]
    pub narration_skips: u32,
}

/// One check of a task's sandbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub at: u64,
    pub passed: bool,
    /// Report metrics: (label, value).
    #[serde(default)]
    pub metrics: Vec<(String, f64)>,
    /// Claims that failed, in words.
    #[serde(default)]
    pub failed: Vec<String>,
    /// Content hash of the sandbox that was judged.
    #[serde(default)]
    pub design: String,
}

/// A hardware (or simulated-rig) lab result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabRecord {
    pub at: u64,
    /// "bench" or "simulated rig".
    pub rig: String,
    /// Steady value measured (e.g. rad/s).
    pub value: f64,
    #[serde(default)]
    pub unit: String,
    /// Where the full trace was saved.
    #[serde(default)]
    pub trace: String,
}
impl QuizRecord {
    /// The question no longer holds the lesson back.
    pub fn passed(&self) -> bool {
        self.correct || self.revealed
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reflection {
    pub text: String,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LessonProgress {
    #[serde(default)]
    pub quizzes: BTreeMap<String, QuizRecord>,
    #[serde(default)]
    pub reflections: BTreeMap<String, Reflection>,
    /// Recall prompts: key points the reader ticked as recalled.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recalled: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blocks: BTreeMap<String, BlockStats>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, Vec<TaskAttempt>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labs: BTreeMap<String, Vec<LabRecord>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub lessons: BTreeMap<String, LessonProgress>,
}

pub fn path() -> PathBuf {
    std::env::var_os("SIM_LESSON_PROGRESS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("runs/lessons/progress.json"))
}
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How an answer was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// First time through the lesson (pacing).
    Learn,
    /// A spaced review.
    Review,
}

impl Progress {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Progress { version: 1, lessons: BTreeMap::new() })
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        sim_annotate::store::write_atomic(path, &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
    }
    pub fn quiz(&self, lesson: &str, id: &str) -> Option<&QuizRecord> {
        self.lessons.get(lesson)?.quizzes.get(id)
    }
    pub fn passed(&self, lesson: &str, id: &str) -> bool {
        self.quiz(lesson, id).is_some_and(QuizRecord::passed)
    }
    /// Record an answer and reschedule review.
    pub fn record(&mut self, lesson: &str, id: &str, correct: bool, answer: &str, mode: Mode, reviewable: bool, at: u64) -> &QuizRecord {
        self.record_with(lesson, id, correct, answer, mode, reviewable, at, None)
    }
    /// As [`Progress::record`], with how sure the reader was. Hints used on
    /// this attempt (see [`Progress::hint`]) are taken from the record.
    #[allow(clippy::too_many_arguments)]
    pub fn record_with(&mut self, lesson: &str, id: &str, correct: bool, answer: &str, mode: Mode, reviewable: bool, at: u64, confidence: Option<Confidence>) -> &QuizRecord {
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(id.into()).or_default();
        if r.first_at == 0 {
            r.first_at = at;
        }
        r.attempts += 1;
        r.last_at = at;
        r.last_answer = answer.into();
        // Support (hints, guessing) means the answer was not yet fluent.
        let supported = r.hints_used > 0 || confidence == Some(Confidence::Guess);
        match mode {
            Mode::Learn if !r.passed() => {
                r.correct = correct;
                if correct {
                    // Right the first time, unaided: straight into the second box.
                    r.box_ = if r.attempts == 1 && !supported { 1 } else { 0 };
                }
            }
            Mode::Learn => {}
            Mode::Review => {
                r.reviews += 1;
                r.box_ = match (correct, supported) {
                    (true, false) => (r.box_ + 1).min(INTERVAL_DAYS.len() as u8 - 1),
                    (true, true) => r.box_,
                    (false, _) => 0,
                };
                r.correct |= correct;
            }
        }
        if !correct && confidence == Some(Confidence::Sure) {
            r.confident_miss = true;
        } else if correct && confidence == Some(Confidence::Sure) && mode == Mode::Review {
            r.confident_miss = false;
        }
        r.history.push(Attempt { at, correct, review: mode == Mode::Review, confidence, hints: r.hints_used, answer: answer.into() });
        if r.history.len() > HISTORY {
            r.history.remove(0);
        }
        if correct {
            r.hints_used = 0;
        }
        if reviewable && r.passed() {
            // A confident miss comes back the next day, whatever its box.
            let days = if r.confident_miss { INTERVAL_DAYS[0] } else { INTERVAL_DAYS[r.box_ as usize] };
            r.due = at + days * DAY;
        }
        r
    }
    /// One more hint revealed; returns how many are now shown.
    pub fn hint(&mut self, lesson: &str, id: &str) -> u32 {
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(id.into()).or_default();
        r.hints_used += 1;
        r.hints_used
    }
    /// The option picked pointed at a misconception.
    pub fn misconception(&mut self, lesson: &str, id: &str, remedy: &str) {
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(id.into()).or_default();
        r.remedies.push(remedy.into());
    }
    /// This attempt's numbers for a varied question: the stored ones, or a
    /// fresh draw (a new seed per review) that is stored.
    pub fn variant(&mut self, lesson: &str, q: &crate::quiz::Quiz) -> BTreeMap<String, f64> {
        if q.vary.is_empty() {
            return BTreeMap::new();
        }
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(q.id.clone()).or_default();
        let wanted = seed_for(lesson, &q.id, r.reviews);
        if r.values.is_empty() || r.seed != wanted {
            r.seed = wanted;
            r.values = q.variant(wanted);
        }
        r.values.clone()
    }
    /// Time and interaction on a block.
    pub fn block(&mut self, lesson: &str, block: &str) -> &mut BlockStats {
        self.lessons.entry(lesson.into()).or_default().blocks.entry(block.into()).or_default()
    }
    /// Accuracy by stated confidence: (confidence, answers, correct).
    pub fn calibration(&self) -> Vec<(Confidence, u32, u32)> {
        [Confidence::Guess, Confidence::FairlySure, Confidence::Sure]
            .into_iter()
            .map(|c| {
                let attempts = self.lessons.values().flat_map(|l| l.quizzes.values()).flat_map(|r| r.history.iter()).filter(|a| a.confidence == Some(c));
                let (n, ok) = attempts.fold((0, 0), |(n, ok), a| (n + 1, ok + a.correct as u32));
                (c, n, ok)
            })
            .collect()
    }
    pub fn reveal(&mut self, lesson: &str, id: &str, reviewable: bool, at: u64) {
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(id.into()).or_default();
        r.revealed = true;
        r.box_ = 0;
        r.last_at = at;
        if reviewable {
            r.due = at + INTERVAL_DAYS[0] * DAY;
        }
    }
    pub fn predict(&mut self, lesson: &str, id: &str, prediction: &str, at: u64) {
        let r = self.lessons.entry(lesson.into()).or_default().quizzes.entry(id.into()).or_default();
        r.prediction = Some(prediction.into());
        r.first_at = if r.first_at == 0 { at } else { r.first_at };
        r.last_at = at;
        r.attempts += 1;
    }
    pub fn reflect(&mut self, lesson: &str, id: &str, text: &str, at: u64) {
        self.lessons.entry(lesson.into()).or_default().reflections.insert(id.into(), Reflection { text: text.into(), at });
    }
    /// A review session: the questions due now, interleaved so that no two
    /// in a row come from the same lesson (or the same concept) when another
    /// is available. `concept(lesson, question)` names each one's concept.
    pub fn session(&self, at: u64, concept: impl Fn(&str, &str) -> String) -> Vec<(String, String)> {
        let mut pool = self.due(at);
        let mut out: Vec<(String, String)> = Vec::new();
        while !pool.is_empty() {
            let last = out.last().map(|(l, q)| (l.clone(), concept(l, q)));
            let pick = pool
                .iter()
                .position(|(l, q)| last.as_ref().is_none_or(|(ll, lc)| l != ll && &concept(l, q) != lc))
                .or_else(|| pool.iter().position(|(l, _)| last.as_ref().is_none_or(|(ll, _)| l != ll)))
                .unwrap_or(0);
            out.push(pool.remove(pick));
        }
        out
    }
    /// Questions due for review now: (lesson, question), soonest first.
    pub fn due(&self, at: u64) -> Vec<(String, String)> {
        let mut out: Vec<(u64, String, String)> = self.lessons.iter().flat_map(|(l, p)| p.quizzes.iter().filter(|(_, r)| r.due > 0 && r.due <= at).map(move |(q, r)| (r.due, l.clone(), q.clone()))).collect();
        out.sort();
        out.into_iter().map(|(_, l, q)| (l, q)).collect()
    }
}

/// The seed of a varied question's `reviews`-th attempt.
pub fn seed_for(lesson: &str, id: &str, reviews: u32) -> u64 {
    let h = blake3::hash(format!("{lesson}/{id}/{reviews}").as_bytes());
    u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn support_holds_questions_back_and_confident_misses_return_soon() {
        let mut p = Progress::default();
        let t0 = 1_000_000;
        // Right first time but with a hint: first box, not second.
        p.hint("m", "q");
        p.record_with("m", "q", true, "x", Mode::Learn, true, t0, Some(Confidence::Sure));
        assert_eq!(p.quiz("m", "q").unwrap().box_, 0);
        assert_eq!(p.quiz("m", "q").unwrap().history[0].hints, 1);
        assert_eq!(p.quiz("m", "q").unwrap().hints_used, 0);
        // A guess that is right does not move up in review.
        p.record_with("m", "q", true, "x", Mode::Review, true, t0 + DAY, Some(Confidence::Guess));
        assert_eq!(p.quiz("m", "q").unwrap().box_, 0);
        // Sure but wrong: flagged; after passing it comes back tomorrow.
        p.record_with("m", "r", false, "y", Mode::Learn, true, t0, Some(Confidence::Sure));
        p.record_with("m", "r", true, "x", Mode::Learn, true, t0 + 5, Some(Confidence::FairlySure));
        let r = p.quiz("m", "r").unwrap();
        assert!(r.confident_miss && r.due == t0 + 5 + DAY);
        let cal = p.calibration();
        assert_eq!(cal[2], (Confidence::Sure, 2, 1));
    }
    #[test]
    fn review_sessions_interleave_lessons_and_concepts() {
        let mut p = Progress::default();
        for (l, q, t) in [("a", "a1", 1), ("a", "a2", 2), ("a", "a3", 3), ("b", "b1", 4), ("b", "b2", 5)] {
            p.record("m", "_", true, "", Mode::Learn, false, 0);
            let r = p.lessons.entry(l.into()).or_default().quizzes.entry(q.into()).or_default();
            r.correct = true;
            r.due = t;
        }
        let s = p.session(100, |_, q| q.to_string());
        let lessons: Vec<&str> = s.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(lessons, vec!["a", "b", "a", "b", "a"]);
    }
    #[test]
    fn varied_questions_keep_their_numbers_until_the_next_review() {
        let q: crate::quiz::Quiz = serde_norway::from_str("id: v
kind: numeric
question: I = {V} / {R}?
vary: { V: { min: 6, max: 12, step: 0.5 }, R: { min: 1, max: 3, step: 0.1 } }
answer_expr: V / R").unwrap();
        q.validate().unwrap();
        let mut p = Progress::default();
        let a = p.variant("m", &q);
        assert_eq!(p.variant("m", &q), a, "stable within an attempt");
        assert!(a["V"] >= 6.0 && a["V"] <= 12.0 && (a["V"] * 2.0).fract() == 0.0);
        p.lessons.get_mut("m").unwrap().quizzes.get_mut("v").unwrap().reviews = 1;
        let b = p.variant("m", &q);
        assert_ne!(a, b, "a new draw per review");
        let answer = q.correct_answer(&b).unwrap().unwrap();
        assert!((answer - b["V"] / b["R"]).abs() < 1e-12);
        assert!(crate::quiz::Quiz::fill(&q.question, &b).starts_with("I = "));
    }
    #[test]
    fn leitner_schedule_moves_up_on_success_and_resets_on_a_miss() {
        let mut p = Progress::default();
        let t0 = 1_000_000;
        // Wrong, then right: passed, first box.
        p.record("m", "q", false, "halves", Mode::Learn, true, t0);
        assert!(!p.passed("m", "q"));
        p.record("m", "q", true, "doubles", Mode::Learn, true, t0 + 10);
        assert!(p.passed("m", "q"));
        assert_eq!(p.quiz("m", "q").unwrap().due, t0 + 10 + DAY);
        assert!(p.due(t0 + 100).is_empty());
        assert_eq!(p.due(t0 + 2 * DAY), vec![("m".to_string(), "q".to_string())]);
        // Review right twice: 3 then 7 days; a miss drops back to 1 day but stays passed.
        let t1 = t0 + 2 * DAY;
        p.record("m", "q", true, "doubles", Mode::Review, true, t1);
        assert_eq!(p.quiz("m", "q").unwrap().due, t1 + 3 * DAY);
        p.record("m", "q", true, "doubles", Mode::Review, true, t1 + 4 * DAY);
        assert_eq!(p.quiz("m", "q").unwrap().due, t1 + 4 * DAY + 7 * DAY);
        p.record("m", "q", false, "halves", Mode::Review, true, t1 + 12 * DAY);
        assert_eq!(p.quiz("m", "q").unwrap().due, t1 + 13 * DAY);
        assert!(p.passed("m", "q"));
        // Right first time goes straight to the 3-day box; revealing unlocks but reviews tomorrow.
        p.record("m", "r", true, "x", Mode::Learn, true, t0);
        assert_eq!(p.quiz("m", "r").unwrap().due, t0 + 3 * DAY);
        p.reveal("m", "s", true, t0);
        assert!(p.passed("m", "s") && p.quiz("m", "s").unwrap().due == t0 + DAY);
    }
}
