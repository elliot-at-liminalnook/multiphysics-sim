//! Reusable native Markdown presentation. Callers own link actions and persistence.
use bevy::prelude::*;
use sim_markdown::{Align, Document, Kind, Link, Span, Table};

pub struct Theme {
    pub regular: Handle<Font>,
    pub strong: Handle<Font>,
    pub italic: Handle<Font>,
    pub mono: Handle<Font>,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub code: Color,
    pub surface: Color,
}
/// Rich text stays in Bevy's text layout (including word/character fallback).
/// Reference buttons expose complete destinations as caller-owned actions.
pub fn render<A: Component + Clone>(
    parent: &mut ChildSpawnerCommands,
    doc: &Document,
    theme: &Theme,
    link_action: impl Fn(&Link) -> Option<A>,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.),
            min_width: Val::Px(0.),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.),
            flex_shrink: 0.,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|body| {
            for block in &doc.blocks {
                if block.kind == Kind::Rule {
                    body.spawn((
                        Node {
                            height: Val::Px(1.),
                            width: Val::Percent(100.),
                            margin: UiRect::vertical(Val::Px(4.)),
                            ..default()
                        },
                        BackgroundColor(theme.muted),
                    ));
                    continue;
                }
                if let (Kind::Table, Some(t)) = (&block.kind, &block.table) {
                    body.spawn(Node { margin: UiRect::left(Val::Px((block.indent.min(5) * 10) as f32)), width: Val::Percent(100.), min_width: Val::Px(0.), flex_shrink: 0., ..default() })
                        .with_children(|c| table(c, theme, t));
                    continue;
                }
                let code = matches!(block.kind, Kind::Code(_));
                let quote = block.kind == Kind::Quote;
                let size = match block.kind {
                    Kind::Heading(1) => 20.,
                    Kind::Heading(2) => 18.,
                    Kind::Heading(_) => 16.,
                    Kind::Code(_) => 12.,
                    _ => 14.,
                };
                body.spawn((
                    Node {
                        width: Val::Percent(100.),
                        min_width: Val::Px(0.),
                        padding: if code || quote {
                            UiRect::all(Val::Px(8.))
                        } else {
                            UiRect::ZERO
                        },
                        margin: UiRect::left(Val::Px((block.indent.min(5) * 10) as f32)),
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 0.,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(if code || quote {
                        theme.surface
                    } else {
                        Color::NONE
                    }),
                    BorderRadius::all(Val::Px(5.)),
                ))
                .with_children(|line| {
                    if let Kind::Code(language) = &block.kind {
                        if !language.is_empty() {
                            line.spawn((
                                Text::new(language),
                                TextFont {
                                    font: theme.regular.clone(),
                                    font_size: 10.,
                                    ..default()
                                },
                                TextColor(theme.muted),
                            ));
                        }
                    }
                    line.spawn((
                        Text::new(""),
                        TextFont {
                            font: theme.regular.clone(),
                            font_size: size,
                            ..default()
                        },
                        TextColor(theme.text),
                        TextLayout::new_with_linebreak(bevy::text::LineBreak::WordOrCharacter),
                        Node {
                            width: Val::Percent(100.),
                            min_width: Val::Px(0.),
                            ..default()
                        },
                    ))
                    .with_children(|text| {
                        for span in &block.spans {
                            let font = if span.style.code {
                                &theme.mono
                            } else if span.style.strong || matches!(block.kind, Kind::Heading(_)) {
                                &theme.strong
                            } else if span.style.emphasis {
                                &theme.italic
                            } else {
                                &theme.regular
                            };
                            let color = if span.style.link.is_some() {
                                theme.accent
                            } else if span.style.code {
                                theme.code
                            } else {
                                theme.text
                            };
                            text.spawn((
                                TextSpan::new(&span.text),
                                TextFont {
                                    font: font.clone(),
                                    font_size: if span.style.code && !code {
                                        size - 1.
                                    } else {
                                        size
                                    },
                                    ..default()
                                },
                                TextColor(color),
                            ));
                        }
                    });
                });
            }
            if !doc.links.is_empty() {
                body.spawn(Node {
                    width: Val::Percent(100.),
                    min_width: Val::Px(0.),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(5.),
                    row_gap: Val::Px(5.),
                    ..default()
                })
                .with_children(|links| {
                    for link in &doc.links {
                        if let Some(action) = link_action(link) {
                            links
                                .spawn((
                                    Button,
                                    action,
                                    Node {
                                        max_width: Val::Percent(100.),
                                        min_width: Val::Px(0.),
                                        padding: UiRect::axes(Val::Px(8.), Val::Px(5.)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.surface),
                                    BorderRadius::all(Val::Px(5.)),
                                ))
                                .with_children(|button| {
                                    button.spawn((
                                        Text::new(format!("↗ {}", link.label)),
                                        TextFont {
                                            font: theme.regular.clone(),
                                            font_size: 12.,
                                            ..default()
                                        },
                                        TextColor(theme.accent),
                                        TextLayout::new_with_linebreak(
                                            bevy::text::LineBreak::WordOrCharacter,
                                        ),
                                    ));
                                });
                        }
                    }
                });
            }
        });
}

