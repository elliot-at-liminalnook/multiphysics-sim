//! The seam between a compiled plant and control code that lives elsewhere.
//!
//! A [`Coupler`] is what an external control element calls at each sample
//! instant: it receives the sensor channels and returns the actuator
//! channels. The simulation blocks until the coupler answers (lockstep), so
//! wall-clock speed on the controller's side never reaches the physics. The
//! plant side sees only named, unit-bearing channels — nothing about states,
//! lanes or islands — which is what lets the controller be written in any
//! language and run against a rig later without change.

use crate::QuantityKind;

/// One named signal crossing the seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    pub kind: QuantityKind,
}

impl Channel {
    pub fn unit(&self) -> &'static str {
        self.kind.unit()
    }
}

/// What the plant offers a controller: the element's name, its sample
/// period, and its channels in frame order.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub element: String,
    pub period: f64,
    pub sensors: Vec<Channel>,
    pub actuators: Vec<Channel>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CouplerError {
    #[error("controller exited: {0}")]
    Exited(String),
    #[error("malformed frame: {0}")]
    Malformed(String),
    #[error("no reply within {0} s")]
    Timeout(f64),
    #[error("{0}")]
    Other(String),
}

pub trait Coupler: Send {
    /// The handshake: called once, before the first sample, with the
    /// contract the element derived from its wiring.
    fn open(&mut self, _contract: &Contract) -> Result<(), CouplerError> {
        Ok(())
    }
    /// One sample instant at simulation time `t`: read `sensors`, write
    /// `actuators` (which arrive holding the previous command).
    fn sample(&mut self, t: f64, sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError>;
    /// The run is over; release whatever the coupler holds.
    fn close(&mut self) {}
}

/// An in-process controller: a closure over `(t, sensors, actuators)`.
/// Rust controllers use the same seam as everything else.
pub struct FnCoupler<F>(pub F);

impl<F> Coupler for FnCoupler<F>
where
    F: FnMut(f64, &[f64], &mut [f64]) + Send,
{
    fn sample(&mut self, t: f64, sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError> {
        (self.0)(t, sensors, actuators);
        Ok(())
    }
}
