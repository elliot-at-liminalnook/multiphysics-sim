//! What lesson annotations point at: a quoted passage of prose (re-attached
//! after edits by `sim_annotate::text`) or a scene, optionally a part in it
//! and a moment of its run.
use crate::{BlockKind, Lesson};
use serde::{Deserialize, Serialize};
use sim_annotate::text::{TextAnchor, TextIndex, normalize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LessonAnchor {
    Text {
        quote: TextAnchor,
    },
    Scene {
        scene: String,
        /// Instance path inside the scene's system.
        #[serde(default)]
        part: Option<String>,
        /// Simulated time the note refers to.
        #[serde(default)]
        time_s: Option<f64>,
        #[serde(default)]
        missing: bool,
    },
}

/// Longest passage captured when a whole block is annotated.
pub const BLOCK_QUOTE: usize = 280;

pub struct LessonIndex {
    pub text: TextIndex,
    /// Character range of each prose block in `text`.
    pub ranges: Vec<(String, usize, usize)>,
    /// Scene IDs and, when the host knows them, the part paths inside.
    pub scenes: BTreeMap<String, Option<BTreeSet<String>>>,
}

impl LessonIndex {
    pub fn build(lesson: &Lesson) -> Self {
        let mut text = String::new();
        let mut ranges = Vec::new();
        for b in &lesson.blocks {
            let plain = match &b.kind {
                BlockKind::Heading { text, .. } => normalize(text),
                BlockKind::Markdown { text } => normalize(&sim_markdown::parse(&crate::refs::plain(text)).plain()),
                _ => continue,
            };
            if plain.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            let start = text.chars().count();
            text.push_str(&plain);
            ranges.push((b.id.clone(), start, start + plain.chars().count()));
        }
        let scenes = lesson.scenes().map(|(_, s)| (s.id.clone(), None)).collect();
        // Already normalized piecewise; TextIndex::new keeps it as is.
        Self { text: TextIndex::new(&text), ranges, scenes }
    }
    /// Record the part paths of a scene once its system is compiled, so
    /// part anchors can be checked too.
    pub fn with_parts(&mut self, scene: &str, parts: BTreeSet<String>) {
        self.scenes.insert(scene.into(), Some(parts));
    }
    /// The prose block containing character `offset`.
    pub fn block_at(&self, offset: usize) -> Option<&str> {
        self.ranges.iter().find(|(_, s, e)| offset >= *s && offset < (*e).max(*s + 1)).map(|(id, _, _)| id.as_str())
    }
    /// Anchor to a whole block (its first `BLOCK_QUOTE` characters).
    pub fn block_anchor(&self, block: &str, section: &str) -> Option<TextAnchor> {
        let (_, s, e) = self.ranges.iter().find(|(id, _, _)| id == block)?;
        TextAnchor::capture(&self.text.text, *s, (*e).min(*s + BLOCK_QUOTE), section).ok()
    }
}

impl LessonAnchor {
    /// The prose block a text anchor currently sits in.
    pub fn block<'a>(&self, index: &'a LessonIndex) -> Option<&'a str> {
        match self {
            LessonAnchor::Text { quote } => quote.locate(&index.text.text).and_then(|(s, _)| index.block_at(s)),
            LessonAnchor::Scene { .. } => None,
        }
    }
    pub fn scene(&self) -> Option<&str> {
        match self {
            LessonAnchor::Scene { scene, .. } => Some(scene),
            _ => None,
        }
    }
}

impl sim_annotate::Anchor for LessonAnchor {
    type Index = LessonIndex;
    fn validate(&self) -> Result<(), String> {
        match self {
            LessonAnchor::Text { quote } => quote.validate(),
            LessonAnchor::Scene { scene, part, time_s, .. } => {
                if scene.is_empty() || scene.len() > 64 || part.as_ref().is_some_and(|p| p.is_empty() || p.len() > 4096) || time_s.is_some_and(|t| !t.is_finite() || t < 0.0) {
                    return Err("invalid scene anchor (scene ID, part path or time)".into());
                }
                Ok(())
            }
        }
    }
    fn label(&self) -> String {
        match self {
            LessonAnchor::Text { quote } => sim_annotate::Anchor::label(quote),
            LessonAnchor::Scene { scene, part, time_s, .. } => {
                let mut s = format!("scene {scene}");
                if let Some(p) = part {
                    s.push_str(&format!(" · {p}"));
                }
                if let Some(t) = time_s {
                    s.push_str(&format!(" @ {t:.2} s"));
                }
                s
            }
        }
    }
    fn missing(&self) -> bool {
        match self {
            LessonAnchor::Text { quote } => quote.missing,
            LessonAnchor::Scene { missing, .. } => *missing,
        }
    }
    fn refresh(&mut self, index: &LessonIndex) -> bool {
        match self {
            LessonAnchor::Text { quote } => sim_annotate::Anchor::refresh(quote, &index.text),
            LessonAnchor::Scene { scene, part, missing, .. } => {
                let found = match index.scenes.get(scene.as_str()) {
                    None => false,
                    Some(None) => true,
                    Some(Some(parts)) => part.as_ref().is_none_or(|p| parts.contains(p) || parts.iter().any(|q| q.starts_with(&format!("{p}/")))),
                };
                *missing = !found;
                found
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_annotate::Anchor;
    use std::path::Path;
    #[test]
    fn block_anchor_reattaches_after_text_is_inserted_above() {
        let src = "---\ntitle: T\nsystems: { w: w.system.json }\n---\n# Intro\n\nFirst paragraph.\n\nThe worm holds the load.\n\n```sim-scene\nid: s1\nsystem: w\n```\n";
        let lesson = Lesson::parse(Path::new("x/lesson.md"), src).unwrap();
        let index = lesson.text_index();
        let quote = index.block_anchor("b3", "Intro").unwrap();
        assert_eq!(quote.exact, "The worm holds the load.");
        let mut anchor = LessonAnchor::Text { quote };
        let edited = Lesson::parse(Path::new("x/lesson.md"), &src.replace("First paragraph.", "A new opening.\n\nFirst paragraph, longer now.")).unwrap();
        let index = edited.text_index();
        assert!(anchor.refresh(&index));
        assert_eq!(anchor.block(&index), Some("b4"));
        let mut scene = LessonAnchor::Scene { scene: "s1".into(), part: Some("gearbox".into()), time_s: Some(1.5), missing: false };
        let mut index = edited.text_index();
        assert!(scene.refresh(&index));
        index.with_parts("s1", ["gearbox/worm".to_string()].into());
        assert!(scene.refresh(&index));
        index.with_parts("s1", ["motor".to_string()].into());
        assert!(!scene.refresh(&index) && scene.missing());
    }
}
