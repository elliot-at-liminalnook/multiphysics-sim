//! The kit's widgets (see the contract in `ui_kit/mod.rs`) and the systems
//! that keep their style and accessibility labels current.
use super::theme::*;
use crate::builder::ui_api::Enabled;
use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;

/// A text bundle: the kit's one way to make UI text.
pub(crate) type TextBundle = (Text, TextFont, TextColor, TextLayout);

/// The widget builder. Holds the fonts; every method returns a bundle (or
/// spawns into a parent) and takes the widget's action as a component.
/// Widgets decide how things look, never what a click means: the action
/// component is read by the mode's own input system (`Changed<Interaction>`
/// on a `Button`) and applied by the mode's `apply`.
pub(crate) struct Kit<'a> {
    pub(crate) f: &'a UiFonts,
}

/// Which window edge a dock panel sits on, with its extent. Bottoms are
/// measured from the top of the switcher strip (`SWITCHER_STRIP`, added by
/// [`Kit::dock`]): `bottom: 0.0` ends a column just above it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Dock {
    /// Toolbar across the top: `height` tall. Background BAR, bottom border.
    Top { height: f32 },
    /// Status bar across the bottom, just above the switcher strip. Background BAR, top border.
    Bottom { height: f32 },
    /// Left column between `top` and `bottom`, `width` wide. SURFACE, right border.
    Left { top: f32, bottom: f32, width: f32 },
    /// Right column. SURFACE, left border.
    Right { top: f32, bottom: f32, width: f32 },
    /// A strip along the bottom of the middle view, between `left` and
    /// `right`, `bottom` above the switcher strip (the graph docks). BAR, top border.
    Under { left: f32, right: f32, bottom: f32, height: f32 },
    /// The switcher strip itself (`app::switcher` only): the window's
    /// bottom `SWITCHER_STRIP` px, full width. BAR, top border.
    Strip,
}

/// Where a dock sits: (left, right, top, bottom, width, height), `None`
/// where the edge is free. Pure, so the layout test reads it without a window.
pub(crate) fn dock_rect(dock: Dock) -> [Option<f32>; 6] {
    let s = SWITCHER_STRIP;
    match dock {
        Dock::Top { height } => [Some(0.), Some(0.), Some(0.), None, None, Some(height)],
        Dock::Bottom { height } => [Some(0.), Some(0.), None, Some(s), None, Some(height)],
        Dock::Left { top, bottom, width } => [Some(0.), None, Some(top), Some(bottom + s), Some(width), None],
        Dock::Right { top, bottom, width } => [None, Some(0.), Some(top), Some(bottom + s), Some(width), None],
        Dock::Under { left, right, bottom, height } => [Some(left), Some(right), None, Some(bottom + s), None, Some(height)],
        Dock::Strip => [Some(0.), Some(0.), None, Some(0.), None, Some(s)],
    }
}

/// A corner of a chart image, for its axis labels.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Corner {
    TopLeft,
    BottomLeft,
    BottomRight,
}

