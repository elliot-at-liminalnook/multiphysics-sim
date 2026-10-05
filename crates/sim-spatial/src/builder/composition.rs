//! Composition in Build mode (docs/architecture/composition.md): add an FMU
//! as a block (its interface and SHA-256 read from the archive), add a robot
//! as a generated assembly (its ports read from its `.simrobot.json`), set a
//! block's timing (`system` command `set_block_timing`), and run the
//! system's acceptance tests off the UI thread, their evidence kept beside
//! the file and judged current or stale against what the system is now.
//! Every edit goes through `Builder::apply` (validation, shared undo).
use super::*;
use sim_runtime::system_evidence::{self, Evidence};

/// One acceptance test running on a background job.
pub(crate) struct TestRun {
    pub(crate) name: String,
    pub(crate) job: crate::jobs::Job<Evidence>,
}

#[derive(Default)]
pub(crate) struct Composition {
    pub(crate) test_run: Option<TestRun>,
    /// The last finished test run (its evidence or why it failed).
    pub(crate) last: Option<(String, Result<Evidence, String>)>,
}

impl Builder {
    /// `path` as the document stores it: relative to the system file's
    /// directory (an absolute path under it is made relative).
    fn relative_to_system(&self, path: &str) -> Result<String, String> {
        let p = std::path::Path::new(path);
        if !p.is_absolute() {
            sim_system::check_relative_path(path)?;
            return Ok(path.to_owned());
        }
        let dir = self.system_dir();
        p.strip_prefix(&dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .map_err(|_| format!("{path} is outside the system file's directory {}: copy it next to the system file (paths are stored relative to it)", dir.display()))
    }

    /// A free display spot at the current level: right of what is there.
    fn free_spot(&self) -> [f32; 3] {
        let right = self.definition().map(|d| d.instances.values().map(|i| i.placement.position[0]).fold(f32::NEG_INFINITY, f32::max)).unwrap_or(f32::NEG_INFINITY);
        [if right.is_finite() { right + 0.25 } else { 0.0 }, 0.0, 0.0]
    }

    fn add_instance(&mut self, pick: Option<&mut Picked>, at: &str, name: Option<String>, spec: sim_system::InstanceSpec, label: &str) -> Result<String, String> {
        let name = match name {
            Some(n) => n,
            None => self.unique_name(&sim_system::kind_base_name(&spec.kind)),
        };
        let spec = spec.at(self.free_spot());
        self.apply(label, vec![sim_system::Command::AddInstance { at: at.to_owned(), name: name.clone(), instance: spec }])?;
        if let Some(pick) = pick {
            pick.sync(self);
            let _ = pick.set([name.clone()]);
        }
        Ok(name)
    }

    /// Add the FMU at `path` as a block instance (`system_add_fmu`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_fmu(&mut self, pick: Option<&mut Picked>, at: &str, name: Option<String>, path: &str, timing: sim_core::BlockTiming, kinds: &BTreeMap<String, String>, parameters: &BTreeMap<String, f64>) -> Result<String, String> {
        let relative = self.relative_to_system(path)?;
        let kinds = kinds.iter().map(|(port, kind)| sim_fmi::units::kind_named(kind).map(|k| (port.clone(), k)).ok_or_else(|| format!("`{kind}` (port `{port}`) is not a quantity a block port can carry (Temperature, HeatFlow, Angle, Torque, …)"))).collect::<Result<BTreeMap<_, _>, _>>()?;
        let spec = sim_runtime::system_blocks::fmu_instance(&self.system_dir(), &relative, timing, &kinds, parameters)?;
        self.add_instance(pick, at, name, spec, &format!("Add FMU {relative}"))
    }

    /// Add a robot generated from the `.simrobot.json` at `source`
    /// (`system_add_robot`); `options` are the robot generator's flags that
    /// are set (`driver_control`, `own_supply`, `own_ambient`).
    pub(crate) fn add_robot(&mut self, pick: Option<&mut Picked>, at: &str, name: Option<String>, source: &str, options: &[&str]) -> Result<String, String> {
        let relative = self.relative_to_system(source)?;
        let parameters: BTreeMap<String, f64> = options.iter().map(|o| (o.to_string(), 1.0)).collect();
        let spec = sim_runtime::system_blocks::robot_instance_with(&self.registry, &self.system_dir(), &relative, &parameters)?;
        self.add_instance(pick, at, name, spec, &format!("Add robot {relative}"))
    }

