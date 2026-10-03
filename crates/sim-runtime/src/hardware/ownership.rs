//! Process-wide acquisition authority shared by calibration and motor bench.
//! A lease spans the live bus, including final STOP/readback and publication.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

fn owners() -> &'static Mutex<BTreeMap<String, String>> {
    static OWNERS: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    OWNERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
/// Exclusive device ownership. Even another session from the same front end
/// is refused; STOP never acquires this lock and never waits for a lease.
#[derive(Debug)]
pub struct DeviceLease {
    device: String,
}
impl DeviceLease {
    pub fn acquire(device: &str, owner: &str) -> Result<Self, String> {
        if device.trim().is_empty() {
            return Err("Hardware device identity is empty".into());
        }
        // Canonical physical paths make symlink aliases contend for one bus.
        // Virtual identities are explicit, never inferred from path spelling.
        let device = if device.starts_with("virtual:") {
            device.to_owned()
        } else {
            std::fs::canonicalize(device)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| device.to_owned())
        };
        let mut map = owners()
            .lock()
            .map_err(|_| "Hardware ownership authority unavailable")?;
        if let Some(current) = map.get(&device) {
            return Err(format!(
                "A capture already owns the serial port ({current})"
            ));
        }
        map.insert(device.clone(), owner.to_owned());
        Ok(Self { device })
    }
}
impl Drop for DeviceLease {
    fn drop(&mut self) {
        // Poison must not strand a lease forever; no hardware operation occurs
        // under the authority lock, so recovering the map is safe here.
        owners()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.device);
    }
}
