//! Build an FMI 3 Co-Simulation FMU from C sources: compile `sources/*.c`
//! into a shared library for this platform (the system C compiler, `cc`,
//! with the standard's headers on the include path) and zip it with
//! `modelDescription.xml`, the sources and any `documentation/` files.
//!
//! A model directory holds:
//! ```text
//! modelDescription.xml        the model's FMI 3 description
//! sources/*.c                 its implementation
//! documentation/*  (optional)
//! ```
//! The archive is reproducible for the same inputs and compiler: entries in
//! a fixed order with a fixed timestamp.
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the FMI 3 headers are (examples/fmi/include in this repository).
pub fn headers() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/fmi/include")
}

fn sorted_files(dir: &Path, filter: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && filter(p))
        .collect();
    files.sort();
    Ok(files)
}

/// Compile the model in `model_dir` and write the FMU to `out`. Returns the
/// archive's SHA-256.
pub fn pack(model_dir: &Path, out: &Path) -> Result<String, String> {
    let xml = std::fs::read_to_string(model_dir.join("modelDescription.xml")).map_err(|e| format!("{}: {e}", model_dir.join("modelDescription.xml").display()))?;
    let md = crate::description::parse(&xml)?;
    let cs = md.co_simulation.as_ref().ok_or("the model description declares no <CoSimulation>")?;
    let sources = sorted_files(&model_dir.join("sources"), |p| p.extension().is_some_and(|e| e == "c"))?;
    if sources.is_empty() {
        return Err(format!("{}: no sources/*.c to compile", model_dir.display()));
    }
    let build = tempfile::tempdir().map_err(|e| e.to_string())?;
    let library = build.path().join(format!("{}{}", cs.model_identifier, std::env::consts::DLL_SUFFIX));
    let mut cc = std::process::Command::new(std::env::var("CC").unwrap_or_else(|_| "cc".into()));
    cc.args(["-std=c99", "-O2", "-fPIC", "-shared", "-Wall", "-Werror"]).arg("-I").arg(headers()).arg("-I").arg(model_dir.join("sources")).args(&sources).arg("-o").arg(&library);
    if cfg!(target_os = "linux") {
        cc.arg("-lm");
    }
    let output = cc.output().map_err(|e| format!("cannot run the C compiler (`cc`): {e}"))?;
    if !output.status.success() {
        return Err(format!("compiling {} failed:\n{}", model_dir.display(), String::from_utf8_lossy(&output.stderr)));
    }
    let mut entries: Vec<(String, Vec<u8>)> = vec![("modelDescription.xml".into(), xml.into_bytes())];
    let platform = crate::fmu::platform()?;
    entries.push((format!("binaries/{platform}/{}{}", cs.model_identifier, std::env::consts::DLL_SUFFIX), std::fs::read(&library).map_err(|e| e.to_string())?));
    for (folder, files) in [("sources", sources), ("documentation", sorted_files(&model_dir.join("documentation"), |_| true)?)] {
        for file in files {
            let name = file.file_name().expect("a file").to_string_lossy().into_owned();
            entries.push((format!("{folder}/{name}"), std::fs::read(&file).map_err(|e| e.to_string())?));
        }
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(crate::fmu::sha256_hex(&bytes))
}
