//! What authors learn from learners' progress files: where a lesson loses
//! people. Aggregates any number of `progress.json` files (one per learner,
//! shared with the author by choice) against a lesson.
//!
//! Per question: learners, first-try right, eventually right, answers
//! revealed, hints used, confident misses and the misconceptions picked.
//! Per block: time on screen, rewinds and narration skips. Blocks where
//! learners spend far longer than reading needs, or rewind a lot, are
//! flagged as stalls.
use crate::progress::Progress;
use crate::{BlockKind, Lesson};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct QuestionStats {
    pub id: String,
    pub line: usize,
    pub learners: u32,
    pub first_try: u32,
    pub eventually: u32,
    pub revealed: u32,
    pub hints: u32,
    pub confident_misses: u32,
    /// Remedy ID → times picked.
    pub misconceptions: BTreeMap<String, u32>,
}
impl QuestionStats {
    pub fn miss_rate(&self) -> f64 {
        if self.learners == 0 { 0.0 } else { 1.0 - self.first_try as f64 / self.learners as f64 }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct BlockReport {
    pub id: String,
    pub line: usize,
    pub kind: String,
    pub learners: u32,
    /// Median seconds on screen among learners who saw it.
    pub median_seconds: f64,
    pub rewinds: u32,
    pub narration_skips: u32,
    /// Why it looks like a stall, if it does.
    pub stall: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AuthorReport {
    pub lesson: String,
    pub learners: usize,
    pub questions: Vec<QuestionStats>,
    pub blocks: Vec<BlockReport>,
    /// Task ID → (learners who tried, learners who passed, median attempts).
    pub tasks: BTreeMap<String, (u32, u32, f64)>,
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
}

/// Seconds a reader needs for a block: reading at `wpm` (prose), else a floor.
fn expected_seconds(lesson: &Lesson, kind: &BlockKind, wpm: f64) -> f64 {
    match kind {
        BlockKind::Markdown { text } => 60.0 * crate::refs::plain(text).split_whitespace().count() as f64 / wpm,
        BlockKind::Quiz(_) => 60.0,
        BlockKind::Scene(_) => 60.0,
        BlockKind::Task(_) => 300.0,
        _ => {
            let _ = lesson;
            20.0
        }
    }
}

pub fn build(lesson: &Lesson, progress: &[Progress]) -> AuthorReport {
    let slug = &lesson.slug;
    let mine: Vec<&crate::progress::LessonProgress> = progress.iter().filter_map(|p| p.lessons.get(slug)).collect();
    let mut questions = Vec::new();
    for (b, q) in lesson.quizzes() {
        let mut s = QuestionStats { id: q.id.clone(), line: b.line, ..Default::default() };
        for p in &mine {
            let Some(r) = p.quizzes.get(&q.id) else { continue };
            s.learners += 1;
            let learn: Vec<_> = r.history.iter().filter(|a| !a.review).collect();
            if learn.first().is_some_and(|a| a.correct && a.hints == 0) || (r.history.is_empty() && r.correct && r.attempts == 1) {
                s.first_try += 1;
            }
            s.eventually += (r.correct) as u32;
            s.revealed += r.revealed as u32;
            s.hints += r.history.iter().map(|a| a.hints).sum::<u32>();
            s.confident_misses += r.confident_miss as u32;
            for m in &r.remedies {
                *s.misconceptions.entry(m.clone()).or_default() += 1;
            }
        }
        questions.push(s);
    }
    let mut blocks = Vec::new();
    for b in &lesson.blocks {
        let stats: Vec<_> = mine.iter().filter_map(|p| p.blocks.get(&b.id)).collect();
        if stats.is_empty() {
            continue;
        }
        let secs = median(stats.iter().map(|s| s.seconds).collect());
        let rewinds = stats.iter().map(|s| s.rewinds).sum();
        let skips = stats.iter().map(|s| s.narration_skips).sum();
        let expected = expected_seconds(lesson, &b.kind, 200.0);
        let n = stats.len() as u32;
        let stall = if secs > 3.0 * expected.max(10.0) {
            Some(format!("median {secs:.0} s on screen; about {expected:.0} s expected"))
        } else if rewinds as f64 >= 2.0 * n as f64 {
            Some(format!("{rewinds} rewinds by {n} learner(s)"))
        } else {
            None
        };
        blocks.push(BlockReport { id: b.id.clone(), line: b.line, kind: b.kind.name().into(), learners: n, median_seconds: secs, rewinds, narration_skips: skips, stall });
    }
    let mut tasks = BTreeMap::new();
    for b in &lesson.blocks {
        if let BlockKind::Task(t) = &b.kind {
            let tries: Vec<_> = mine.iter().filter_map(|p| p.tasks.get(&t.id)).filter(|a| !a.is_empty()).collect();
            let passed = tries.iter().filter(|a| a.iter().any(|x| x.passed)).count() as u32;
            let attempts = median(tries.iter().map(|a| a.iter().position(|x| x.passed).map(|i| i + 1).unwrap_or(a.len()) as f64).collect());
            tasks.insert(t.id.clone(), (tries.len() as u32, passed, attempts));
        }
    }
    AuthorReport { lesson: slug.clone(), learners: mine.len(), questions, blocks, tasks }
}

/// The report as text for the terminal.
pub fn text(r: &AuthorReport) -> String {
    let mut out = format!("{}: {} learner(s)\n", r.lesson, r.learners);
    out.push_str("\nQuestions (hardest first)\n");
    let mut qs = r.questions.clone();
    qs.sort_by(|a, b| b.miss_rate().total_cmp(&a.miss_rate()));
    for q in &qs {
        if q.learners == 0 {
            out.push_str(&format!("  {:<24} line {:>4}  not answered yet\n", q.id, q.line));
            continue;
        }
        let m = if q.misconceptions.is_empty() { String::new() } else { format!("  misconceptions: {}", q.misconceptions.iter().map(|(k, v)| format!("{k}×{v}")).collect::<Vec<_>>().join(", ")) };
        out.push_str(&format!("  {:<24} line {:>4}  {:>3.0} % missed first try · {} revealed · {} hints · {} confident misses{m}\n", q.id, q.line, q.miss_rate() * 100.0, q.revealed, q.hints, q.confident_misses));
    }
    let stalls: Vec<&BlockReport> = r.blocks.iter().filter(|b| b.stall.is_some()).collect();
    out.push_str(&format!("\nStalls ({})\n", stalls.len()));
    for b in stalls {
        out.push_str(&format!("  {:<10} line {:>4} {:<9} {}\n", b.id, b.line, b.kind, b.stall.clone().unwrap_or_default()));
    }
    let skips: u32 = r.blocks.iter().map(|b| b.narration_skips).sum();
    if skips > 0 {
        out.push_str(&format!("\nNarration skipped {skips} time(s)\n"));
    }
    for (id, (tried, passed, attempts)) in &r.tasks {
        out.push_str(&format!("\nTask {id}: {passed}/{tried} solved, median {attempts:.0} attempt(s)\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{Confidence, Mode};
    #[test]
    fn reports_hard_questions_misconceptions_and_stalls() {
        let src = "---\ntitle: T\n---\nShort text.\n\n```sim-quiz\nid: q\nquestion: pick\noptions:\n  - { text: a, correct: true }\n  - { text: b, remedy: harder }\n```\n\n```sim-remedy\nid: harder\nmisconception: The motor tries harder\nbody: It does not.\n```\n";
        let l = Lesson::parse(std::path::Path::new("x/t/lesson.md"), src).unwrap();
        let mut a = Progress::default();
        a.misconception("t", "q", "harder");
        a.record_with("t", "q", false, "b", Mode::Learn, true, 1, Some(Confidence::Sure));
        a.record_with("t", "q", true, "a", Mode::Learn, true, 2, None);
        a.block("t", "b1").seconds = 400.0;
        a.block("t", "b1").visits = 1;
        let mut b = Progress::default();
        b.record("t", "q", true, "a", Mode::Learn, true, 1);
        b.block("t", "b1").seconds = 380.0;
        let r = build(&l, &[a, b]);
        let q = &r.questions[0];
        assert_eq!((q.learners, q.first_try, q.eventually, q.confident_misses), (2, 1, 2, 1));
        assert_eq!(q.misconceptions["harder"], 1);
        assert!(r.blocks.iter().find(|x| x.id == "b1").unwrap().stall.is_some());
        let t = text(&r);
        assert!(t.contains("50 % missed") && t.contains("harder×1") && t.contains("Stalls (1)"), "{t}");
    }
}
