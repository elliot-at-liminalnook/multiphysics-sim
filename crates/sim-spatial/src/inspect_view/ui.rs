//! The inspection panels: the toolbar, the parts list, the inspector and the
//! status bar, and the systems that keep them current.
use super::{BOTTOM, Inspector, InspectorScroll, PartsPanel, SpatialScene, Status, TOP, UiRoot};
use crate::{animation, inspect, linked, notes, ui_kit};
use bevy::{input::mouse::MouseWheel, prelude::*};
use sim_inspect::ObservationLocation;
use sim_inspect::selection::SelectionTarget;
use sim_inspect::spatial::SpatialCommand;

/// Whether an inspect button's selection or switch is on now (`None`: the
/// button is an ordinary action, not a toggle). Toggles are kit chips, lit
/// while on; the others are secondary buttons.
fn toggled(scene: &SpatialScene, action: &inspect::InspectAction) -> Option<bool> {
    use inspect::{InspectAction as A, Toggle};
    match action {
        A::Display { action: SpatialCommand::Select { component } } => Some(scene.details.components.contains(component)),
        A::Select { target: SelectionTarget::Nets { ids } } => Some(matches!(&scene.shown, SelectionTarget::Nets { ids: selected } if ids.iter().all(|id| selected.contains(id)))),
        A::Toggle(Toggle::Parts) => Some(scene.parts_visible),
        A::Toggle(Toggle::Exploded) => Some(scene.state.exploded),
        A::Toggle(Toggle::Connections) => Some(scene.state.connections),
        _ => None,
    }
}
/// An inspect button: a kit button with its action, a chip for a toggle.
fn action_button(k: &ui_kit::Kit<'_>, scene: &SpatialScene, label: &str, action: inspect::InspectAction) -> impl Bundle + use<> {
    let look = match toggled(scene, &action) {
        Some(on) => ui_kit::Look::Chip(on),
        None => ui_kit::Look::Secondary,
    };
    k.button(label, action, look, true)
}
pub(super) fn setup_ui(mut commands: Commands, scene: Option<Res<SpatialScene>>, fonts: Res<ui_kit::UiFonts>) {
    if let Some(scene) = scene {
        spawn_ui(&mut commands, &scene, &fonts);
    }
}
pub(crate) fn spawn_ui(commands: &mut Commands, scene: &SpatialScene, fonts: &ui_kit::UiFonts) {
    use inspect::{InspectAction as A, Toggle};
    use ui_kit::{ACCENT, Dock, TEXT};
    if scene.builder_mode {
        return; // Build mode draws its own chrome.
    }
    let k = ui_kit::Kit::new(fonts);
    commands
        .spawn((k.dock(Dock::Top { height: TOP }, Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)), row_gap: Val::Px(13.0), ..default() }), UiRoot))
        .with_children(|root| {
            // The title row: the assembly's name, and the link status at the right.
            root.spawn((
                Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() },
                children![
                    k.text(format!("ASSEMBLY / {}", scene.spatial.title), if scene.compact { 18. } else { 24. }, TEXT, 2),
                    (k.text("Standalone assembly", 13.0, ACCENT, 1), linked::LinkStatus)
                ],
            ));
            root.spawn(ui_kit::wrap()).with_children(|row| {
                row.spawn(action_button(&k, scene, "Parts", A::Toggle(Toggle::Parts)));
                row.spawn(action_button(&k, scene, "Clear", A::Select { target: SelectionTarget::None }));
                row.spawn(action_button(&k, scene, "Explode", A::Toggle(Toggle::Exploded)));
                row.spawn(action_button(&k, scene, "Connections", A::Toggle(Toggle::Connections)));
                row.spawn(action_button(&k, scene, "Fit view", A::Fit));
                row.spawn(action_button(&k, scene, "Hide selected", A::Display { action: SpatialCommand::HideSelected }));
                row.spawn(action_button(&k, scene, "Show all", A::Display { action: SpatialCommand::ShowAll }));
            });
        });
    let parts_layout = Node { display: if scene.parts_visible { Display::Flex } else { Display::None }, padding: UiRect::all(Val::Px(18.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(9.0), ..default() };
    commands
        .spawn((k.dock(Dock::Left { top: TOP, bottom: BOTTOM, width: scene.left() }, parts_layout), PartsPanel, UiRoot))
        .with_children(|column| {
            column.spawn(k.section("Components"));
            // Full-width rows in the column, so long names wrap inside the panel.
            for (i, (id, label)) in scene.representatives().iter().enumerate() {
                column.spawn(action_button(&k, scene, &format!("{}  {}", i + 1, label), A::Display { action: SpatialCommand::Select { component: id.clone() } }));
            }
            // A gap above the hint (layout only).
            column.spawn((k.caption("Select a part in the assembly or here.\n\nGold marks the selection.\nLive temperature colors retain their scale when selected."), Node { margin: UiRect::top(Val::Px(16.0)), ..default() }));
        });
    commands
        .spawn((k.dock(Dock::Right { top: TOP, bottom: BOTTOM, width: scene.right() }, Node { flex_direction: FlexDirection::Column, ..default() }), UiRoot))
        .with_children(|dock| {
            // The column scrolls inside the dock (`scroll_inspector`).
            let layout = Node { flex_grow: 1.0, min_height: Val::Px(0.0), padding: UiRect::all(Val::Px(20.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), ..default() };
            dock.spawn((k.scroll_area(layout, 0.0), InspectorScroll)).with_children(|column| {
                // The notes panel's slot (filled by `notes::update`).
                column.spawn((notes::NotesPanel, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), flex_shrink: 0., ..default() }));
                column.spawn(k.section("Inspector"));
                // Readouts keep their height in the scrolling column (layout only).
                column.spawn((k.text("", 13.0, TEXT, 0), animation::LiveReadouts, Node { flex_shrink: 0.0, ..default() }));
                column.spawn(k.caption("Select a connection:"));
                // Full-width rows in the column, so long connection names wrap.
                for (i, id) in scene.description.nets.keys().enumerate() {
                    let members = scene.description.nets[id].ports.iter().map(|p| scene.description.components[&scene.description.ports[p].component].label.as_str()).collect::<Vec<_>>().join(" / ");
                    column.spawn(action_button(&k, scene, &format!("{} {}", i + 1, members), A::Select { target: SelectionTarget::net(id.clone()) }));
                }
                column.spawn((k.text("", 14.0, TEXT, 0), Inspector, Node { flex_shrink: 0.0, ..default() }));
            });
        });
    commands
        .spawn((k.dock(Dock::Bottom { height: BOTTOM }, Node { padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(5.0), ..default() }), UiRoot))
        .with_children(|column| {
            column.spawn(k.text(format!("Right-drag: orbit | Shift-drag: pan | Scroll: zoom | F: fit | 1-{}: select", scene.representatives().len().min(9)), 13.0, TEXT, 0));
            column.spawn((k.caption(""), Status));
        });
}

