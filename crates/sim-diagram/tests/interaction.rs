use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_diagram::{Diagram, Selection, initial_state, route};
use sim_inspect::{
    SystemDescription,
    model::{IdentityBindings, describe},
};

fn model() -> SystemDescription {
    let mut registry = BehaviorRegistry::default();
    sim_domain_thermal::register(&mut registry).unwrap();
    let mut model = ModelWorld::default();
    let mut ports = Vec::new();
    for name in ["a", "b", "c"] {
        let c = model
            .part(
                &registry,
                name,
                sim_domain_thermal::CAPACITANCE,
                [("heat_capacity", 1.)],
            )
            .unwrap();
        ports.push(c.port("node"));
    }
    model.connect(ports);
    describe(
        &model,
        &registry,
        "fixture",
        1,
        &IdentityBindings::default(),
    )
    .unwrap()
    .description
}

#[test]
fn routing_retains_one_branched_connection_and_is_deterministic() {
    let model = model();
    let state = initial_state(&model);
    let first = route(&model, &state);
    let second = route(&model, &state);
    assert_eq!(first.nets, second.nets);
    assert_eq!(first.ports, second.ports);
    assert_eq!(first.nets.len(), 1);
    let net = first.nets.values().next().unwrap();
    assert_eq!(net.branches.len(), 3);
    assert!(
        net.branches
            .iter()
            .any(|branch| branch[branch.len() - 2].x == net.junction.x)
    );
    for branch in &net.branches {
        assert_eq!(branch.last(), Some(&net.junction));
        for pair in branch.windows(2) {
            assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y);
        }
    }
}

fn settle(diagram: &mut Diagram) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while diagram.is_layout_pending() {
        diagram.poll_layout();
        assert!(
            std::time::Instant::now() < deadline,
            "layout worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn frame(
    context: &egui::Context,
    diagram: &mut Diagram,
    model: &SystemDescription,
    events: Vec<Event>,
) -> Pos2 {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::PointerButton { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let mut events = events;
    events.insert(0, Event::ModifiersChanged(modifiers));
    settle(diagram);
    let mut origin = Pos2::ZERO;
    let mut output = context.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 600.))),
            events,
            ..Default::default()
        },
        |ui| {
            origin = ui.next_widget_position();
            diagram.show(ui, model);
        },
    );
    assert!(!output.shapes.is_empty());
    // This headless interaction harness has no renderer to upload textures.
    output.textures_delta.clear();
    settle(diagram);
    origin
}

#[test]
fn actual_pointer_click_selects_a_component_without_changing_model() {
    let model = model();
    let before = model.id.clone();
    let context = egui::Context::default();
    let mut diagram = Diagram::new(&model);
    let origin = frame(&context, &mut diagram, &model, vec![]);
    let component = model
        .components
        .values()
        .min_by(|a, b| a.label.cmp(&b.label))
        .unwrap();
    let position = diagram.state.positions[&component.id];
    let target = origin
        + Vec2::new(
            diagram.state.camera.x + (position.x + 35.) * diagram.state.zoom,
            diagram.state.camera.y + (position.y + 25.) * diagram.state.zoom,
        );
    frame(
        &context,
        &mut diagram,
        &model,
        vec![
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
        ],
    );
    frame(
        &context,
        &mut diagram,
        &model,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        }],
    );
    assert_eq!(
        diagram.selected,
        Some(Selection::Component(component.id.clone()))
    );
    assert_eq!(model.id, before);
    model.validate().unwrap();
}

#[test]
fn drag_updates_presentation_and_reroutes_the_same_terminals() {
    let model = model();
    let context = egui::Context::default();
    let mut diagram = Diagram::new(&model);
    let origin = frame(&context, &mut diagram, &model, vec![]);
    let component = model
        .components
        .values()
        .min_by(|a, b| a.label.cmp(&b.label))
        .unwrap();
    let initial = diagram.state.positions[&component.id];
    let target = origin
        + Vec2::new(
            diagram.state.camera.x + (initial.x + 35.) * diagram.state.zoom,
            diagram.state.camera.y + (initial.y + 25.) * diagram.state.zoom,
        );
    frame(
        &context,
        &mut diagram,
        &model,
        vec![
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
        ],
    );
    let moved = target + Vec2::new(40., 30.);
    frame(
        &context,
        &mut diagram,
        &model,
        vec![Event::PointerMoved(moved)],
    );
    frame(
        &context,
        &mut diagram,
        &model,
        vec![Event::PointerButton {
            pos: moved,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        }],
    );
    assert_ne!(diagram.state.positions[&component.id], initial);
    assert!(diagram.state.pinned.contains(&component.id));
    assert_eq!(
        diagram
            .layout()
            .nets
            .values()
            .next()
            .unwrap()
            .branches
            .len(),
        3
    );
    diagram.state.validate(&model).unwrap();
    model.validate().unwrap();
}

