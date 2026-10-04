//! Deterministic CPU PNG rendering of immutable presentation snapshots.
//! No windows, GPU, physics stepping, filesystem writes or source mutation.
pub mod cad;
mod canvas;
pub mod diagram;
pub mod graphs;
pub mod physical;
pub mod video;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}
impl Default for Size {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 900,
        }
    }
}
impl Size {
    pub fn validate(self) -> Result<Self, String> {
        if self.width < 400
            || self.height < 300
            || self.width > 2400
            || self.height > 2400
            || self.width as u64 * self.height as u64 > 4_000_000
        {
            return Err(
                "image dimensions must be 400..2400 by 300..2400, at most 4 million pixels".into(),
            );
        }
        Ok(self)
    }
}
pub struct Rendered {
    pub png: Vec<u8>,
    pub metadata: serde_json::Value,
}

/// A shared discussion group, resolved to visible component IDs by the host.
#[derive(Clone)]
pub struct Region {
    pub label: String,
    pub color: [u8; 3],
    pub components: std::collections::BTreeSet<String>,
}
