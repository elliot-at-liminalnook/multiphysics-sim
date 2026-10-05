//! An FMU archive: read, checked against the supported profile, extracted,
//! inspected, and turned into a block declaration or a running instance.
use crate::description::{self, Causality, ModelDescription, ValueType, Variability, Variable};
use crate::{FmiError, units};
use serde::Serialize;
use sha2::Digest;
use sim_core::{BlockInterface, BlockPort, QuantityKind};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The FMI 3 platform tuple of this process (`x86_64-darwin`, …).
pub fn platform() -> Result<String, String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "x86",
        other => return Err(format!("no FMI 3 platform tuple for the {other} architecture")),
    };
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => return Err(format!("no FMI 3 platform tuple for {other}")),
    };
    Ok(format!("{arch}-{os}"))
}

/// The SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A loaded FMU: validated against the profile, extracted to a private
/// directory that lives as long as this does.
pub struct Fmu {
    pub path: PathBuf,
    pub sha256: String,
    pub description: ModelDescription,
    dir: tempfile::TempDir,
}

impl std::fmt::Debug for Fmu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fmu").field("path", &self.path).field("model", &self.description.model_name).field("sha256", &self.sha256).finish()
    }
}

/// What the FMU offers, for inspection (REST, UI, CLI).
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub path: String,
    pub sha256: String,
    pub model_name: String,
    pub description: Option<String>,
    pub fmi_version: String,
    pub generation_tool: Option<String>,
    pub instantiation_token: String,
    pub co_simulation: Option<description::CoSimulation>,
    pub default_step: Option<f64>,
    pub inputs: Vec<VariableSummary>,
    pub outputs: Vec<VariableSummary>,
    pub parameters: Vec<VariableSummary>,
    /// Why a block cannot use the FMU as it is (empty: it can).
    pub unsupported: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VariableSummary {
    pub name: String,
    pub value_type: ValueType,
    pub variability: Variability,
    pub unit: Option<String>,
    /// The quantity a port of it carries, or why it cannot be inferred.
    pub kind: Result<String, String>,
    pub start: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub description: Option<String>,
}