    /// Record the ports a generated instance's source offers now
    /// (`system_refresh_generated`): after its file changed its joints.
    pub(crate) fn refresh_generated(&mut self, at: &str, name: &str) -> Result<String, String> {
        let definition = sim_system::Resolver::new(&self.document, &self.registry).definition_id_at(at).map_err(|e| e.to_string())?;
        let instance = self.document.definitions.get(&definition).and_then(|d| d.instances.get(name)).ok_or_else(|| format!("no instance `{name}` here"))?;
        let InstanceKind::Generated { generator, source, ports } = &instance.kind else { return Err(format!("`{name}` is not a generated assembly")) };
        let parameters: BTreeMap<String, f64> = instance.parameters.iter().filter_map(|(k, b)| match b {
            sim_system::ParameterBinding::Value { value, .. } => Some((k.clone(), *value)),
            _ => None,
        }).collect();
        let now = sim_runtime::robot_generator::generators(&self.system_dir()).ports(&self.registry, generator, source, &parameters)?;
        if now == *ports {
            return Ok(format!("{name} already matches {source}"));
        }
        let (added, removed): (Vec<&String>, Vec<&String>) = (now.keys().filter(|k| !ports.contains_key(*k)).collect(), ports.keys().filter(|k| !now.contains_key(*k)).collect());
        let message = format!("Refreshed {name} from {source}: {} port(s) added{}, {} removed{}", added.len(), if added.is_empty() { String::new() } else { format!(" ({})", added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) }, removed.len(), if removed.is_empty() { String::new() } else { format!(" ({})", removed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) });
        self.apply(&format!("Refresh {name}"), vec![sim_system::Command::RefreshGenerated { at: at.to_owned(), name: name.to_owned(), ports: now }])?;
        Ok(message)
    }

    /// Change a block's clock, keeping its delays and deadline.
    pub(crate) fn set_block_clock(&mut self, name: &str, period: f64, offset: f64) -> Result<(), String> {
        let spec = self.spec(name).ok_or_else(|| format!("no instance `{name}` here"))?;
        let InstanceKind::Block { timing, .. } = spec.kind else { return Err(format!("`{name}` is not a block")) };
        let timing = sim_core::BlockTiming { clock: sim_core::Clock::Periodic { period, offset }, ..timing };
        self.apply(&format!("Clock of {name}"), vec![sim_system::Command::SetBlockTiming { at: self.level.clone(), name: name.to_owned(), timing }]).map(|_| ())
    }

    /// The draft "path [period_s]": an FMU block at the current level
    /// (period: as given, else the FMU's default step).
    pub(crate) fn add_fmu_from_text(&mut self, pick: Option<&mut Picked>, text: &str) -> Result<(), String> {
        let mut parts = text.split_whitespace();
        let path = parts.next().ok_or("Type the FMU's path (relative to the system file), then optionally its period in seconds")?;
        let period = match parts.next() {
            Some(p) => p.parse::<f64>().map_err(|_| format!("`{p}` is not a period in seconds"))?,
            None => self.inspect_fmu(path)?.get("default_step").and_then(|v| v.as_f64()).ok_or("The FMU states no default step: type its period after the path (e.g. fmus/controller.fmu 0.01)")?,
        };
        let level = self.level.clone();
        self.add_fmu(pick, &level, None, path, sim_core::BlockTiming::periodic(period), &BTreeMap::new(), &BTreeMap::new()).map(|name| self.status = format!("Added FMU block {name}: connect its ports, then Run."))
    }

    /// What an FMU offers (`system_inspect_fmu`).
    pub(crate) fn inspect_fmu(&self, path: &str) -> Result<serde_json::Value, String> {
        let p = std::path::Path::new(path);
        let resolved = if p.is_absolute() { p.to_path_buf() } else { self.system_dir().join(p) };
        let fmu = sim_fmi::Fmu::load(&resolved).map_err(|e| e.to_string())?;
        serde_json::to_value(fmu.summary()).map_err(|e| e.to_string())
    }

