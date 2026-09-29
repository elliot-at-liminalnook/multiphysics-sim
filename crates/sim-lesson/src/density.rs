//! Concept pacing of a lesson's text: how much a reader meets before being
//! asked to use it. A *segment* is the text between two checkpoints (a
//! question or a reflection). Each new term (written in **bold** where it
//! is introduced) and each equation counts against the segment.
//!
//! The rules follow cognitive-load theory and the multimedia-learning
//! principles: working memory holds only a few new elements at once, so
//! introduce one idea at a time (segmenting), meet the pieces before
//! combining them (pre-training), and use each idea soon after meeting it
//! (retrieval practice). The limits are authoring defaults within that
//! evidence, not measured constants; "Key ideas" (a recap) and "Going
//! further" (optional depth) sections are exempt.
use crate::{BlockKind, Lesson};

#[derive(Clone, Debug, PartialEq)]
pub struct DensityRules {
    /// New bold terms before the reader is asked something.
    pub max_terms: usize,
    /// Equation lines (in plain code blocks) before a check.
    pub max_equations: usize,
    /// New symbols (τ, k, V, ω₀ …) in those equations before a check.
    pub max_symbols: usize,
    /// Words of text before a check.
    pub max_words: usize,
    /// Reading rate for the time estimate, words per minute.
    pub reading_wpm: f64,
}
impl Default for DensityRules {
    fn default() -> Self {
        Self { max_terms: 2, max_equations: 2, max_symbols: 3, max_words: 260, reading_wpm: 150. }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    /// First line of the segment's text.
    pub line: usize,
    pub section: String,
    pub words: usize,
    pub terms: Vec<String>,
    pub equations: usize,
    /// Symbols met for the first time in this segment's equations.
    pub symbols: Vec<String>,
    /// Closed by a question or reflection (false for the lesson's tail).
    pub checked: bool,
}

fn exempt(section: &str) -> bool {
    let s = section.to_lowercase();
    s.contains("key ideas") || s.contains("going further")
}

/// Bold spans that name a term: short, not a sentence or a label with a colon.
fn bold_terms(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("**") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("**") else { break };
        let term = after[..j].trim();
        if !term.is_empty() && term.split_whitespace().count() <= 5 && !term.ends_with(':') && !term.ends_with('.') {
            out.push(term.to_string());
        }
        rest = &after[j + 2..];
    }
    out
}

/// Symbols in an equation line: letters (Latin or Greek) with any
/// subscript, before a trailing comment in parentheses. Function names and
/// units in brackets are not symbols.
fn symbols(line: &str) -> Vec<String> {
    let body = line.split("  (").next().unwrap_or(line);
    let mut out = vec![];
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut Vec<String>| {
        let t = std::mem::take(token);
        let name = t.trim_end_matches(|c: char| c.is_ascii_digit() || ('₀'..='₉').contains(&c) || c == '_');
        if !name.is_empty() && name.chars().count() <= 2 && !["tan", "sin", "cos", "arctan", "max", "min", "exp", "ln"].contains(&name) {
            out.push(t);
        }
    };
    for c in body.chars() {
        if c.is_alphabetic() || ('₀'..='₉').contains(&c) || (c == '_' && !token.is_empty()) || (c.is_ascii_digit() && !token.is_empty()) {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
        }
    }
    flush(&mut token, &mut out);
    out
}

/// Equation lines inside plain (non-`sim-`) code fences, and prose words outside them.
fn prose_and_equations(text: &str) -> (usize, usize) {
    let (mut words, mut equations, mut fenced) = (0, 0, false);
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            equations += usize::from(line.contains('='));
        } else if !line.trim_start().starts_with("![") && !line.trim_start().starts_with('|') {
            words += line.split_whitespace().count();
        }
    }
    (words, equations)
}

