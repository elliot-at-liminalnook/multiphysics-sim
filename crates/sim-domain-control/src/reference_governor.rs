//! Explicitly changes a requested angle into a speed/acceleration-limited command.
//! This is reference conditioning, not improved tracking of the original signal.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ParameterDeclaration as P,
    QuantityKind as Q, RegistryError, StateDeclaration, View, param, signal_in, signal_out,
};
pub const KIND: &str = "control.reference_governor";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub period_s: f64,
    pub maximum_speed_rad_s: f64,
    pub maximum_acceleration_rad_s2: f64,
    pub response_rate_per_s: f64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub angle_rad: f64,
    pub velocity_rad_s: f64,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.period_s,
            self.maximum_speed_rad_s,
            self.maximum_acceleration_rad_s2,
            self.response_rate_per_s,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x <= 0.)
            || self.period_s * self.response_rate_per_s > 0.25
        {
            return Err(
                "Governor requires finite positive bounds and response_rate * period <= 0.25"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn update(&self, state: State, desired_rad: f64) -> Result<State, String> {
        self.validate()?;
        if !desired_rad.is_finite()
            || !state.angle_rad.is_finite()
            || !state.velocity_rad_s.is_finite()
        {
            return Err("Governor requires a finite target and bounded prior state".into());
        }
        let w = self.response_rate_per_s;
        let acceleration =
            (w * w * (desired_rad - state.angle_rad) - 2. * w * state.velocity_rad_s).clamp(
                -self.maximum_acceleration_rad_s2,
                self.maximum_acceleration_rad_s2,
            );
        let desired_velocity = (state.velocity_rad_s + self.period_s * acceleration)
            .clamp(-self.maximum_speed_rad_s, self.maximum_speed_rad_s);
        // A live reduction of the requested speed must decelerate within the
        // same acceleration bound, rather than discontinuously clipping velocity.
        let dv = self.period_s * self.maximum_acceleration_rad_s2;
        let velocity = desired_velocity.clamp(state.velocity_rad_s - dv, state.velocity_rad_s + dv);
        let angle = state.angle_rad + self.period_s * velocity;
        if !angle.is_finite() || !velocity.is_finite() {
            return Err("Nonfinite governor update".into());
        }
        Ok(State {
            angle_rad: angle,
            velocity_rad_s: velocity,
        })
    }
}
struct Registered(Config);
impl Behavior for Registered {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("samples", Q::Dimensionless, 0.),
            StateDeclaration::new("angle", Q::Angle, 0.),
            StateDeclaration::new("velocity", Q::AngularVelocity, 0.),
        ]
    }
    fn residual(&self, c: &mut Context) {
        for i in 0..3 {
            c.set_state_residual(i, c.state_rate(i));
        }
        c.set_signal(0, c.state(1));
        c.set_signal(1, c.state(2));
    }
    fn guards(&self, v: &View, out: &mut Vec<f64>) {
        out.push(v.state(0) * self.0.period_s - v.time);
    }
    fn scheduled_events(&self, v: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, v.state(0) * self.0.period_s));
    }
    fn jump(&mut self, _: usize, v: &View, s: &mut [f64]) {
        match self.0.update(
            State {
                angle_rad: v.state(1),
                velocity_rad_s: v.state(2),
            },
            v.signal_in(0),
        ) {
            Ok(n) => {
                s[1] = n.angle_rad;
                s[2] = n.velocity_rad_s;
            }
            Err(_) => s[1] = f64::NAN,
        }
        s[0] = v.state(0) + 1.;
    }
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(
        BehaviorDescriptor::new(
            KIND,
            "Sampled bounded angular reference",
            vec![
                signal_in("desired", Q::Angle),
                signal_out("command", Q::Angle),
                signal_out("velocity", Q::AngularVelocity),
            ],
            |p| {
                let config = Config {
                    period_s: param(p, "period_s")?,
                    maximum_speed_rad_s: param(p, "maximum_speed_rad_s")?,
                    maximum_acceleration_rad_s2: param(p, "maximum_acceleration_rad_s2")?,
                    response_rate_per_s: param(p, "response_rate_per_s")?,
                };
                config
                    .validate()
                    .map_err(|e| sim_core::EquationError::InvalidParameter(KIND.into(), e))?;
                Ok(Box::new(Registered(config)))
            },
        )
        .with_parameters(vec![
            P::required("period_s", "s").positive(),
            P::required("maximum_speed_rad_s", "rad/s").positive(),
            P::required("maximum_acceleration_rad_s2", "rad/s²").positive(),
            P::required("response_rate_per_s", "1/s").positive(),
        ]),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_and_reversals_retain_bounds_and_settle() {
        let c = Config {
            period_s: 0.01,
            maximum_speed_rad_s: 2.,
            maximum_acceleration_rad_s2: 10.,
            response_rate_per_s: 20.,
        };
        let mut s = State::default();
        for i in 0..800 {
            let target = if i < 100 {
                1.
            } else if i < 200 {
                -1.
            } else {
                0.
            };
            let n = c.update(s, target).unwrap();
            assert!(n.velocity_rad_s.abs() <= 2.);
            assert!((n.velocity_rad_s - s.velocity_rad_s).abs() <= 0.10000000001);
            assert!(((n.angle_rad - s.angle_rad) / 0.01 - n.velocity_rad_s).abs() < 1e-10);
            s = n;
        }
        assert!(s.angle_rad.abs() < 1e-8 && s.velocity_rad_s.abs() < 1e-8);
        assert!(c.update(s, f64::NAN).is_err());
    }
    #[test]
    fn live_speed_reduction_decelerates_without_a_velocity_jump() {
        let c = Config {
            period_s: 0.01,
            maximum_speed_rad_s: 0.2,
            maximum_acceleration_rad_s2: 1.,
            response_rate_per_s: 10.,
        };
        let mut state = State {
            angle_rad: 0.,
            velocity_rad_s: 2.,
        };
        for _ in 0..200 {
            let next = c.update(state, 100.).unwrap();
            assert!((next.velocity_rad_s - state.velocity_rad_s).abs() <= 0.010000001);
            state = next;
        }
        assert!((state.velocity_rad_s - 0.2).abs() < 1e-8);
    }
}
