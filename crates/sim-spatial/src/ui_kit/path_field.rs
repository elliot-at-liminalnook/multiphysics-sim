//! The path field (window-first-usability): the one way to type a path in
//! the window. A text input, the typed directory's matching entries (read
//! off the UI thread by [`request`] on a `jobs::Latest`), `~/` expansion,
//! "..", an optional submit button, and accessible labels. CAD's path form
//! (`cad::files::form`) and the mode switcher's document picker
//! (`app::picker`) both use it.
//!
//! No intent logic: the caller owns the draft (`form::TextDraft` keys),
//! the listing job and what a submit means; [`PathHit`] says which part was
//! pressed and the caller passes one action component per part. The pure
//! helpers ([`expand`], [`dir_of`], [`file_of`], [`listing_key`], [`pick`],
//! [`up`], [`list`]) are tested without a window.
use super::Kit;
use super::theme::*;
use crate::jobs::{Latest, Pool};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Value, json};

/// Entries the field shows under it (the typed file name narrows them).
pub(crate) const SHOWN: usize = 12;
/// Entries a listing keeps (names only; the typed file name narrows them
/// when drawn, so a listing must hold every entry it can reach).
const MAX_LISTED: usize = 20_000;

/// `~` and `~/…` expanded with `$HOME` (unchanged without one). A last
/// component of `.` or `..` gets a trailing `/`, so it names a directory to
/// list rather than a prefix to narrow by.
pub(crate) fn expand(path: &str) -> String {
    let home = std::env::var("HOME").ok().map(|h| h.trim_end_matches('/').to_string()).map(|h| if h.is_empty() { "/".to_string() } else { h });
    let path = match (path, home) {
        ("~", Some(home)) => home,
        (p, Some(home)) if p.starts_with("~/") => format!("{}/{}", home.trim_end_matches('/'), &p[2..]),
        (p, _) => p.to_string(),
    };
    if matches!(file_of(&path), "." | "..") { format!("{path}/") } else { path }
}

/// The directory part of `path` with a trailing `/` (`~/` expanded); empty
/// for a bare name or an empty path (no directory, so nothing to list).
pub(crate) fn dir_of(path: &str) -> String {
    let path = expand(path);
    if path.ends_with('/') {
        return path;
    }
    match std::path::Path::new(&path).parent().map(|p| p.display().to_string()) {
        Some(p) if !p.is_empty() => format!("{}/", p.trim_end_matches('/')),
        // "/x" has the parent "/", so only a relative bare name or "" gets here.
        _ if path.starts_with('/') => "/".into(),
        _ => String::new(),
    }
}

/// The file name part of `path` (after the last `/`).
pub(crate) fn file_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

/// Whether a file `name` has one of `suffixes` (without the dot,
/// case-insensitive; "system.json" matches "board.system.json"). No
/// suffixes: no file matches (a directory field lists directories only).
pub(crate) fn matches<S: AsRef<str>>(name: &str, suffixes: &[S]) -> bool {
    let name = name.to_lowercase();
    suffixes.iter().any(|s| name.ends_with(&format!(".{}", s.as_ref().to_lowercase())))
}

/// A directory's listing: subdirectories and the files with one of the
/// suffixes (hidden ones left out), directories first, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listing {
    /// What was asked ([`listing_key`]): the directory and the suffixes.
    pub key: String,
    pub dir: String,
    /// (name, is a directory), at most `MAX_LISTED`.
    pub entries: Vec<(String, bool)>,
    /// Entries left out past `MAX_LISTED`.
    pub more: usize,
    pub error: Option<String>,
}
impl Listing {
    pub(crate) fn json(&self) -> Value {
        json!({"dir": self.dir, "entries": self.entries.iter().map(|(n, d)| json!({"name": n, "dir": d})).collect::<Vec<_>>(), "more": self.more, "error": self.error})
    }
}

/// Reads `dir` (call it on `Pool::Io`, never on the UI thread).
pub(crate) fn list<S: AsRef<str>>(key: String, dir: String, suffixes: &[S]) -> Listing {
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(e) => return Listing { key, dir: dir.clone(), error: Some(format!("{dir}: {e}")), ..Default::default() },
    };
    let mut entries: Vec<(String, bool)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            // Follows symlinks, so a linked directory lists as one.
            let is_dir = std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
            (is_dir || matches(&name, suffixes)).then_some((name, is_dir))
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let more = entries.len().saturating_sub(MAX_LISTED);
    entries.truncate(MAX_LISTED);
    Listing { key, dir, entries, more, error: None }
}

/// The listing `path` asks for: (key, directory); None for a path that is
/// not absolute once `~/` is expanded.
pub(crate) fn listing_key<S: AsRef<str>>(path: &str, suffixes: &[S]) -> Option<(String, String)> {
    let dir = dir_of(path.trim());
    if !std::path::Path::new(&dir).is_absolute() {
        return None;
    }
    let exts: Vec<&str> = suffixes.iter().map(AsRef::as_ref).collect();
    Some((format!("{dir}|{}", exts.join(",")), dir))
}

/// Starts reading the listing `key` names on `Pool::Io` (a newer request
/// drops the older one: `Latest`).
pub(crate) fn request(latest: &mut Latest<Listing>, name: &str, key: String, dir: String, suffixes: Vec<String>) {
    latest.start(Pool::Io, name.to_string(), move |_| Ok(list(key, dir, &suffixes)));
}

/// JobResults: a finished listing into `listed` (an error as the listing's
/// error). True when it changed.
pub(crate) fn receive(latest: &mut Latest<Listing>, listed: &mut Option<Listing>) -> bool {
    if latest.pending().is_none() {
        return false;
    }
    let Some((_, result)) = latest.poll() else { return false };
    *listed = Some(result.unwrap_or_else(|e| Listing { error: Some(e), ..Default::default() }));
    true
}