impl Fmu {
    /// Read, validate and extract an FMU. Everything outside the supported
    /// profile is refused here, by name, before any binary is loaded.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FmiError> {
        let path = path.as_ref().to_path_buf();
        let shown = path.display().to_string();
        let archive = |message: String| FmiError::Archive(format!("{shown}: {message}"));
        let bytes = std::fs::read(&path).map_err(|e| archive(format!("cannot read the FMU: {e}")))?;
        let sha256 = sha256_hex(&bytes);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|e| archive(format!("not an FMU (zip) archive: {e}")))?;
        let mut xml = String::new();
        zip.by_name("modelDescription.xml").map_err(|_| archive("the archive has no modelDescription.xml".into()))?
            .read_to_string(&mut xml).map_err(|e| archive(format!("cannot read modelDescription.xml: {e}")))?;
        let description = description::parse(&xml).map_err(archive)?;
        let entries: Vec<String> = zip.file_names().map(str::to_owned).collect();
        let reasons = profile_problems(&description, &entries);
        if !reasons.is_empty() {
            return Err(FmiError::Unsupported { fmu: shown, reasons });
        }
        let dir = tempfile::Builder::new().prefix("sim-fmu-").tempdir().map_err(|e| archive(format!("cannot create a directory to extract into: {e}")))?;
        zip.extract(dir.path()).map_err(|e| archive(format!("cannot extract: {e}")))?;
        Ok(Self { path, sha256, description, dir })
    }

    pub fn model_identifier(&self) -> &str {
        &self.description.co_simulation.as_ref().expect("checked at load").model_identifier
    }

    /// The extracted binary for this platform.
    pub(crate) fn binary_path(&self) -> PathBuf {
        self.dir.path().join(binary_entry(self.model_identifier()).expect("checked at load"))
    }

    /// `resourcePath` as FMI 3 defines it: the absolute path of the
    /// extracted `resources/` directory with a trailing separator.
    pub(crate) fn resource_path(&self) -> String {
        let mut p = self.dir.path().join("resources").display().to_string();
        p.push(std::path::MAIN_SEPARATOR);
        p
    }

    pub fn summary(&self) -> Summary {
        let md = &self.description;
        let describe = |v: &Variable| VariableSummary {
            name: v.name.clone(),
            value_type: v.value_type,
            variability: v.variability,
            unit: v.unit.clone(),
            kind: scalar_problem(v).map_or_else(|| units::kind_of(md, v, None).map(|k| format!("{k:?}")), Err),
            start: v.start,
            min: v.min,
            max: v.max,
            description: v.description.clone(),
        };
        let of = |c: Causality| md.variables.iter().filter(|v| v.causality == c).map(describe).collect::<Vec<_>>();
        let unsupported = self.interface(&BTreeMap::new()).err().map(|e| match e {
            FmiError::Binding(m) => m.split("; ").map(str::to_owned).collect(),
            other => vec![other.to_string()],
        }).unwrap_or_default();
        Summary {
            path: self.path.display().to_string(),
            sha256: self.sha256.clone(),
            model_name: md.model_name.clone(),
            description: md.description.clone(),
            fmi_version: md.fmi_version.clone(),
            generation_tool: md.generation_tool.clone(),
            instantiation_token: md.instantiation_token.clone(),
            co_simulation: md.co_simulation.clone(),
            default_step: md.default_step,
            inputs: of(Causality::Input),
            outputs: of(Causality::Output),
            parameters: of(Causality::Parameter),
            unsupported,
        }
    }

    /// The block interface the FMU offers: every input and output variable
    /// as a port of the same name, typed by `kinds` where given (checked
    /// against the variable's unit) or by inference from its unit; output
    /// start values and declared ranges carried over. Co-Simulation outputs
    /// are end-of-step values, so `feedthrough = false`.
    pub fn interface(&self, kinds: &BTreeMap<String, QuantityKind>) -> Result<BlockInterface, FmiError> {
        let md = &self.description;
        let mut problems = Vec::new();
        let mut port = |v: &Variable| -> Option<BlockPort> {
            if let Some(p) = scalar_problem(v) {
                problems.push(p);
                return None;
            }
            match units::kind_of(md, v, kinds.get(&v.name)) {
                Ok(kind) => {
                    let mut port = BlockPort::new(v.name.clone(), kind).range(v.min, v.max);
                    port.start = v.start;
                    Some(port)
                }
                Err(e) => {
                    problems.push(e);
                    None
                }
            }
        };
        let inputs: Vec<BlockPort> = md.variables.iter().filter(|v| v.causality == Causality::Input).filter_map(&mut port).collect();
        let outputs: Vec<BlockPort> = md.variables.iter().filter(|v| v.causality == Causality::Output).filter_map(&mut port).collect();
        for name in kinds.keys() {
            if !md.variables.iter().any(|v| &v.name == name && matches!(v.causality, Causality::Input | Causality::Output)) {
                problems.push(format!("the FMU has no input or output named `{name}`"));
            }
        }
        if !problems.is_empty() {
            return Err(FmiError::Binding(problems.join("; ")));
        }
        Ok(BlockInterface { inputs, outputs, feedthrough: false })
    }

    /// Check a block's declaration against the FMU: every port names an
    /// FMU input/output of a matching quantity (exact units), feedthrough
    /// is false, and every parameter is a settable FMU parameter. FMU
    /// inputs the block leaves out keep their start values.
    pub fn check(&self, block: &str, interface: &BlockInterface, parameters: &BTreeMap<String, f64>) -> Result<Binding, FmiError> {
        let md = &self.description;
        let mut problems = Vec::new();
        if interface.feedthrough {
            problems.push("an FMI 3 Co-Simulation block has feedthrough = false (its outputs are end-of-step values)".into());
        }
        let mut map = |ports: &[BlockPort], causality: Causality| -> Vec<(u32, ValueType)> {
            ports.iter().filter_map(|p| {
                let Some(v) = md.variable(&p.name) else {
                    problems.push(format!("port `{}`: the FMU has no variable of that name", p.name));
                    return None;
                };
                if v.causality != causality {
                    problems.push(format!("port `{}`: the FMU variable is {:?}, the block uses it as {causality:?}", p.name, v.causality));
                    return None;
                }
                if let Some(e) = scalar_problem(v) {
                    problems.push(e);
                    return None;
                }
                if let Err(e) = units::kind_of(md, v, Some(&p.kind)) {
                    problems.push(e);
                    return None;
                }
                Some((v.value_reference, v.value_type))
            }).collect()
        };
        let inputs = map(&interface.inputs, Causality::Input);
        let outputs = map(&interface.outputs, Causality::Output);
        let mut settings = Vec::new();
        for (name, value) in parameters {
            match md.variable(name) {
                None => problems.push(format!("parameter `{name}`: the FMU has no variable of that name")),
                Some(v) if v.causality == Causality::StructuralParameter => problems.push(format!("parameter `{name}` is a structural parameter (configuration mode is not supported)")),
                Some(v) if v.causality != Causality::Parameter => problems.push(format!("parameter `{name}`: the FMU variable is {:?}, not a parameter", v.causality)),
                Some(v) if !matches!(v.variability, Variability::Fixed | Variability::Tunable) => problems.push(format!("parameter `{name}` has variability {:?}: only fixed and tunable parameters are set", v.variability)),
                Some(v) => match scalar_problem(v) {
                    Some(e) => problems.push(e),
                    None if !value.is_finite() => problems.push(format!("parameter `{name}` = {value} is not finite")),
                    None => settings.push((v.value_reference, v.value_type, *value)),
                },
            }
        }
        if !problems.is_empty() {
            return Err(FmiError::Binding(format!("block `{block}`: {}", problems.join("; "))));
        }
        Ok(Binding { inputs, outputs, parameters: settings })
    }
}

