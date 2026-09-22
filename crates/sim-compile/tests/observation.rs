use sim_compile::{Runtime, observation::ObservationError};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, ModelWorld, PortId,
    StateDeclaration, View, acausal,
};
use sim_dynamics::Integrator;
use std::sync::atomic::{AtomicUsize, Ordering};
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Storage;
impl Behavior for Storage {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, c: &mut Context) {
        c.add_through(0, c.across_rate(0));
    }
}
struct Drain {
    noise: bool,
    switch: bool,
    switched: bool,
}
impl Behavior for Drain {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, c: &mut Context) {
        CALLS.fetch_add(1, Ordering::Relaxed);
        c.add_through(0, c.across(0) + if self.switched { 1. } else { 0. });
        if self.noise {
            c.add_noise(0, 0.2);
        }
    }
    fn guards(&self, _v: &View, out: &mut Vec<f64>) {
        if self.switch {
            out.push(1.);
        }
    }
    fn scheduled_events(&self, _v: &View, out: &mut Vec<(usize, f64)>) {
        if self.switch && !self.switched {
            out.push((0, 0.05));
        }
    }
    fn jump(&mut self, _i: usize, _v: &View, _x: &mut [f64]) {
        self.switched = true;
    }
}
fn model(noise: bool, switch: bool, integrator: Integrator) -> (Runtime, PortId, PortId) {
    let mut r = BehaviorRegistry::default();
    r.register(BehaviorDescriptor::new(
        "storage",
        "Storage",
        vec![acausal("node", ConnectorKind::Electrical)],
        |_| Ok(Box::new(Storage)),
    ))
    .unwrap();
    let factory: sim_core::Equations = match (noise, switch) {
        (true, _) => |_| {
            Ok(Box::new(Drain {
                noise: true,
                switch: false,
                switched: false,
            }))
        },
        (_, true) => |_| {
            Ok(Box::new(Drain {
                noise: false,
                switch: true,
                switched: false,
            }))
        },
        _ => |_| {
            Ok(Box::new(Drain {
                noise: false,
                switch: false,
                switched: false,
            }))
        },
    };
    r.register(BehaviorDescriptor::new(
        "drain",
        "Drain",
        vec![acausal("node", ConnectorKind::Electrical)],
        factory,
    ))
    .unwrap();
    let mut m = ModelWorld::default();
    let a = m
        .part(&r, "storage", "storage", [("initial.voltage", 1.)])
        .unwrap();
    let b = m.part(&r, "drain", "drain", []).unwrap();
    m.connect([a.port("node"), b.port("node")]);
    let runtime = Runtime::new(m, &r, integrator).unwrap();
    (runtime, a.port("node"), b.port("node"))
}
#[test]
fn accepted_flows_have_matching_rates_and_stage_times_without_extra_evaluations() {
    // Keep all counter assertions in one test, avoiding test-thread interference.
    for (integrator, time, voltage) in [
        (Integrator::implicit_midpoint(), 0.05, 1. / 1.05),
        (Integrator::BackwardEuler(Default::default()), 0.1, 1. / 1.1),
    ] {
        let (mut r, storage, drain) = model(false, false, integrator);
        assert_eq!(
            r.observe_through(storage, 0),
            Err(ObservationError::Unavailable)
        );
        r.set_observation_capture(true);
        r.advance(0.1, 0.1).unwrap();
        let binding = r.bind_through(drain, 0).unwrap();
        assert_eq!(binding.unit(), "A");
        let (other, _, _) = model(false, false, integrator);
        assert_eq!(
            other.read_flow(&binding),
            Err(ObservationError::ForeignBinding)
        );
        let count = CALLS.load(Ordering::Relaxed);
        let a = r.observe_through(storage, 0).unwrap();
        let b = r.observe_through(drain, 0).unwrap();
        assert!((a.value + b.value).abs() < 1e-9);
        assert!((b.value - voltage).abs() < 1e-9);
        assert_eq!(a.evaluation_time, time);
        assert_eq!(a.step_start, 0.);
        assert_eq!(a.step_end, 0.1);
        for _ in 0..100 {
            assert_eq!(r.read_flow(&binding).unwrap(), b);
        }
        assert_eq!(
            CALLS.load(Ordering::Relaxed),
            count,
            "sampling called equations"
        );
        assert_eq!(
            r.observe_through(drain, 5),
            Err(ObservationError::UnknownLane)
        );
        let snapshot = r.snapshot();
        r.restore(&snapshot).unwrap();
        assert_eq!(
            r.observe_through(storage, 0),
            Err(ObservationError::Unavailable)
        );
    }
    for noise in [false, true] {
        let run = |capture: bool| {
            let (mut r, storage, drain) = model(noise, false, Integrator::implicit_midpoint());
            r.seed(12345);
            r.set_observation_capture(capture);
            let count = CALLS.load(Ordering::Relaxed);
            let mut snapshots = Vec::new();
            for _ in 0..30 {
                r.advance(0.01, 0.01).unwrap();
                if capture {
                    let a = r.observe_through(storage, 0).unwrap();
                    let b = r.observe_through(drain, 0).unwrap();
                    assert!((a.value + b.value).abs() < 1e-8, "{}", a.value + b.value);
                }
                snapshots.push(r.snapshot());
            }
            (snapshots, CALLS.load(Ordering::Relaxed) - count)
        };
        let off = run(false);
        let on = run(true);
        assert_eq!(off, on, "capture changed trajectory or residual call count");
    }
    let (mut r, _, drain) = model(false, true, Integrator::implicit_midpoint());
    r.set_observation_capture(true);
    r.advance(0.05, 0.05).unwrap();
    assert_eq!(
        r.observe_through(drain, 0),
        Err(ObservationError::Unavailable),
        "pre-jump flow survived a mode change"
    );
    r.advance(0.01, 0.01).unwrap();
    assert!(r.observe_through(drain, 0).is_ok());
}

