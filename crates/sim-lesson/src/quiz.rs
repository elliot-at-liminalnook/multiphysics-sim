//! Practice questions inside lessons (`sim-quiz`) and self-explanation
//! prompts (`sim-reflect`).
//!
//! ````text
//! ```sim-quiz
//! id: current-depends
//! question: When the load torque doubles, the motor's current…
//! options:
//!   - { text: doubles, correct: true, feedback: "Yes: τ = k·i, so current follows torque alone." }
//!   - { text: halves, feedback: "The motor slows, but torque still needs current: τ = k·i." }
//! explain: Current is set by the torque the load demands; speed then settles where V = R·i + k·ω.
//! ```
//! ````
//!
//! - `choice`: pick one option; every option can carry feedback that names
//!   the misconception behind it.
//! - `numeric`: type a number (units after it are ignored); `tolerance` is
//!   absolute (`0.1`) or relative (`"5%"`).
//! - `predict`: before a scene may play, commit to a prediction (a number,
//!   or one of the options); after the run the prediction is compared with
//!   the simulated value (`observe` + `reduce` + `window`).
//!
//! - `sketch`: before a scene may play, draw the curve you expect for
//!   `observe` over `window` on axes spanning `range`; after the run the
//!   sketch is drawn over the simulated curve and scored by its RMS gap as a
//!   fraction of the range (`tolerance`, default 0.15).
//!
//! - `steps`: a worked example with some steps left blank; each blank is
//!   a number (with a unit) the reader fills in. Authors fade support by
//!   blanking more steps in successive questions.
//!
//! Any question may add `hints` (a ladder: nudge, key idea, worked step),
//! `pretest: true` (asked before the teaching; never counts as a miss),
//! `concepts` (for mastery), and numeric questions may `vary` their numbers
//! per attempt with `answer_expr` computing the answer. A wrong option may
//! name a `remedy`: a `sim-remedy` block about that misconception.
//!
//! Choice and numeric questions gate the rest of the lesson by default
//! (`gate: false` to only practise); a predict or sketch question locks its scene.
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuizKind {
    #[default]
    Choice,
    Numeric,
    Predict,
    Sketch,
    Steps,
}
impl QuizKind {
    /// Committed before a scene plays, judged against its run.
    pub fn predicts(self) -> bool {
        matches!(self, QuizKind::Predict | QuizKind::Sketch)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuizOption {
    pub text: String,
    #[serde(default)]
    pub correct: bool,
    #[serde(default)]
    pub feedback: String,
    /// The `sim-remedy` block about the misconception behind this option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

/// One step of a `steps` question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// What this step works out (Markdown).
    pub prompt: String,
    /// Shown as done: the working (Markdown). Absent: the reader fills in `answer`.
    #[serde(default)]
    pub worked: Option<String>,
    #[serde(default)]
    pub answer: Option<f64>,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
}
impl Step {
    pub fn blank(&self) -> bool {
        self.worked.is_none()
    }
}

/// A number that changes per attempt: drawn from `min..=max` on `step`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vary {
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub step: Option<f64>,
}

/// How `sim-lesson check` confirms a varied question's formula against the
/// simulation: run `scene` with `set` (values may name the question's
/// variables) and reduce `observe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckWith {
    pub scene: String,
    pub observe: String,
    #[serde(default)]
    pub reduce: Option<crate::Reduce>,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    /// `instance/path.parameter: expression` (e.g. `supply.voltage: V`).
    #[serde(default)]
    pub set: std::collections::BTreeMap<String, String>,
    /// Relative agreement required (default 5 %).
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Tolerance {
    Absolute(f64),
    /// Fraction of the correct value (from "5%").
    Relative(f64),
}
impl<'de> Deserialize<'de> for Tolerance {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(v) if v.is_finite() && v >= 0.0 => Ok(Tolerance::Absolute(v)),
            Raw::S(s) if s.trim().ends_with('%') => s.trim().trim_end_matches('%').trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| Tolerance::Relative(v / 100.0)).ok_or_else(|| serde::de::Error::custom(format!("tolerance `{s}` must be a number or a percentage like \"5%\""))),
            _ => Err(serde::de::Error::custom("tolerance must be ≥ 0, or a percentage like \"5%\"")),
        }
    }
}
impl Tolerance {
    pub fn accepts(self, correct: f64, given: f64) -> bool {
        match self {
            Tolerance::Absolute(t) => (given - correct).abs() <= t + 1e-12,
            Tolerance::Relative(r) => (given - correct).abs() <= r * correct.abs() + 1e-12,
        }
    }
}

