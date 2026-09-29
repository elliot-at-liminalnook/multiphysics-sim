//! Lesson categories: `lessons/categories.yaml` lists them in reading
//! order; a lesson names one in its front matter (`category: id`).
//!
//! ```yaml
//! - id: mechanisms
//!   title: Mechanisms and motion
//!   summary: Inertia, gears, friction, springs …
//! ```
//!
//! Lessons without a category, or naming one that is not listed, are grouped
//! under "Other lessons" at the end (and `sim-lesson check` reports the
//! unknown name), so a lesson is never hidden.
use crate::index::Entry;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

/// The group for lessons with no (known) category.
pub const OTHER: &str = "other";

/// Categories from `<dir>/categories.yaml`, in order. No file means none.
pub fn load(dir: &Path) -> Result<Vec<Category>, String> {
    let path = dir.join("categories.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    let list: Vec<Category> = serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut seen = std::collections::BTreeSet::new();
    for c in &list {
        if c.id.trim().is_empty() || c.id == OTHER {
            return Err(format!("{}: category id `{}` is reserved or empty", path.display(), c.id));
        }
        if !seen.insert(c.id.clone()) {
            return Err(format!("{}: category `{}` is listed twice", path.display(), c.id));
        }
    }
    Ok(list)
}

#[derive(Debug, Clone, Serialize)]
pub struct Group<'a> {
    pub category: Category,
    pub lessons: Vec<&'a Entry>,
}

/// Lessons grouped by category, in the categories' order (each group keeps
/// the lessons' own order); empty categories are left out and "Other
/// lessons" comes last.
pub fn group<'a>(entries: &'a [Entry], categories: &[Category]) -> Vec<Group<'a>> {
    let mut groups: Vec<Group> = categories.iter().map(|c| Group { category: c.clone(), lessons: Vec::new() }).collect();
    let mut other = Vec::new();
    for e in entries {
        match e.category.as_deref().and_then(|id| groups.iter_mut().find(|g| g.category.id == id)) {
            Some(g) => g.lessons.push(e),
            None => other.push(e),
        }
    }
    groups.retain(|g| !g.lessons.is_empty());
    if !other.is_empty() {
        groups.push(Group { category: Category { id: OTHER.into(), title: "Other lessons".into(), summary: String::new() }, lessons: other });
    }
    groups
}

/// Lessons naming a category that is not in the list: (slug, name).
pub fn unknown(entries: &[Entry], categories: &[Category]) -> Vec<(String, String)> {
    entries.iter().filter_map(|e| e.category.as_ref().filter(|c| !categories.iter().any(|k| &&k.id == c)).map(|c| (e.slug.clone(), c.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(slug: &str, category: Option<&str>) -> Entry {
        Entry { slug: slug.into(), path: Default::default(), title: slug.into(), summary: String::new(), order: None, requires: vec![], minutes: None, scenes: 0, error: None, category: category.map(String::from) }
    }
    #[test]
    fn groups_follow_the_category_order_and_collect_the_rest() {
        let cats = vec![Category { id: "b".into(), title: "B".into(), summary: String::new() }, Category { id: "a".into(), title: "A".into(), summary: String::new() }, Category { id: "empty".into(), title: "E".into(), summary: String::new() }];
        let entries = vec![entry("x", Some("a")), entry("y", Some("b")), entry("z", None), entry("w", Some("nope")), entry("v", Some("a"))];
        let g = group(&entries, &cats);
        let ids: Vec<_> = g.iter().map(|g| g.category.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", OTHER]);
        assert_eq!(g[1].lessons.iter().map(|e| e.slug.as_str()).collect::<Vec<_>>(), ["x", "v"]);
        assert_eq!(g[2].lessons.len(), 2);
        assert_eq!(unknown(&entries, &cats), vec![("w".to_string(), "nope".to_string())]);
    }
}
