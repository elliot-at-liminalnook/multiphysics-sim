//! Sequential evidence host for the shared validation primitives.
//! All physical preparation, simulation and acceptance remain in Rust library
//! hosts. This process only schedules them and retains immutable stage receipts.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_runtime::{fidelity::EnvironmentCapture, physics_context::RuntimeIdentity};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    name: String,
    source_spec: PathBuf,
    aliases: Value,
    qualify: bool,
    cases: Vec<String>,
    timestep_case: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    candidates: Vec<Candidate>,
    cases: PathBuf,
    profile: PathBuf,
    motion_gates: PathBuf,
    geometry_gates: PathBuf,
    numerical_gates: PathBuf,
    expected_physics_runtime: RuntimeIdentity,
    /// Explicit archived or built executables; no PATH lookup or rebuilding.
    tools: BTreeMap<String, PathBuf>,
}
fn read<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn write(p: impl AsRef<Path>, v: &impl Serialize) -> Result<()> {
    let f = OpenOptions::new().create_new(true).write(true).open(p)?;
    let mut w = BufWriter::new(f);
    serde_json::to_writer(&mut w, v)?;
    w.flush()?;
    w.get_ref().sync_all()?;
    Ok(())
}
fn hash(p: &Path) -> Result<String> {
    Ok(blake3::hash(&fs::read(p)?).to_hex().to_string())
}
fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
struct Host {
    tools: BTreeMap<String, PathBuf>,
    hashes: BTreeMap<String, String>,
    cancel: PathBuf,
}
impl Host {
    fn check_cancel(&self) -> Result<()> {
        if self.cancel.exists() {
            return Err(
                "campaign cancelled; all completed and interrupted evidence retained".into(),
            );
        }
        Ok(())
    }
    fn run(
        &self,
        dir: &Path,
        stage: &str,
        tool: &str,
        args: &[String],
        stdout: &Path,
    ) -> Result<bool> {
        self.check_cancel()?;
        let exe = self.tools.get(tool).ok_or("missing executable")?;
        if hash(exe)? != self.hashes[tool] {
            return Err("executable changed during campaign".into());
        }
        let prefix = dir.join(stage);
        write(
            prefix.with_extension("started.json"),
            &json!({
                "tool":tool,"executable":exe,"executable_blake3":self.hashes[tool],
                "args":args,"stdout":stdout,"started_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
            }),
        )?;
        let out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(stdout)?;
        let err = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(prefix.with_extension("stderr.log"))?;
        eprintln!("{}: {stage}", dir.display());
        let start = Instant::now();
        let mut child = Command::new(exe)
            .args(args)
            .env("RAYON_NUM_THREADS", "1")
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()?;
        let mut cancelled = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if self.cancel.exists() {
                cancelled = true;
                // run_environment has no incremental capture stream. Preserve
                // its partial stdout, but never treat it as a completed capture.
                child.kill()?;
                break child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        write(
            prefix.with_extension("receipt.json"),
            &json!({
                "success":status.success(),"exit_code":status.code(),"cancelled":cancelled,
                "wall_s":start.elapsed().as_secs_f64(),"stdout_blake3":hash(stdout)?
            }),
        )?;
        if cancelled {
            self.check_cancel()?;
        }
        Ok(status.success())
    }
    fn command(&self, dir: &Path, stage: &str, tool: &str, args: &[String]) -> Result<bool> {
        self.run(
            dir,
            stage,
            tool,
            args,
            &dir.join(format!("{stage}.stdout.log")),
        )
    }
}
fn validate_capture(h: &Host, config: &Config, dir: &Path) -> Result<Value> {
    let capture_path = dir.join("capture.json");
    let run_success = h.run(
        dir,
        "simulate",
        "simulate",
        &["--experiment".into(), path(&dir.join("spec.json"))],
        &capture_path,
    )?;
    // A failed simulation may still save a useful failed capture. Evaluate it;
    // exit status alone is never interpreted as physical acceptance.
    let capture: EnvironmentCapture = read(&capture_path)?;
    if capture.recording.runtime_identity.as_ref() != Some(&config.expected_physics_runtime) {
        return Err("capture physics runtime differs from declared frozen runtime".into());
    }
    let geometry = dir.join("geometry.json");
    if !h.run(
        dir,
        "audit",
        "audit",
        &[path(&capture_path), "--pairs".into()],
        &geometry,
    )? {
        return Err("geometry audit failed; see retained stage receipt".into());
    }
    let evaluation = dir.join("evaluation.json");
    if !h.command(
        dir,
        "evaluate",
        "evaluate",
        &[
            path(&capture_path),
            path(&geometry),
            path(&config.motion_gates),
            path(&config.geometry_gates),
            path(&evaluation),
        ],
    )? {
        return Err("capture evaluation failed; see retained stage receipt".into());
    }
    let report: Value = read(&evaluation)?;
    Ok(
        json!({"directory":dir,"simulation_exit_success":run_success,
        "passed":run_success && report["passed"]==true,"evaluation":evaluation}),
    )
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: run_validation_campaign config.json fresh-directory cancel-file".into(),
        );
    }
    let config: Config = read(&args[0])?;
    let cases: Vec<sim_runtime::experiment_variants::Case> = read(&config.cases)?;
    let names = cases
        .iter()
        .map(|c| c.name.as_str())
        .collect::<BTreeSet<_>>();
    if cases.is_empty() || names.len() != cases.len() || names.iter().any(|n| !safe_name(n)) {
        return Err("explicit uniquely named cases required".into());
    }
    let mut candidate_names = BTreeSet::new();
    for c in &config.candidates {
        if !safe_name(&c.name)
            || !candidate_names.insert(&c.name)
            || c.cases.is_empty()
            || c.cases.iter().collect::<BTreeSet<_>>().len() != c.cases.len()
            || c.cases.iter().any(|n| !names.contains(n.as_str()))
            || c.timestep_case
                .as_ref()
                .is_some_and(|n| !c.cases.contains(n))
        {
            return Err("invalid candidate names or case selection".into());
        }
    }
    if config.candidates.is_empty() {
        return Err("no candidates".into());
    }
    for tool in [
        "qualify",
        "prepare",
        "refine",
        "simulate",
        "audit",
        "evaluate",
        "numerical",
    ] {
        if !config.tools.contains_key(tool) {
            return Err(format!("missing tool {tool}").into());
        }
    }
    let mut tools = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for (name, p) in &config.tools {
        tools.insert(name.clone(), p.canonicalize()?);
        hashes.insert(name.clone(), hash(p)?);
    }
    let h = Host {
        tools,
        hashes,
        cancel: PathBuf::from(&args[2]),
    };
    h.check_cancel()?;
    let root = Path::new(&args[1]);
    fs::create_dir(root)?;
    write(
        root.join("manifest.json"),
        &json!({"config":config,"tools":h.tools,
        "executable_hashes":h.hashes,"host_runtime":RuntimeIdentity::current(),
        "scope":"Sequential orchestration only; all preparation, physics and gates use shared Rust library hosts"}),
    )?;
    let case_dir = root.join("cases");
    fs::create_dir(&case_dir)?;
    for c in &cases {
        write(case_dir.join(format!("{}.json", c.name)), c)?;
    }
    let mut outcomes = vec![];
    for c in &config.candidates {
        h.check_cancel()?;
        let dir = root.join(&c.name);
        fs::create_dir(&dir)?;
        let source = dir.join("source.spec.json");
        // Retain exact input bytes, independent of the original working tree.
        let bytes = fs::read(&c.source_spec)?;
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&source)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        write(
            dir.join("candidate.json"),
            &json!({"candidate":c,"source_blake3":blake3::hash(&bytes).to_hex().to_string()}),
        )?;
        let mut qualification = Value::Null;
        if c.qualify {
            let qdir = dir.join("qualification");
            let result = (|| -> Result<Value> {
                if !h.command(
                    &dir,
                    "qualification-prepare",
                    "qualify",
                    &[
                        "prepare".into(),
                        path(&source),
                        path(&config.profile),
                        path(&qdir),
                        "--fresh".into(),
                    ],
                )? {
                    return Err("qualification preparation failed".into());
                }
                let ok = h.command(
                    &dir,
                    "qualification-run",
                    "qualify",
                    &["qualify".into(), path(&qdir), path(&h.cancel)],
                )?;
                Ok(json!({"exit_success":ok,"directory":qdir,
                    "report":read::<Value>(qdir.join("qualification.json"))?}))
            })();
            qualification =
                result.unwrap_or_else(|e| json!({"error":e.to_string(),"passed":false}));
            write(dir.join("qualification-outcome.json"), &qualification)?;
        }
        let mut case_outcomes = BTreeMap::new();
        for name in &c.cases {
            h.check_cancel()?;
            let target = dir.join(name);
            let result = (|| -> Result<Value> {
                if !h.command(
                    &dir,
                    &format!("prepare-{name}"),
                    "prepare",
                    &[
                        path(&source),
                        path(&case_dir.join(format!("{name}.json"))),
                        path(&target),
                    ],
                )? {
                    return Err("case preparation failed".into());
                }
                validate_capture(&h, &config, &target)
            })();
            let outcome = result.unwrap_or_else(
                |e| json!({"directory":target,"passed":false,"error":e.to_string()}),
            );
            write(dir.join(format!("{name}.outcome.json")), &outcome)?;
            case_outcomes.insert(name, outcome);
        }
        let mut numerical = Value::Null;
        if let Some(name) = &c.timestep_case {
            h.check_cancel()?;
            let reference = dir.join(name);
            let refined = dir.join(format!("{name}-half-step"));
            let result = (|| -> Result<Value> {
                let gates: sim_runtime::numerical_validation::Gates =
                    read(&config.numerical_gates)?;
                if !h.command(
                    &dir,
                    "refine",
                    "refine",
                    &[
                        path(&reference.join("spec.json")),
                        gates.step_divisor.to_string(),
                        path(&refined),
                    ],
                )? {
                    return Err("timestep preparation failed".into());
                }
                let fine = validate_capture(&h, &config, &refined)?;
                let report = dir.join("numerical.json");
                if !h.command(
                    &dir,
                    "numerical",
                    "numerical",
                    &[
                        path(&reference.join("capture.json")),
                        path(&refined.join("capture.json")),
                        path(&config.numerical_gates),
                        path(&report),
                    ],
                )? {
                    return Err("numerical evaluation failed".into());
                }
                let comparison: Value = read(&report)?;
                Ok(
                    json!({"passed":fine["passed"]==true && case_outcomes[name]["passed"]==true
                    && comparison["report"]["passed"]==true,"refined":fine,"comparison":report}),
                )
            })();
            numerical = result.unwrap_or_else(|e| json!({"passed":false,"error":e.to_string()}));
            write(dir.join("numerical-outcome.json"), &numerical)?;
        }
        let outcome = json!({"candidate":c,"qualification":qualification,"cases":case_outcomes,"numerical":numerical});
        write(dir.join("outcome.json"), &outcome)?;
        outcomes.push(outcome);
    }
    write(
        root.join("outcomes.json"),
        &json!({"all_scheduled_work_attempted":true,"candidates":outcomes,
        "scope":"Scheduling completion is not acceptance; inspect every case, qualification and numerical result"}),
    )?;
    eprintln!(
        "Campaign finished; every scheduled outcome retained. Inspect gates before accepting finalists."
    );
    Ok(())
}