impl Kit<'_> {
    pub(crate) fn new(f: &UiFonts) -> Kit<'_> {
        Kit { f }
    }

    /// Text in the interface face. `weight`: 0 regular, 1 medium, 2 semibold.
    pub(crate) fn text(&self, value: impl Into<String>, size: f32, color: Color, weight: u8) -> TextBundle {
        (Text::new(value), TextFont { font: self.f.weight(weight).into(), font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter))
    }

    /// Text in the monospaced face (readouts, code, source text).
    pub(crate) fn mono(&self, value: impl Into<String>, size: f32, color: Color) -> TextBundle {
        (Text::new(value), TextFont { font: self.f.mono.clone().into(), font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter))
    }

    /// A panel title (16 px semibold).
    pub(crate) fn title(&self, value: impl Into<String>) -> TextBundle {
        self.text(value, size::TITLE, TEXT, 2)
    }

    /// Secondary text under a title or in a panel (12 px, SUBTLE).
    pub(crate) fn caption(&self, value: impl Into<String>) -> TextBundle {
        self.text(value, size::SMALL, SUBTLE, 0)
    }

    /// A quiet footnote (11.5 px, FAINT).
    pub(crate) fn note(&self, value: impl Into<String>) -> TextBundle {
        self.text(value, size::CAPTION, FAINT, 0)
    }

    /// A panel header: title, and a caption under it when `subtitle` is not
    /// empty, 4 px apart (the inspector column's gap).
    pub(crate) fn header(&self, parent: &mut ChildSpawnerCommands, title: &str, subtitle: &str) {
        parent.spawn(self.title(title));
        if !subtitle.is_empty() {
            parent.spawn(self.caption(subtitle));
        }
    }

    /// A button with its label. `action` is what a press means (a typed
    /// action component read by the mode's input system); `enabled` is
    /// the `Enabled` flag `system_ui` reports. The button is a `bevy::ui`
    /// `Button` (press-time `Interaction`, see the kit decisions) with an
    /// accessible label and its `Look`, which `repaint_buttons` keeps painted.
    pub(crate) fn button<A: Component>(&self, label: &str, action: A, look: Look, enabled: bool) -> impl Bundle + use<A> {
        let p = look.paint(enabled);
        (
            Button,
            action,
            Enabled(enabled),
            look,
            AccessibleLabel::new(label),
            Node {
                border_radius: BorderRadius::all(Val::Px(p.radius)),
                padding: UiRect::axes(Val::Px(p.pad.0), Val::Px(p.pad.1)),
                border: if p.underline { UiRect::bottom(Val::Px(2.)) } else { UiRect::all(Val::Px(1.)) },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.,
                ..default()
            },
            BorderColor::all(p.border),
            BackgroundColor(p.idle),
            children![self.text(label, p.size, p.text, p.weight)],
        )
    }

    /// A tab of a tab strip (`on`: the current tab).
    pub(crate) fn tab<A: Component>(&self, label: &str, action: A, on: bool) -> impl Bundle + use<A> {
        self.button(label, action, Look::Tab(on), true)
    }

    /// A toggle chip (`on`: lit).
    pub(crate) fn chip<A: Component>(&self, label: &str, action: A, on: bool, enabled: bool) -> impl Bundle + use<A> {
        self.button(label, action, Look::Chip(on), enabled)
    }

    /// One option of a segmented control (`on`: chosen). Put segments in a
    /// [`Kit::segments`] frame.
    pub(crate) fn segment<A: Component>(&self, label: &str, action: A, on: bool, enabled: bool) -> impl Bundle + use<A> {
        self.button(label, action, Look::Segment(on), enabled)
    }

    /// The row a tab strip's tabs go in: wraps, bottom border.
    pub(crate) fn tab_strip(&self) -> impl Bundle + use<> {
        (Node { padding: UiRect::horizontal(Val::Px(10.)), column_gap: Val::Px(6.), flex_wrap: FlexWrap::Wrap, border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor::all(BORDER))
    }

    /// The frame a segmented control's segments go in.
    pub(crate) fn segments(&self) -> impl Bundle + use<> {
        (Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::all(Val::Px(2.)), border: UiRect::all(Val::Px(1.)), column_gap: Val::Px(2.), ..default() }, BorderColor::all(BORDER))
    }

    /// An inspector section heading: upper case, faint, rule under it.
    pub(crate) fn section(&self, title: &str) -> impl Bundle + use<> {
        (
            Node { margin: UiRect::top(Val::Px(14.)), padding: UiRect::bottom(Val::Px(6.)), border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(BORDER),
            children![self.text(title.to_uppercase(), size::SECTION, FAINT, 2)],
        )
    }

    /// A clickable two-line list row: icon tinted with `accent`, title,
    /// subtitle; `selected` draws the accent fill and left edge. The edge is
    /// drawn at spawn only: rebuild the row to change its selection.
    pub(crate) fn list_item<A: Component>(&self, icon: &str, accent: Color, title: &str, subtitle: &str, action: A, selected: bool) -> impl Bundle + use<A> {
        (
            Button,
            action,
            Tint::selectable(selected),
            AccessibleLabel::new(if subtitle.is_empty() { title.to_string() } else { format!("{title}, {subtitle}") }),
            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(8.), Val::Px(6.)), column_gap: Val::Px(9.), align_items: AlignItems::Center, border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
            BorderColor::all(if selected { ACCENT } else { Color::NONE }),
            BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
            children![
                (Node { width: Val::Px(28.), height: Val::Px(28.), flex_shrink: 0., ..default() }, ImageNode::new(self.icon(icon)).with_color(accent)),
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(1.), flex_grow: 1., min_width: Val::Px(0.), ..default() },
                    children![self.text(title, size::ITEM, TEXT, 1), self.text(subtitle, size::DETAIL, SUBTLE, 0)]
                )
            ],
        )
    }

    /// The builder's library row: [`Kit::list_item`] with the icon tinted by
    /// the domain tag's colour (`builder::ui::tag_color`).
    pub(crate) fn item<A: Component>(&self, icon: &str, title: &str, subtitle: &str, tag: &str, action: A, selected: bool) -> impl Bundle + use<A> {
        self.list_item(icon, crate::builder::ui::tag_color(tag), title, subtitle, action, selected)
    }

    /// An icon by name (`sim_core::icons`), the generic component icon if unknown.
    pub(crate) fn icon(&self, name: &str) -> Handle<Image> {
        self.f.icons.get(name).or_else(|| self.f.icons.get("component")).cloned().unwrap_or_default()
    }

    /// A property row: key on the left, value (and unit) on the right. With
    /// an action the value is a button (for editing); `editing` frames it.
    pub(crate) fn property<A: Component>(&self, parent: &mut ChildSpawnerCommands, key: &str, value: &str, unit: &str, action: Option<A>, editing: bool) {
        let value_text = if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") };
        parent
            .spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(10.), padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((self.text(key, size::BODY, SUBTLE, 0), Node { flex_shrink: 1., ..default() }));
                let body = self.text(value_text.clone(), size::BODY, if editing { TEXT } else { VALUE }, 1);
                match action {
                    Some(action) => {
                        row.spawn((
                            Button,
                            action,
                            Tint::new(if editing { RAISED } else { Color::NONE }, HOVER_BG),
                            AccessibleLabel::new(format!("{key}: {value_text}")),
                            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(7.), Val::Px(3.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
                            BorderColor::all(if editing { ACCENT } else { BORDER }),
                            BackgroundColor(if editing { RAISED } else { Color::NONE }),
                            children![body],
                        ));
                    }
                    None => {
                        row.spawn((Node { padding: UiRect::axes(Val::Px(7.), Val::Px(3.)), flex_shrink: 0., ..default() }, children![body]));
                    }
                }
            });
    }

    /// Text-entry styling for the builder's draft fields. The draft itself
    /// (keys, caret, commit, cancel) stays in the builder's `text_input`;
    /// this shows `shown` (or the placeholder), with a caret while `focused`.
    /// A press on it is `action` (typically "focus this field").
    pub(crate) fn input<A: Component>(&self, shown: &str, placeholder: &str, action: A, focused: bool) -> impl Bundle + use<A> {
        self.input_selectable(shown, placeholder, action, focused, false)
    }

    /// [`Kit::input`] whose text can be shown selected (`selected`, while
    /// `focused`: the next key replaces it): the text on the accent fill,
    /// without the caret.
    pub(crate) fn input_selectable<A: Component>(&self, shown: &str, placeholder: &str, action: A, focused: bool, selected: bool) -> impl Bundle + use<A> {
        let empty = shown.is_empty();
        let marked = focused && selected && !empty;
        let line = if empty && !focused {
            placeholder.to_string()
        } else if focused && !marked {
            format!("{shown}|")
        } else {
            shown.to_string()
        };
        (
            Button,
            action,
            super::text::KitInput,
            Tint::RAISED,
            AccessibleLabel::new(if empty { placeholder.to_string() } else { shown.to_string() }),
            Node { border_radius: BorderRadius::all(Val::Px(5.)), padding: UiRect::axes(Val::Px(10.), Val::Px(7.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(if focused { ACCENT } else { BORDER }),
            BackgroundColor(RAISED),
            children![(self.text(line, size::BODY, if empty && !focused { FAINT } else { TEXT }, 0), BackgroundColor(if marked { ACCENT_BG } else { Color::NONE }))],
        )
    }

    /// A small status dot.
    pub(crate) fn dot(&self, color: Color) -> impl Bundle + use<> {
        (Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Px(7.), height: Val::Px(7.), flex_shrink: 0., ..default() }, BackgroundColor(color))
    }

    /// A docked panel on a window edge. `layout` carries only the inside
    /// layout (direction, padding, gaps, alignment, overflow); the kit sets
    /// the position, size, background and border for the edge.
    pub(crate) fn dock(&self, dock: Dock, layout: Node) -> impl Bundle + use<> {
        let [left, right, top, bottom, width, height] = dock_rect(dock);
        let (border, background) = match dock {
            Dock::Top { .. } => (UiRect::bottom(Val::Px(1.)), BAR),
            Dock::Bottom { .. } | Dock::Under { .. } | Dock::Strip => (UiRect::top(Val::Px(1.)), BAR),
            Dock::Left { .. } => (UiRect::right(Val::Px(1.)), SURFACE),
            Dock::Right { .. } => (UiRect::left(Val::Px(1.)), SURFACE),
        };
        // Free edges keep the layout's own value (Auto unless it sets one).
        let node = Node {
            position_type: PositionType::Absolute,
            left: left.map_or(layout.left, Val::Px),
            right: right.map_or(layout.right, Val::Px),
            top: top.map_or(layout.top, Val::Px),
            bottom: bottom.map_or(layout.bottom, Val::Px),
            width: width.map_or(layout.width, Val::Px),
            height: height.map_or(layout.height, Val::Px),
            border,
            ..layout
        };
        (node, BackgroundColor(background), BorderColor::all(BORDER))
    }

    /// A chart texture (`crate::chart`) in a node with the caller's layout
    /// (`layout`: its size); `framed` draws the 1 px BORDER frame.
    pub(crate) fn chart_image(&self, image: Handle<Image>, layout: Node, framed: bool) -> impl Bundle + use<> {
        let node = if framed { Node { border: UiRect::all(Val::Px(1.)), ..layout } } else { layout };
        (node, BorderColor::all(if framed { BORDER } else { Color::NONE }), ImageNode::new(image))
    }

    /// An axis label in a corner of a chart image (spawn it as the image's child).
    pub(crate) fn chart_label<S: Into<String>>(&self, value: S, corner: Corner) -> impl Bundle + use<S> {
        let node = match corner {
            Corner::TopLeft => Node { position_type: PositionType::Absolute, left: Val::Px(4.), top: Val::Px(2.), ..default() },
            Corner::BottomLeft => Node { position_type: PositionType::Absolute, left: Val::Px(4.), bottom: Val::Px(2.), ..default() },
            Corner::BottomRight => Node { position_type: PositionType::Absolute, right: Val::Px(4.), bottom: Val::Px(2.), ..default() },
        };
        (node, children![self.text(value, 10., FAINT, 0)])
    }
}

/// A vertical divider between toolbar groups.
pub(crate) fn divider() -> impl Bundle + use<> {
    (Node { width: Val::Px(1.), height: Val::Px(22.), margin: UiRect::horizontal(Val::Px(8.)), ..default() }, BackgroundColor(BORDER))
}

/// A wrapping row of buttons or chips, 6 px apart (a layout `Node`).
pub(crate) fn wrap() -> Node {
    Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.), row_gap: Val::Px(6.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }
}