/// Value references and types a block reads and writes, and its settings.
#[derive(Clone, Debug)]
pub struct Binding {
    pub inputs: Vec<(u32, ValueType)>,
    pub outputs: Vec<(u32, ValueType)>,
    pub parameters: Vec<(u32, ValueType, f64)>,
}

fn binary_entry(model_identifier: &str) -> Result<String, String> {
    Ok(format!("binaries/{}/{model_identifier}{}", platform()?, std::env::consts::DLL_SUFFIX))
}

/// Why a variable cannot be a scalar block signal or setting (None: it can).
fn scalar_problem(v: &Variable) -> Option<String> {
    if !v.value_type.is_numeric() {
        return Some(format!("variable `{}` is {:?}: only numeric and Boolean scalars are supported", v.name, v.value_type));
    }
    if v.dimensions > 0 {
        return Some(format!("variable `{}` is an array: arrays are not supported", v.name));
    }
    if v.clocked {
        return Some(format!("variable `{}` is clocked: clocks are not supported", v.name));
    }
    None
}

/// The FMU-level reasons it falls outside FMI 3.0 Co-Simulation as
/// supported here (empty: inside).
fn profile_problems(md: &ModelDescription, entries: &[String]) -> Vec<String> {
    let mut reasons = Vec::new();
    if !md.fmi_version.starts_with("3.") {
        reasons.push(format!("FMI {}: only FMI 3.0 is supported", md.fmi_version));
        return reasons;
    }
    let Some(cs) = &md.co_simulation else {
        let offered: Vec<&str> = [(md.model_exchange, "Model Exchange"), (md.scheduled_execution, "Scheduled Execution")].into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect();
        reasons.push(format!("the FMU offers {} but not Co-Simulation, the interface this importer runs", if offered.is_empty() { "no interface".to_owned() } else { offered.join(" and ") }));
        return reasons;
    };
    if cs.needs_execution_tool {
        reasons.push("needsExecutionTool: the FMU needs its authoring tool at run time".into());
    }
    if let Some(v) = md.variables.iter().find(|v| v.value_type == ValueType::Clock) {
        reasons.push(format!("clocks (`{}`): they need event mode, which is not supported", v.name));
    }
    match binary_entry(&cs.model_identifier) {
        Err(e) => reasons.push(e),
        Ok(entry) if !entries.contains(&entry) => {
            let present: Vec<&str> = entries.iter().filter_map(|e| e.strip_prefix("binaries/")).filter_map(|e| e.split('/').next()).filter(|p| !p.is_empty()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            reasons.push(format!("no binary for this platform ({entry}); the FMU has binaries for: {}", if present.is_empty() { "none".to_owned() } else { present.join(", ") }));
        }
        Ok(_) => {}
    }
    reasons
}
