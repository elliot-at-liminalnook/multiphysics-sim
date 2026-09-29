//! Components written as equations (`*.part`), compiled at run time into
//! ordinary registry components: typed ports, parameters with units and
//! help, notes with derived values, and an exact Jacobian by forward-mode
//! differentiation. No Rust recompile; edits hot-reload.
//!
//! Registry factories are plain `fn` pointers, so each authored type is
//! bound to one of [`SLOTS`] fixed trampolines. Reloading a file swaps the
//! definition behind its slot; running models keep the behaviors they built.
pub mod expr;
pub mod part;
pub mod units;

pub use part::{parse, PartBehavior, PartDef};
use sim_core::{BehaviorDescriptor, BehaviorRegistry, ComponentNotes, DerivedValue, ParameterDeclaration};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// Authored part types one process can hold at once.
pub const SLOTS: usize = 128;

static DEFINITIONS: RwLock<Vec<Option<Arc<PartDef>>>> = RwLock::new(Vec::new());
static SLOT_OF: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());
/// Leaked text per (type, field): bounded by what authors write, reused on reload.
static STRINGS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());

fn leak(key: String, value: &str) -> &'static str {
    let mut map = STRINGS.lock().unwrap();
    match map.get(&key) {
        Some(s) if *s == value => s,
        _ => {
            let s: &'static str = Box::leak(value.to_string().into_boxed_str());
            map.insert(key, s);
            s
        }
    }
}

fn make(slot: usize, p: &BTreeMap<String, f64>) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> {
    let def = DEFINITIONS.read().unwrap().get(slot).cloned().flatten().ok_or_else(|| sim_core::EquationError::InvalidParameter("part".into(), "authored part is not loaded".into()))?;
    let mut params = Vec::new();
    for d in &def.params {
        let v = match (p.get(&d.name), d.default) {
            (Some(v), _) => *v,
            (None, Some(v)) => v,
            (None, None) => return Err(sim_core::EquationError::MissingParameter(d.name.clone())),
        };
        if !v.is_finite() {
            return Err(sim_core::EquationError::InvalidParameter(d.name.clone(), "must be finite".into()));
        }
        params.push(v);
    }
    Ok(Box::new(PartBehavior { def, params }))
}

macro_rules! trampolines {
    ($($i:literal)*) => {
        const TRAMPOLINES: [sim_core::Equations; SLOTS] = [$({
            fn f(p: &BTreeMap<String, f64>) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> { make($i, p) }
            f
        }),*];
    };
}
trampolines!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127);

/// The descriptor for a parsed part, bound to its slot.
fn descriptor(def: &Arc<PartDef>, slot: usize) -> BehaviorDescriptor {
    let id = def.type_id();
    let k = |f: &str| format!("{id}#{f}");
    let ports = def
        .ports
        .iter()
        .map(|p| sim_core::acausal(leak(k(&format!("port.{}", p.name)), &p.name), p.kind.connector()))
        .chain(def.inputs.iter().map(|s| sim_core::signal_in(leak(k(&format!("in.{}", s.name)), &s.name), s.quantity.clone())))
        .chain(def.outputs.iter().map(|s| sim_core::signal_out(leak(k(&format!("out.{}", s.name)), &s.name), s.quantity.clone())))
        .collect();
    let parameters = def
        .params
        .iter()
        .map(|p| match p.default {
            Some(d) => ParameterDeclaration::optional(p.name.clone(), &p.unit, d),
            None => ParameterDeclaration::required(p.name.clone(), &p.unit),
        })
        .collect();
    let help: Vec<(&'static str, &'static str)> = def.params.iter().map(|p| (leak(k(&format!("pname.{}", p.name)), &p.name), leak(k(&format!("phelp.{}", p.name)), &p.help))).collect();
    let typical: Vec<(&'static str, f64)> = def.params.iter().filter_map(|p| p.typical.map(|t| (leak(k(&format!("pname.{}", p.name)), &p.name), t))).collect();
    let equations: Vec<&'static str> = def.text_equations.iter().enumerate().map(|(i, e)| leak(k(&format!("eq.{i}")), e)).collect();
    let pairs: Vec<&'static str> = def.pairs_with.iter().enumerate().map(|(i, e)| leak(k(&format!("pair.{i}")), e)).collect();
    let derive_def = def.clone();
    let derived_with: Option<&'static sim_core::DeriveFn> = (!def.derived.is_empty()).then(|| {
        let f: Box<sim_core::DeriveFn> = Box::new(move |params: &BTreeMap<String, f64>| {
            let values: Vec<f64> = derive_def.params.iter().map(|p| params.get(&p.name).copied().or(p.default).or(p.typical).unwrap_or(f64::NAN)).collect();
            derive_def
                .derived
                .iter()
                .map(|d| {
                    let v = expr::eval(&d.expr, &|v| if let expr::Var::Param(i) = v { values[i] } else { f64::NAN }, None).v;
                    DerivedValue::new(&d.label, v, &d.unit, &d.source)
                })
                .collect()
        });
        &*Box::leak(f)
    });
    let notes: &'static ComponentNotes = Box::leak(Box::new(ComponentNotes {
        icon: leak(k("icon"), &def.icon),
        summary: leak(k("summary"), &def.summary),
        category: leak(k("category"), &def.category),
        explanation: leak(k("explanation"), &def.explanation),
        equations: Box::leak(equations.into_boxed_slice()),
        tradeoffs: leak(k("tradeoffs"), &def.tradeoffs),
        limits: leak(k("limits"), &format!("{}{}Written as equations (part {} in a .part file).", def.limits, if def.limits.is_empty() { "" } else { " " }, def.name)),
        parameters: Box::leak(help.into_boxed_slice()),
        pairs_with: Box::leak(pairs.into_boxed_slice()),
        active: def.active,
        realtime: Box::leak(def.realtime.iter().map(|(n, v)| (leak(k(&format!("pname.{n}")), n), *v)).collect::<Vec<_>>().into_boxed_slice()),
        typical: Box::leak(typical.into_boxed_slice()),
        derived: None,
        derived_with,
    }));
    BehaviorDescriptor::new(&id, leak(k("label"), &def.label), ports, TRAMPOLINES[slot]).with_parameters(parameters).with_notes(notes)
}