    /// Start running acceptance test `name` (refused while one runs).
    pub(crate) fn run_test(&mut self, name: &str) -> Result<(), String> {
        if let Some(run) = &self.composition.test_run {
            return Err(format!("test `{}` is still running; wait for it (system_state.composition.running)", run.name));
        }
        if !self.document.tests.contains_key(name) {
            return Err(format!("no test `{name}` (system_test {{action: set, name, test}} saves one)"));
        }
        let (document, registry, base, test) = (self.document.clone(), self.registry.clone(), self.system_dir(), name.to_owned());
        let job = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, self.document.revision, format!("system test {name}"), move |ctx| {
            system_evidence::assess(&document, &registry, &base, &test, Some(ctx.cancel_flag()))
        });
        self.composition.test_run = Some(TestRun { name: name.to_owned(), job });
        self.status = format!("Running test {name} on a background thread…");
        self.panel_dirty = true;
        Ok(())
    }

    /// Keep a finished test run's evidence (called every frame).
    pub(crate) fn finish_test(&mut self) {
        let Some(run) = &self.composition.test_run else { return };
        let Some(result) = run.job.poll() else { return };
        let name = self.composition.test_run.take().expect("polled above").name;
        let kept = result.and_then(|evidence| system_evidence::save(&self.store.path, &evidence).map(|_| evidence));
        self.status = match &kept {
            Ok(e) => format!("Test {name}: {:?} ({} requirement{}); evidence kept in {}", e.verdict, e.results.len(), if e.results.len() == 1 { "" } else { "s" }, system_evidence::path_for(&self.store.path).display()),
            Err(e) => format!("Test {name} did not finish: {e}"),
        };
        self.composition.last = Some((name, kept));
        self.panel_dirty = true;
    }

    /// Tests, where each stands, the running one and the last result.
    pub(crate) fn composition_json(&self) -> serde_json::Value {
        let standing = system_evidence::standing(&self.document, &self.store.path);
        let evidence = system_evidence::load(&self.store.path);
        serde_json::json!({
            "tests": self.document.tests,
            "standing": standing.as_ref().map_err(|e| e.to_string()),
            "evidence": evidence.as_ref().map_err(|e| e.to_string()),
            "evidence_file": system_evidence::path_for(&self.store.path),
            "verdict_rule": system_evidence::VERDICT_RULE,
            "running": self.composition.test_run.as_ref().map(|r| &r.name),
            "last": self.composition.last.as_ref().map(|(name, r)| serde_json::json!({"test": name, "result": r.as_ref().map_err(|e| e.clone())})),
            "blocks": self.flattened_blocks(),
        })
    }

    /// Every block at every level: path, source, interface and timing.
    fn flattened_blocks(&self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        fn walk(doc: &SystemDocument, definition: &str, path: &str, out: &mut Vec<serde_json::Value>, depth: usize) {
            let Some(d) = doc.definitions.get(definition) else { return };
            if depth > 64 {
                return;
            }
            for (name, i) in &d.instances {
                let here = sim_system::join_path(path, name);
                match &i.kind {
                    InstanceKind::Block { implementation, interface, timing } => out.push(serde_json::json!({"path": here, "implementation": implementation, "interface": interface, "timing": timing, "parameters": i.parameters})),
                    InstanceKind::Generated { generator, source, ports } => out.push(serde_json::json!({"path": here, "generator": generator, "source": source, "ports": ports.keys().collect::<Vec<_>>()})),
                    InstanceKind::Subsystem { definition } => walk(doc, definition, &here, out, depth + 1),
                    InstanceKind::Element { .. } => {}
                }
            }
        }
        walk(&self.document, &self.document.root, "", &mut out, 0);
        out
    }
}

/// Keep a finished test run's evidence.
pub(super) fn finish_tests(mut builder: ResMut<Builder>) {
    if builder.composition.test_run.is_some() {
        builder.finish_test();
    }
}
