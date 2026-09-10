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
        let values=self.network.normalized_output(&self.normalize(sensors)?, false)?;
        self.scale_outputs(&values)
    }
    /// Convert normalized neural actions, including unbounded Gaussian samples,
    /// into the same typed actuator correction slots as deterministic inference.
    pub fn scale_outputs(&self, values: &[f64]) -> Result<Vec<f64>, String> {
        if values.len()!=self.network.outputs.len() || values.iter().any(|x|!x.is_finite()) {
            return Err("invalid normalized neural action".into());
        }
        let mut output = vec![0.0; self.actuator_count];
        for ((o,&i),value) in self.network.outputs.iter().zip(&self.outputs).zip(values) { output[i] = value * o.scale; }
        if output.iter().any(|x|!x.is_finite()) { return Err("nonfinite scaled neural action".into()); }
        Ok(output)
    }
    pub fn definition(&self) -> &Network { &self.network }
}

impl Network {
    fn activations(&self, inputs: &[f64], linear_output: bool) -> Result<Vec<Vec<f64>>,String> {
        self.validate()?;
        if inputs.len()!=self.features.len() || inputs.iter().any(|x|!x.is_finite()) {
            return Err("invalid normalized neural inputs".into());
        }
        let mut activations=vec![inputs.to_vec()];
        for (i,layer) in self.layers.iter().enumerate() {
            let previous=activations.last().unwrap();
            let mut next=Vec::with_capacity(layer.biases.len());
            for (row,bias) in layer.weights.iter().zip(&layer.biases) {
                let z=row.iter().zip(previous).fold(*bias,|s,(w,x)|s+w*x);
                if !z.is_finite() { return Err("nonfinite neural activation".into()); }
                next.push(if linear_output && i+1==self.layers.len() {z} else {z.tanh()});
            }
            activations.push(next);
        }
        Ok(activations)
    }
    /// Normalized forward pass. Linear output is for value-function fitting;
    /// actuator policies always use the artifact's existing tanh output.
    pub fn normalized_output(&self, inputs: &[f64], linear_output: bool) -> Result<Vec<f64>,String> {
        Ok(self.activations(inputs,linear_output)?.pop().unwrap())
    }
    /// Vector-Jacobian product with respect to parameters(), for arbitrary losses.
    pub fn output_gradient(&self, inputs: &[f64], derivative: &[f64], linear_output: bool) -> Result<Vec<f64>,String> {
        let activations=self.activations(inputs,linear_output)?;
        if derivative.len()!=self.outputs.len() || derivative.iter().any(|x|!x.is_finite()) {
            return Err("invalid neural output derivative".into());
        }
        let mut gradient=self.layers.iter().map(|l| Layer {weights:l.weights.iter().map(|r|vec![0.;r.len()]).collect(),biases:vec![0.;l.biases.len()]}).collect::<Vec<_>>();
        let mut delta=derivative.iter().zip(activations.last().unwrap())
            .map(|(d,y)|d*if linear_output {1.} else {1.-y*y}).collect::<Vec<_>>();
        for l in (0..self.layers.len()).rev() {
            for (j,&d) in delta.iter().enumerate() {
                gradient[l].biases[j]=d;
                for (k,&x) in activations[l].iter().enumerate() {gradient[l].weights[j][k]=d*x;}
            }
            if l>0 {
                delta=(0..activations[l].len()).map(|k|
                    delta.iter().enumerate().map(|(j,v)|self.layers[l].weights[j][k]*v).sum::<f64>()
                        *(1.-activations[l][k].powi(2))).collect();
            }
        }
        let flat=gradient.iter().flat_map(|l|l.weights.iter().flatten().chain(&l.biases)).copied().collect::<Vec<_>>();
        if flat.iter().any(|x|!x.is_finite()) {return Err("nonfinite neural gradient".into());}
        Ok(flat)
    }
    /// Vector-Jacobian product with respect to normalized inputs. This does not
    /// include feature normalization/clipping or physical output scaling.
    pub fn input_gradient(&self, inputs: &[f64], derivative: &[f64], linear_output: bool) -> Result<Vec<f64>,String> {
        let activations=self.activations(inputs,linear_output)?;
        if derivative.len()!=self.outputs.len()||derivative.iter().any(|x|!x.is_finite()) {
            return Err("invalid neural input-gradient derivative".into());
        }
        let mut delta=derivative.to_vec();
        for l in (0..self.layers.len()).rev() {
            if !(linear_output&&l+1==self.layers.len()) {
                for(d,y)in delta.iter_mut().zip(&activations[l+1]){*d*=1.-y*y;}
            }
            delta=(0..activations[l].len()).map(|k|self.layers[l].weights.iter().zip(&delta).map(|(row,d)|row[k]*d).sum()).collect();
        }
        if delta.iter().any(|x|!x.is_finite()){return Err("nonfinite neural input gradient".into());}Ok(delta)
    }
    /// Mean squared normalized-output error and exact parameter gradient.
    pub fn supervised_gradient(&self, samples: &[SupervisedSample]) -> Result<(f64,Vec<f64>),String> {
        self.validate()?;
        if samples.is_empty() {return Err("supervised batch must not be empty".into());}
        let mut gradient=vec![0.;self.parameters().len()];
        let mut loss=0.;
        let denominator=(samples.len()*self.outputs.len()) as f64;
        for sample in samples {
            if sample.targets.len()!=self.outputs.len() || sample.targets.iter().any(|v|!v.is_finite()||v.abs()>1.) {
                return Err("invalid supervised targets".into());
            }
            let output=self.normalized_output(&sample.inputs,false)?;
            let derivative=output.iter().zip(&sample.targets).map(|(y,t)|2.*(y-t)/denominator).collect::<Vec<_>>();
            loss+=output.iter().zip(&sample.targets).map(|(y,t)|(y-t).powi(2)/denominator).sum::<f64>();
            for (total,value) in gradient.iter_mut().zip(self.output_gradient(&sample.inputs,&derivative,false)?) {*total+=value;}
        }
        if !loss.is_finite() || gradient.iter().any(|v|!v.is_finite()) {return Err("nonfinite supervised loss/gradient".into());}
        Ok((loss,gradient))
    }
}
