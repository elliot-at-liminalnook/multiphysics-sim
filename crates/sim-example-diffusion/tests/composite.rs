//! Named and nested custom composites use the same authoring/compiler path.
use sim_compile::Runtime;
use sim_core::definitions::*;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, ModelWorld,
    QuantityKind, StateDeclaration, acausal,
};
use sim_dynamics::Integrator;
use sim_example_diffusion::{SPECIES, register};
struct TwoVolumes;
impl Behavior for TwoVolumes {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        for lane in 0..2 {
            ctx.add_through_lane(0, lane, ctx.across_rate_lane(0, lane));
        }
    }
}
struct TwoBoundaries;
impl Behavior for TwoBoundaries {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("left_flow", QuantityKind::MolarFlow, 0.),
            StateDeclaration::new("right_flow", QuantityKind::MolarFlow, 0.),
        ]
    }
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 7.), (0, 1, 8.)]
    }
    fn residual(&self, ctx: &mut Context) {
        for lane in 0..2 {
            ctx.set_state_residual(lane, ctx.across_lane(0, lane) - (7. + lane as f64));
            ctx.add_through_lane(0, lane, ctx.state(lane));
        }
    }
}
#[test]
fn nested_registered_composites_expand_connect_pin_integrate_and_inspect() {
    let mut registry = BehaviorRegistry::default();
    register(&mut registry).unwrap();
    let pair = ConnectorKind::named("example.diffusion.pair", 1);
    let socket = ConnectorKind::named("example.diffusion.socket", 1);
    for (id, members) in [
        (
            pair.definition_id(),
            vec![
                ConnectorMember {
                    name: "left".into(),
                    connector: SPECIES.definition_id(),
                },
                ConnectorMember {
                    name: "right".into(),
                    connector: SPECIES.definition_id(),
                },
            ],
        ),
        (
            socket.definition_id(),
            vec![ConnectorMember {
                name: "pair".into(),
                connector: pair.definition_id(),
            }],
        ),
    ] {
        registry
            .register_connector(&ConnectorDescriptor {
                id,
                label: "Species socket".into(),
                lanes: vec![],
                rule: ConnectionRule::Composite { members },
                energy: PortEnergy::Unavailable,
            })
            .unwrap();
    }
    registry
        .register(
            BehaviorDescriptor::new(
                "pair.storage",
                "Two volumes",
                vec![acausal("socket", socket.clone())],
                |_| Ok(Box::new(TwoVolumes)),
            )
            .with_parameters(vec![]),
        )
        .unwrap();
    registry
        .register(BehaviorDescriptor::new(
            "pair.boundary",
            "Two reservoirs",
            vec![acausal("socket", socket)],
            |_| Ok(Box::new(TwoBoundaries)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    let a = model.part(&registry, "A", "pair.storage", []).unwrap();
    let b = model.part(&registry, "B", "pair.boundary", []).unwrap();
    assert!(a.try_port("socket.pair.left").is_some());
    assert!(a.try_port("socket.pair.right").is_some());
    model.connect([a.port("socket"), b.port("socket")]);
    assert_eq!(model.connections.len(), 2);
    let bytes = serde_json::to_vec(&model).unwrap();
    let decoded: ModelWorld = serde_json::from_slice(&bytes).unwrap();
    let mut runtime = Runtime::new(decoded, &registry, Integrator::implicit_midpoint()).unwrap();
    runtime.set_observation_capture(true);
    runtime.advance(0.1, 0.01).unwrap();
    assert_eq!(
        runtime.observe_through(a.port("socket"), 0).unwrap(),
        runtime
            .observe_through(a.port("socket.pair.left"), 0)
            .unwrap()
    );
    assert_eq!(
        runtime.observe_through(a.port("socket"), 1).unwrap(),
        runtime
            .observe_through(a.port("socket.pair.right"), 0)
            .unwrap()
    );
    for (lane, expected) in [(0, 7.), (1, 8.)] {
        assert!(
            (runtime.get(runtime.across_lane_id(a.port("socket"), lane)) - expected).abs() < 1e-10
        );
        assert!(
            (runtime.get(runtime.across_lane_id(a.port("socket.pair"), lane)) - expected).abs()
                < 1e-10
        );
    }
    let d = sim_inspect::model::describe(
        &runtime.model,
        &registry,
        "nested-composite",
        1,
        &Default::default(),
    )
    .unwrap()
    .description;
    d.validate().unwrap();
    assert_eq!(
        d.ports
            .values()
            .filter(|p| p.composite_parent.is_some())
            .count(),
        6
    );
    // Tampering with a serialized member map cannot silently change a socket.
    let mut invalid: ModelWorld = serde_json::from_slice(&bytes).unwrap();
    invalid.ports[a.port("socket.pair")].members.swap(0, 1);
    assert!(
        sim_compile::compile(&invalid, &registry)
            .unwrap_err()
            .to_string()
            .contains("invalid member")
    );
}
