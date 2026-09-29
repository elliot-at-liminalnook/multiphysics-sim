//! Promote one axis of a characterization campaign into an actuator family.
//!
//!     promote_actuator_family CAMPAIGN_DIR AXIS_ID BASE_FAMILY.json NAME OUT_DIR
//!
//! Writes OUT_DIR/NAME.json (the family) and OUT_DIR/NAME.derivation.json.
//! Accept it by adding it to the actuator registry with its printed content
//! hash; nothing uses it until then.
use sim_runtime::acquisition::{actuator_promotion::{Sources, derive_family}, characterization::Report};
use std::path::Path;

/// SHA-256 (the evidence schema's digest) via the system `shasum`.
fn sha(path: &Path) -> Result<String, String> {
    let out = std::process::Command::new("shasum").args(["-a", "256"]).arg(path).output().map_err(|e| format!("shasum: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let digest = text.split_whitespace().next().unwrap_or_default();
    if !out.status.success() || digest.len() != 64 {
        return Err(format!("shasum failed for {}", path.display()));
    }
    Ok(digest.to_string())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 5 {
        return Err("usage: promote_actuator_family CAMPAIGN_DIR AXIS_ID BASE_FAMILY.json NAME OUT_DIR".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let campaign = Path::new(&a[0]).canonicalize()?;
    let id: u8 = a[1].parse()?;
    let report_path = campaign.join("report.json");
    let report: Report = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    let base = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let plan: serde_json::Value = serde_json::from_slice(&std::fs::read(campaign.join("plan.json"))?)?;
    let role = plan["axes"].as_array().and_then(|x| x.iter().find(|x| x["id"] == id)).and_then(|x| x["role"].as_str()).unwrap_or("unknown").to_string();
    let rel = |p: &Path| p.strip_prefix(&root).map(|p| p.display().to_string()).unwrap_or(p.display().to_string());
    let code = root.join("crates/sim-runtime/src/acquisition/actuator_promotion.rs");
    let sources = Sources {
        report_path: rel(&report_path),
        report_sha256: sha(&report_path)?,
        scope: format!("Characterization campaign {}, servo ID {id} ({role}) on the printed test leg, suspended; stages A–K as recorded in the report.", campaign.file_name().unwrap().to_string_lossy()),
        code_path: rel(&code),
        code_sha256: sha(&code)?,
    };
    let description = format!("HX-30HM {role} servo: back-EMF/torque constant, gear friction and operating envelope from campaign {}; other values remain the base family's estimates.", campaign.file_name().unwrap().to_string_lossy());
    let (family, derivation) = derive_family(&report, id, &base, &description, &sources)?;
    let out = Path::new(&a[4]);
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join(format!("{}.json", a[3])), serde_json::to_vec_pretty(&family)?)?;
    std::fs::write(out.join(format!("{}.derivation.json", a[3])), serde_json::to_vec_pretty(&derivation)?)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"family": a[3], "content_hash": family.content_hash(), "results": derivation["results"]}))?);
    Ok(())
}
