//! Deterministic library-source identity, shared by native and WASM builds.
//! Binary/host identities and measured numerical parity remain separate evidence.
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn library_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if !dir.exists() {
        return;
    }
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir).expect("read library sources") {
        let entry = entry.expect("library source entry");
        let kind = entry.file_type().expect("library source type");
        assert!(
            !kind.is_symlink(),
            "source identity does not follow symlinks"
        );
        if kind.is_dir() {
            library_files(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
}
fn main() {
    let package = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = package.parent().unwrap().parent().unwrap();
    let mut files = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    println!("cargo:rerun-if-changed={}", root.join("crates").display());
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        // UI/application bindings do not define the underlying simulation model.
        if matches!(entry.file_name().to_str(), Some("sim-app" | "sim-web")) {
            continue;
        }
        let cargo = entry.path().join("Cargo.toml");
        if !cargo.exists() {
            continue;
        }
        files.push(cargo);
        let build = entry.path().join("build.rs");
        if build.exists() {
            files.push(build);
        }
        library_files(&entry.path().join("src"), &mut files);
        library_files(&entry.path().join("native"), &mut files);
    }
    files.sort_by_key(|p| p.strip_prefix(root).unwrap().to_owned());
    let mut hash = blake3::Hasher::new();
    hash.update(b"sim-runtime-library-source-v1\0");
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let bytes = fs::read(&file).expect("read source identity input");
        hash.update(&(relative.len() as u64).to_le_bytes());
        hash.update(relative.as_bytes());
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    println!(
        "cargo:rustc-env=SIM_RUNTIME_SOURCE_BLAKE3={}",
        hash.finalize().to_hex()
    );
    let mut features = env::vars()
        .filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .collect::<Vec<_>>();
    features.sort();
    println!(
        "cargo:rustc-env=SIM_RUNTIME_IDENTITY_FEATURES={}",
        features.join(",")
    );
}
