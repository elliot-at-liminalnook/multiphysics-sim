//! Portable bounded neural corrections with named, typed input/output bindings.
//! No robot topology or physics is implemented here. All layers use tanh;
//! outputs are dimensionless until multiplied by their declared physical scale.
use serde::{Deserialize, Serialize};
use sim_core::{Channel, QuantityKind};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub source: String,
    pub subtract: Option<String>,
    pub kind: QuantityKind,
    pub center: f64,
    pub scale: f64,
    pub clip: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub target: String,
    pub kind: QuantityKind,
    pub scale: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Output-major rows; each row has one weight per preceding activation.
    pub weights: Vec<Vec<f64>>,
    pub biases: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub version: u32,
    pub features: Vec<Feature>,
    pub outputs: Vec<Output>,
    pub layers: Vec<Layer>,
}
pub struct BoundNetwork {
    network: Network,
    features: Vec<(usize, Option<usize>)>,
    outputs: Vec<usize>,
    sensor_count: usize,
    actuator_count: usize,
}
impl Network {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.features.is_empty() || self.outputs.is_empty()
            || self.layers.is_empty() || self.layers.len() > 8 {
            return Err("neural policy requires version 1, features, outputs and 1..8 layers".into());
        }
        let mut width = self.features.len();
        let mut parameters = 0usize;
        for layer in &self.layers {
            if layer.weights.is_empty() || layer.weights.len() > 4096 || layer.biases.len() != layer.weights.len()
                || layer.weights.iter().any(|r| r.len() != width || r.iter().any(|v| !v.is_finite()))
                || layer.biases.iter().any(|v| !v.is_finite()) {
                return Err("invalid neural layer dimensions or nonfinite parameters".into());
            }
            parameters = parameters.saturating_add(layer.weights.len().saturating_mul(width + 1));
            width = layer.weights.len();
        }
        let mut names = BTreeSet::new();
        if parameters > 1_000_000 || width != self.outputs.len()
            || self.features.iter().any(|f| f.source.trim().is_empty() || !f.center.is_finite()
                || !f.scale.is_finite() || f.scale <= 0.0 || !f.clip.is_finite() || f.clip <= 0.0)
            || self.outputs.iter().any(|o| o.target.trim().is_empty() || !names.insert(&o.target)
                || !o.scale.is_finite() || o.scale <= 0.0) {
            return Err("invalid neural feature/output declarations or parameter budget".into());
        }
        Ok(())
    }
    pub fn parameters(&self) -> Vec<f64> {
        self.layers.iter().flat_map(|l| l.weights.iter().flatten().chain(&l.biases)).copied().collect()
    }
    pub fn with_parameters(&self, parameters: &[f64]) -> Result<Self, String> {
        if parameters.len() != self.parameters().len() || parameters.iter().any(|v| !v.is_finite()) {
            return Err("neural parameter count/nonfinite mismatch".into());
        }
        let mut next = self.clone();
        let mut values = parameters.iter();
        for layer in &mut next.layers {
            for v in layer.weights.iter_mut().flatten().chain(&mut layer.biases) { *v = *values.next().unwrap(); }
        }
        next.validate()?;
        Ok(next)
    }
    pub fn bind(self, sensors: &[Channel], actuators: &[Channel]) -> Result<BoundNetwork, String> {
        self.validate()?;
        let index = |channels: &[Channel], name: &str, kind: QuantityKind| -> Result<usize, String> {
            let found: Vec<_> = channels.iter().enumerate().filter(|(_,c)| c.name == name && c.kind == kind).map(|(i,_)| i).collect();
            if found.len() == 1 { Ok(found[0]) } else { Err(format!("neural binding requires unique named channel with matching units: {name}")) }
        };
        let features = self.features.iter().map(|f| Ok((index(sensors,&f.source,f.kind)?,
            f.subtract.as_ref().map(|s| index(sensors,s,f.kind)).transpose()?))).collect::<Result<_,String>>()?;
        let outputs = self.outputs.iter().map(|o| index(actuators,&o.target,o.kind)).collect::<Result<_,_>>()?;
        Ok(BoundNetwork {network:self,features,outputs,sensor_count:sensors.len(),actuator_count:actuators.len()})
    }
}
impl BoundNetwork {
    pub fn sample(&self, sensors: &[f64]) -> Result<Vec<f64>, String> {
        if sensors.len() != self.sensor_count || sensors.iter().any(|v| !v.is_finite()) {
            return Err("neural observation count/nonfinite mismatch".into());
        }
        let mut values = Vec::with_capacity(self.features.len());
        for (f, &(i,j)) in self.network.features.iter().zip(&self.features) {
            let value = (sensors[i] - j.map_or(0.0, |j| sensors[j]) - f.center) / f.scale;
            if !value.is_finite() { return Err("nonfinite neural normalized feature".into()); }
            values.push(value.clamp(-f.clip, f.clip));
        }
        for layer in &self.network.layers {
            let mut next = Vec::with_capacity(layer.biases.len());
            for (row,bias) in layer.weights.iter().zip(&layer.biases) {
                let value = row.iter().zip(&values).fold(*bias, |s,(w,x)| s+w*x);
                if !value.is_finite() { return Err("nonfinite neural activation".into()); }
                next.push(value.tanh());
            }
            values = next;
        }
        let mut output = vec![0.0; self.actuator_count];
        for ((o,&i),value) in self.network.outputs.iter().zip(&self.outputs).zip(values) { output[i] = value * o.scale; }
        Ok(output)
    }
    pub fn definition(&self) -> &Network { &self.network }
}