/// The toggles (toolbar switches, parts and connection chips): lit while
/// what they select or switch on is. Their presses are actions
/// (`inspect::input`); the kit paints the look and the hover.
pub(super) fn buttons(mut looks: Query<(&inspect::InspectAction, &mut ui_kit::Look), With<Button>>, scene: Res<SpatialScene>) {
    for (action, mut look) in &mut looks {
        if let (ui_kit::Look::Chip(_), Some(on)) = (*look, toggled(&scene, action)) {
            look.set_if_neq(ui_kit::Look::Chip(on));
        }
    }
}

pub(super) fn update_layout(scene: Res<SpatialScene>, mut panels: Query<&mut Node, With<PartsPanel>>) {
    if !scene.is_changed() {
        return;
    }
    for mut panel in &mut panels {
        panel.display = if scene.parts_visible && !scene.builder_mode {
            Display::Flex
        } else {
            Display::None
        };
        panel.width = Val::Px(scene.left());
    }
}

pub(super) fn scroll_inspector(
    scene: Res<SpatialScene>,
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window>,
    mut panel: Single<&mut ScrollPosition, With<InspectorScroll>>,
) {
    let delta = ui_kit::wheel_delta(&mut wheel, 24.0);
    if window.cursor_position().is_some_and(|p| {
        p.x >= window.width() - scene.right() && p.y > TOP && p.y < window.height() - scene.bottom()
    }) {
        panel.y = (panel.y - delta).max(0.0);
    }
}