fn yes() -> Option<bool> {
    None
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quiz {
    pub id: String,
    #[serde(default)]
    pub kind: QuizKind,
    /// Markdown.
    pub question: String,
    #[serde(default)]
    pub options: Vec<QuizOption>,
    #[serde(default)]
    pub answer: Option<f64>,
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
    #[serde(default)]
    pub unit: String,
    /// Offered after a wrong answer.
    #[serde(default)]
    pub hint: String,
    /// Worked explanation, shown once answered (Markdown).
    #[serde(default)]
    pub explain: String,
    /// Hide the rest of the lesson until answered (default: yes, except predict).
    #[serde(default = "yes")]
    pub gate: Option<bool>,
    /// Include in spaced review (default yes).
    #[serde(default = "yes")]
    pub review: Option<bool>,
    // Predict questions:
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub observe: Option<String>,
    #[serde(default)]
    pub reduce: Option<crate::Reduce>,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    /// Sketch questions: the value axis the reader draws on.
    #[serde(default)]
    pub range: Option<[f64; 2]>,
    /// "Show me": the scene time this question is about, offered as a jump.
    #[serde(default)]
    pub moment: Option<f64>,
    /// A hint ladder, revealed one at a time (`hint` is a one-step ladder).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// Asked before the idea is taught: answered, then shown, never a miss.
    #[serde(default)]
    pub pretest: bool,
    /// Concepts this question practises (default: the lesson's `teaches`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concepts: Vec<String>,
    /// Steps questions: the worked example.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    /// Numbers that change per attempt, used as `{name}` in the question.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub vary: std::collections::BTreeMap<String, Vary>,
    /// The answer from the varied numbers (an expression, e.g. `V / R`).
    #[serde(default)]
    pub answer_expr: Option<String>,
    /// Model values the expression may use: `name: instance/path.parameter`
    /// (read from the lesson's system, never hand-copied).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub given: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub check_with: Option<CheckWith>,
}

/// RMS gap between a sketch (points `[t, v]`) and a recorded series, as a
/// fraction of the axis range, over the sketch's own times.
pub fn sketch_error(sketch: &[[f64; 2]], times: &[f64], values: &[f64], range: [f64; 2]) -> Option<f64> {
    let span = (range[1] - range[0]).abs();
    if sketch.is_empty() || times.len() < 2 || times.len() != values.len() || span <= 0. {
        return None;
    }
    let at = |t: f64| {
        let i = times.partition_point(|x| *x <= t).clamp(1, times.len() - 1);
        let (t0, t1) = (times[i - 1], times[i]);
        let f = if t1 > t0 { ((t - t0) / (t1 - t0)).clamp(0., 1.) } else { 0. };
        values[i - 1] + f * (values[i] - values[i - 1])
    };
    let sum: f64 = sketch.iter().map(|[t, v]| (v - at(*t)).powi(2)).sum();
    Some((sum / sketch.len() as f64).sqrt() / span)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reflect {
    pub id: String,
    /// Markdown.
    pub prompt: String,
    /// An expert's explanation, shown after the reader has written theirs.
    #[serde(default)]
    pub model_answer: String,
    #[serde(default)]
    pub gate: bool,
    /// The ideas a good answer contains, each with cue words; used to show
    /// what the reader's explanation covers (free, instant, on this machine).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_points: Vec<KeyPoint>,
    /// `explain` (a self-explanation) or `recall` (write what you remember,
    /// then tick the key points you had).
    #[serde(default)]
    pub kind: ReflectKind,
    /// Recall prompts: what to recall (a lesson slug or a section title).
    #[serde(default)]
    pub of: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectKind {
    #[default]
    Explain,
    Recall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyPoint {
    pub idea: String,
    /// Words or phrases that show the idea is there; any one counts. Use `a+b`
    /// for "both a and b" (e.g. `back-EMF+zero`).
    #[serde(default)]
    pub cues: Vec<String>,
}

/// Which key points a written explanation covers (cue words, case- and
/// hyphen-insensitive).
pub fn coverage(text: &str, points: &[KeyPoint]) -> Vec<(String, bool)> {
    let norm = |s: &str| s.to_lowercase().replace(['-', '–', '’', '\'', '"'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let t = format!(" {} ", norm(text));
    points
        .iter()
        .map(|p| {
            let hit = p.cues.iter().any(|cue| cue.split('+').all(|part| {
                let part = norm(part);
                !part.is_empty() && (t.contains(&format!(" {part}")) || t.contains(&part))
            }));
            (p.idea.clone(), hit)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Answer {
    Choice(usize),
    Number(f64),
    /// Typed text, possibly with a unit ("24 mN·m").
    Text(String),
    /// A `steps` question: the text typed for each step (worked steps ignored).
    Steps(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    pub correct: bool,
    /// Feedback for this answer (the option's, or a numeric comparison).
    pub feedback: String,
}

/// First number in `text`, ignoring a unit after it ("3.3 A", "4.4e2").
pub fn parse_number(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', "");
    let end = t.char_indices().find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || ((*c == 'e' || *c == 'E') && *i > 0))).map(|(i, _)| i).unwrap_or(t.len());
    let mut s = &t[..end];
    while !s.is_empty() && s.parse::<f64>().is_err() {
        s = &s[..s.len() - 1];
    }
    s.parse().ok().filter(|v: &f64| v.is_finite())
}

impl Quiz {
    pub fn gates(&self) -> bool {
        self.gate.unwrap_or(!self.kind.predicts() && !self.pretest)
    }
    pub fn reviewable(&self) -> bool {
        self.review.unwrap_or(true) && !self.kind.predicts()
    }
    /// The hint ladder (`hints`, else the single `hint`).
    pub fn hint_ladder(&self) -> Vec<String> {
        if self.hints.is_empty() && !self.hint.is_empty() { vec![self.hint.clone()] } else { self.hints.clone() }
    }
    /// This attempt's numbers for a varied question: drawn from `seed`
    /// (the same seed always gives the same numbers).
    pub fn variant(&self, seed: u64) -> std::collections::BTreeMap<String, f64> {
        let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            // splitmix64
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        };
        self.vary
            .iter()
            .map(|(name, v)| {
                let x = v.min + next() * (v.max - v.min);
                let x = match v.step.filter(|s| *s > 0.0) {
                    Some(s) => (v.min + ((x - v.min) / s).round() * s).clamp(v.min, v.max),
                    None => x,
                };
                // Keep printed values tidy: at most 6 significant figures.
                let x = crate::units::significant(x, 6).parse::<f64>().unwrap_or(x);
                (name.clone(), x)
            })
            .collect()
    }
    /// `text` with `{name}` replaced by this attempt's numbers.
    pub fn fill(text: &str, values: &std::collections::BTreeMap<String, f64>) -> String {
        let mut out = text.to_string();
        for (k, v) in values {
            let shown = if v.fract() == 0.0 && v.abs() < 1e9 { format!("{v:.0}") } else { crate::units::significant(*v, 4).trim_end_matches('0').trim_end_matches('.').to_string() };
            out = out.replace(&format!("{{{k}}}"), &shown);
        }
        out
    }
    /// The correct answer: `answer`, or `answer_expr` with the attempt's values.
    pub fn correct_answer(&self, values: &std::collections::BTreeMap<String, f64>) -> Result<Option<f64>, String> {
        match &self.answer_expr {
            Some(e) => sim_script::expr::eval(e, values).map(Some),
            None => Ok(self.answer),
        }
    }
    pub fn tolerance(&self) -> Tolerance {
        self.tolerance.unwrap_or(Tolerance::Relative(0.05))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.question.trim().is_empty() {
            return Err("question is empty".into());
        }
        let correct = self.options.iter().filter(|o| o.correct).count();
        match self.kind {
            QuizKind::Choice => {
                if self.options.len() < 2 {
                    return Err("a choice question needs at least two options".into());
                }
                if correct == 0 {
                    return Err("mark at least one option `correct: true`".into());
                }
            }
            QuizKind::Numeric => {
                if !self.answer.is_some_and(f64::is_finite) && self.answer_expr.is_none() {
                    return Err("a numeric question needs `answer` (or `answer_expr` with `vary`)".into());
                }
            }
            QuizKind::Steps => {
                if self.steps.is_empty() {
                    return Err("a steps question needs `steps`".into());
                }
                if !self.steps.iter().any(Step::blank) {
                    return Err("a steps question needs at least one step without `worked` for the reader to fill in".into());
                }
                for (i, st) in self.steps.iter().enumerate() {
                    if st.blank() && !st.answer.is_some_and(f64::is_finite) {
                        return Err(format!("step {}: a step to fill in needs `answer`", i + 1));
                    }
                    if !st.unit.is_empty() {
                        crate::units::parse_unit(&st.unit).map_err(|e| format!("step {}: unit: {e}", i + 1))?;
                    }
                }
            }
            QuizKind::Sketch => {
                if self.scene.is_none() || self.observe.is_none() {
                    return Err("a sketch question needs `scene` and `observe`".into());
                }
                if !self.range.is_some_and(|[a, b]| a.is_finite() && b.is_finite() && a < b) {
                    return Err("a sketch question needs `range: [low, high]` for its value axis".into());
                }
            }
            QuizKind::Predict => {
                if self.scene.is_none() {
                    return Err("a predict question needs `scene` (the scene it unlocks)".into());
                }
                if self.options.is_empty() && self.observe.is_none() {
                    return Err("a predict question needs `observe` (numeric prediction) or `options`".into());
                }
                if !self.options.is_empty() && correct == 0 {
                    return Err("mark which option the simulation shows (`correct: true`)".into());
                }
            }
        }
        if self.window.is_some_and(|[a, b]| !(a.is_finite() && b.is_finite() && a < b)) {
            return Err("window must be [start, end] with start < end".into());
        }
        if !self.unit.is_empty() && matches!(self.kind, QuizKind::Numeric | QuizKind::Predict) {
            crate::units::parse_unit(&self.unit).map_err(|e| format!("unit: {e}"))?;
        }
        for (name, v) in &self.vary {
            if !(v.min.is_finite() && v.max.is_finite() && v.min <= v.max) || v.step.is_some_and(|s| !(s > 0.0)) {
                return Err(format!("vary {name}: needs min ≤ max and a positive step"));
            }
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Err(format!("vary: `{name}` must be a simple name (letters, digits, _)"));
            }
        }
        for (name, target) in &self.given {
            if !target.contains('.') {
                return Err(format!("given {name}: write instance/path.parameter"));
            }
        }
        if !self.vary.is_empty() && self.answer_expr.is_none() {
            return Err("a question that varies its numbers needs `answer_expr` to compute the answer".into());
        }
        if let Some(e) = &self.answer_expr {
            let names: Vec<&str> = self.vary.keys().chain(self.given.keys()).map(String::as_str).collect();
            sim_script::expr::check(e, &names).map_err(|m| format!("answer_expr: {m}"))?;
        }
        if let Some(c) = &self.check_with {
            let names: Vec<&str> = self.vary.keys().chain(self.given.keys()).map(String::as_str).collect();
            for (k, e) in &c.set {
                sim_script::presentation::split_parameter(k).map_err(|m| format!("check_with set: {m}"))?;
                sim_script::expr::check(e, &names).map_err(|m| format!("check_with set {k}: {m}"))?;
            }
        }
        if self.pretest && self.kind.predicts() {
            return Err("pretest is for choice or numeric questions (predictions already come first)".into());
        }
        Ok(())
    }
    /// Judge an answer. For numeric predictions pass the simulated value.
    pub fn check(&self, answer: &Answer, simulated: Option<f64>) -> Result<Verdict, String> {
        self.check_with_values(answer, simulated, &Default::default())
    }
    /// Judge an answer to this attempt's numbers (`variant`).
    pub fn check_with_values(&self, answer: &Answer, simulated: Option<f64>, values: &std::collections::BTreeMap<String, f64>) -> Result<Verdict, String> {
        if let Answer::Steps(texts) = answer {
            return self.check_steps(texts);
        }
        if let Answer::Text(text) = answer {
            if !self.options.is_empty() {
                return Err("pick one of the options".into());
            }
            let reading = crate::units::read(text, &self.unit)?;
            let target = if self.kind == QuizKind::Predict { simulated } else { self.correct_answer(values)? };
            let mut v = self.check_with_values(&Answer::Number(reading.value), simulated, values)?;
            if let (false, Some(target)) = (v.correct, target) {
                let tol = self.tolerance();
                if let Some(slip) = crate::units::slip(reading.value, target, &self.unit, |x| tol.accepts(target, x)) {
                    v.feedback = format!("{} {slip}", v.feedback);
                }
            }
            return Ok(v);
        }
        if let (Answer::Number(v), true) = (answer, self.options.is_empty()) {
            if self.kind != QuizKind::Predict && self.answer_expr.is_some() {
                let target = self.correct_answer(values)?.ok_or("no answer")?;
                let ok = self.tolerance().accepts(target, *v);
                let rel = if target != 0.0 { format!(" ({:+.0} %)", (v - target) / target.abs() * 100.0) } else { String::new() };
                let feedback = if ok { format!("{v} {} is right (≈ {} {}).", self.unit, crate::units::significant(target, 4), self.unit) } else { format!("Not quite: {v} {} vs {} {}{rel}.", self.unit, crate::units::significant(target, 4), self.unit) };
                return Ok(Verdict { correct: ok, feedback: feedback.replace("  ", " ") });
            }
        }
        match (answer, self.options.is_empty()) {
            (Answer::Choice(i), false) => {
                let o = self.options.get(*i).ok_or("no such option")?;
                Ok(Verdict { correct: o.correct, feedback: o.feedback.clone() })
            }
            (Answer::Number(v), true) => {
                let target = if self.kind == QuizKind::Predict { simulated } else { self.answer };
                let Some(target) = target else {
                    // A prediction before the run: recorded, judged later.
                    return Ok(Verdict { correct: true, feedback: format!("Prediction recorded: {v} {}", self.unit).trim().to_string() });
                };
                let ok = self.tolerance().accepts(target, *v);
                let rel = if target != 0.0 { format!(" ({:+.0} %)", (v - target) / target.abs() * 100.0) } else { String::new() };
                let feedback = if ok { format!("{v} {} is right (≈ {target:.4} {}).", self.unit, self.unit) } else { format!("Not quite: {v} {} vs {target:.4} {}{rel}.", self.unit, self.unit) };
                Ok(Verdict { correct: ok, feedback: feedback.replace("  ", " ") })
            }
            (Answer::Choice(_), true) => Err("this question takes a number".into()),
            (Answer::Number(_), false) => Err("pick one of the options".into()),
            (Answer::Text(_), _) | (Answer::Steps(_), _) => unreachable!("handled above"),
        }
    }
    /// Judge a steps question: every blank step, in order.
    fn check_steps(&self, texts: &[String]) -> Result<Verdict, String> {
        let blanks: Vec<&Step> = self.steps.iter().filter(|s| s.blank()).collect();
        if texts.len() != blanks.len() {
            return Err(format!("fill in all {} step(s)", blanks.len()));
        }
        let mut wrong = Vec::new();
        for (i, (st, text)) in blanks.iter().zip(texts).enumerate() {
            let correct = st.answer.unwrap_or(f64::NAN);
            let tol = st.tolerance.unwrap_or(Tolerance::Relative(0.05));
            match crate::units::read(text, &st.unit) {
                Err(e) => wrong.push(format!("step {}: {e}", i + 1)),
                Ok(r) if tol.accepts(correct, r.value) => {}
                Ok(r) => {
                    let slip = crate::units::slip(r.value, correct, &st.unit, |x| tol.accepts(correct, x)).map(|s| format!(" {s}")).unwrap_or_default();
                    wrong.push(format!("step {}: not quite{slip}", i + 1));
                }
            }
        }
        Ok(if wrong.is_empty() { Verdict { correct: true, feedback: "Every step is right.".into() } } else { Verdict { correct: false, feedback: wrong.join(" · ") } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answers_are_judged_with_tolerance_and_feedback() {
        let q: Quiz = serde_norway::from_str("id: q\nkind: numeric\nquestion: speed?\nanswer: 444\ntolerance: 2%\nunit: rad/s").unwrap();
        q.validate().unwrap();
        assert!(q.check(&Answer::Number(450.0), None).unwrap().correct);
        let v = q.check(&Answer::Number(500.0), None).unwrap();
        assert!(!v.correct && v.feedback.contains("+13 %"), "{}", v.feedback);
        assert_eq!(parse_number(" 4.4e2 rad/s"), Some(440.0));
        assert_eq!(parse_number("3.3A"), Some(3.3));
        assert_eq!(parse_number("-0.5"), Some(-0.5));
        assert_eq!(parse_number("about"), None);
        let c: Quiz = serde_norway::from_str("id: c\nquestion: current?\noptions:\n  - { text: doubles, correct: true, feedback: yes }\n  - { text: halves, feedback: no }").unwrap();
        assert!(c.gates() && c.check(&Answer::Choice(0), None).unwrap().correct);
        assert_eq!(c.check(&Answer::Choice(1), None).unwrap().feedback, "no");
        let p: Quiz = serde_norway::from_str("id: p\nkind: predict\nquestion: speed after?\nscene: s\nobserve: rotor.shaft.speed\nreduce: mean\nwindow: [0.55, 0.6]").unwrap();
        p.validate().unwrap();
        assert!(!p.gates() && !p.reviewable());
        assert!(p.check(&Answer::Number(600.0), None).unwrap().feedback.contains("recorded"));
        assert!(!p.check(&Answer::Number(600.0), Some(444.8)).unwrap().correct);
        let bad: Result<Quiz, _> = serde_norway::from_str("id: b\nquestion: x\ntolerance: lots");
        assert!(bad.is_err());
    }
    #[test]
    fn gates_hide_what_follows_and_predictions_must_precede_their_scene() {
        use crate::Lesson;
        use std::path::Path;
        let src = "---\ntitle: T\nsystems: { m: m.system.json }\n---\nIntro.\n\n```sim-quiz\nid: q1\nquestion: pick\noptions:\n  - { text: a, correct: true }\n  - { text: b }\n```\n\nAfter.\n\n```sim-quiz\nid: p\nkind: predict\nscene: s\nquestion: guess\nobserve: x.y\n```\n\n```sim-scene\nid: s\nsystem: m\n```\n\n```sim-reflect\nid: r\nprompt: why?\ngate: true\n```\n\nEnd.\n";
        let l = Lesson::parse(Path::new("t/lesson.md"), src).unwrap();
        assert_eq!(l.gate(|_| false), Some(2), "blocks after q1 are hidden");
        assert_eq!(l.gate(|id| id == "q1"), Some(6), "predict does not gate; the reflection does");
        assert_eq!(l.gate(|_| true), None);
        assert_eq!(l.prediction_for("s").map(|q| q.id.as_str()), Some("p"));
        let late = src.replace("```sim-quiz\nid: p\nkind: predict\nscene: s\nquestion: guess\nobserve: x.y\n```\n\n```sim-scene\nid: s\nsystem: m\n```", "```sim-scene\nid: s\nsystem: m\n```\n\n```sim-quiz\nid: p\nkind: predict\nscene: s\nquestion: guess\nobserve: x.y\n```");
        let e = Lesson::parse(Path::new("t/lesson.md"), &late).unwrap_err();
        assert!(e.message.contains("must come before"), "{e}");
        let e = Lesson::parse(Path::new("t/lesson.md"), &src.replace("  - { text: a, correct: true }", "  - { text: a }")).unwrap_err();
        assert!(e.message.contains("correct: true") && e.line == 7, "{e}");
    }

    #[test]
    fn typed_answers_take_units_and_name_slips() {
        let q: Quiz = serde_norway::from_str("id: q\nkind: numeric\nquestion: torque?\nanswer: 0.024\nunit: N·m").unwrap();
        q.validate().unwrap();
        assert!(q.check(&Answer::Text("24 mN·m".into()), None).unwrap().correct);
        assert!(q.check(&Answer::Text("0.024".into()), None).unwrap().correct);
        let v = q.check(&Answer::Text("24".into()), None).unwrap();
        assert!(!v.correct && v.feedback.contains("1000 times too large"), "{}", v.feedback);
        assert!(q.check(&Answer::Text("24 V".into()), None).unwrap_err().contains("a voltage"));
        let w: Quiz = serde_norway::from_str("id: w\nkind: numeric\nquestion: speed?\nanswer: 450\nunit: rad/s").unwrap();
        assert!(w.check(&Answer::Text("4297 rpm".into()), None).unwrap().correct, "rpm converts");
        assert!(w.check(&Answer::Text("4297".into()), None).unwrap().feedback.contains("rpm"));
    }
    #[test]
    fn steps_judge_each_blank_and_reflections_show_coverage() {
        let q: Quiz = serde_norway::from_str("id: s\nkind: steps\nquestion: current at speed\nsteps:\n  - { prompt: back-EMF, worked: \"e = k·ω = 0.012 × 500 = 6 V\" }\n  - { prompt: voltage left, answer: 6, unit: V }\n  - { prompt: current, answer: 3.33, unit: A }").unwrap();
        q.validate().unwrap();
        assert!(q.check(&Answer::Steps(vec!["6 V".into(), "3.3 A".into()]), None).unwrap().correct);
        let v = q.check(&Answer::Steps(vec!["6".into(), "3330 A".into()]), None).unwrap();
        assert!(!v.correct && v.feedback.contains("step 2") && v.feedback.contains("1000"), "{}", v.feedback);
        assert!(q.check(&Answer::Steps(vec!["6".into()]), None).is_err());
        let all_worked: Quiz = serde_norway::from_str("id: s\nkind: steps\nquestion: q\nsteps:\n  - { prompt: p, worked: w }").unwrap();
        assert!(all_worked.validate().unwrap_err().contains("at least one step"));
        let points: Vec<KeyPoint> = serde_norway::from_str("- { idea: No back-EMF when stalled, cues: [\"back-emf+zero\", \"no back emf\"] }\n- { idea: Current is V/R, cues: [\"v/r\", \"only the resistance\"] }\n- { idea: Heat grows with i², cues: [\"i²r\", \"i^2\", \"heat\"] }").unwrap();
        let c = coverage("When it is jammed there is no back-EMF, so only the resistance limits the current.", &points);
        assert_eq!(c.iter().map(|(_, hit)| *hit).collect::<Vec<_>>(), vec![true, true, false]);
    }
    #[test]
    fn sketches_score_by_their_gap_to_the_run() {
        let times: Vec<f64> = (0..=100).map(|i| i as f64 * 0.01).collect();
        let values: Vec<f64> = times.iter().map(|t| 5. * (1. - (-t / 0.1f64).exp())).collect();
        let good: Vec<[f64; 2]> = times.iter().step_by(10).zip(values.iter().step_by(10)).map(|(t, v)| [*t, v + 0.1]).collect();
        let flat: Vec<[f64; 2]> = times.iter().step_by(10).map(|t| [*t, 0.]).collect();
        let e = sketch_error(&good, &times, &values, [0., 6.]).unwrap();
        assert!((e - 0.1 / 6.).abs() < 1e-9);
        assert!(sketch_error(&flat, &times, &values, [0., 6.]).unwrap() > 0.5);
        let q: Quiz = serde_norway::from_str("id: s\nkind: sketch\nscene: x\nobserve: y\nquestion: draw\nrange: [0, 6]").unwrap();
        q.validate().unwrap();
        assert!(!q.gates() && !q.reviewable() && q.kind.predicts());
        let bad: Quiz = serde_norway::from_str("id: s\nkind: sketch\nscene: x\nobserve: y\nquestion: draw").unwrap();
        assert!(bad.validate().unwrap_err().contains("range"));
    }
}
