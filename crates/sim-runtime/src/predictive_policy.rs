//! Causal predictive observations for a neural actuator controller. The current
//! Rhai proposal is held for each forecast, never read from a future recording.
use crate::{
    motion_data::MotionSnapshot,
    motion_forecast::{TrajectoryForecaster, forecast_input},
};
use serde::{Deserialize, Serialize};
use sim_core::{Channel, QuantityKind};
use sim_domain_control::neural::{Feature, Network};
use sim_domain_robot::{Articulated, Generalized};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastBundle {
    pub version: u32,
    pub heads: Vec<TrajectoryForecaster>,
}
impl ForecastBundle {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.heads.is_empty() {
            return Err("forecast bundle requires version 1 and heads".into());
        }
        let mut common = None;
        let mut horizon = 0;
        for h in &self.heads {
            h.validate()?;
            if h.recipe.horizons_steps.len() != 1 || h.recipe.horizons_steps[0] <= horizon {
                return Err("each causal head must have one strictly increasing horizon".into());
            }
            horizon = h.recipe.horizons_steps[0];
            let mut recipe = serde_json::to_value(&h.recipe).map_err(|e| e.to_string())?;
            recipe["horizons_steps"] = serde_json::json!([]);
            if common.as_ref().is_some_and(|c| c != &recipe) {
                return Err("forecast heads must share their physical/input recipe".into());
            }
            common = Some(recipe);
        }
        Ok(())
    }
    pub fn channels(&self) -> Result<Vec<Channel>, String> {
        self.validate()?;
        let mut out = vec![Channel {
            name: "forecast.valid".into(),
            kind: QuantityKind::Dimensionless,
        }];
        out.extend(
            self.heads
                .last()
                .unwrap()
                .network
                .features
                .iter()
                .map(|f| Channel {
                    name: format!("forecast.input.{}", f.source),
                    kind: f.kind,
                }),
        );
        for head in &self.heads {
            for output in &head.network.outputs {
                out.push(Channel {
                    name: format!("forecast.prediction.{}", output.target),
                    kind: output.kind,
                });
                out.push(Channel {
                    name: format!("forecast.prior.{}", output.target),
                    kind: output.kind,
                });
            }
        }
        Ok(out)
    }
    /// Normalize current dynamics with training statistics. Forecast features
    /// subtract their explicit kinematic prior and use learned residual scales.
    pub fn actor_features(&self) -> Result<Vec<Feature>, String> {
        self.validate()?;
        let mut features = vec![Feature {
            source: "forecast.valid".into(),
            subtract: None,
            kind: QuantityKind::Dimensionless,
            center: 0.,
            scale: 1.,
            clip: 1.,
        }];
        features.extend(self.heads.last().unwrap().network.features.iter().map(|f| {
            let mut f = f.clone();
            f.source = format!("forecast.input.{}", f.source);
            f
        }));
        for h in &self.heads {
            for o in &h.network.outputs {
                features.push(Feature {
                    source: format!("forecast.prediction.{}", o.target),
                    subtract: Some(format!("forecast.prior.{}", o.target)),
                    kind: o.kind,
                    center: 0.,
                    scale: o.scale,
                    clip: f64::MAX,
                });
            }
        }
        Ok(features)
    }
    /// Add predictive inputs without changing the actor's initial output. New
    /// first-layer columns are zero and can acquire weights during learning.
    pub fn augment_actor(&self, actor: &Network) -> Result<Network, String> {
        actor.validate()?;
        if actor.features.iter().any(|f| {
            f.source.starts_with("forecast.")
                || f.subtract
                    .as_ref()
                    .is_some_and(|s| s.starts_with("forecast."))
        }) {
            return Err("actor already uses the reserved forecast namespace".into());
        }
        let mut out = actor.clone();
        let features = self.actor_features()?;
        for row in &mut out.layers[0].weights {
            row.extend(vec![0.; features.len()]);
        }
        out.features.extend(features);
        out.validate()?;
        Ok(out)
    }
    /// A head sees only the action prefix ending at its own prediction time.
    pub fn predict_sequence(
        &self,
        previous: &MotionSnapshot,
        current: &MotionSnapshot,
        previous_targets: &[f64],
        proposals: &[Vec<f64>],
    ) -> Result<ForecastObservation, String> {
        self.validate()?;
        if proposals.len() != self.heads.last().unwrap().recipe.horizons_steps[0] {
            return Err("forecast proposal length must match maximum horizon".into());
        }
        if !self.heads[0].recipe.controller_inputs.is_empty(){return Err("actuator forecast requires actuator targets; use the controller trajectory prediction API for controller inputs".into());}
        let mut out = ForecastObservation {
            time_s: current.time_s,
            history_valid: true,
            proposed_targets_rad: proposals.to_vec(),
            inputs: vec![],
            predictions: vec![],
            priors: vec![],
        };
        for head in &self.heads {
            let (inputs, prior) = forecast_input(
                &head.recipe,
                previous,
                current,
                previous_targets,
                &proposals[..head.recipe.horizons_steps[0]],
            )?;
            out.predictions.push(head.predict(&inputs, &prior)?);
            out.priors.push(prior);
            out.inputs = inputs;
        }
        Ok(out)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForecastObservation {
    pub time_s: f64,
    pub history_valid: bool,
    pub proposed_targets_rad: Vec<Vec<f64>>,
    pub inputs: Vec<f64>,
    pub predictions: Vec<Vec<f64>>,
    pub priors: Vec<Vec<f64>>,
}
impl ForecastObservation {
    pub fn values(&self) -> Vec<f64> {
        let mut out = vec![if self.history_valid { 1. } else { 0. }];
        out.extend(&self.inputs);
        for (prediction, prior) in self.predictions.iter().zip(&self.priors) {
            for (p, b) in prediction.iter().zip(prior) {
                out.extend([*p, *b]);
            }
        }
        out
    }
}
pub(crate) struct OnlineForecast {
    bundle: ForecastBundle,
    previous: Option<MotionSnapshot>,
}
impl OnlineForecast {
    pub fn new(
        bundle: ForecastBundle,
        art: &Articulated,
        period: f64,
        actuators: &[Channel],
    ) -> Result<Self, String> {
        bundle.validate()?;
        let recipe = &bundle.heads[0].recipe;
        if !recipe.controller_inputs.is_empty(){return Err("servo forecast requires actuator-target actions".into());}
        recipe.validate_robot(art)?;
        if (recipe.period_s - period).abs() > 1e-12
            || recipe.actuator_targets != actuators.iter().map(|a| a.name.clone()).collect::<Vec<_>>() {
            return Err("online forecast clock or actuator contract mismatch".into());
        }
        Ok(Self {
            bundle,
            previous: None,
        })
    }
    pub fn channels(&self) -> Result<Vec<Channel>, String> {
        self.bundle.channels()
    }
    pub fn sample(
        &mut self,
        art: &Articulated,
        state: &Generalized,
        time: f64,
        previous_targets: &[f64],
        proposal: &[f64],
    ) -> Result<ForecastObservation, String> {
        if proposal
            .iter()
            .chain(previous_targets)
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite forecast command".into());
        }
        let mut current = MotionSnapshot::from_state(art, state, time);
        current.observe_ground(&self.bundle.heads[0].recipe.terrain_relative_links, |x, y| art.floor_height(x, y))?;
        let proposals =
            vec![proposal.to_vec(); self.bundle.heads.last().unwrap().recipe.horizons_steps[0]];
        let out = if let Some(previous) = &self.previous {
            self.bundle
                .predict_sequence(previous, &current, previous_targets, &proposals)?
        } else {
            // Startup placeholders normalize to zero, with a separate invalid
            // flag. They are not measured dynamics or certified predictions.
            ForecastObservation {
                time_s: time,
                history_valid: false,
                proposed_targets_rad: proposals,
                inputs: self
                    .bundle
                    .heads
                    .last()
                    .unwrap()
                    .network
                    .features
                    .iter()
                    .map(|f| f.center)
                    .collect(),
                predictions: self
                    .bundle
                    .heads
                    .iter()
                    .map(|h| vec![0.; h.network.outputs.len()])
                    .collect(),
                priors: self
                    .bundle
                    .heads
                    .iter()
                    .map(|h| vec![0.; h.network.outputs.len()])
                    .collect(),
            }
        };
        if out.values().iter().any(|x| !x.is_finite()) {
            return Err("nonfinite online forecast".into());
        }
        self.previous = Some(current);
        Ok(out)
    }
}