pub(super) fn inspector(scene: &SpatialScene) -> String {
    if let Some(text) = linked::selection_inspector(scene) {
        return text;
    }
    let Some(id) = &scene.state.selected else {
        return format!(
            "{}\n\n{} components / {} physical nets\n\nClick a part to inspect its shared model identity, parameters and connections.\n\nGEOMETRY\nIllustrative assembly study. Shapes and placement are not CAD geometry.\n\nSIMULATION\n{}",
            scene.spatial.title,
            scene.description.components.len(),
            scene.description.nets.len(),
            scene.live_status()
        );
    };
    let c = &scene.description.components[id];
    let label = scene
        .spatial
        .parts
        .iter()
        .find(|p| &p.component == id)
        .map(|p| p.label.as_str())
        .unwrap_or(&c.label);
    let mut s = format!("{label}\n{}\n\nMODEL PARAMETERS\n", c.component_type);
    if c.parameters.is_empty() {
        s.push_str("No parameters declared.\n");
    }
    for (name, p) in &c.parameters {
        s.push_str(&format!(
            "{}: {} {}\n",
            name.replace('_', " "),
            p.value,
            p.unit
                .as_deref()
                .unwrap_or("[unit undeclared]")
                .replace('Ω', "ohm")
                .replace('·', " ")
                .replace('²', "^2")
                .replace('³', "^3")
        ));
    }
    s.push_str("\nCONNECTIONS\n");
    let ports: Vec<_> = scene
        .description
        .ports
        .values()
        .filter(|p| &p.component == id && p.composite_parent.is_none())
        .collect();
    // Include physical leaves of composite connectors for actual terminal membership.
    let leaves: Vec<_> = scene
        .description
        .ports
        .values()
        .filter(|p| {
            &p.component == id
                && !scene
                    .description
                    .ports
                    .values()
                    .any(|child| child.composite_parent.as_ref() == Some(&p.id))
        })
        .collect();
    for port in &leaves {
        let others: Vec<_> = scene
            .description
            .nets
            .values()
            .filter(|n| n.ports.contains(&port.id))
            .flat_map(|n| n.ports.iter())
            .filter_map(|pid| scene.description.ports.get(pid))
            .filter(|p| &p.component != id)
            .map(|p| {
                format!(
                    "{}.{}",
                    scene.description.components[&p.component].label, p.name
                )
            })
            .collect();
        s.push_str(&format!(
            "{}: {}\n",
            port.name,
            if others.is_empty() {
                "open".into()
            } else {
                others.join(", ")
            }
        ));
    }
    let observations = scene
        .description
        .observables
        .values()
        .filter(|o| match &o.location {
            ObservationLocation::State { component, .. }
            | ObservationLocation::Diagnostic {
                component: Some(component),
                ..
            } => component == id,
            ObservationLocation::Across { port, .. }
            | ObservationLocation::Through { port, .. }
            | ObservationLocation::Signal { port } => scene
                .description
                .ports
                .get(port)
                .is_some_and(|p| &p.component == id),
            _ => false,
        })
        .count();
    s.push_str(&format!("\nOBSERVATIONS\n{observations} authored quantities; live readouts above\n{} top-level ports\n\nSOURCE ID\n{id}\n\nPROVENANCE\nParameters: provenance / uncertainty unspecified.\nGeometry: illustrative, display only.\n", ports.len()));
    if !scene.spatial.parts.iter().any(|p| &p.component == id) {
        s.push_str("\nNo geometry for this component.");
    }
    if scene.state.hidden.contains(id) {
        s.push_str("\nHidden in assembly. Select to reveal.");
    }
    s
}

pub(super) fn update_ui(
    scene: Res<SpatialScene>,
    mut texts: ParamSet<(
        Query<&mut Text, With<Inspector>>,
        Query<&mut Text, With<Status>>,
        Query<&mut Text, With<animation::LiveReadouts>>,
    )>,
) {
    if !scene.is_changed() {
        return;
    }
    for mut t in &mut texts.p0() {
        t.0 = inspector(&scene);
    }
    for mut t in &mut texts.p1() {
        t.0 = format!(
            "Illustrative geometry | {} hidden | {}",
            scene.state.hidden.len(),
            scene.live_status().replace('\n', " | ")
        );
    }
    for mut t in &mut texts.p2() {
        t.0 = scene.live_readouts();
    }
}
