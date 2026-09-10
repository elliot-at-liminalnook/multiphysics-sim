use sim_runtime::{
    contact_audit::floor_contact_profile,
    session::{Scene, Session},
    tracking::CaptureConfig,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: inspect_floor_contact scene markers".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let session = Session::new(scene, 0)?;
    let links = markers
        .markers
        .iter()
        .map(|m| m.link.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let profile = floor_contact_profile(
        &session.robot.art,
        &links,
        markers
            .expected_cad_sha256
            .as_deref()
            .ok_or("explicit CAD hash required")?,
    )?;
    println!("{}", serde_json::to_string_pretty(&profile)?);
    Ok(())
}
