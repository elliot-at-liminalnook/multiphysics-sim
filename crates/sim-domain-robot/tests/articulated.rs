mod common;
use common::*;
use sim_domain_robot::model::*;

#[test]
fn pendulum_period_and_energy() {
    let mut m = empty_model();
    m.links.push(box_link("ground", [0.05, 0.05, 0.05], 1.0, [0.0, 0.0, 1.0], true));
    let mut bob = box_link("bob", [0.02, 0.02, 0.02], 0.1, [0.0, 0.0, 0.8], false);
    bob.inertia = [[1e-7, 0.0, 0.0], [0.0, 1e-7, 0.0], [0.0, 0.0, 1e-7]];
    m.links.push(bob);
    let mut j = joint("pend", "revolute", Some("ground"), "bob", [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    j.physics.friction = Friction::default();
    m.joints.push(j);
    let theta0 = 0.3;
    let mut rig = Rig::new(m, &[("contact", 0.0), ("initial.joint.pend.angle", theta0)], midpoint());
    assert!((rig.angle(0) - theta0).abs() < 1e-9, "initial angle seeded: {}", rig.angle(0));
    let e0 = rig.runtime.energy();
    let l: f64 = 0.2;
    let i_pivot = 0.1 * l * l + 1e-7;
    let period = 2.0 * std::f64::consts::PI * (i_pivot / (0.1 * 9.81 * l)).sqrt() * (1.0 + theta0 * theta0 / 16.0);
    let h = 1e-3;
    let mut crossings = Vec::new();
    let mut prev = rig.angle(0);
    let mut drift: f64 = 0.0;
    let steps = (3.0 * period / h) as usize;
    for k in 0..steps {
        rig.runtime.advance(h, h).unwrap();
        let a = rig.angle(0);
        if prev > 0.0 && a <= 0.0 {
            crossings.push((k as f64 + 1.0) * h - a / (a - prev) * h);
        }
        prev = a;
        drift = drift.max((rig.runtime.energy() - e0).abs());
    }
    assert!(crossings.len() >= 2, "crossings {crossings:?}");
    let measured = crossings[1] - crossings[0];
    println!("pendulum period {measured:.5} s vs analytic {period:.5} s; energy drift {drift:.3e} J of {:.3e}", 0.1 * 9.81 * l * (1.0 - theta0.cos()));
    assert!((measured - period).abs() / period < 0.01, "period {measured} vs {period}");
    assert!(drift < 1e-4 * 0.1 * 9.81 * l, "energy drift {drift}");
}

#[test]
fn box_drops_rests_and_bounces() {
    let mut m = empty_model();
    m.links.push(box_link("box", [0.1, 0.1, 0.1], 0.5, [0.0, 0.0, 0.2], false));
    let mut rig = Rig::new(m, &[], euler());
    let h = 5e-4;
    let mut zs = Vec::new();
    for _ in 0..(1.0 / h) as usize {
        rig.runtime.advance(h, h).unwrap();
        zs.push(rig.state("base.z"));
    }
    let z_end = *zs.last().unwrap();
    let vz_end = rig.state("base.vz");
    // Bottom vertices: four corners and the face centre share the load.
    let bottom = rig.art.links[0].contact.iter().filter(|c| c.z < -0.049).count();
    let expected = 0.05 - 0.5 * 9.81 / (rig.art.floor_k * bottom as f64);
    println!("box rests at z = {z_end:.6} (expected {expected:.6} with {bottom} bottom vertices), vz = {vz_end:.2e}");
    assert!((z_end - expected).abs() < 2e-4, "rest height");
    assert!(vz_end.abs() < 1e-3, "at rest");
    let first_min = zs.iter().position(|z| *z < 0.051).unwrap();
    let rebound = zs[first_min..].iter().cloned().fold(0.0, f64::max);
    assert!(rebound < 0.2 && rebound >= z_end, "bounce decays: rebound {rebound}");
}

#[test]
fn sliding_block_stops_by_friction() {
    let mut m = empty_model();
    let k = m.world.floor_stiffness;
    let d0 = 0.5 * 9.81 / (k * 5.0);
    m.links.push(box_link("box", [0.1, 0.1, 0.1], 0.5, [0.0, 0.0, 0.05 - d0], false));
    let v0 = 1.0;
    let mut rig = Rig::new(m, &[("initial.base.vx", v0)], euler());
    let h = 5e-4;
    let x0 = rig.state("base.x");
    for _ in 0..(1.5 / h) as usize {
        rig.runtime.advance(h, h).unwrap();
    }
    let travelled = rig.state("base.x") - x0;
    let expected = v0 * v0 / (2.0 * 0.3 * 9.81);
    println!("sliding block stopped after {travelled:.4} m (Coulomb µk = 0.3 predicts {expected:.4} m); vx = {:.2e}", rig.state("base.vx"));
    assert!(rig.state("base.vx").abs() < 1e-2, "stopped");
    assert!((travelled - expected).abs() / expected < 0.15, "stopping distance");
}

#[test]
fn four_bar_loop_holds_closure() {
    let mut m = empty_model();
    m.links.push(box_link("ground", [0.4, 0.05, 0.02], 1.0, [0.15, 0.0, -0.01], true));
    m.links.push(box_link("crank", [0.02, 0.02, 0.1], 0.05, [0.0, 0.0, 0.05], false));
    m.links.push(box_link("coupler", [0.3, 0.02, 0.02], 0.1, [0.15, 0.0, 0.1], false));
    m.links.push(box_link("rocker", [0.02, 0.02, 0.1], 0.05, [0.3, 0.0, 0.05], false));
    let y = [0.0, 1.0, 0.0];
    m.joints.push(joint("a", "revolute", Some("ground"), "crank", [0.0, 0.0, 0.0], y));
    m.joints.push(joint("b", "revolute", Some("crank"), "coupler", [0.0, 0.0, 0.1], y));
    m.joints.push(joint("d", "revolute", Some("ground"), "rocker", [0.3, 0.0, 0.0], y));
    m.joints.push(joint("c", "loop_revolute", Some("coupler"), "rocker", [0.3, 0.0, 0.1], y));
    for j in &mut m.joints {
        j.physics.friction = Friction::default();
    }
    let mut rig = Rig::new(m, &[("contact", 0.0), ("initial.joint.a.angle", 0.4), ("initial.joint.b.angle", -0.4), ("initial.joint.d.angle", 0.4)], midpoint());
    let h = 1e-3;
    let mut worst: f64 = 0.0;
    let mut swing = 0.0f64;
    for _ in 0..(5.0 / h) as usize {
        rig.runtime.advance(h, h).unwrap();
        let g = rig.generalized();
        let poses = rig.art.poses(&g);
        let lp = &rig.art.loops[0];
        let pa = poses[lp.a].1 + poses[lp.a].0 * lp.r_a;
        let pb = poses[lp.b].1 + poses[lp.b].0 * lp.r_b;
        worst = worst.max((pa - pb).norm());
        swing = swing.max(rig.angle(0).abs());
    }
    println!("four-bar closure error over 5 s: {worst:.2e} m; crank swing up to {swing:.3} rad");
    assert!(worst < 1e-6, "closure drift {worst}");
    assert!(swing > 0.3, "the linkage moved");
}

#[test]
fn cantilever_sags_by_its_modal_data_and_softens_when_hot() {
    let k_modal = (2.0 * std::f64::consts::PI * 20.0).powi(2);
    let participation = [0.0, 0.0, -0.01, 0.0, 0.0, 0.0];
    let sag = 0.01 * 9.81 / k_modal;
    let mut m = empty_model();
    m.links.push(box_link("wall", [0.05, 0.05, 0.05], 1.0, [0.0, 0.0, 0.5], true));
    let mut beam = box_link("beam", [0.2, 0.02, 0.01], 0.05, [0.125, 0.0, 0.5], false);
    beam.flex = Some(Flex {
        normalization: ModalNormalization::Displacement,
        modes: 1,
        frequencies_hz: vec![20.0],
        damping_ratio: 0.03,
        boundary_frames: vec![BoundaryFrame { name: "root".into(), point: [-0.1, 0.0, 0.0], ..Default::default() }, BoundaryFrame { name: "tip".into(), point: [0.1, 0.0, 0.0], ..Default::default() }],
        modal_stiffness: vec![k_modal],
        modal_mass: vec![1.0],
        boundary_shapes: vec![vec![[0.0; 6], [0.0, 0.0, -1.0, 0.0, 0.0, 0.0]]],
        participation: vec![participation],
        stress_cells: vec![[-0.09, 0.0, 0.0]],
        stress_per_mode: vec![vec![[1.0e9, 0.0, 0.0, 0.0, 0.0, 0.0]]],
        gravity_sag_m: sag,
        softening: Softening { tg_c: 60.0, width_c: 5.0, ratio_above: 0.05 },
    });
    m.links.push(beam);
    let mut j = joint("root", "fixed", Some("wall"), "beam", [0.025, 0.0, 0.5], [0.0, 0.0, 1.0]);
    j.physics.stiffness = JointStiffness { radial: 1e7, axial: 1e7, bending: 1e5 };
    j.physics.damping_ratio = 0.5;
    m.joints.push(j);
    let mut rig = Rig::new(m, &[("contact", 0.0)], euler());
    let h = 1e-3;
    for _ in 0..(3.0 / h) as usize {
        rig.runtime.advance(h, h).unwrap();
    }
    let eta = rig.state("beam.eta0");
    println!("cantilever modal amplitude {eta:.4e} vs gravity sag {sag:.4e} m; stress {:.3e} Pa", rig.art.stress(1, &[eta])[0]);
    assert!((eta - sag).abs() / sag < 0.03, "sag");
    // Hot mount: stiffness falls to 5 %, the sag grows twentyfold.
    let m2 = rig.art.model.as_ref().clone();
    let hot = HotRig::new(m2, 373.15);
    assert!((hot / sag - 20.0).abs() / 20.0 < 0.05, "hot sag ratio {}", hot / sag);
}

/// The beam with its `temperature.beam` input driven by a constant.
struct HotRig;
impl HotRig {
    fn new(model: PhysicalModel, temperature: f64) -> f64 {
        use sim_core::ModelWorld;
        use std::sync::Arc;
        let registry = registry();
        let opts = options_from(&[("contact", 0.0)]);
        let art = sim_domain_robot::Articulated::new(Arc::new(model.clone()), &opts).unwrap();
        let handle = sim_domain_robot::register_model(model);
        let mut params: Vec<(&'static str, f64)> = vec![("model", handle), ("contact", 0.0)];
        for (k, v) in art.port_parameters() {
            params.push((Box::leak(k.into_boxed_str()), v));
        }
        let mut m = ModelWorld::default();
        let inst = m.part(&registry, "robot", sim_domain_robot::ARTICULATED, params).unwrap();
        m.connect([inst.port("frame.base")]);
        for name in art.port_names.iter().chain(&art.signal_out_names) {
            m.connect([inst.port(Box::leak(name.clone().into_boxed_str()))]);
        }
        let heat = m.part(&registry, "heat", sim_domain_control::elements::CONSTANT, [("value", temperature)]).unwrap();
        m.connect([heat.port("value"), inst.port("temperature.beam")]);
        let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
        let id = rt.state_id(inst.behavior, "beam.eta0");
        let h = 1e-3;
        for _ in 0..(3.0 / h) as usize {
            rt.advance(h, h).unwrap();
        }
        rt.get(id)
    }
}

#[test]
fn links_collide_through_their_distance_fields() {
    // A box resting on a grounded slab, both from geometry alone.
    let mut m = empty_model();
    m.links.push(box_link("slab", [0.3, 0.3, 0.02], 1.0, [0.0, 0.0, 0.11], true));
    m.links.push(box_link("box", [0.05, 0.05, 0.05], 0.2, [0.0, 0.0, 0.2], false));
    let mut rig = Rig::new(m, &[], euler());
    let h = 5e-4;
    for _ in 0..(1.0 / h) as usize {
        rig.runtime.advance(h, h).unwrap();
    }
    let z = rig.state("base1.z");
    println!("box on slab rests at z = {z:.5} (slab top at 0.12, box half 0.025)");
    assert!((z - 0.145).abs() < 1e-3, "rests on the slab, not the floor: z = {z}");
    assert!(rig.state("base1.vz").abs() < 1e-3);
}
