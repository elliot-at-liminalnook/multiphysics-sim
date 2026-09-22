//! Typed sampled angular-feedback/duty seam and a reusable bounded position PID.
//! No actuator physics; hardware and simulated hosts call the same control law.
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, ParameterDeclaration as P, QuantityKind as Q,
    RegistryError, signal_in, signal_out,
};
pub const FEEDBACK: &str = "control.angular_pwm_feedback";
pub const SCHEDULED_FEEDBACK: &str = "control.scheduled_angular_pwm_feedback";
pub const ELECTRICAL_FEEDBACK: &str = "control.electrical_angular_pwm_feedback";
pub const SCHEDULED_ELECTRICAL_FEEDBACK: &str = "control.scheduled_electrical_angular_pwm_feedback";
fn electrical_external(
    p: &std::collections::BTreeMap<String, f64>,
    scheduled: bool,
) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> {
    let mut e = crate::external::External::new(
        vec![
            "angle".into(),
            "supply_voltage".into(),
            "supply_current".into(),
            "winding_current".into(),
        ],
        vec!["duty".into()],
        if scheduled {
            1.
        } else {
            sim_core::param(p, "period")?
        },
    );
    if scheduled {
        let count = sim_core::param(p, "count")? as usize;
        let times = (0..count)
            .map(|i| sim_core::param(p, &format!("time.{i}")))
            .collect::<Result<Vec<_>, _>>()?;
        if times.is_empty()
            || times.iter().any(|t| !t.is_finite() || *t < 0.)
            || times.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(sim_core::EquationError::InvalidParameter(
                "time.*".into(),
                "Expected ordered nonnegative schedule".into(),
            ));
        }
        e.offset = times[0];
        e.schedule = Some(times);
    }
    Ok(Box::new(e))
}
pub fn register(r: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    for (kind, scheduled) in [
        (ELECTRICAL_FEEDBACK, false),
        (SCHEDULED_ELECTRICAL_FEEDBACK, true),
    ] {
        r.register(
            BehaviorDescriptor::new(
                kind,
                "Sampled angle and electrical feedback with PWM duty",
                vec![
                    signal_in("sense.angle", Q::Angle),
                    signal_in("sense.supply_voltage", Q::Voltage),
                    signal_in("sense.supply_current", Q::Current),
                    signal_in("sense.winding_current", Q::Current),
                    signal_out("act.duty", Q::Dimensionless),
                ],
                if scheduled {
                    |p| electrical_external(p, true)
                } else {
                    |p| electrical_external(p, false)
                },
            )
            .with_parameters(if scheduled {
                vec![
                    P::required("count", "1").integer(1., 300_000.),
                    P::alternative("time.*", "s"),
                ]
            } else {
                vec![P::required("period", "s").positive()]
            }),
        )?;
    }

    r.register(
        BehaviorDescriptor::new(
            FEEDBACK,
            "Sampled angular feedback and PWM duty",
            vec![
                signal_in("sense.angle", Q::Angle),
                signal_out("act.duty", Q::Dimensionless),
            ],
            |p| {
                Ok(Box::new(crate::external::External::new(
                    vec!["angle".into()],
                    vec!["duty".into()],
                    sim_core::param(p, "period")?,
                )))
            },
        )
        .with_parameters(vec![P::required("period", "s").positive()]),
    )?;
    r.register(
        BehaviorDescriptor::new(
            SCHEDULED_FEEDBACK,
            "Angular feedback/PWM at captured event times",
            vec![
                signal_in("sense.angle", Q::Angle),
                signal_out("act.duty", Q::Dimensionless),
            ],
            |p| {
                let count = sim_core::param(p, "count")? as usize;
                let times = (0..count)
                    .map(|i| sim_core::param(p, &format!("time.{i}")))
                    .collect::<Result<Vec<_>, _>>()?;
                if times.is_empty()
                    || times.iter().any(|t| !t.is_finite() || *t < 0.)
                    || times.windows(2).any(|w| w[0] >= w[1])
                {
                    return Err(sim_core::EquationError::InvalidParameter(
                        "time.*".into(),
                        "schedule requires ordered nonnegative times".into(),
                    ));
                }
                let mut external =
                    crate::external::External::new(vec!["angle".into()], vec!["duty".into()], 1.);
                external.offset = times[0];
                external.schedule = Some(times);
                Ok(Box::new(external))
            },
        )
        .with_parameters(vec![
            P::required("count", "1").integer(1., 300_000.),
            P::alternative("time.*", "s"),
        ]),
    )
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pid {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub integral_limit: f64,
    pub duty_limit: f64,
}
impl Default for Pid {
    fn default() -> Self {
        Self {
            kp: 0.8,
            ki: 0.,
            kd: 0.03,
            integral_limit: 0.2,
            duty_limit: 0.2,
        }
    }
}
impl Pid {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.kp,
            self.ki,
            self.kd,
            self.integral_limit,
            self.duty_limit,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
            || self.duty_limit == 0.
            || self.duty_limit > 1.
        {
            Err("PID values must be finite/nonnegative and duty limit in (0,1]".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PidState {
    pub integral: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub duty: f64,
    pub unsaturated: f64,
    pub saturated: bool,
}
impl PidState {
    pub fn step(
        &mut self,
        c: &Pid,
        target: f64,
        angle: f64,
        velocity: f64,
        dt: f64,
    ) -> Result<Action, String> {
        c.validate()?;
        if [target, angle, velocity, dt].iter().any(|v| !v.is_finite()) || dt <= 0. {
            return Err("Invalid feedback state or elapsed time".into());
        }
        let error = target - angle;
        let candidate = (self.integral + error * dt).clamp(-c.integral_limit, c.integral_limit);
        let raw = c.kp * error + c.ki * candidate - c.kd * velocity;
        // Conditional integration permits unwinding while preventing further saturation.
        if raw.abs() <= c.duty_limit || error * raw < 0. {
            self.integral = candidate;
        }
        let raw = c.kp * error + c.ki * self.integral - c.kd * velocity;
        if !raw.is_finite() {
            return Err("Nonfinite PID output".into());
        }
        Ok(Action {
            duty: raw.clamp(-c.duty_limit, c.duty_limit),
            unsaturated: raw,
            saturated: raw.abs() > c.duty_limit,
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EncoderEstimate {
    pub last_time: Option<f64>,
    pub last_angle: f64,
    pub position: f64,
    pub velocity: f64,
}
impl EncoderEstimate {
    /// Unwrap a cyclic encoder; repeated observations retain the previous estimate.
    pub fn observe(
        &mut self,
        time: f64,
        angle: f64,
        wrap: f64,
        tau: f64,
    ) -> Result<(f64, f64), String> {
        if [time, angle, wrap, tau].iter().any(|v| !v.is_finite()) || wrap <= 0. || tau < 0. {
            return Err("Invalid encoder observation".into());
        }
        if let Some(previous) = self.last_time {
            if time < previous {
                return Err("Encoder observations arrived out of time order".into());
            }
            if time == previous {
                return Ok((self.position, self.velocity));
            }
            let delta = (angle - self.last_angle + wrap * 0.5).rem_euclid(wrap) - wrap * 0.5;
            let dt = time - previous;
            self.position += delta;
            let alpha = if tau == 0. { 1. } else { dt / (tau + dt) };
            self.velocity += alpha * (delta / dt - self.velocity);
        } else {
            self.position = angle;
        }
        self.last_angle = angle;
        self.last_time = Some(time);
        Ok((self.position, self.velocity))
    }
}
