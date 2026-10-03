//! Focused headless consumer of the same archive/kernel/mass library as CAD mode.
//! Source reviewed only; this example was not run for the migration batch.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: inspect_archive FILE.rcad")?;
    let archive = sim_cad::ArchiveDocument::open(std::path::Path::new(&path))?;
    let bodies = sim_cad::geometry::load_geometry(&archive, &|| false, &|_| {})?;
    let masses = sim_cad::mass::derive_document(&archive, &bodies)?;
    println!("{}", serde_json::to_string_pretty(&masses)?);
    Ok(())
}
