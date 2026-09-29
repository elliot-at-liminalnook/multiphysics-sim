//! The lesson list: every `<dir>/<slug>/lesson.md`, ordered by `order` then
//! title. A lesson that does not parse is listed with its error so a broken
//! edit never hides the lesson.
use crate::Lesson;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub slug: String,
    pub path: PathBuf,
    pub title: String,
    pub summary: String,
    pub order: Option<i64>,
    pub requires: Vec<String>,
    pub minutes: Option<u32>,
    pub scenes: usize,
    pub error: Option<String>,
    /// Category ID from the front matter (see `categories`).
    pub category: Option<String>,
}

pub fn scan(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else { return out };
    for e in read.flatten() {
        let path = e.path().join("lesson.md");
        if !path.is_file() {
            continue;
        }
        let slug = e.file_name().to_string_lossy().into_owned();
        out.push(match Lesson::load(&path) {
            Ok(l) => Entry { slug, title: l.meta.title.clone(), summary: l.meta.summary.clone(), order: l.meta.order, requires: l.meta.requires.clone(), minutes: l.meta.minutes, scenes: l.scenes().count(), path, error: None, category: l.meta.category.clone() },
            Err(err) => Entry { title: slug.clone(), slug, path, summary: String::new(), order: None, requires: vec![], minutes: None, scenes: 0, error: Some(err.to_string()), category: None },
        });
    }
    out.sort_by(|a, b| a.order.unwrap_or(i64::MAX).cmp(&b.order.unwrap_or(i64::MAX)).then_with(|| a.title.cmp(&b.title)));
    out
}

/// Prerequisites that name no lesson in the list.
pub fn unknown_requires(entries: &[Entry]) -> Vec<(String, String)> {
    entries.iter().flat_map(|e| e.requires.iter().filter(|r| !entries.iter().any(|x| &x.slug == *r)).map(|r| (e.slug.clone(), r.clone()))).collect()
}
