//! Couplers that carry the seam across a process or socket boundary, so a
//! controller can be written in any language.
//!
//! # The frame protocol
//!
//! Newline-delimited JSON, lockstep. The simulation speaks first:
//!
//! ```text
//! → {"type":"hello","element":"controller","period":0.001,
//!    "sensors":[{"name":"angle","unit":"rad"}],
//!    "actuators":[{"name":"voltage","unit":"V"}]}
//! ← {"type":"ready"}
//! → {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
//! ← {"type":"act","seq":0,"actuators":[2.5]}
//! → {"type":"sample","seq":1,"t":0.001,"sensors":[0.09]}
//! ← {"type":"act","seq":1,"actuators":[2.4]}
//! …
//! → {"type":"close"}
//! ```
//!
//! Every `sample` carries the simulation time; the controller never sees a
//! wall clock. A reply whose `seq` does not match, a malformed line, a
//! closed pipe or a reply later than the timeout is a [`CouplerError`],
//! which the runtime reports as an error naming the element.

pub mod environment;
pub use environment::{Environment, Frame, Spaces, serve};

use serde::{Deserialize, Serialize};
use sim_core::Contract;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Named {
    pub name: String,
    pub unit: String,
}

/// Frames the simulation sends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    Hello { element: String, period: f64, sensors: Vec<Named>, actuators: Vec<Named> },
    Sample { seq: u64, t: f64, sensors: Vec<f64> },
    Close,
}

/// Frames the controller sends back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inbound {
    Ready,
    Act { seq: u64, actuators: Vec<f64> },
}

impl Outbound {
    pub fn hello(contract: &Contract) -> Self {
        let named = |channels: &[sim_core::Channel]| channels.iter().map(|c| Named { name: c.name.clone(), unit: c.unit().to_owned() }).collect();
        Self::Hello { element: contract.element.clone(), period: contract.period, sensors: named(&contract.sensors), actuators: named(&contract.actuators) }
    }
}


// Browser consumers retain contracts without pulling in OS transports.
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