#[test]
fn owned_frame_flow_includes_the_registered_owner_balance_rows() {
    struct Load;
    impl Behavior for Load {
        fn states(&self) -> Vec<StateDeclaration> {
            vec![]
        }
        fn residual(&self, c: &mut Context) {
            c.add_through_lane(0, 0, -4.);
        }
    }
    let mut registry = BehaviorRegistry::default();
    sim_domain_multibody::contact::register(&mut registry).unwrap();
    registry
        .register(BehaviorDescriptor::new(
            "frame.load",
            "Applied force",
            vec![acausal("frame", ConnectorKind::PlanarFrame)],
            |_| Ok(Box::new(Load)),
        ))
        .unwrap();
    let mut m = ModelWorld::default();
    let body = m
        .part(
            &registry,
            "body",
            sim_domain_multibody::contact::PLANAR_RIGID_BODY,
            [("mass", 2.), ("inertia", 1.), ("gravity", 0.)],
        )
        .unwrap();
    let load = m.part(&registry, "load", "frame.load", []).unwrap();
    m.connect([body.port("frame"), load.port("frame")]);
    let mut r = Runtime::new(m, &registry, Integrator::implicit_midpoint()).unwrap();
    r.set_observation_capture(true);
    r.advance(0.1, 0.1).unwrap();
    let body_flow = r.observe_through(body.port("frame"), 0).unwrap();
    let load_flow = r.observe_through(load.port("frame"), 0).unwrap();
    assert!((body_flow.value - 4.).abs() < 1e-9, "{body_flow:?}");
    assert_eq!(load_flow.value, -4.);
    assert!((body_flow.value + load_flow.value).abs() < 1e-9);
    assert!((r.get(r.across_lane_id(body.port("frame"), 3)) - 0.2).abs() < 1e-9);
}
