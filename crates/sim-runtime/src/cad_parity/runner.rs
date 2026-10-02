//! Paired execution is opt-in. Planning constructs honest not-run receipts.
use super::{
    contract::*,
    isolation::Workspace,
    native::NativeAdapter,
    process::{OwnedProcess, ProcessCompletion},
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn reference_identity() -> AdapterIdentity {
    AdapterIdentity {
        name: "robocad-reference".into(),
        version: "1".into(),
        implementation: "python-direct-ops".into(),
        kernel: "python-occt".into(),
        derivation: "robocad-physical-v1".into(),
        independent: false,
    }
}
pub fn not_run(identity: AdapterIdentity, m: &Manifest, s: &Scenario, why: &str) -> AdapterRun {
    AdapterRun {
        identity,
        source: m.source.clone(),
        execution_issues: vec![],
        receipts: s
            .operations
            .iter()
            .map(|step| Receipt {
                step_id: step.id.clone(),
                status: ExecutionStatus::NotRun,
                expected: step.expected.clone(),
                message: why.into(),
                process_document_id: None,
                revision: None,
                observations: BTreeMap::new(),
                executed_at: None,
            })
            .collect(),
    }
}
fn incomplete(
    identity: AdapterIdentity,
    m: &Manifest,
    s: &Scenario,
    message: String,
    cancelled: bool,
) -> AdapterRun {
    let mut run = not_run(identity, m, s, "adapter stopped before complete evidence");
    if let Some(first) = run.receipts.first_mut() {
        first.status = if cancelled {
            ExecutionStatus::Cancelled
        } else {
            ExecutionStatus::Incomplete
        };
        first.message = message;
        first.executed_at = None;
    }
    run
}
pub fn validate_run(
    run: &AdapterRun,
    m: &Manifest,
    s: &Scenario,
    expected: &AdapterIdentity,
) -> Result<(), String> {
    if &run.identity != expected || run.source != m.source {
        return Err("adapter/source identity mismatch".into());
    }
    if run.receipts.len() != s.operations.len() {
        return Err("incomplete adapter receipt set".into());
    }
    for (r, step) in run.receipts.iter().zip(&s.operations) {
        if r.step_id != step.id || r.expected != step.expected {
            return Err("receipt identity/outcome mismatch".into());
        }
        if r.status == ExecutionStatus::NotRun && r.executed_at.is_some() {
            return Err("not-run receipt contains execution timestamp".into());
        }
        if r.executed_at.is_some()
            && r.process_document_id.as_deref() != Some(m.document_id.as_str())
        {
            return Err("durable document identity mismatch in executed receipt".into());
        }
        if r.status == ExecutionStatus::Passed
            && (r.executed_at.is_none()
                || r.process_document_id.is_none()
                || r.revision.is_none()
                || r.observations.is_empty())
        {
            return Err("passed receipt lacks execution evidence".into());
        }
    }
    Ok(())
}
/// Preserve the raw mismatched identities/receipts rather than replacing them
/// with an expected identity when validation refuses evidence.
fn retain_invalid_run(mut run: AdapterRun, error: String) -> AdapterRun {
    run.execution_issues
        .push(format!("adapter evidence refused: {error}"));
    run
}
/// Shutdown outcome is independent from operation evidence. Even interrupted
/// children may have cooperatively published complete or partial receipts.
fn recover_reference(
    bytes: Result<Vec<u8>, String>,
    completion: &ProcessCompletion,
    m: &Manifest,
    s: &Scenario,
) -> AdapterRun {
    let mut issues = Vec::new();
    if let Some(reason) = &completion.interruption {
        issues.push(format!("reference interrupted: {reason}"));
    }
    if let Some(error) = &completion.cleanup_error {
        issues.push(format!("reference shutdown incomplete: {error}"));
    }
    match completion.exit {
        Some(exit) if !exit.success() => issues.push(format!("reference process {exit}")),
        None => issues.push("reference process exit unavailable".into()),
        _ => {}
    }
    let decoded = bytes.and_then(|bytes| {
        serde_json::from_slice::<AdapterRun>(&bytes)
            .map_err(|error| format!("malformed reference response: {error}"))
    });
    let mut run = match decoded {
        Ok(run) => match validate_run(&run, m, s, &reference_identity()) {
            Ok(()) => run,
            Err(error) => retain_invalid_run(run, error),
        },
        Err(error) => {
            issues.push(format!("reference evidence unavailable: {error}"));
            // No usable receipts exist. These placeholders have no timestamps.
            incomplete(reference_identity(), m, s, issues.join("; "), false)
        }
    };
    run.execution_issues.extend(issues);
    run
}
pub fn validate_model(
    cad_dir: &Path,
    space: &Workspace,
    m: &Manifest,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let manifest = space.root.join("validation-manifest.json");
    let mut f = space
        .directory
        .new_file(Path::new("validation-manifest.json"))?;
    serde_json::to_writer(&mut f, m).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(crate::cad_client::service::interpreter(cad_dir)?);
    cmd.args(["-m", "robocad.parity_reference", "--validate-model"])
        .arg(&space.model)
        .arg("--manifest")
        .arg(manifest)
        .env("TMPDIR", space.root.join("tmp"))
        .env("ROBOCAD_LOG_DIR", space.root.join("logs"))
        .current_dir(cad_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(space.directory.new_file(Path::new("validation.log"))?);
    let mut child = OwnedProcess::spawn(&mut cmd)?;
    let exit = child.wait(Instant::now() + Duration::from_secs(30), cancelled)?;
    if exit.success() {
        Ok(())
    } else {
        Err(format!("archive/dependency preflight refused: {exit}"))
    }
}
fn python_sources(
    root: &Path,
    dir: &Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    super::isolation::absolute_no_symlinks(dir)?;
    if dir.components().count() > 64 {
        return Err("Python source traversal depth bound exceeded".into());
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            return Err(format!(
                "Python source symlink refused: {}",
                entry.path().display()
            ));
        }
        if ty.is_dir() {
            if entry.file_name() != "__pycache__" {
                python_sources(root, &entry.path(), files)?;
            }
        } else if ty.is_file() && entry.path().extension().is_some_and(|e| e == "py") {
            files.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_owned(),
            );
            if files.len() > 5000 {
                return Err("Python source fingerprint file bound exceeded".into());
            }
        }
    }
    Ok(())
}
fn source_identity(repository: &Path, code_identity: &str) -> Result<String, String> {
    let directory = super::owned_path::Directory::open(repository)?;
    let mut code_hash = sha2::Sha256::new();
    use sha2::Digest;
    for path in [
        "crates/sim-runtime/src/cad_parity/contract.rs",
        "crates/sim-runtime/src/cad_parity/compare.rs",
        "crates/sim-runtime/src/cad_parity/gates.rs",
        "crates/sim-runtime/src/cad_parity/native.rs",
        "crates/sim-runtime/src/cad_parity/runner.rs",
        "crates/sim-runtime/src/cad_parity/runner_recovery_fixtures.rs",
        "crates/sim-runtime/src/cad_parity/process_fixtures.rs",
        "crates/sim-runtime/src/cad_parity/isolation.rs",
        "crates/sim-runtime/src/cad_parity/publication.rs",
        "crates/sim-runtime/src/cad_parity/process.rs",
        "crates/sim-runtime/src/cad_parity/native_observations.rs",
        "crates/sim-runtime/src/cad_parity/owned_path.rs",
        "Cargo.lock",
        "cad/robocad/parity_reference.py",
        "cad/robocad/parity_operations.py",
        "cad/robocad/parity_observations.py",
        "cad/robocad/api.py",
        "cad/robocad/document.py",
        "cad/robocad/commands.py",
        "cad/robocad/physical.py",
        "cad/robocad/robotics.py",
    ] {
        code_hash.update(path.as_bytes());
        code_hash.update([0]);
        code_hash.update(directory.read(Path::new(path), super::isolation::MAX_FILE)?);
    }
    let mut python_files = Vec::new();
    python_sources(
        repository,
        &repository.join("cad/robocad"),
        &mut python_files,
    )?;
    python_files.sort();
    let mut source_bytes = 0u64;
    for path in python_files {
        let bytes = directory.read(&path, 2 * 1024 * 1024)?;
        source_bytes += bytes.len() as u64;
        if bytes.len() > 2 * 1024 * 1024 || source_bytes > 64 * 1024 * 1024 {
            return Err("Python source fingerprint byte bound exceeded".into());
        }
        let name = path.to_str().ok_or("Python source path not UTF8")?;
        code_hash.update((name.len() as u64).to_le_bytes());
        code_hash.update(name.as_bytes());
        code_hash.update((bytes.len() as u64).to_le_bytes());
        code_hash.update(bytes);
    }
    let result = format!(
        "label={code_identity};source-sha256={:x};binary-version={};compiled-library-blake3={}",
        code_hash.finalize(),
        env!("CARGO_PKG_VERSION"),
        env!("SIM_RUNTIME_SOURCE_BLAKE3")
    );

    Ok(result)
}
pub fn paired(
    repository: &Path,
    m: &Manifest,
    s: &Scenario,
    code_identity: String,
    execute: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Report, String> {
    if code_identity.trim().is_empty() {
        return Err("code identity required".into());
    }
    if m.schema_version != SCHEMA_VERSION {
        return Err("unsupported manifest schema".into());
    }
    if m.id.trim().is_empty()
        || m.document_id.trim().is_empty()
        || m.model_schema.trim().is_empty()
        || m.units.trim().is_empty()
        || m.frame.trim().is_empty()
    {
        return Err("manifest model identity/schema/units/frame required".into());
    }
    let mut scenario_ids = std::collections::BTreeSet::new();
    if m.scenarios
        .iter()
        .any(|scenario| !scenario_ids.insert(scenario.id.as_str()))
        || !m.scenarios.iter().any(|scenario| scenario == s)
    {
        return Err("scenario must uniquely belong to declared manifest".into());
    }
    let identity_before = source_identity(repository, &code_identity)?;
    // Validation and hashing also happen for plans, but plans never spawn a child.
    for source in std::iter::once(&m.source).chain(m.dependencies.iter()) {
        super::isolation::read_checked(repository, source)?;
    }
    let mut reference = not_run(reference_identity(), m, s, "planning only; no execution");
    let mut native = not_run(
        super::native::identity(),
        m,
        s,
        "planning only; no execution",
    );
    if execute {
        #[cfg(not(unix))]
        return Err("process group ownership requires Unix; execution unsupported".into());
        let reference_space = Workspace::create(repository, m, "reference")?;
        let native_space = Workspace::create(repository, m, "native")?;
        let cad_dir = repository.join("cad");
        let reference_result = (|| {
            validate_model(&cad_dir, &reference_space, m, cancelled)?;
            let manifest_path = reference_space.root.join("manifest.json");
            let mut f = reference_space
                .directory
                .new_file(Path::new("manifest.json"))?;
            serde_json::to_writer(&mut f, m).map_err(|e| e.to_string())?;
            let output = reference_space.root.join("reference.json");
            let mut cmd = Command::new(crate::cad_client::service::interpreter(&cad_dir)?);
            cmd.args(["-m", "robocad.parity_reference", "--manifest"])
                .arg(manifest_path)
                .args(["--scenario", &s.id, "--model"])
                .arg(&reference_space.model)
                .arg("--output")
                .arg(&output)
                .env("ROBOCAD_LOG_DIR", reference_space.root.join("logs"))
                .env("TMPDIR", reference_space.root.join("tmp"))
                .env("TMP", reference_space.root.join("tmp"))
                .env("TEMP", reference_space.root.join("tmp"))
                .current_dir(&cad_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(
                    reference_space
                        .directory
                        .new_file(Path::new("reference.log"))?,
                );
            let mut child = OwnedProcess::spawn(&mut cmd)?;
            let completion =
                child.wait_completion(Instant::now() + Duration::from_secs(300), cancelled);
            // Always inspect owned output after bounded shutdown, including
            // cancellation, timeout, nonzero exit and cleanup errors.
            let bytes = reference_space
                .directory
                .read(Path::new("reference.json"), super::isolation::MAX_FILE);
            Ok(recover_reference(bytes, &completion, m, s))
        })();
        reference = reference_result
            .unwrap_or_else(|e| incomplete(reference_identity(), m, s, e, cancelled()));
        let mut adapter = NativeAdapter {
            cad_dir,
            workspace: native_space,
        };
        native = match adapter.run(m, s, cancelled) {
            Ok(run) => match validate_run(&run, m, s, &super::native::identity()) {
                Ok(()) => run,
                Err(e) => retain_invalid_run(run, e),
            },
            Err(e) => incomplete(super::native::identity(), m, s, e, cancelled()),
        };
    }
    if source_identity(repository, &code_identity)? != identity_before {
        for run in [&mut reference, &mut native] {
            run.execution_issues.push("authority source files changed during paired operation; identity cannot be certified".into());
        }
    }
    let code_identity = identity_before;
    let diagnostics = super::compare::compare(s, &reference, &native);
    let gates = super::gates::aggregate(s, &reference, &native, &diagnostics);
    Ok(Report {
        schema_version: SCHEMA_VERSION,
        code_identity,
        manifest: m.clone(),
        scenario_id: s.id.clone(),
        reference,
        native,
        diagnostics,
        gates,
    })
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn empty_run_cannot_manufacture_success() {
        let m = Manifest {
            schema_version: 1,
            id: "fixture".into(),
            source: SourceFile {
                path: "model.rcad".into(),
                sha256: "0".repeat(64),
            },
            dependencies: vec![],
            model_schema: "1".into(),
            document_id: "durable".into(),
            units: "mm".into(),
            frame: "cad-world".into(),
            provenance: "fixture".into(),
            coverage: vec![],
            unavailable: vec![],
            scenarios: vec![],
        };
        let s = Scenario {
            id: "lifecycle".into(),
            operations: vec![Step {
                id: "observe".into(),
                operation: Operation::Observe,
                expected: ExpectedOutcome::Success,
            }],
            comparisons: vec![],
            required_coverage: vec![],
        };
        let mut r = not_run(reference_identity(), &m, &s, "plan");
        assert!(r.receipts[0].executed_at.is_none());
        r.receipts[0].executed_at = Some("forged".into());
        assert!(validate_run(&r, &m, &s, &reference_identity()).is_err());
        let mut mismatched = not_run(reference_identity(), &m, &s, "raw identity");
        mismatched.source.sha256 = "f".repeat(64);
        mismatched.receipts[0].process_document_id = Some("unexpected-document".into());
        let preserved = retain_invalid_run(mismatched, "source mismatch".into());
        assert_eq!(preserved.source.sha256, "f".repeat(64));
        assert_eq!(
            preserved.receipts[0].process_document_id.as_deref(),
            Some("unexpected-document")
        );
        assert_eq!(preserved.receipts[0].status, ExecutionStatus::NotRun);
        assert!(!preserved.execution_issues.is_empty());
        assert!(preserved.receipts[0].executed_at.is_none());
        r.receipts.clear();
        assert!(validate_run(&r, &m, &s, &reference_identity()).is_err());
    }
}

#[cfg(test)]
#[path = "runner_recovery_fixtures.rs"]
mod recovery_fixtures;
