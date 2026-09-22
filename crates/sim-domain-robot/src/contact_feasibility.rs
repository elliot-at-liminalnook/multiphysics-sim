//! Instantaneous support/friction/actuator screening. A feasible supplied force
//! allocation is not a stability proof; a failed allocation is not proof that
//! every other allocation fails. No trajectories or contact events are advanced.
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    primitive::{Descriptor, Field as F},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentroidalInput {
    pub mass_kg: f64,
    /// World-frame inertia about COM, at the evaluated configuration.
    pub inertia_world_kg_m2: [[f64; 3]; 3],
    pub acceleration_world_m_s2: [f64; 3],
    pub gravity_world_m_s2: [f64; 3],
    pub angular_velocity_world_rad_s: [f64; 3],
    pub angular_acceleration_world_rad_s2: [f64; 3],
    pub external_force_world_n: [f64; 3],
    pub external_moment_about_com_world_nm: [f64; 3],
}
/// Required contact wrench about COM, with an explicit locked-body inertia
/// approximation. Omitted limb momentum must be audited against the full model.
pub fn required_wrench(input: CentroidalInput) -> Result<[f64; 6], String> {
    let i = Matrix3::from_fn(|r, c| input.inertia_world_kg_m2[r][c]);
    if !input.mass_kg.is_finite()
        || input.mass_kg <= 0.
        || i.iter().any(|x| !x.is_finite())
        || (i - i.transpose()).amax() > 1e-12 * (1. + i.amax())
        || i.cholesky().is_none()
    {
        return Err("positive mass and symmetric positive definite COM inertia required".into());
    }
    for v in [
        input.acceleration_world_m_s2,
        input.gravity_world_m_s2,
        input.angular_velocity_world_rad_s,
        input.angular_acceleration_world_rad_s2,
        input.external_force_world_n,
        input.external_moment_about_com_world_nm,
    ] {
        if v.iter().any(|x| !x.is_finite()) {
            return Err("finite world-frame centroidal inputs required".into());
        }
    }
    let omega = Vector3::from(input.angular_velocity_world_rad_s);
    let f = input.mass_kg
        * (Vector3::from(input.acceleration_world_m_s2) - Vector3::from(input.gravity_world_m_s2))
        - Vector3::from(input.external_force_world_n);
    let m = i * Vector3::from(input.angular_acceleration_world_rad_s2) + omega.cross(&(i * omega))
        - Vector3::from(input.external_moment_about_com_world_nm);
    let out = [f[0], f[1], f[2], m[0], m[1], m[2]];
    if out.iter().any(|x| !x.is_finite()) {
        return Err("centroidal wrench overflow".into());
    }
    Ok(out)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub point_world_m: [f64; 3],
    pub normal_world: [f64; 3],
    pub force_world_n: [f64; 3],
    pub friction_coefficient: f64,
    pub maximum_normal_force_n: f64,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub reference_world_m: [f64; 3],
    pub required_wrench_world: [f64; 6],
    pub contacts: Vec<Contact>,
    pub motor_torques_nm: Vec<f64>,
    pub motor_torque_bounds_nm: Vec<[f64; 2]>,
    pub force_tolerance_n: f64,
    pub moment_tolerance_nm: f64,
    pub torque_tolerance_nm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactCheck {
    pub normal_force_n: f64,
    pub tangential_force_n: f64,
    /// Positive values violate the bound. Negative margins remain observable.
    pub unilateral_violation_n: f64,
    pub friction_violation_n: f64,
    pub capacity_violation_n: f64,
    pub passes: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub contacts: Vec<ContactCheck>,
    pub wrench_residual_world: [f64; 6],
    pub motor_torque_violation_nm: Vec<f64>,
    pub force_balance_passes: bool,
    pub moment_balance_passes: bool,
    pub supplied_allocation_passes: bool,
}
pub fn evaluate(r: Request) -> Result<Report, String> {
    if [
        r.force_tolerance_n,
        r.moment_tolerance_nm,
        r.torque_tolerance_nm,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v < 0.)
        || r.reference_world_m
            .iter()
            .chain(r.required_wrench_world.iter())
            .any(|v| !v.is_finite())
        || r.motor_torques_nm.len() != r.motor_torque_bounds_nm.len()
    {
        return Err("invalid explicit feasibility inputs/tolerances".into());
    }
    let mut force = Vector3::zeros();
    let mut moment = Vector3::zeros();
    let mut contacts = vec![];
    for c in &r.contacts {
        let n = Vector3::from(c.normal_world);
        let f = Vector3::from(c.force_world_n);
        if c.point_world_m
            .iter()
            .chain(n.iter())
            .chain(f.iter())
            .any(|v| !v.is_finite())
            || (n.norm() - 1.).abs() > 1e-9
            || !c.friction_coefficient.is_finite()
            || c.friction_coefficient < 0.
            || !c.maximum_normal_force_n.is_finite()
            || c.maximum_normal_force_n < 0.
        {
            return Err(
                "finite contact, unit normal and nonnegative friction/capacity required".into(),
            );
        }
        let normal = n.dot(&f);
        let tangent = (f - n * normal).norm();
        let unilateral = -normal;
        let friction = tangent - c.friction_coefficient * normal;
        let capacity = normal - c.maximum_normal_force_n;
        let passes = if c.enabled {
            unilateral <= r.force_tolerance_n
                && friction <= r.force_tolerance_n
                && capacity <= r.force_tolerance_n
        } else {
            f.norm() <= r.force_tolerance_n
        };
        contacts.push(ContactCheck {
            normal_force_n: normal,
            tangential_force_n: tangent,
            unilateral_violation_n: unilateral,
            friction_violation_n: friction,
            capacity_violation_n: capacity,
            passes,
        });
        force += f;
        moment += (Vector3::from(c.point_world_m) - Vector3::from(r.reference_world_m)).cross(&f);
    }
    let residual = std::array::from_fn(|i| {
        if i < 3 {
            force[i] - r.required_wrench_world[i]
        } else {
            moment[i - 3] - r.required_wrench_world[i]
        }
    });
    let motor: Vec<f64> = r
        .motor_torques_nm
        .iter()
        .zip(&r.motor_torque_bounds_nm)
        .map(|(t, b)| {
            if !t.is_finite() || b.iter().any(|v| !v.is_finite()) || b[0] > b[1] {
                return Err("finite ordered torque bounds required".to_string());
            }
            Ok((b[0] - t).max(t - b[1]))
        })
        .collect::<Result<_, _>>()?;
    if residual.iter().chain(motor.iter()).any(|v| !v.is_finite())
        || contacts.iter().any(|c| {
            [
                c.normal_force_n,
                c.tangential_force_n,
                c.friction_violation_n,
                c.capacity_violation_n,
            ]
            .iter()
            .any(|v| !v.is_finite())
        })
    {
        return Err("feasibility calculation overflow".into());
    }
    let force_ok =
        Vector3::new(residual[0], residual[1], residual[2]).norm() <= r.force_tolerance_n;
    let moment_ok =
        Vector3::new(residual[3], residual[4], residual[5]).norm() <= r.moment_tolerance_nm;
    let pass = force_ok
        && moment_ok
        && contacts.iter().all(|c| c.passes)
        && motor.iter().all(|v| *v <= r.torque_tolerance_nm);
    Ok(Report {
        contacts,
        wrench_residual_world: residual,
        motor_torque_violation_nm: motor,
        force_balance_passes: force_ok,
        moment_balance_passes: moment_ok,
        supplied_allocation_passes: pass,
    })
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    registry.register_primitive(Descriptor::new("mechanics.centroidal_wrench","Required contact wrench about the center of mass",
        vec![F::structured("mass_kg","kg","scalar"),F::structured("inertia_world_kg_m2","kg·m²","3x3 matrix"),
            F::quantity("acceleration_world_m_s2",Q::LinearAcceleration,"xyz"),F::quantity("gravity_world_m_s2",Q::LinearAcceleration,"xyz"),
            F::quantity("angular_velocity_world_rad_s",Q::AngularVelocity,"xyz"),F::quantity("angular_acceleration_world_rad_s2",Q::AngularAcceleration,"xyz"),
            F::quantity("external_force_world_n",Q::Force,"xyz"),F::quantity("external_moment_about_com_world_nm",Q::Torque,"xyz")],
        vec![F::structured("$","N,N,N,N·m,N·m,N·m","six-vector")],
        &["World-frame quantities about COM; supplied locked-body inertia omits relative limb momentum", "Not a whole-body dynamics replacement"]),required_wrench)?;
    registry.register_primitive(Descriptor::new("contact.force_feasibility","Support, arbitrary-normal friction and motor load checks",
        vec![F::quantity("reference_world_m",Q::Length,"xyz"),F::structured("required_wrench_world","N,N,N,N·m,N·m,N·m","six-vector"),
            F::quantity("contacts[].point_world_m",Q::Length,"xyz"),F::quantity("contacts[].normal_world",Q::Dimensionless,"unit xyz"),
            F::quantity("contacts[].force_world_n",Q::Force,"xyz"),F::quantity("contacts[].friction_coefficient",Q::Dimensionless,"scalar"),
            F::quantity("contacts[].maximum_normal_force_n",Q::Force,"scalar"),F::structured("contacts[].enabled","1","boolean"),
            F::quantity("motor_torques_nm",Q::Torque,"vector"),F::quantity("motor_torque_bounds_nm",Q::Torque,"array of pairs"),
            F::quantity("force_tolerance_n",Q::Force,"scalar"),F::quantity("moment_tolerance_nm",Q::Torque,"scalar"),F::quantity("torque_tolerance_nm",Q::Torque,"scalar")],
        vec![F::structured("contacts","N; boolean passes","ContactCheck array"),F::structured("wrench_residual_world","N,N,N,N·m,N·m,N·m","six-vector"),
            F::quantity("motor_torque_violation_nm",Q::Torque,"vector"),F::structured("force_balance_passes","1","boolean"),F::structured("moment_balance_passes","1","boolean"),F::structured("supplied_allocation_passes","1","boolean")],
        &["Instantaneous supplied force allocation; does not prove trajectory stability or impossibility", "Motor bounds are authored or obtained from an explicit actuator model"]),evaluate)
}