#[test]
fn arranging_preserves_pins_and_cancelled_jobs_cannot_publish() {
    let model = model();
    let mut diagram = Diagram::new(&model);
    diagram.cancel_layout();
    assert!(!diagram.is_layout_pending());
    std::thread::sleep(std::time::Duration::from_millis(10));
    diagram.poll_layout();
    assert!(diagram.state.positions.is_empty());
    diagram.reset_layout(&model);
    settle(&mut diagram);
    let id = model.components.keys().next().unwrap().clone();
    let pinned = sim_inspect::Point { x: -500., y: 200. };
    diagram.state.positions.insert(id.clone(), pinned);
    diagram.state.pinned.insert(id.clone());
    diagram.reset_layout(&model);
    settle(&mut diagram);
    assert_eq!(diagram.state.positions[&id], pinned);
}

#[test]
fn actual_terminal_click_selects_the_port_and_restoration_keeps_camera() {
    let model = model();
    let context = egui::Context::default();
    let mut diagram = Diagram::new(&model);
    let origin = frame(&context, &mut diagram, &model, vec![]);
    let id = model.ports.keys().next().unwrap().clone();
    let position = diagram.layout().ports[&id];
    let target = origin
        + Vec2::new(
            diagram.state.camera.x + position.x * diagram.state.zoom,
            diagram.state.camera.y + position.y * diagram.state.zoom,
        );
    for pressed in [true, false] {
        frame(
            &context,
            &mut diagram,
            &model,
            vec![
                Event::PointerMoved(target),
                Event::PointerButton {
                    pos: target,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::default(),
                },
            ],
        );
    }
    assert_eq!(diagram.selected, Some(Selection::Port(id)));
    diagram.state.camera = sim_inspect::Point { x: 123., y: 234. };
    diagram.state.zoom = 0.8;
    let saved = diagram.state.clone();
    let mut reopened = Diagram::new(&model);
    reopened.restore_state(saved.clone()).unwrap();
    settle(&mut reopened);
    assert_eq!(reopened.state, saved);
}

#[test]
fn shift_selection_drags_and_pins_both_cards_without_moving_the_third() {
    let model = model();
    let before = serde_json::to_vec(&model).unwrap();
    let context = egui::Context::default();
    let mut diagram = Diagram::new(&model);
    let origin = frame(&context, &mut diagram, &model, vec![]);
    let ids: Vec<_> = model.components.keys().cloned().collect();
    let initial = diagram.state.positions.clone();
    let mut targets = Vec::new();
    for id in &ids[..2] {
        let p = initial[id];
        let target = origin
            + Vec2::new(
                diagram.state.camera.x + (p.x + 35.) * diagram.state.zoom,
                diagram.state.camera.y + (p.y + 25.) * diagram.state.zoom,
            );
        targets.push(target);
        for pressed in [true, false] {
            frame(
                &context,
                &mut diagram,
                &model,
                vec![
                    Event::PointerMoved(target),
                    Event::PointerButton {
                        pos: target,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::SHIFT,
                    },
                ],
            );
        }
    }
    assert_eq!(
        diagram.marked_components,
        ids[..2].iter().cloned().collect()
    );
    let target = targets[0];
    frame(
        &context,
        &mut diagram,
        &model,
        vec![
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
        ],
    );
    let moved = target + Vec2::new(40., 30.);
    frame(
        &context,
        &mut diagram,
        &model,
        vec![Event::PointerMoved(moved)],
    );
    frame(
        &context,
        &mut diagram,
        &model,
        vec![Event::PointerButton {
            pos: moved,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        }],
    );
    let delta = |id: &String| {
        let p = diagram.state.positions[id];
        Vec2::new(p.x - initial[id].x, p.y - initial[id].y)
    };
    assert!(delta(&ids[0]).length() > 1.);
    // Translation at different f32 coordinates may differ by a few ULPs.
    assert!((delta(&ids[0]) - delta(&ids[1])).length() < 0.001);
    assert_eq!(delta(&ids[2]), Vec2::ZERO);
    assert!(ids[..2].iter().all(|id| diagram.state.pinned.contains(id)));
    assert_eq!(serde_json::to_vec(&model).unwrap(), before);
    diagram.state.validate(&model).unwrap();
}