/// A listing entry pressed: a directory descends (keeping the typed file
/// name when `keep_file`, as a save field does), a file fills the path.
pub(crate) fn pick(path: &str, dir: &str, name: &str, is_dir: bool, keep_file: bool) -> String {
    // The file name as `dir_of` splits it: after `~` expansion.
    let mut keep = if keep_file { file_of(&expand(path.trim())).to_string() } else { String::new() };
    // A typed name the directory starts with was narrowing the listing to
    // find it, not a file name to keep ("/w/su" + "sub" is "/w/sub/").
    if is_dir && name.to_lowercase().starts_with(&keep.to_lowercase()) {
        keep.clear();
    }
    let dir = dir.trim_end_matches('/');
    if is_dir { format!("{dir}/{name}/{keep}") } else { format!("{dir}/{name}") }
}

/// "..": the parent of the path's directory (keeping the file name when `keep_file`).
pub(crate) fn up(path: &str, keep_file: bool) -> String {
    let keep = if keep_file { file_of(&expand(path.trim())).to_string() } else { String::new() };
    let dir = dir_of(path.trim());
    let parent = std::path::Path::new(dir.trim_end_matches('/')).parent().map_or_else(|| "/".to_string(), |p| p.display().to_string());
    format!("{}/{keep}", parent.trim_end_matches('/'))
}

/// Which part of the field was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathHit {
    /// The text input (it takes the keyboard).
    Field,
    /// A listing entry, by its index in `Listing::entries`.
    Entry(usize),
    /// "..": the parent directory.
    Up,
    /// The submit button (the caller's "Open").
    Submit,
}

/// What the field shows.
pub(crate) struct PathView<'a> {
    /// The field's label (also its accessible label).
    pub label: &'a str,
    pub text: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
    /// The focused text is selected (the next key replaces it).
    pub selected: bool,
    /// The submit button's label, or None for no button (Enter still submits).
    pub submit: Option<&'a str>,
    pub submit_enabled: bool,
    /// The listing of the path's directory, only when it is the one the
    /// path asks for now ([`listing_key`]); None while it is read.
    pub listing: Option<&'a Listing>,
}

impl Kit<'_> {
    /// The path field: label, the input with the submit button beside it,
    /// then the listing ([`Kit::path_listing`]).
    pub(crate) fn path_field<A: Component>(&self, parent: &mut ChildSpawnerCommands, view: &PathView, hit: impl Fn(PathHit) -> A) {
        parent.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), flex_shrink: 0.0, ..default() }).with_children(|cell| {
            cell.spawn(self.text(view.label, size::CAPTION, SUBTLE, 1));
            cell.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|line| {
                // The kit input labels itself with its text; the field's label
                // names it, followed by the typed text (or the placeholder).
                let shown = if view.text.is_empty() { view.placeholder } else { view.text };
                line.spawn(self.input_selectable(view.text, view.placeholder, hit(PathHit::Field), view.focused, view.selected)).insert((AccessibleLabel::new(format!("{}: {shown}", view.label)), Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    border_radius: BorderRadius::all(Val::Px(5.)),
                    padding: UiRect::axes(Val::Px(10.), Val::Px(7.)),
                    border: UiRect::all(Val::Px(1.)),
                    ..default()
                }));
                if let Some(label) = view.submit {
                    line.spawn(self.button(label, hit(PathHit::Submit), Look::Primary, view.submit_enabled));
                }
            });
            self.path_listing(cell, view.text, view.listing, &hit);
        });
    }

    /// The listing under a path: "In <dir>", its error, "..", the entries
    /// the typed file name narrows (a case-insensitive prefix, at most
    /// [`SHOWN`]) and how many more. Without a listing: a hint for a path
    /// that is not absolute, else nothing (it is being read).
    pub(crate) fn path_listing<A: Component>(&self, parent: &mut ChildSpawnerCommands, path: &str, listing: Option<&Listing>, hit: &impl Fn(PathHit) -> A) {
        let Some(listing) = listing else {
            if !std::path::Path::new(&dir_of(path.trim())).is_absolute() {
                parent.spawn(self.text("Type an absolute path (~/ works) to list its directory.", size::SMALL, FAINT, 0));
            }
            return;
        };
        // Split as `dir_of` splits it (after `~` expansion), so "~" narrows
        // the listing of its parent to the home directory, not to nothing.
        let prefix = file_of(&expand(path.trim())).to_lowercase();
        let narrows = |n: &String| n.to_lowercase().starts_with(&prefix);
        parent.spawn(self.caption(format!("In {}", listing.dir)));
        if let Some(e) = &listing.error {
            parent.spawn(self.text(e.clone(), size::SMALL, FAINT, 0));
        }
        parent.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), flex_shrink: 0.0, ..default() }).with_children(|list| {
            if listing.dir != "/" {
                list.spawn(self.button("..", hit(PathHit::Up), Look::Ghost, true));
            }
            for (i, (entry, is_dir)) in listing.entries.iter().enumerate().filter(|(_, (n, _))| narrows(n)).take(SHOWN) {
                let label = if *is_dir { format!("{entry}/") } else { entry.clone() };
                list.spawn(self.button(&label, hit(PathHit::Entry(i)), Look::Ghost, true));
            }
        });
        // Entries past MAX_LISTED were never read, so they only count
        // while nothing narrows the listing.
        let unread = if prefix.is_empty() { listing.more } else { 0 };
        let more = listing.entries.iter().filter(|(n, _)| narrows(n)).count().saturating_sub(SHOWN) + unread;
        if more > 0 {
            parent.spawn(self.text(format!("{more} more: type to narrow the path."), size::SMALL, FAINT, 0));
        }
    }
}