/// Paint kit buttons from their `Look` and `Enabled` flag whenever the
/// pointer, the look or the flag changes: background (hover while hovered
/// or pressed), border, underline and the label's colour and weight.
#[allow(clippy::type_complexity)]
pub(crate) fn repaint_buttons(
    fonts: Option<Res<UiFonts>>,
    mut buttons: Query<(&Look, Option<&Enabled>, &Interaction, &mut BackgroundColor, &mut BorderColor, &mut Node, Option<&Children>), Or<(Changed<Look>, Changed<Enabled>, Changed<Interaction>)>>,
    mut labels: Query<(&mut TextColor, &mut TextFont)>,
) {
    for (look, enabled, interaction, mut bg, mut border, mut node, children) in &mut buttons {
        let p = look.paint(enabled.is_none_or(|e| e.0));
        let fill = match interaction {
            Interaction::Hovered | Interaction::Pressed => p.hover,
            Interaction::None => p.idle,
        };
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(p.border));
        let edge = if p.underline { UiRect::bottom(Val::Px(2.)) } else { UiRect::all(Val::Px(1.)) };
        let radius = BorderRadius::all(Val::Px(p.radius));
        let padding = UiRect::axes(Val::Px(p.pad.0), Val::Px(p.pad.1));
        if node.border != edge || node.border_radius != radius || node.padding != padding {
            node.border = edge;
            node.border_radius = radius;
            node.padding = padding;
        }
        let Some(children) = children else { continue };
        for child in children.iter() {
            if let Ok((mut color, mut font)) = labels.get_mut(child) {
                color.set_if_neq(TextColor(p.text));
                if !matches!(font.font_size, FontSize::Px(px) if px == p.size) {
                    font.font_size = FontSize::Px(p.size);
                }
                if let Some(fonts) = &fonts {
                    let face = FontSource::from(fonts.weight(p.weight));
                    if font.font != face {
                        font.font = face;
                    }
                }
            }
        }
    }
}

