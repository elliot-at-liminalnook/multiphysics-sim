//! Read-only canonical observations; serialization belongs to RoboCAD owners.
use super::{CadClient, CadError};
use crate::cad_parity::contract::Observation;
use std::collections::BTreeMap;
impl CadClient {
    pub fn parity_observations(
        &self,
        source_sha256: &str,
    ) -> Result<BTreeMap<String, Observation>, CadError> {
        self.get(&format!(
            "/parity/observations?source_sha256={}",
            crate::hardware_client::encode_uri_component(source_sha256)
        ))
    }
}

impl CadClient {
    pub fn parity_captured(
        &self,
        capture: &str,
        action: &str,
        source_sha256: &str,
    ) -> Result<serde_json::Value, CadError> {
        use crate::hardware_client::encode_uri_component as enc;
        self.get(&format!(
            "/parity/captured?capture={}&action={}&source_sha256={}",
            enc(capture),
            enc(action),
            enc(source_sha256)
        ))
    }
}
