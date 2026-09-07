mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::articulated::constraints::RankConfig;
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn four_bar() -> (Articulated, Generalized) {
    let mut m = empty_model();
    m.links.push(box_link(
        "ground",
        [0.4, 0.05, 0.02],
        1.0,
        [0.15, 0.0, -0.01],
        true,
    ));
    m.links.push(box_link(
        "crank",
        [0.02, 0.02, 0.1],
        0.05,
        [0.0, 0.0, 0.05],
        false,
    ));
    m.links.push(box_link(
        "coupler",
        [0.3, 0.02, 0.02],
        0.1,
        [0.15, 0.0, 0.1],
        false,
    ));
    m.links.push(box_link(
        "rocker",
        [0.02, 0.02, 0.1],
        0.05,
        [0.3, 0.0, 0.05],
        false,
    ));
    let y = [0.0, 1.0, 0.0];
    m.joints.push(joint(
        "a",
        "revolute",
        Some("ground"),
        "crank",
        [0.0, 0.0, 0.0],
        y,
    ));
    m.joints.push(joint(
        "b",
        "revolute",
        Some("crank"),
        "coupler",
        [0.0, 0.0, 0.1],
        y,
    ));
    m.joints.push(joint(
        "d",
        "revolute",
        Some("ground"),
        "rocker",
        [0.3, 0.0, 0.0],
        y,
    ));
    m.joints.push(joint(
        "c",
        "loop_revolute",
        Some("coupler"),
        "rocker",
        [0.3, 0.0, 0.1],
        y,
    ));
    let art = Articulated::new(
        Arc::new(m),
        &Options {
            contact: false,
            structural_loop_identities: true,
            ..Options::default()
        },
    )
    .unwrap();
    let states = art.states().iter().map(|s| s.initial).collect();
    let g = art.generalized(
        states,
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    (art, g)
}

#[test]
fn original_closure_and_rank_track_a_moving_four_bar_and_its_toggle() {
    let (art, mut g) = four_bar();
    let config = RankConfig {
        length_scale_m: 0.1,
        ..RankConfig::default()
    };
    for angle in [-1.0_f64, -0.4, 0.0, 0.4, 1.0, std::f64::consts::FRAC_PI_2] {
        // Parallel crank and rocker, coupler orientation fixed.
        for (i, (j, _)) in art.dofs().enumerate() {
            g.q[i] = if j.name == "b" { -angle } else { angle };
            g.qd[i] = if j.name == "b" { -0.3 } else { 0.3 };
        }
        let audit = art.audit_constraints(&g, &config).unwrap();
        let direct = art.rigid_closure_velocity_jacobian(&g).unwrap();
        for i in 0..direct.nrows() {
            for j in 0..direct.ncols() {
                assert!(
                    (direct[(i, j)] * audit.column_scales[j] / audit.row_scales[i]
                        - audit.scaled_velocity_matrix[i][j])
                        .abs()
                        < 1e-12
                );
            }
        }
        assert_eq!(audit.rows.len(), 5);
        assert!(
            audit
                .rows
                .iter()
                .all(|r| r.position.abs() < 1e-14 && r.velocity.abs() < 1e-14)
        );
        assert_eq!(
            audit.rows.iter().filter(|r| r.certified_identity).count(),
            2
        );
        let expected = if angle == std::f64::consts::FRAC_PI_2 {
            1
        } else {
            2
        };
        assert_eq!(
            audit.svd_rank, expected,
            "angle {angle}: {:?}",
            audit.singular_values
        );
        assert_eq!(audit.qr_rank, expected);
        let selected = nalgebra::DMatrix::from_fn(expected, audit.coordinates.len(), |i, j| {
            audit.scaled_velocity_matrix[audit.independent_rows[i]][j]
        });
        assert!(
            selected
                .svd(false, false)
                .singular_values
                .iter()
                .all(|s| *s > 1e-10)
        );
        // Independent configuration differences check the velocity-basis map.
        for col in 0..g.q.len() {
            let mut plus = g.clone();
            let mut minus = g.clone();
            let eps = 1e-6;
            plus.q[col] += eps;
            minus.q[col] -= eps;
            let p = art.original_closure(&plus);
            let m = art.original_closure(&minus);
            for row in 0..p.len() {
                let fd = (p[row].position - m[row].position) / (2.0 * eps);
                let scaled = fd * audit.column_scales[col] / audit.row_scales[row];
                assert!((scaled - audit.scaled_velocity_matrix[row][col]).abs() < 1e-8);
            }
        }
    }
}

#[test]
fn direct_closure_map_matches_probes_for_spatial_mixed_joints_and_multiple_bases() {
    let mut model = empty_model();
    model.links = vec![
        box_link("base", [0.1; 3], 1.0, [0.0, 0.0, 1.0], false),
        box_link("ball", [0.1; 3], 1.0, [0.2, 0.1, 0.9], false),
        box_link("compliant", [0.1; 3], 1.0, [0.4, 0.1, 0.8], false),
        box_link("slide", [0.1; 3], 1.0, [-0.2, 0.1, 0.9], false),
        box_link("free", [0.1; 3], 1.0, [1.0, 0.0, 1.0], false),
    ];
    model.joints = vec![
        joint(
            "ball",
            "ball",
            Some("base"),
            "ball",
            [0.1, 0.0, 1.0],
            [0.2, 0.3, 0.8],
        ),
        joint(
            "fixed",
            "fixed",
            Some("ball"),
            "compliant",
            [0.3, 0.1, 0.9],
            [0.4, 0.3, 0.7],
        ),
        joint(
            "slide",
            "prismatic",
            Some("base"),
            "slide",
            [-0.1, 0.0, 1.0],
            [0.5, -0.2, 0.7],
        ),
        joint(
            "loop",
            "loop_revolute",
            Some("compliant"),
            "slide",
            [0.0, 0.1, 0.8],
            [0.3, 0.7, 0.2],
        ),
        joint(
            "cross_base",
            "loop_revolute",
            Some("ball"),
            "free",
            [0.6, 0.0, 0.9],
            [-0.2, 0.4, 0.7],
        ),
    ];
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            contact: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mut states: Vec<_> = art.states().iter().map(|s| s.initial).collect();
    for b in &art.bases {
        states[b.state + 3..b.state + 7].copy_from_slice(&[0.9, 0.1, -0.2, 0.3]);
    }
    let mut g = art.generalized(
        states,
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    for phase in [-0.7_f64, 0.0, 0.4] {
        for (i, q) in g.q.iter_mut().enumerate() {
            *q = 0.1 * (i as f64 + phase).sin();
        }
        let direct = art.rigid_closure_velocity_jacobian(&g).unwrap();
        let audit = art
            .audit_constraints(
                &g,
                &RankConfig {
                    length_scale_m: 0.1,
                    angle_scale_rad: 0.3,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(direct.shape(), (10, 22));
        for i in 0..direct.nrows() {
            for j in 0..direct.ncols() {
                let expected = audit.scaled_velocity_matrix[i][j] * audit.row_scales[i]
                    / audit.column_scales[j];
                assert!(
                    (direct[(i, j)] - expected).abs() < 2e-14,
                    "row {i}, col {j}, phase {phase}: {} vs {expected}",
                    direct[(i, j)]
                );
            }
        }
        // Misaligned axes have nonzero angular derivatives; testing only planar
        // identity rows would miss signs and the second body's contribution.
        assert!(direct.rows(3, 2).amax() > 0.1);
    }
    let mut bad = g.clone();
    bad.q.pop();
    assert!(art.rigid_closure_velocity_jacobian(&bad).is_err());
}

#[test]
fn direct_closure_map_preserves_signed_transmission_rows() {
    let (old, _) = four_bar();
    let mut model = (*old.model).clone();
    model
        .transmissions
        .push(sim_domain_robot::model::Transmission {
            name: "signed gear".into(),
            driver_joint: "a".into(),
            driven_joint: "d".into(),
            ratio: -2.0,
        });
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            contact: false,
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let direct = art.rigid_closure_velocity_jacobian(&g).unwrap();
    let audit = art.audit_constraints(&g, &Default::default()).unwrap();
    assert_eq!(direct.nrows(), 6);
    let a = art.dofs().position(|(j, _)| j.name == "a").unwrap();
    let d = art.dofs().position(|(j, _)| j.name == "d").unwrap();
    assert_eq!(direct[(5, a)], 1.0);
    assert_eq!(direct[(5, d)], 2.0);
    for i in 0..direct.nrows() {
        for j in 0..direct.ncols() {
            assert!((direct[(i, j)] - audit.scaled_velocity_matrix[i][j]).abs() < 1e-14);
        }
    }
}

#[test]
fn closure_metadata_covers_alignment_and_transmission_rows_in_original_order() {
    use sim_domain_robot::model::Transmission;
    let (old,_) = four_bar();
    let mut model=(*old.model).clone();
    model.transmissions.push(Transmission{name:"signed gear".into(),driver_joint:"a".into(),driven_joint:"d".into(),ratio:-2.0});
    let art=Articulated::new(Arc::new(model),&Options {flex:false,contact:false,..Default::default()}).unwrap();
    let mut g=art.generalized(art.states().iter().map(|s|s.initial).collect(),vec![0.0;art.state_count],&vec![0.0;art.port_names.len()+1],vec![]);
    assert_eq!(art.original_closure_units(),["m","m","m","1","1","rad"]);
    for phase in [-0.4,0.0,0.7] {
        for i in 0..g.q.len(){g.q[i]=phase*(i+1) as f64;g.qd[i]=0.2-phase;g.qdd[i]=phase+0.3;}
        let values=art.original_closure_values(&g);
        let named=art.original_closure(&g);
        assert_eq!(named.iter().map(|r|r.name.as_str()).collect::<Vec<_>>(),
            ["c.position.0","c.position.1","c.position.2","c.alignment.0","c.alignment.1","signed gear"]);
        for ((value,name),unit) in values.iter().zip(&named).zip(art.original_closure_units()){
            assert_eq!(value.unit,unit);assert_eq!(name.unit,unit);
            assert_eq!([value.position,value.velocity,value.acceleration,value.stabilized].map(f64::to_bits),
                [name.position,name.velocity,name.acceleration,name.stabilized].map(f64::to_bits));
        }
        for (i,t) in art.transmissions.iter().enumerate(){
            let v=values[5+i];
            assert_eq!(v.position,g.q[t.driver]-t.ratio*g.q[t.driven]);
            assert_eq!(v.velocity,g.qd[t.driver]-t.ratio*g.qd[t.driven]);
            assert_eq!(v.acceleration,g.qdd[t.driver]-t.ratio*g.qdd[t.driven]);
        }
    }
}

#[test]
fn audit_keeps_every_stabilization_and_cfm_term_without_mutating_the_model() {
    let (art, mut g) = four_bar();
    g.q[0] = 0.17;
    g.qd[0] = 0.23;
    g.qdd[0] = -0.41;
    for lp in &art.loops {
        for k in 0..lp.rows {
            g.states[lp.lambda_state + k] = 0.7 + k as f64;
        }
    }
    // Use the unmodified equation evaluator as an independent reference.
    let mut original = art.clone();
    for lp in &mut original.loops {
        lp.angular_redundant = false;
    }
    let expected = original.evaluate(&g).loop_rows;
    let rows = art.original_closure(&g);
    let values = art.original_closure_values(&g);
    let units = art.original_closure_units();
    assert_eq!(values.len(), rows.len());
    assert_eq!(units.len(), rows.len());
    for ((v, r), unit) in values.iter().zip(&rows).zip(units) {
        assert_eq!(v.unit,unit);
        assert_eq!(unit,r.unit);
        assert_eq!([v.position,v.velocity,v.acceleration,v.stabilized].map(f64::to_bits),
            [r.position,r.velocity,r.acceleration,r.stabilized].map(f64::to_bits));
        assert_eq!(v.certified_identity,r.certified_identity);
    }
    for (row, expected) in rows.iter().zip(expected) {
        assert!((row.stabilized - expected).abs() < 1e-12, "{}", row.name);
    }
    assert!(art.loops[0].angular_redundant);
    assert!(
        art.audit_constraints(
            &g,
            &RankConfig {
                length_scale_m: 0.0,
                ..RankConfig::default()
            }
        )
        .is_err()
    );
}

#[test]
fn empty_and_identically_zero_constraint_maps_have_rank_zero() {
    let (mut art, g) = four_bar();
    art.loops[0].b = art.loops[0].a;
    art.loops[0].r_b = art.loops[0].r_a;
    let audit = art.audit_constraints(&g, &Default::default()).unwrap();
    assert_eq!((audit.qr_rank, audit.svd_rank), (0, 0));
    assert!(audit.independent_rows.is_empty());
    // Recompile without the closing joint. Clearing compiled rows in place
    // leaves their multiplier lanes in state_count and breaks the state schema.
    let mut model = (*art.model).clone();
    model.joints.retain(|j| j.kind != "loop_revolute");
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            contact: false,
            ..Options::default()
        },
    )
    .unwrap();
    let states = art.states().iter().map(|s| s.initial).collect();
    let g = art.generalized(
        states,
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let audit = art.audit_constraints(&g, &Default::default()).unwrap();
    assert!(audit.rows.is_empty());
    assert_eq!((audit.qr_rank, audit.svd_rank), (0, 0));
}

#[test]
fn generalized_reactions_match_inverse_dynamics_and_ignore_a_null_multiplier() {
    let (art, mut g) = four_bar();
    g.q[0] = 0.2;
    g.q[1] = -0.1;
    let mut original = art.clone();
    for lp in &mut original.loops {
        lp.angular_redundant = false;
    }
    let unloaded = original.evaluate(&g);
    let lp = &art.loops[0];
    g.states[lp.lambda_state] = 1.2;
    g.states[lp.lambda_state + 2] = -0.8;
    let loaded = original.evaluate(&g);
    let audit = art.audit_constraints(&g, &Default::default()).unwrap();
    let mut col = 0;
    for (j, (a, b)) in art
        .joints
        .iter()
        .zip(unloaded.joints.iter().zip(&loaded.joints))
    {
        for k in 0..j.dofs.len() {
            assert!(
                (audit.generalized_reactions[col] - (a.tau_needed[k] - b.tau_needed[k])).abs()
                    < 1e-12
            );
            col += 1;
        }
    }
    // The out-of-plane multiplier does no generalized work in this planar rig.
    g.states[lp.lambda_state + 1] = 1e6;
    let changed = art.audit_constraints(&g, &Default::default()).unwrap();
    for (a, b) in audit
        .generalized_reactions
        .iter()
        .zip(&changed.generalized_reactions)
    {
        assert!((a - b).abs() < 1e-12);
    }
}
