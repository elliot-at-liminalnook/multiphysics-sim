//! Future headless launch. Default is planning; this batch never launches it.
use sim_runtime::cad_parity::{contract::Manifest, isolation, publication::Publication, runner};
use std::path::PathBuf;
fn main() {
    if let Err(e) = run() {
        eprintln!("cad parity: {e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let mut execute = false;
    let mut cancel_file = None;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--execute" => execute = true,
            "--cancel-file" => {
                let path = PathBuf::from(args.next().ok_or("--cancel-file needs a path")?);
                isolation::absolute_no_symlinks(path.parent().ok_or("cancel file needs parent")?)?;
                cancel_file = Some(path)
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() != 5 {
        return Err("usage: cad_parity REPOSITORY MANIFEST SCENARIO NEW_OUTPUT_DIR CODE_LABEL [--execute] [--cancel-file PATH]".into());
    }
    let root = PathBuf::from(&positional[0]);
    let manifest_path = std::path::absolute(&positional[1]).map_err(|e| e.to_string())?;
    let manifest_dir = sim_runtime::cad_parity::owned_path::Directory::open(
        manifest_path.parent().ok_or("manifest parent missing")?,
    )?;
    let bytes = manifest_dir.read(
        std::path::Path::new(manifest_path.file_name().ok_or("manifest name missing")?),
        8 * 1024 * 1024,
    )?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let scenario = manifest
        .scenarios
        .iter()
        .find(|s| s.id == positional[2])
        .ok_or("unknown scenario")?;
    let cancelled = || {
        cancel_file
            .as_ref()
            .is_some_and(|p| std::fs::symlink_metadata(p).is_ok())
    };
    let report = runner::paired(
        &root,
        &manifest,
        scenario,
        positional[4].clone(),
        execute,
        &cancelled,
    )?;
    Publication::create(&PathBuf::from(&positional[3]))?.publish(&report)?;
    Ok(())
}
