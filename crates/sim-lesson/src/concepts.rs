//! The concept map: `lessons/concepts.yaml` names the ideas lessons teach;
//! each lesson's front matter says which it `teaches` and `needs`; questions
//! practise their lesson's concepts (or their own `concepts`). Mastery of a
//! concept comes from the learner's answers to those questions, and the
//! next lesson is the first unfinished one whose needs are mastered.
//!
//! ```yaml
//! torque-constant: { title: "Torque constant", summary: "τ = k·i" }
//! back-emf: { title: "Back-EMF", summary: "e = k·ω", needs: [torque-constant] }
//! ```
use crate::progress::Progress;
use crate::Lesson;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// Concepts to understand first.
    #[serde(default)]
    pub needs: Vec<String>,
}

pub type ConceptMap = BTreeMap<String, Concept>;

/// `concepts.yaml` in the lessons folder (absent: an empty map).
pub fn load(dir: &Path) -> Result<ConceptMap, String> {
    let path = dir.join("concepts.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(ConceptMap::new()) };
    let map: ConceptMap = serde_norway::from_str(&text).map_err(|e| format!("{}:{}: {e}", path.display(), e.location().map(|l| l.line()).unwrap_or(1)))?;
    for (id, c) in &map {
        if let Some(bad) = c.needs.iter().find(|n| !map.contains_key(*n)) {
            return Err(format!("{}: concept `{id}` needs `{bad}`, which is not defined", path.display()));
        }
    }
    Ok(map)
}

/// Concept IDs a lesson uses that the map does not define.
pub fn unknown(map: &ConceptMap, lesson: &Lesson) -> Vec<String> {
    let mut used: Vec<&String> = lesson.meta.teaches.iter().chain(&lesson.meta.needs).collect();
    used.extend(lesson.quizzes().flat_map(|(_, q)| q.concepts.iter()));
    let mut out: Vec<String> = used.into_iter().filter(|c| !map.contains_key(*c)).cloned().collect();
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mastery {
    pub concept: String,
    pub title: String,
    /// Questions practising it, across lessons.
    pub questions: usize,
    /// Passed without revealing the answer.
    pub solid: usize,
    /// Passed after revealing, or missed in the last review.
    pub shaky: usize,
    /// 0…1: solid / questions (0 when there are none yet).
    pub level: f64,
    /// Lessons that teach it.
    pub taught_by: Vec<String>,
}

/// Mastery of every concept in the map from the learner's records.
pub fn mastery(map: &ConceptMap, lessons: &[Lesson], progress: &Progress) -> Vec<Mastery> {
    let mut out: BTreeMap<String, Mastery> = map.iter().map(|(id, c)| (id.clone(), Mastery { concept: id.clone(), title: c.title.clone(), questions: 0, solid: 0, shaky: 0, level: 0.0, taught_by: vec![] })).collect();
    for l in lessons {
        for id in &l.meta.teaches {
            if let Some(m) = out.get_mut(id) {
                m.taught_by.push(l.slug.clone());
            }
        }
        // Predictions and pretests come before the teaching: not evidence of mastery.
        for (_, q) in l.quizzes().filter(|(_, q)| !q.kind.predicts() && !q.pretest) {
            for c in l.concepts_of(q) {
                let Some(m) = out.get_mut(&c) else { continue };
                m.questions += 1;
                match progress.quiz(&l.slug, &q.id) {
                    Some(r) if r.correct && !r.revealed && r.box_ > 0 => m.solid += 1,
                    Some(r) if r.correct && !r.revealed => m.solid += 1,
                    Some(r) if r.passed() => m.shaky += 1,
                    _ => {}
                }
            }
        }
    }
    out.into_values()
        .map(|mut m| {
            m.level = if m.questions == 0 { 0.0 } else { m.solid as f64 / m.questions as f64 };
            m
        })
        .collect()
}

/// A concept counts as mastered at this level.
pub const MASTERED: f64 = 0.75;

/// The next lesson to read: the first (in reading order) that is not done
/// and whose `needs` are mastered, with the reason; else the first not done.
pub fn next_lesson(lessons: &[Lesson], mastery: &[Mastery], done: impl Fn(&Lesson) -> bool) -> Option<(String, String)> {
    let level = |c: &str| mastery.iter().find(|m| m.concept == c).map(|m| m.level).unwrap_or(0.0);
    let open: Vec<&Lesson> = lessons.iter().filter(|l| !done(l)).collect();
    if let Some(l) = open.iter().find(|l| l.meta.needs.iter().all(|n| level(n) >= MASTERED) && l.meta.requires.iter().all(|r| lessons.iter().find(|x| &x.slug == r).is_none_or(|x| done(x)))) {
        let why = if l.meta.needs.is_empty() { "nothing to master first".to_string() } else { format!("you have mastered what it needs ({})", l.meta.needs.join(", ")) };
        return Some((l.slug.clone(), why));
    }
    open.first().map(|l| {
        let missing: Vec<&String> = l.meta.needs.iter().filter(|n| level(n) < MASTERED).collect();
        (l.slug.clone(), format!("review first: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Mode;
    fn lesson(slug: &str, teaches: &str, needs: &str, quiz: &str) -> Lesson {
        let src = format!("---\ntitle: {slug}\nteaches: [{teaches}]\nneeds: [{needs}]\n---\nText.\n\n```sim-quiz\nid: {quiz}\nquestion: q\noptions:\n  - {{ text: a, correct: true }}\n  - {{ text: b }}\n```\n");
        Lesson::parse(Path::new(&format!("x/{slug}/lesson.md")), &src).unwrap()
    }
    #[test]
    fn mastery_follows_answers_and_picks_the_next_lesson() {
        let map: ConceptMap = serde_norway::from_str("kt: { title: Torque constant }\nemf: { title: Back-EMF, needs: [kt] }").unwrap();
        let a = lesson("motor", "kt", "", "q1");
        let b = lesson("generator", "emf", "kt", "q2");
        let lessons = vec![a, b];
        let mut p = Progress::default();
        let m = mastery(&map, &lessons, &p);
        assert_eq!(m.iter().find(|m| m.concept == "kt").unwrap().level, 0.0);
        assert_eq!(next_lesson(&lessons, &m, |_| false).unwrap().0, "motor");
        p.record("motor", "q1", true, "a", Mode::Learn, true, 100);
        let m = mastery(&map, &lessons, &p);
        assert_eq!(m.iter().find(|m| m.concept == "kt").unwrap().level, 1.0);
        let done = |l: &Lesson| l.slug == "motor";
        let (next, why) = next_lesson(&lessons, &m, done).unwrap();
        assert_eq!(next, "generator");
        assert!(why.contains("kt"));
        assert!(unknown(&map, &lesson("x", "nope", "", "q")).contains(&"nope".to_string()));
    }
}
