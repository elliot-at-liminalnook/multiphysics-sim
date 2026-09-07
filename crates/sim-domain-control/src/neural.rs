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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisedSample {
    /// Already normalized using this artifact's named feature definitions.
    pub inputs: Vec<f64>,
    /// Physical target divided by the corresponding output scale.
    pub targets: Vec<f64>,
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
    pub fn normalize(&self, sensors: &[f64]) -> Result<Vec<f64>, String> {
        if sensors.len() != self.sensor_count || sensors.iter().any(|v| !v.is_finite()) {
            return Err("neural observation count/nonfinite mismatch".into());
        }
        let mut values = Vec::with_capacity(self.features.len());
        for (f, &(i,j)) in self.network.features.iter().zip(&self.features) {
            let value = (sensors[i] - j.map_or(0.0, |j| sensors[j]) - f.center) / f.scale;
            if !value.is_finite() { return Err("nonfinite neural normalized feature".into()); }
            values.push(value.clamp(-f.clip, f.clip));
        }
        Ok(values)
    }
    pub fn supervised_sample(&self, sensors: &[f64], targets: &[f64]) -> Result<SupervisedSample, String> {
        if targets.len()!=self.actuator_count || targets.iter().any(|v| !v.is_finite()) { return Err("invalid supervised target dimensions/values".into()); }
        let targets=self.network.outputs.iter().zip(&self.outputs).map(|(o,&i)| targets[i]/o.scale).collect::<Vec<_>>();
        if targets.iter().any(|v| !v.is_finite() || v.abs()>1.0) { return Err("teacher target exceeds declared student output range".into()); }
        Ok(SupervisedSample {inputs:self.normalize(sensors)?,targets})
    }
    pub fn sample(&self, sensors: &[f64]) -> Result<Vec<f64>, String> {
        let mut values=self.normalize(sensors)?;
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

impl Network {
    /// Mean squared normalized-output error and exact parameter gradient.
    /// Ordering matches parameters(): output-major weights then biases per layer.
    pub fn supervised_gradient(&self, samples: &[SupervisedSample]) -> Result<(f64,Vec<f64>),String> {
        self.validate()?;
        if samples.is_empty() { return Err("supervised batch must not be empty".into()); }
        let mut gradient=self.layers.iter().map(|l| Layer {weights:l.weights.iter().map(|r|vec![0.;r.len()]).collect(),biases:vec![0.;l.biases.len()]}).collect::<Vec<_>>();
        let mut loss=0.0;
        let denominator=(samples.len()*self.outputs.len()) as f64;
        for sample in samples {
            if sample.inputs.len()!=self.features.len() || sample.targets.len()!=self.outputs.len()
                || sample.inputs.iter().chain(&sample.targets).any(|v|!v.is_finite())
                || sample.targets.iter().any(|v|v.abs()>1.0) { return Err("invalid supervised sample shape/values".into()); }
            let mut activations=vec![sample.inputs.clone()];
            for layer in &self.layers {
                let previous=activations.last().unwrap();
                let mut next=Vec::with_capacity(layer.biases.len());
                for (row,bias) in layer.weights.iter().zip(&layer.biases) {
                    let z=row.iter().zip(previous).fold(*bias,|s,(w,x)|s+w*x);
                    if !z.is_finite() { return Err("nonfinite supervised activation".into()); }
                    next.push(z.tanh());
                }
                activations.push(next);
            }
            let output=activations.last().unwrap();
            let mut delta=Vec::with_capacity(output.len());
            for (&y,&target) in output.iter().zip(&sample.targets) {
                loss+=(y-target).powi(2)/denominator;
                delta.push(2.0*(y-target)*(1.0-y*y)/denominator);
            }
            for l in (0..self.layers.len()).rev() {
                for (j,&d) in delta.iter().enumerate() {
                    gradient[l].biases[j]+=d;
                    for (k,&x) in activations[l].iter().enumerate() {gradient[l].weights[j][k]+=d*x;}
                }
                if l>0 {
                    let mut previous=vec![0.0;activations[l].len()];
                    for (k,d) in previous.iter_mut().enumerate() {
                        *d=delta.iter().enumerate().map(|(j,v)|self.layers[l].weights[j][k]*v).sum::<f64>()*(1.0-activations[l][k].powi(2));
                    }
                    delta=previous;
                }
            }
        }
        let gradient=gradient.iter().flat_map(|l|l.weights.iter().flatten().chain(&l.biases)).copied().collect::<Vec<_>>();
        if !loss.is_finite() || gradient.iter().any(|v|!v.is_finite()) {return Err("nonfinite supervised loss/gradient".into());}
        Ok((loss,gradient))
    }
}