/// The lesson's segments, in reading order.
pub fn segments(lesson: &Lesson) -> Vec<Segment> {
    let mut out = vec![];
    let mut current = Segment::default();
    let mut seen = std::collections::BTreeSet::new();
    for b in &lesson.blocks {
        match &b.kind {
            BlockKind::Markdown { text } if !exempt(&b.section) => {
                let text = &crate::refs::plain(text);
                if current.line == 0 {
                    current.line = b.line;
                    current.section = b.section.clone();
                }
                let (w, e) = prose_and_equations(text);
                current.words += w;
                current.equations += e;
                let mut fenced = false;
                for line in text.lines() {
                    if line.trim_start().starts_with("```") {
                        fenced = !fenced;
                    } else if fenced && line.contains('=') {
                        for sym in symbols(line) {
                            if seen.insert(format!("symbol:{sym}")) {
                                current.symbols.push(sym);
                            }
                        }
                    } else if !fenced {
                        // Greek or subscripted symbols named in prose are met there.
                        for sym in symbols(line).into_iter().filter(|t| t.chars().any(|c| ('\u{391}'..='\u{3c9}').contains(&c) || ('₀'..='₉').contains(&c))) {
                            if seen.insert(format!("symbol:{sym}")) {
                                current.symbols.push(sym);
                            }
                        }
                    }
                }
                for t in bold_terms(text) {
                    if seen.insert(t.to_lowercase()) {
                        current.terms.push(t);
                    }
                }
            }
            BlockKind::Quiz(_) | BlockKind::Reflect(_) | BlockKind::Task(_) | BlockKind::Lab(_) => {
                current.checked = true;
                if current.line != 0 {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => {}
        }
    }
    if current.line != 0 {
        out.push(current);
    }
    out
}

/// Stretches that ask the reader to hold too much before using it.
pub fn warnings(lesson: &Lesson, rules: &DensityRules) -> Vec<(usize, String)> {
    let mut out = vec![];
    for s in segments(lesson) {
        let mut problems = vec![];
        if s.terms.len() > rules.max_terms {
            problems.push(format!("{} new terms ({})", s.terms.len(), s.terms.join(", ")));
        }
        if s.equations > rules.max_equations {
            problems.push(format!("{} equations", s.equations));
        }
        if s.symbols.len() > rules.max_symbols {
            problems.push(format!("{} new symbols ({})", s.symbols.len(), s.symbols.join(" ")));
        }
        if s.words > rules.max_words {
            problems.push(format!("{} words", s.words));
        }
        if !problems.is_empty() {
            out.push((s.line, format!("\"{}\": {} before the next question; split it or add a check (at most {} new terms, {} equations, {} new symbols, {} words)", s.section, problems.join(", "), rules.max_terms, rules.max_equations, rules.max_symbols, rules.max_words)));
        }
    }
    out
}

/// Estimated minutes to work through the lesson: reading, one minute per
/// question and two per reflection (scenes are timed separately).
pub fn reading_minutes(lesson: &Lesson, rules: &DensityRules) -> f64 {
    let mut minutes = 0.;
    for b in &lesson.blocks {
        match &b.kind {
            BlockKind::Markdown { text } => minutes += prose_and_equations(&crate::refs::plain(text)).0 as f64 / rules.reading_wpm,
            BlockKind::Quiz(q) => minutes += if q.kind == crate::quiz::QuizKind::Steps { 1. + 0.5 * q.steps.iter().filter(|s| s.blank()).count() as f64 } else { 1. },
            BlockKind::Reflect(_) => minutes += 2.,
            BlockKind::Task(_) => minutes += 5.,
            BlockKind::Lab(_) => minutes += 3.,
            BlockKind::Equation(_) | BlockKind::Measured(_) => minutes += 0.5,
            _ => {}
        }
    }
    minutes
}

#[cfg(test)]
mod tests {
    use super::*;
    const SRC: &str = "---\ntitle: T\n---\n# T\n\n## Two ideas\n\nA **torque constant** and **back-EMF** and a **budget** arrive at once.\n\n```text\nτ = k i\nV = R i + k w\nw0 = V/k\n```\n\n```sim-quiz\nid: q\nquestion: pick\noptions:\n  - { text: a, correct: true }\n  - { text: b }\n```\n\n## One idea\n\nThe **stall torque** is where it stops.\n\n```sim-reflect\nid: r\nprompt: why?\n```\n\n## Key ideas\n\n- **torque constant**, **everything**, **at once** recapped.\n";

    #[test]
    fn counts_terms_equations_and_words_between_checks() {
        let l = Lesson::parse(std::path::Path::new("x/lesson.md"), SRC).unwrap();
        let s = segments(&l);
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!(s[0].terms, ["torque constant", "back-EMF", "budget"]);
        assert_eq!(s[0].equations, 3);
        assert_eq!(s[0].symbols, ["τ", "k", "i", "V", "R", "w", "w0"]);
        assert!(s[0].checked && s[1].checked);
        assert_eq!(s[1].terms, ["stall torque"]);
        let w = warnings(&l, &DensityRules::default());
        assert_eq!(w.len(), 1);
        assert!(w[0].1.contains("3 new terms") && w[0].1.contains("3 equations") && w[0].1.contains("7 new symbols"), "{}", w[0].1);
        assert_eq!(symbols("τ = k·i          (torque constant k, N·m/A)"), ["τ", "k", "i"]);
        assert_eq!(symbols("tan λ = z₁·m / d₁"), ["λ", "z₁", "m", "d₁"]);
        assert!(reading_minutes(&l, &DensityRules::default()) > 3.);
    }
}
