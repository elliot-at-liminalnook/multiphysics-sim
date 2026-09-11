//! Read-only motion-response measurements. These do not alter rewards, commands,
//! contacts or termination criteria, and do not infer hardware sensors.
use crate::environment::{Axis, ObservationSource, Task, Transition};
use serde::{Deserialize, Serialize};
use sim_domain_control::displacement::{DisplacementAxes, measure};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeadingSample {
    pub angle_rad: f64,
    pub rate_rad_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BodySample {
    pub time_s: f64,
    pub position_world_m: [f64; 3],
    pub velocity_world_m_s: [f64; 3],
    pub heading_world_z: Option<HeadingSample>,
}

/// Binds semantic observation sources, independently of channel names/order.
/// Heading is opt-in and requires a declared body direction and angular velocity.
pub struct BodyBinding {
    count: usize,
    position: [Vec<usize>; 3],
    velocity: [Vec<usize>; 3],
    heading: Option<([Vec<usize>; 3], [Vec<usize>; 3])>,
}
fn axis(a: &Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}
impl BodyBinding {
    pub fn new(task: &Task, link: &str, forward_axis: Option<Axis>) -> Result<Self, String> {
        let select = |kind: usize, component: usize| -> Result<Vec<usize>, String> {
            let found: Vec<_> = task
                .observations
                .iter()
                .enumerate()
                .filter_map(|(i, o)| {
                    let matched = match &o.source {
                        ObservationSource::BodyPosition { link: l, axis: a } => {
                            kind == 0 && l == link && axis(a) == component
                        }
                        ObservationSource::BodyVelocity { link: l, axis: a } => {
                            kind == 1 && l == link && axis(a) == component
                        }
                        ObservationSource::BodyAngularVelocity { link: l, axis: a } => {
                            kind == 2 && l == link && axis(a) == component
                        }
                        ObservationSource::BodyAxis {
                            link: l,
                            body_axis,
                            world_axis,
                        } => {
                            kind == 3
                                && l == link
                                && forward_axis
                                    .as_ref()
                                    .is_some_and(|a| axis(a) == axis(body_axis))
                                && axis(world_axis) == component
                        }
                        _ => false,
                    };
                    matched.then_some(i)
                })
                .collect();
            if found.is_empty() {
                return Err(format!(
                    "declared body observation required: link={link}, kind={kind}, axis={component}"
                ));
            }
            Ok(found)
        };
        let vector = |kind| Ok::<_, String>([select(kind, 0)?, select(kind, 1)?, select(kind, 2)?]);
        Ok(Self {
            count: task.observations.len(),
            position: vector(0)?,
            velocity: vector(1)?,
            heading: forward_axis
                .as_ref()
                .map(|_| Ok::<_, String>((vector(3)?, vector(2)?)))
                .transpose()?,
        })
    }
    pub fn sample(&self, t: &Transition) -> Result<BodySample, String> {
        if t.observations.len() != self.count || !t.time_s.is_finite() {
            return Err("motion response observation shape/time mismatch".into());
        }
        let vector = |indices: &[Vec<usize>; 3]| -> Result<[f64; 3], String> {
            let mut v = [0.; 3];
            for (component, aliases) in indices.iter().enumerate() {
                v[component] = t.observations[aliases[0]];
                if !v[component].is_finite()
                    || aliases.iter().any(|i| t.observations[*i] != v[component])
                {
                    return Err("nonfinite or conflicting aliased body observations".into());
                }
            }
            Ok(v)
        };
        let heading_world_z = self
            .heading
            .as_ref()
            .map(|(forward, omega)| {
                let [angle_rad, rate_rad_s] =
                    sim_domain_control::heading::world_z_heading(vector(forward)?, vector(omega)?)?;
                Ok::<_, String>(HeadingSample {
                    angle_rad,
                    rate_rad_s,
                })
            })
            .transpose()?;
        Ok(BodySample {
            time_s: t.time_s,
            position_world_m: vector(&self.position)?,
            velocity_world_m_s: vector(&self.velocity)?,
            heading_world_z,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowResponse {
    pub start_s: f64,
    pub end_s: f64,
    pub samples: usize,
    pub axes: DisplacementAxes,
    pub displacement_world_m: [f64; 3],
    pub net_distance_m: f64,
    pub net_speed_m_s: f64,
    /// Trapezoidal integral of selected velocity magnitude, divided by duration.
    pub mean_speed_m_s: f64,
    pub start_speed_m_s: f64,
    pub end_speed_m_s: f64,
    pub maximum_sampled_speed_m_s: f64,
    /// Maximum norm of interval-average delta-velocity / delta-time. Not an
    /// instantaneous acceleration peak or an additional physics state.
    pub maximum_interval_acceleration_m_s2: f64,
    /// Integral of sampled projected heading rate; can exceed pi without wrap
    /// aliasing. None if any sample lacks heading. Quadrature error remains.
    pub integrated_heading_change_rad: Option<f64>,
    /// Endpoint orientation change in [-pi, pi]; does not count full turns.
    pub wrapped_heading_change_rad: Option<f64>,
    /// Sum of shortest signed changes between sampled orientations. Correct
    /// turn count requires less than pi true rotation between samples.
    pub sampled_unwrapped_heading_change_rad: Option<f64>,
    pub maximum_sampled_heading_increment_rad: Option<f64>,
}

pub fn summarize(samples: &[BodySample], axes: DisplacementAxes) -> Result<WindowResponse, String> {
    if samples.len() < 2 {
        return Err("motion response needs at least two samples".into());
    }
    let norm = |v: [f64; 3]| measure([0.; 3], v, axes).map(|m| m.distance_m);
    for s in samples {
        if !s.time_s.is_finite()
            || s.position_world_m
                .iter()
                .chain(&s.velocity_world_m_s)
                .any(|v| !v.is_finite())
            || s.heading_world_z
                .as_ref()
                .is_some_and(|h| !h.angle_rad.is_finite() || !h.rate_rad_s.is_finite())
        {
            return Err("finite motion response samples required".into());
        }
    }
    let first = &samples[0];
    let last = samples.last().unwrap();
    let duration = last.time_s - first.time_s;
    if !duration.is_finite() || duration <= 0. {
        return Err("positive finite response duration required".into());
    }
    let displacement = measure(first.position_world_m, last.position_world_m, axes)?;
    let mut integral = 0.;
    let mut maximum_speed = norm(first.velocity_world_m_s)?;
    let mut acceleration: f64 = 0.;
    let mut turn = Some(0.);
    let mut unwrapped = Some(0.);
    let mut maximum_heading_increment = Some(0_f64);
    let wrap = |angle: f64| angle.sin().atan2(angle.cos());
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let dt = b.time_s - a.time_s;
        if !dt.is_finite() || dt <= 0. {
            return Err("strictly increasing response timestamps required".into());
        }
        let (va, vb) = (norm(a.velocity_world_m_s)?, norm(b.velocity_world_m_s)?);
        integral += (va / 2. + vb / 2.) * dt;
        maximum_speed = maximum_speed.max(vb);
        acceleration = acceleration.max(norm(std::array::from_fn(|i| {
            (b.velocity_world_m_s[i] - a.velocity_world_m_s[i]) / dt
        }))?);
        turn = match (turn, &a.heading_world_z, &b.heading_world_z) {
            (Some(sum), Some(ha), Some(hb)) => {
                Some(sum + (ha.rate_rad_s / 2. + hb.rate_rad_s / 2.) * dt)
            }
            _ => None,
        };
        match (&a.heading_world_z, &b.heading_world_z) {
            (Some(ha), Some(hb)) => {
                let delta = wrap(hb.angle_rad - ha.angle_rad);
                unwrapped = unwrapped.map(|sum| sum + delta);
                maximum_heading_increment = maximum_heading_increment.map(|v| v.max(delta.abs()));
            }
            _ => {
                unwrapped = None;
                maximum_heading_increment = None;
            }
        }
    }
    if !integral.is_finite()
        || turn.is_some_and(|v| !v.is_finite())
        || unwrapped.is_some_and(|v| !v.is_finite())
    {
        return Err("motion response integral overflow".into());
    }
    let result = WindowResponse {
        start_s: first.time_s,
        end_s: last.time_s,
        samples: samples.len(),
        axes,
        displacement_world_m: displacement.displacement_m,
        net_distance_m: displacement.distance_m,
        net_speed_m_s: displacement.distance_m / duration,
        mean_speed_m_s: integral / duration,
        start_speed_m_s: norm(first.velocity_world_m_s)?,
        end_speed_m_s: norm(last.velocity_world_m_s)?,
        maximum_sampled_speed_m_s: maximum_speed,
        maximum_interval_acceleration_m_s2: acceleration,
        integrated_heading_change_rad: turn,
        wrapped_heading_change_rad: first
            .heading_world_z
            .as_ref()
            .zip(last.heading_world_z.as_ref())
            .map(|(a, b)| wrap(b.angle_rad - a.angle_rad)),
        sampled_unwrapped_heading_change_rad: unwrapped,
        maximum_sampled_heading_increment_rad: maximum_heading_increment,
    };
    if !result.net_speed_m_s.is_finite() || !result.mean_speed_m_s.is_finite() {
        return Err("motion response rate overflow".into());
    }
    Ok(result)
}
