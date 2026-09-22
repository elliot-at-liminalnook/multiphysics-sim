//! Reference calculations, not a physical episode or optimizer run.
use sim_core::QuantityKind as Q;
use sim_domain_control::{
    motion_parameters::*, motion_primitives::*, reference_governor, trajectory::*,
};
use std::collections::BTreeMap;
fn constant(value: f64) -> Scalar {
    Scalar::Constant { value }
}
fn template() -> ContactTemplate {
    ContactTemplate {
        space: ParameterSpace {
            parameters: vec![Parameter {
                name: "stride".into(),
                kind: Q::Length,
                bounds: [-0.2, 0.2],
                integer: false,
            }],
        },
        body: TrajectoryTemplate {
            channels: (0..6)
                .map(|i| MotionChannel {
                    name: format!("body{i}"),
                    kind: if i < 3 { Q::Length } else { Q::Angle },
                })
                .collect(),
            reference: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=4)
                    .map(|i| Keyframe {
                        time_s: i as f64 / 4.,
                        values: vec![0.; 6],
                    })
                    .collect(),
            },
            transforms: vec![],
        },
        period_s: constant(1.),
        displacement_world_m: [
            Scalar::Parameter {
                name: "stride".into(),
            },
            constant(0.),
            constant(0.),
        ],
        feet: [0., 0.4]
            .into_iter()
            .map(|phase| FootTemplate {
                center_world_m: [constant(0.), constant(phase), constant(0.)],
                phase_offset: constant(phase),
                stance_fraction: constant(0.6),
                swing_offset_world_m: [constant(0.), constant(0.), constant(0.02)],
                return_ramp_fraction: None,
            })
            .collect(),
    }
}
#[test]
fn independent_contact_template_uses_shared_trajectory_and_checked_units() {
    let t = template();
    let values = BTreeMap::from([("stride".into(), 0.1)]);
    let motion = t.materialize(&values).unwrap();
    assert_eq!(motion.feet.len(), 2);
    let sampler = sim_domain_control::contact_phase::ContactPhaseMotion::new(motion).unwrap();
    let s = sampler.sample(0.1).unwrap();
    assert!(s.feet[0].in_contact);
    assert!(!s.feet[1].in_contact);
    let next = sampler.sample(1.1).unwrap();
    assert!((next.body.values[0] - s.body.values[0] - 0.1).abs() < 1e-12);
    let mut wrong = t.clone();
    wrong.space.parameters[0].kind = Q::Time;
    assert!(wrong.materialize(&values).is_err());
    let mut period = t;
    period.period_s = constant(2.);
    assert!(period.materialize(&values).is_err());
}
fn request() -> Govern {
    Govern {
        channels: vec![AngularChannel {
            name: "axis".into(),
            bounds_rad: [-1., 1.],
            governor: reference_governor::Config {
                period_s: 0.02,
                maximum_speed_rad_s: 1.,
                maximum_acceleration_rad_s2: 5.,
                response_rate_per_s: 10.,
            },
        }],
        states: BTreeMap::from([(
            "axis".into(),
            reference_governor::State {
                angle_rad: 0.,
                velocity_rad_s: 0.,
            },
        )]),
        requested_rad: BTreeMap::from([("axis".into(), 2.)]),
    }
}
#[test]
fn governor_bank_preserves_raw_request_and_shared_kernel_limits() {
    let r = request();
    let expected = r.channels[0].governor.update(r.states["axis"], 1.).unwrap();
    let result = govern(r).unwrap();
    assert_eq!(
        result.states["axis"].angle_rad.to_bits(),
        expected.angle_rad.to_bits()
    );
    assert_eq!(result.requested_rad["axis"], 2.);
    assert_eq!(result.target_clipped, vec!["axis"]);
}
#[test]
fn governor_rejects_unknown_channels_and_infeasible_boundary_state() {
    let mut r = request();
    r.requested_rad.insert("typo".into(), 0.);
    assert!(govern(r).is_err());
    let mut r = request();
    r.states.get_mut("axis").unwrap().angle_rad = 1.;
    r.states.get_mut("axis").unwrap().velocity_rad_s = 1.;
    assert!(govern(r).is_err());
    let mut r = request();
    r.requested_rad.insert("axis".into(), f64::NAN);
    assert!(govern(r).is_err());
}