/// Rich text for one run of spans (paragraphs and table cells).
fn spans(parent: &mut ChildSpawnerCommands, theme: &Theme, spans: &[Span], size: f32, strong: bool, justify: JustifyText) {
    parent
        .spawn((
            Text::new(""),
            TextFont { font: theme.regular.clone(), font_size: size, ..default() },
            TextColor(theme.text),
            TextLayout { justify, linebreak: bevy::text::LineBreak::WordBoundary },
            Node { max_width: Val::Percent(100.), min_width: Val::Px(0.), ..default() },
        ))
        .with_children(|text| {
            for span in spans {
                let font = if span.style.code {
                    &theme.mono
                } else if span.style.strong || strong {
                    &theme.strong
                } else if span.style.emphasis {
                    &theme.italic
                } else {
                    &theme.regular
                };
                let color = if span.style.link.is_some() {
                    theme.accent
                } else if span.style.code {
                    theme.code
                } else {
                    theme.text
                };
                text.spawn((TextSpan::new(&span.text), TextFont { font: font.clone(), font_size: if span.style.code { size - 1. } else { size }, ..default() }, TextColor(color)));
            }
        });
}

/// A table as a CSS grid: columns shrink to their longest word and share
/// the rest of the width; a header row, horizontal rules and per-column
/// alignment. Used for Markdown tables and for hosts' data tables.
pub fn table(parent: &mut ChildSpawnerCommands, theme: &Theme, t: &Table) {
    let columns = t.columns().max(1);
    let rule = theme.muted.with_alpha(0.25);
    parent
        .spawn((
            Node {
                display: Display::Grid,
                width: Val::Percent(100.),
                min_width: Val::Px(0.),
                grid_template_columns: vec![RepeatedGridTrack::minmax(columns as u16, MinTrackSizingFunction::MinContent, MaxTrackSizingFunction::Fraction(1.0))],
                border: UiRect::all(Val::Px(1.)),
                overflow: Overflow::clip(),
                flex_shrink: 0.,
                ..default()
            },
            BorderColor(rule),
            BorderRadius::all(Val::Px(5.)),
        ))
        .with_children(|grid| {
            let rows = std::iter::once((true, &t.head)).chain(t.rows.iter().map(|r| (false, r)));
            let count = t.rows.len();
            for (i, (head, row)) in rows.enumerate() {
                for c in 0..columns {
                    let align = t.align.get(c).copied().unwrap_or_default();
                    let (justify, content) = match align {
                        Align::Right => (JustifyText::Right, JustifyContent::FlexEnd),
                        Align::Center => (JustifyText::Center, JustifyContent::Center),
                        _ => (JustifyText::Left, JustifyContent::FlexStart),
                    };
                    let last = !head && i == count;
                    grid.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(10.), Val::Px(6.)),
                            border: UiRect::bottom(Val::Px(if last { 0. } else { 1. })),
                            justify_content: content,
                            align_items: AlignItems::Center,
                            min_width: Val::Px(0.),
                            ..default()
                        },
                        BorderColor(rule),
                        BackgroundColor(if head { theme.surface } else { Color::NONE }),
                    ))
                    .with_children(|cell| {
                        if let Some(s) = row.get(c) {
                            spans(cell, theme, s, if head { 12.5 } else { 13. }, head, justify);
                        }
                    });
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Component, Clone)]
    struct Open(String);
    #[test]
    fn native_markdown_keeps_destinations_out_of_text_and_exposes_link_actions() {
        let mut app = App::new();
        let doc = sim_markdown::parse(
            "## Model\n**CAD** owns `mass`. See [source](AGENTS.md:6).\n\n```rust\nlet mass = 2.0;\n```",
        );
        let theme = Theme {
            regular: default(),
            strong: default(),
            italic: default(),
            mono: default(),
            text: Color::WHITE,
            muted: Color::WHITE,
            accent: Color::WHITE,
            code: Color::WHITE,
            surface: Color::BLACK,
        };
        app.add_systems(Update, move |mut commands:Commands| {
            commands.spawn(Node::default()).with_children(|p|render(p,&doc,&theme,|link|Some(Open(link.target.clone()))));
        });
        app.update();
        let world = app.world_mut();
        let spans = world
            .query::<&TextSpan>()
            .iter(world)
            .map(|s| s.0.clone())
            .collect::<Vec<_>>();
        assert!(spans.iter().any(|s| s == "source"));
        assert!(!spans.iter().any(|s| s.contains("AGENTS.md:6")));
        let actions = world
            .query_filtered::<&Open, With<Button>>()
            .iter(world)
            .map(|a| a.0.clone())
            .collect::<Vec<_>>();
        assert_eq!(actions, vec!["AGENTS.md:6"]);
    }
}
