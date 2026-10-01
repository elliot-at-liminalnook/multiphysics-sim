//! RoboCAD's menu bar (`_build_menus`, app.py:434-438): one tab per
//! category in RoboCAD's order ("File" … "Help"), each opening
//! `Surface::Menu { category }`; the open menu's tab is lit and a press
//! on it closes the menu. The menu itself (`surfaces::draw`, a popup under
//! its tab) lists that category's commands in registry order with their
//! keys and enabled state; commands of the categories without a menu
//! ("General", "Window", "Tools") are in Help, as RoboCAD's
//! `menus.get(c["category"], menus["Help"])` puts them. A click runs the
//! command and closes the menu.
use super::registry::CATEGORIES;
use super::toolbar::Hint;
use super::{Open, Surface};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::CadButton;
use crate::ui_kit::Kit;
use bevy::prelude::*;

/// The menu bar's row (a press on it does not close the open menu: its tabs switch menus).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct MenuRow;
/// The container of the menu tabs.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Tabs;
/// A menu tab: its category (the open menu is placed under it).
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct MenuTab(pub &'static str);

/// The menu open now, if any.
pub(super) fn open_menu(doc: &CadDocument) -> Option<&str> {
    match &doc.ops.surface {
        Some(Open { surface: Surface::Menu { category }, .. }) => Some(category.as_str()),
        _ => None,
    }
}

/// The menu tabs (into the bar's `Tabs`).
pub(super) fn tabs(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let open = open_menu(doc);
    for category in CATEGORIES {
        let on = open == Some(category);
        let surface = if on { Surface::Closed } else { Surface::Menu { category: category.to_string() } };
        p.spawn(k.tab(category, CadButton(CadAction::CadSurface { surface }), on)).insert((MenuTab(category), Hint(format!("{category} menu"))));
    }
}