/// Hover for `Tint` surfaces (list rows, fields, cards): the hover colour
/// while hovered or pressed, else idle; also when the tint itself changes.
pub(crate) fn repaint_tints(mut surfaces: Query<(&Interaction, &Tint, &mut BackgroundColor), Or<(Changed<Interaction>, Changed<Tint>)>>) {
    for (interaction, tint, mut bg) in &mut surfaces {
        let fill = match interaction {
            Interaction::Hovered | Interaction::Pressed => tint.hover,
            Interaction::None => tint.idle,
        };
        bg.set_if_neq(BackgroundColor(fill));
    }
}

/// A kit button whose label text a mode rewrites (the robot's "×2" speed,
/// an overlay chip's "on"/"off") keeps the same accessible label: follow
/// the text child into the button's `AccessibleLabel`.
pub(crate) fn follow_button_text(mut commands: Commands, texts: Query<(&Text, &ChildOf), Changed<Text>>, buttons: Query<&AccessibleLabel, With<Look>>) {
    for (text, child_of) in &texts {
        let button = child_of.parent();
        if buttons.get(button).is_ok_and(|label| label.0 != text.0) {
            commands.entity(button).insert(AccessibleLabel::new(text.0.clone()));
        }
    }
}

/// `bevy::ui`'s accessibility system labels a `Button` from its direct text
/// children when it changes (and clears the label when there is none,
/// as for a list row whose text is nested). Re-apply the kit's
/// `AccessibleLabel` after it, before AccessKit reads the tree.
pub(crate) fn keep_labels(mut nodes: Query<(&AccessibleLabel, &mut AccessibilityNode), Or<(Changed<Button>, Changed<AccessibleLabel>)>>) {
    for (label, mut node) in &mut nodes {
        if node.label() != Some(label.0.as_str()) {
            node.set_label(label.0.as_str());
        }
    }
}