/// Register (or replace) one parsed part in `registry`.
pub fn register(registry: &mut BehaviorRegistry, def: PartDef) -> Result<String, String> {
    let def = Arc::new(def);
    let id = def.type_id();
    if registry.contains(&id.as_str().into()) && !SLOT_OF.lock().unwrap().contains_key(&id) {
        return Err(format!("`{id}` is a built-in component; choose another part name"));
    }
    let slot = {
        let mut slots = SLOT_OF.lock().unwrap();
        let next = slots.len();
        *slots.entry(id.clone()).or_insert(next)
    };
    if slot >= SLOTS {
        return Err(format!("more than {SLOTS} authored part types in one process"));
    }
    {
        let mut defs = DEFINITIONS.write().unwrap();
        if defs.len() <= slot {
            defs.resize(slot + 1, None);
        }
        defs[slot] = Some(def.clone());
    }
    registry.replace(descriptor(&def, slot)).map_err(|e| e.to_string())?;
    Ok(id)
}

/// The current definition of an authored type, if loaded.
pub fn definition(type_id: &str) -> Option<Arc<PartDef>> {
    let slot = *SLOT_OF.lock().unwrap().get(type_id)?;
    DEFINITIONS.read().unwrap().get(slot).cloned().flatten()
}

/// Outcome of loading one file.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Loaded {
    pub path: String,
    pub type_id: Option<String>,
    pub error: Option<String>,
    pub source_hash: String,
}

/// A directory of `*.part` files, reloaded when their contents change.
#[derive(Debug, Clone, Default)]
pub struct PartLibrary {
    pub dir: PathBuf,
    hashes: BTreeMap<PathBuf, String>,
    pub last: Vec<Loaded>,
}

impl PartLibrary {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), ..Default::default() }
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(&self.dir).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "part")).collect()).unwrap_or_default();
        out.sort();
        out
    }

    /// Load every file whose contents changed since the last call. Returns
    /// `None` when nothing changed.
    pub fn refresh(&mut self, registry: &mut BehaviorRegistry) -> Option<Vec<Loaded>> {
        let mut changed = Vec::new();
        for path in self.files() {
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
            if self.hashes.get(&path) == Some(&hash) {
                continue;
            }
            self.hashes.insert(path.clone(), hash.clone());
            let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let loaded = match parse(&file, &source).and_then(|def| register(registry, def)) {
                Ok(id) => Loaded { path: path.display().to_string(), type_id: Some(id), error: None, source_hash: hash },
                Err(e) => Loaded { path: path.display().to_string(), type_id: None, error: Some(e), source_hash: hash },
            };
            changed.push(loaded);
        }
        if changed.is_empty() {
            return None;
        }
        for l in &changed {
            self.last.retain(|x| x.path != l.path);
            self.last.push(l.clone());
        }
        Some(changed)
    }
}

/// Load a directory of parts into `registry` once (tests, CLI, headless runs).
pub fn load_dir(registry: &mut BehaviorRegistry, dir: &Path) -> Vec<Loaded> {
    PartLibrary::new(dir).refresh(registry).unwrap_or_default()
}
