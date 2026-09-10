//! Read authored, scheduled IMUs through one native/WASM policy and environment
//! adapter. Sampling, noise and sensor axes remain owned by robot.articulated.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_core::{Channel, QuantityKind as Q};
use sim_domain_robot::{Articulated, Generalized, articulated::ImuReading};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImuChannel {
    Ax,
    Ay,
    Az,
    Gx,
    Gy,
    Gz,
    Available,
    Age,
}

impl ImuChannel {
    pub const ALL: [Self; 8] = [
        Self::Ax,
        Self::Ay,
        Self::Az,
        Self::Gx,
        Self::Gy,
        Self::Gz,
        Self::Available,
        Self::Age,
    ];
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Ax => "ax",
            Self::Ay => "ay",
            Self::Az => "az",
            Self::Gx => "gx",
            Self::Gy => "gy",
            Self::Gz => "gz",
            Self::Available => "available",
            Self::Age => "age_s",
        }
    }
    pub fn kind(self) -> Q {
        match self {
            Self::Ax | Self::Ay | Self::Az => Q::LinearAcceleration,
            Self::Gx | Self::Gy | Self::Gz => Q::AngularVelocity,
            Self::Available => Q::Dimensionless,
            Self::Age => Q::Time,
        }
    }
    /// Before the first sensor tick, physical values and age are zero placeholders.
    /// Policies and forecasts receive `available`; environment samples retain
    /// the optional timestamp directly instead of flattening unavailable values.
    pub fn read(self, sample: &ImuReading, time_s: f64) -> Result<f64, String> {
        if !time_s.is_finite()
            || time_s < 0.
            || !sample.next_sample_time_s.is_finite()
            || sample.next_sample_time_s <= time_s
            || sample
                .sample_time_s
                .is_some_and(|t| !t.is_finite() || t < 0. || t > time_s + 1e-12)
            || sample
                .specific_force_m_s2
                .iter()
                .chain(&sample.angular_velocity_rad_s)
                .any(|v| !v.is_finite())
        {
            return Err("invalid committed IMU value or sample clock".into());
        }
        let Some(sample_time) = sample.sample_time_s else {
            return Ok(0.);
        };
        Ok(match self {
            Self::Ax => sample.specific_force_m_s2[0],
            Self::Ay => sample.specific_force_m_s2[1],
            Self::Az => sample.specific_force_m_s2[2],
            Self::Gx => sample.angular_velocity_rad_s[0],
            Self::Gy => sample.angular_velocity_rad_s[1],
            Self::Gz => sample.angular_velocity_rad_s[2],
            Self::Available => 1.,
            Self::Age => (time_s - sample_time).max(0.),
        })
    }
}

pub struct ImuObserver {
    names: Vec<String>,
    channels: Vec<Channel>,
    metadata: Value,
}
impl ImuObserver {
    pub fn new(art: &Articulated, names: &[String]) -> Result<Self, String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut channels = Vec::new();
        let mut sensors = Vec::new();
        for name in names {
            if name.trim().is_empty() || !seen.insert(name) {
                return Err("unique nonempty IMU observation names required".into());
            }
            let authored: Vec<_> = art
                .model
                .sensors
                .iter()
                .filter(|s| &s.name == name)
                .collect();
            let compiled: Vec<_> = art.imus.iter().filter(|s| &s.name == name).collect();
            if authored.len() != 1 || authored[0].kind != "imu" || compiled.len() != 1 {
                return Err(format!("unique authored and compiled IMU required: {name}"));
            }
            let sensor = authored[0];
            let runtime = compiled[0];
            channels.extend(ImuChannel::ALL.map(|c| Channel {
                name: format!("imu.{name}.{}", c.suffix()),
                kind: c.kind(),
            }));
            sensors.push(json!({"name":name,"cad_id":sensor.id,"link":sensor.link,
                "axes":sensor.axes,"point_m":sensor.point,
                "model_rate_hz":sensor.rate_hz,"runtime_period_s":runtime.period,
                "runtime_latency_s":runtime.latency,"resolved_definition":sensor}));
        }
        let metadata = json!({"component":sim_domain_robot::ARTICULATED,"sensors":sensors,
            "definition_source":"parsed CAD model, possibly including legacy schema defaults; inspect the original RobotDocument for authored omissions and provenance",
            "frame":"authored sensor axes; acceleration channels are specific force, not world acceleration",
            "timing":"held latest committed sample; age is observation time minus sample time",
            "before_first_sample":"available=0; physical values and age are zero placeholders, not measurements",
            "availability":"available=1 after first committed sensor tick; simulated sensor model, not hardware calibration"});
        Ok(Self {
            names: names.to_vec(),
            channels,
            metadata,
        })
    }
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
    /// Preserve missing-sample information when selecting neural features.
    pub fn validate_network(
        &self,
        network: &sim_domain_control::neural::Network,
    ) -> Result<(), String> {
        for channels in self.channels.chunks_exact(8) {
            let consumes = network.features.iter().any(|f| {
                channels
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != 6)
                    .any(|(_, c)| f.source == c.name || f.subtract.as_ref() == Some(&c.name))
            });
            if consumes
                && !network
                    .features
                    .iter()
                    .any(|f| f.source == channels[6].name && f.subtract.is_none())
            {
                return Err(format!(
                    "sensor actor must consume {} for unavailable startup samples",
                    channels[6].name
                ));
            }
        }
        Ok(())
    }
    pub fn observe(
        &self,
        art: &Articulated,
        state: &Generalized,
        time_s: f64,
    ) -> Result<Vec<f64>, String> {
        if self.names.is_empty() {
            return Ok(Vec::new());
        }
        let readings = art.imu_readings(state);
        let mut values = Vec::with_capacity(self.channels.len());
        for name in &self.names {
            let sample = readings
                .iter()
                .find(|s| &s.name == name)
                .ok_or("missing bound IMU")?;
            for channel in ImuChannel::ALL {
                values.push(channel.read(sample, time_s)?);
            }
        }
        Ok(values)
    }
}

pub fn validate_readings(readings: &[ImuReading], time_s: f64) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    for sample in readings {
        if sample.name.trim().is_empty()
            || sample.link.trim().is_empty()
            || !names.insert(&sample.name)
        {
            return Err("invalid or duplicate IMU reading identity".into());
        }
        ImuChannel::Available.read(sample, time_s)?;
    }
    Ok(())
}
