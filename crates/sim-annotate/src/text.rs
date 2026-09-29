//! Text-quote anchors: an annotation on a passage of prose keeps the exact
//! quote plus a little context on each side and a position hint, so it
//! re-attaches after surrounding text is edited (the W3C Web Annotation
//! "TextQuoteSelector" idea). If the quote itself is deleted or changed
//! beyond recognition the anchor is marked missing and kept.
use serde::{Deserialize, Serialize};

/// Characters of context captured on each side of the quote.
pub const CONTEXT: usize = 32;
/// Longest quote kept.
pub const MAX_QUOTE: usize = 2000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextAnchor {
    /// Section (heading) the passage was in when annotated; shown in labels.
    #[serde(default)]
    pub section: String,
    pub exact: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    /// Character offset of the quote in the indexed text when last resolved.
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub missing: bool,
}

/// Collapse runs of whitespace to one space and trim. Hosts index normalized
/// text and capture from it, so line wrapping never breaks an anchor.
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

impl TextAnchor {
    /// Capture characters `start..end` of normalized `text`.
    pub fn capture(text: &str, start: usize, end: usize, section: &str) -> Result<Self, String> {
        let c = chars(text);
        if start >= end || end > c.len() {
            return Err("empty or out-of-range passage".into());
        }
        if end - start > MAX_QUOTE {
            return Err(format!("passage longer than {MAX_QUOTE} characters"));
        }
        Ok(Self {
            section: section.into(),
            exact: c[start..end].iter().collect(),
            prefix: c[start.saturating_sub(CONTEXT)..start].iter().collect(),
            suffix: c[end..(end + CONTEXT).min(c.len())].iter().collect(),
            offset: start,
            missing: false,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.exact.trim().is_empty()
            || self.exact.chars().count() > MAX_QUOTE
            || self.prefix.chars().count() > CONTEXT * 4
            || self.suffix.chars().count() > CONTEXT * 4
            || self.section.len() > 512
        {
            return Err("invalid text anchor (empty quote or oversized context)".into());
        }
        Ok(())
    }
    /// Character range of the best match in `text`, if any. Among exact
    /// occurrences the one whose surrounding text agrees most with the stored
    /// context wins; ties go to the one nearest the stored offset.
    pub fn locate(&self, text: &str) -> Option<(usize, usize)> {
        let hay = chars(text);
        let needle = chars(&self.exact);
        if needle.is_empty() || needle.len() > hay.len() {
            return None;
        }
        let prefix = chars(&self.prefix);
        let suffix = chars(&self.suffix);
        let mut best: Option<(usize, usize, usize)> = None; // (score, distance, start)
        for start in 0..=hay.len() - needle.len() {
            if hay[start..start + needle.len()] != needle[..] {
                continue;
            }
            let before = prefix.iter().rev().zip(hay[..start].iter().rev()).take_while(|(a, b)| a == b).count();
            let after = suffix.iter().zip(hay[start + needle.len()..].iter()).take_while(|(a, b)| a == b).count();
            let score = before + after;
            let distance = start.abs_diff(self.offset);
            if best.is_none_or(|(s, d, _)| score > s || (score == s && distance < d)) {
                best = Some((score, distance, start));
            }
        }
        best.map(|(_, _, start)| (start, start + needle.len()))
    }
}

/// Index for text anchors: normalized plain text of the annotated source.
pub struct TextIndex {
    pub text: String,
}
impl TextIndex {
    pub fn new(text: &str) -> Self {
        Self { text: normalize(text) }
    }
}

impl crate::Anchor for TextAnchor {
    type Index = TextIndex;
    fn validate(&self) -> Result<(), String> {
        TextAnchor::validate(self)
    }
    fn label(&self) -> String {
        let quote: String = self.exact.chars().take(48).collect();
        if self.exact.chars().count() > 48 { format!("“{quote}…”") } else { format!("“{quote}”") }
    }
    fn missing(&self) -> bool {
        self.missing
    }
    fn refresh(&mut self, index: &TextIndex) -> bool {
        match self.locate(&index.text) {
            Some((start, end)) => {
                // Refresh the context too, so the next edit is judged against today's text.
                if let Ok(fresh) = TextAnchor::capture(&index.text, start, end, &self.section) {
                    *self = fresh;
                }
                true
            }
            None => {
                self.missing = true;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Anchor;
    #[test]
    fn quote_survives_edits_around_it_and_picks_the_right_repeat() {
        let text = normalize("A worm drive trades efficiency for holding torque. The worm drive holds the load.");
        let start = text.find("worm drive holds").unwrap();
        let mut a = TextAnchor::capture(&text, start, start + "worm drive".len(), "Intro").unwrap();
        // Insert text before, rewrap lines: still the second "worm drive".
        let edited = TextIndex::new("Preface.\nA worm drive trades   efficiency for holding torque.\nThe worm drive holds the load.");
        assert!(a.refresh(&edited));
        let (s, e) = a.locate(&edited.text).unwrap();
        assert_eq!(&edited.text[s..e], "worm drive");
        assert!(edited.text[e..].starts_with(" holds"));
        // Deleting the quote detaches, but keeps, the anchor.
        assert!(!a.refresh(&TextIndex::new("Nothing about gears here.")));
        assert!(a.missing);
        assert_eq!(a.exact, "worm drive");
    }
}
