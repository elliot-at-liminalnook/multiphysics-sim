//! The `.part` file: a component written as equations.
//!
//! ```text
//! part coreless_motor "Coreless DC motor"
//! summary "A brushed motor without iron in the rotor."
//! port p electrical
//! port shaft rotational
//! param R Ω ~ 2.0 "Winding resistance"        # required, typical 2.0
//! param L H = 0 "Inductance"                   # optional, default 0
//! state i Current = 0
//! let w = shaft.w - case.w
//! eq L*der(i) = p.v - n.v - R*i - k*w
//! flow p = i                                   # through, positive into the part
//! derive "stall torque per volt" = k/R N·m/V
//! ```
//!
//! Port variables: electrical `v`; rotational `phi`, `w`; translational `s`,
//! `v`; thermal `T`. Every line is checked for SI dimensions; an error names
//! the file and line.
use crate::expr::{self, Dual, Expr, Names, Var};
use crate::units::{self, Dim};
use sim_core::{ConnectorKind, QuantityKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct PortDef {
    pub name: String,
    pub kind: PortKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Electrical,
    Rotational,
    Translational,
    Thermal,
}

impl PortKind {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "electrical" => Self::Electrical,
            "rotational" => Self::Rotational,
            "translational" => Self::Translational,
            "thermal" => Self::Thermal,
            _ => return None,
        })
    }
    pub fn connector(self) -> ConnectorKind {
        match self {
            Self::Electrical => ConnectorKind::Electrical,
            Self::Rotational => ConnectorKind::Rotational,
            Self::Translational => ConnectorKind::Translational,
            Self::Thermal => ConnectorKind::Thermal,
        }
    }
    /// (variable, is rate, dimension); and the through dimension.
    fn variables(self) -> (&'static [(&'static str, bool, &'static str)], &'static str) {
        match self {
            Self::Electrical => (&[("v", false, "V")], "A"),
            Self::Rotational => (&[("phi", false, "rad"), ("w", true, "rad/s")], "N·m"),
            Self::Translational => (&[("s", false, "m"), ("v", true, "m/s")], "N"),
            Self::Thermal => (&[("T", false, "K")], "W"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub unit: String,
    pub dim: Dim,
    pub default: Option<f64>,
    pub typical: Option<f64>,
    pub help: String,
}

#[derive(Debug, Clone)]
pub struct StateDef {
    pub name: String,
    pub quantity: QuantityKind,
    pub dim: Dim,
    pub initial: f64,
}

#[derive(Debug, Clone)]
pub struct SignalDef {
    pub name: String,
    pub quantity: QuantityKind,
    pub dim: Dim,
}

#[derive(Debug, Clone)]
pub struct DeriveDef {
    pub label: String,
    pub expr: Expr,
    pub unit: String,
    pub source: String,
}

/// A parsed, dimension-checked part.
#[derive(Debug, Clone)]
pub struct PartDef {
    pub name: String,
    pub label: String,
    pub summary: String,
    pub explanation: String,
    pub tradeoffs: String,
    pub limits: String,
    pub pairs_with: Vec<String>,
    /// Declared a source (`active`): may add energy.
    pub active: bool,
    /// Palette section (`category Actuators`).
    pub category: String,
    pub icon: String,
    /// `realtime NAME = VALUE`: parameter values of the realtime model.
    pub realtime: Vec<(String, f64)>,
    pub ports: Vec<PortDef>,
    pub inputs: Vec<SignalDef>,
    pub outputs: Vec<SignalDef>,
    pub params: Vec<ParamDef>,
    pub states: Vec<StateDef>,
    /// Residual rows (lhs − rhs), one per state.
    pub equations: Vec<Expr>,
    /// Through into the part per acausal port (None = no flow).
    pub flows: Vec<Option<Expr>>,
    pub sets: Vec<Option<Expr>>,
    pub energy: Option<Expr>,
    pub derived: Vec<DeriveDef>,
    /// Equation lines as written, for the notes.
    pub text_equations: Vec<String>,
    /// BLAKE3 of the source text.
    pub source_hash: String,
}

impl PartDef {
    pub fn type_id(&self) -> String {
        format!("part.{}", self.name)
    }
}

struct Scope<'a> {
    part: &'a PartDef,
    lets: &'a BTreeMap<String, Expr>,
}

impl Names for Scope<'_> {
    fn ident(&self, name: &str) -> Result<Expr, String> {
        if name == "t" {
            return Ok(Expr::Var(Var::Time));
        }
        if let Some(e) = self.lets.get(name) {
            return Ok(e.clone());
        }
        if let Some(i) = self.part.params.iter().position(|p| p.name == name) {
            return Ok(Expr::Var(Var::Param(i)));
        }
        if let Some(i) = self.part.states.iter().position(|p| p.name == name) {
            return Ok(Expr::Var(Var::State(i)));
        }
        if let Some(i) = self.part.inputs.iter().position(|p| p.name == name) {
            return Ok(Expr::Var(Var::Signal(i)));
        }
        if self.part.ports.iter().any(|p| p.name == name) {
            return Err(format!("`{name}` is a port: use {name}.{} (or another variable of it)", port_hint(self.part, name)));
        }
        Err(format!("unknown name `{name}` (declare it with param, state, input or let before use)"))
    }
    fn member(&self, port: &str, variable: &str) -> Result<Expr, String> {
        let (i, p) = self.part.ports.iter().enumerate().find(|(_, p)| p.name == port).ok_or_else(|| format!("unknown port `{port}`"))?;
        let (vars, _) = p.kind.variables();
        let (_, rate, _) = vars.iter().find(|(n, _, _)| *n == variable).ok_or_else(|| format!("{port} is {:?}: its variables are {}", p.kind, vars.iter().map(|v| v.0).collect::<Vec<_>>().join(", ")))?;
        Ok(Expr::Var(if *rate { Var::AcrossRate(i) } else { Var::Across(i) }))
    }
    fn derivative(&self, name: &str) -> Result<Expr, String> {
        let i = self.part.states.iter().position(|p| p.name == name).ok_or_else(|| format!("der({name}): `{name}` is not a state"))?;
        Ok(Expr::Var(Var::StateRate(i)))
    }
}

fn port_hint(part: &PartDef, name: &str) -> String {
    part.ports.iter().find(|p| p.name == name).map(|p| p.kind.variables().0[0].0.to_string()).unwrap_or_default()
}

/// Split a line into words, keeping "quoted strings" whole (quotes removed,
/// marked with a leading \u{1} so they are never mistaken for syntax).
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        if quoted {
            if c == '"' {
                out.push(format!("\u{1}{current}"));
                current.clear();
                quoted = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            quoted = true;
        } else if c.is_whitespace() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quoted {
        return Err("unterminated string".into());
    }
    if !current.is_empty() {
        out.push(current);
    }
    Ok(out)
}

fn text(w: &str) -> Option<&str> {
    w.strip_prefix('\u{1}')
}

fn quantity_or_unit(s: &str) -> Result<(Dim, QuantityKind), String> {
    if let Some(q) = units::quantity(s) {
        return Ok(q);
    }
    let dim = units::parse(s)?;
    for name in ["Dimensionless", "Voltage", "Current", "Torque", "AngularVelocity", "Force", "LinearVelocity", "Length", "Temperature", "HeatFlow", "Energy", "Power", "MagneticFlux", "Time", "Pressure", "Mass"] {
        let (d, q) = units::quantity(name).unwrap();
        if d == dim {
            return Ok((d, q));
        }
    }
    Err(format!("no registry quantity has the unit `{s}`; name one (Current, Torque, Temperature…)"))
}

/// Parse and check a part. Errors read `file:line: message`.
pub fn parse(file: &str, source: &str) -> Result<PartDef, String> {
    let mut part = PartDef {
        name: String::new(),
        label: String::new(),
        summary: String::new(),
        explanation: String::new(),
        tradeoffs: String::new(),
        limits: String::new(),
        pairs_with: Vec::new(),
        active: false,
        category: String::new(),
        icon: String::new(),
        realtime: Vec::new(),
        ports: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        params: Vec::new(),
        states: Vec::new(),
        equations: Vec::new(),
        flows: Vec::new(),
        sets: Vec::new(),
        energy: None,
        derived: Vec::new(),
        text_equations: Vec::new(),
        source_hash: blake3::hash(source.as_bytes()).to_hex().to_string(),
    };
    let mut lets: BTreeMap<String, Expr> = BTreeMap::new();
    let mut flows: BTreeMap<String, (usize, Expr)> = BTreeMap::new();
    let mut sets: BTreeMap<String, (usize, Expr)> = BTreeMap::new();
    let mut eq_lines = Vec::new();
    for (n, raw) in source.lines().enumerate() {
        let line_no = n + 1;
        let at = |m: String| format!("{file}:{line_no}: {m}");
        let line = raw.split_once('#').map(|(a, _)| a).unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }
        let (keyword, rest) = line.split_once(char::is_whitespace).map(|(k, r)| (k, r.trim())).unwrap_or((line, ""));
        let scope_expr = |part: &PartDef, lets: &BTreeMap<String, Expr>, src: &str| expr::parse(src, &Scope { part, lets }).map_err(|e| at(e));
        match keyword {
            "part" => {
                let w = words(rest).map_err(at)?;
                part.name = w.first().filter(|n| valid(n)).cloned().ok_or_else(|| at("part needs a name (letters, digits, _)".into()))?;
                part.label = w.get(1).and_then(|l| text(l)).map(str::to_string).unwrap_or_else(|| part.name.clone());
            }
            "summary" | "explain" | "tradeoffs" | "limits" => {
                let w = words(rest).map_err(at)?;
                let t = w.iter().filter_map(|x| text(x)).collect::<Vec<_>>().join(" ");
                let target = match keyword {
                    "summary" => &mut part.summary,
                    "explain" => &mut part.explanation,
                    "tradeoffs" => &mut part.tradeoffs,
                    _ => &mut part.limits,
                };
                if !target.is_empty() {
                    target.push(' ');
                }
                target.push_str(&t);
            }
            "active" => part.active = true,
            "icon" => {
                if !sim_core::icons::NAMES.contains(&rest.trim()) { return Err(at("unknown icon name".into())); }
                part.icon=rest.trim().to_string();
            }
            "category" => part.category = rest.trim().to_string(),
            "realtime" => {
                let (name, v) = rest.split_once('=').ok_or_else(|| at("realtime PARAMETER = VALUE".into()))?;
                let name = name.trim();
                if !part.params.iter().any(|p| p.name == name) {
                    return Err(at(format!("realtime: `{name}` is not a declared parameter (declare it first)")));
                }
                let v: f64 = v.trim().parse().map_err(|_| at("realtime value must be a number".into()))?;
                part.realtime.push((name.to_string(), v));
            }
            "pairs" => part.pairs_with.extend(rest.split_whitespace().map(str::to_string)),
            "port" => {
                let w: Vec<&str> = rest.split_whitespace().collect();
                let [name, kind] = w[..] else { return Err(at("port NAME KIND (electrical, rotational, translational, thermal)".into())) };
                let kind = PortKind::parse(kind).ok_or_else(|| at(format!("unknown port kind `{kind}` (electrical, rotational, translational, thermal)")))?;
                if !valid(name) || taken(&part, &lets, name) {
                    return Err(at(format!("`{name}` is not a free name")));
                }
                part.ports.push(PortDef { name: name.into(), kind });
            }
            "input" | "output" => {
                let w: Vec<&str> = rest.split_whitespace().collect();
                let [name, q] = w[..] else { return Err(at(format!("{keyword} NAME QUANTITY"))) };
                let (dim, quantity) = quantity_or_unit(q).map_err(at)?;
                if !valid(name) || taken(&part, &lets, name) {
                    return Err(at(format!("`{name}` is not a free name")));
                }
                let s = SignalDef { name: name.into(), quantity, dim };
                if keyword == "input" { part.inputs.push(s) } else { part.outputs.push(s) }
            }
            "param" => {
                // param NAME UNIT [= default | ~ typical] ["help"]
                let w = words(rest).map_err(at)?;
                let name = w.first().cloned().ok_or_else(|| at("param NAME UNIT [= default | ~ typical] \"help\"".into()))?;
                let unit = w.get(1).cloned().ok_or_else(|| at(format!("param {name} needs a unit (use 1 for dimensionless)")))?;
                let dim = units::parse(&unit).map_err(|e| at(format!("param {name}: {e}")))?;
                if !valid(&name) || taken(&part, &lets, &name) {
                    return Err(at(format!("`{name}` is not a free name")));
                }
                let mut p = ParamDef { name, unit, dim, default: None, typical: None, help: String::new() };
                let mut i = 2;
                while i < w.len() {
                    match w[i].as_str() {
                        "=" | "~" => {
                            let v: f64 = w.get(i + 1).and_then(|v| v.parse().ok()).ok_or_else(|| at(format!("`{}` needs a number", w[i])))?;
                            if w[i] == "=" { p.default = Some(v) } else { p.typical = Some(v) }
                            i += 2;
                        }
                        s if text(s).is_some() => {
                            p.help = text(s).unwrap().to_string();
                            i += 1;
                        }
                        s => return Err(at(format!("unexpected `{s}` (units are one word, e.g. N·m/A)"))),
                    }
                }
                part.params.push(p);
            }
            "state" => {
                let (decl, initial) = rest.split_once('=').map(|(a, b)| (a.trim(), Some(b.trim()))).unwrap_or((rest, None));
                let w: Vec<&str> = decl.split_whitespace().collect();
                let [name, q] = w[..] else { return Err(at("state NAME QUANTITY [= initial]".into())) };
                let (dim, quantity) = quantity_or_unit(q).map_err(at)?;
                if !valid(name) || taken(&part, &lets, name) {
                    return Err(at(format!("`{name}` is not a free name")));
                }
                let initial = initial.map(|v| v.parse::<f64>().map_err(|_| at(format!("initial value `{v}` is not a number")))).transpose()?.unwrap_or(0.);
                part.states.push(StateDef { name: name.into(), quantity, dim, initial });
            }
            "let" => {
                let (name, e) = rest.split_once('=').ok_or_else(|| at("let NAME = EXPRESSION".into()))?;
                let name = name.trim();
                if !valid(name) || taken(&part, &lets, name) {
                    return Err(at(format!("`{name}` is not a free name")));
                }
                let e = scope_expr(&part, &lets, e)?;
                dim_of(&part, &e).map_err(at)?;
                part.text_equations.push(format!("{name} = {}", pretty(&part, rest.split_once('=').map_or("", |x| x.1))));
                lets.insert(name.into(), e);
            }
            "eq" => {
                let (l, r) = rest.split_once('=').ok_or_else(|| at("eq LEFT = RIGHT".into()))?;
                let (l, r) = (scope_expr(&part, &lets, l)?, scope_expr(&part, &lets, r)?);
                let (dl, dr) = (dim_of(&part, &l).map_err(|e| at(format!("left side: {e}")))?, dim_of(&part, &r).map_err(|e| at(format!("right side: {e}")))?);
                if let (Some(a), Some(b)) = (dl, dr) {
                    if a != b {
                        return Err(at(format!("the two sides differ in dimension: {a} vs {b}")));
                    }
                }
                eq_lines.push(line_no);
                part.equations.push(Expr::Sub(Box::new(l), Box::new(r)));
                part.text_equations.push(pretty(&part, rest));
            }
            "flow" | "set" => {
                let (name, e) = rest.split_once('=').ok_or_else(|| at(format!("{keyword} NAME = EXPRESSION")))?;
                let name = name.trim().to_string();
                let e = scope_expr(&part, &lets, e)?;
                let d = dim_of(&part, &e).map_err(at)?;
                let expected = if keyword == "flow" {
                    let p = part.ports.iter().find(|p| p.name == name).ok_or_else(|| at(format!("no port `{name}`")))?;
                    units::parse(p.kind.variables().1).unwrap()
                } else {
                    part.outputs.iter().find(|p| p.name == name).ok_or_else(|| at(format!("no output `{name}`")))?.dim
                };
                if let Some(d) = d {
                    if d != expected {
                        return Err(at(format!("{keyword} {name} must be {expected}, got {d}")));
                    }
                }
                let map = if keyword == "flow" { &mut flows } else { &mut sets };
                if map.insert(name.clone(), (line_no, e)).is_some() {
                    return Err(at(format!("{name} already has a {keyword}")));
                }
                part.text_equations.push(if keyword == "flow" {
                    let symbol = match part.ports.iter().find(|p| p.name == name).map(|p| p.kind) {
                        Some(PortKind::Electrical) => "i",
                        Some(PortKind::Rotational) => "τ",
                        Some(PortKind::Translational) => "F",
                        _ => "Q̇",
                    };
                    format!("{symbol}_{name} = {}", pretty(&part, rest.split_once('=').map_or("", |x| x.1)))
                } else {
                    pretty(&part, rest)
                });
            }
            "energy" => {
                let e = scope_expr(&part, &lets, rest)?;
                if let Some(d) = dim_of(&part, &e).map_err(at)? {
                    if d != units::parse("J").unwrap() {
                        return Err(at(format!("energy must be in J, got {d}")));
                    }
                }
                part.energy = Some(e);
            }
            "derive" => {
                let w = words(rest).map_err(at)?;
                let label = w.first().and_then(|l| text(l)).ok_or_else(|| at("derive \"label\" = EXPRESSION UNIT".into()))?.to_string();
                let after = rest.splitn(2, '=').nth(1).ok_or_else(|| at("derive \"label\" = EXPRESSION UNIT".into()))?.trim();
                // The last word is the display unit when it parses as one.
                let (src, unit) = match after.rsplit_once(char::is_whitespace) {
                    Some((e, u)) if units::parse(u).is_ok() && !u.chars().all(|c| c.is_ascii_digit()) => (e, u.to_string()),
                    _ => (after, "1".to_string()),
                };
                let e = scope_expr(&part, &lets, src)?;
                let mut used = BTreeSet::new();
                expr::vars(&e, &mut used);
                if used.iter().any(|v| !matches!(v, Var::Param(_))) {
                    return Err(at("derived values may only use parameters".into()));
                }
                if let Some(d) = dim_of(&part, &e).map_err(at)? {
                    let u = units::parse(&unit).map_err(at)?;
                    if d != u {
                        return Err(at(format!("`{label}` is {d}, not {unit}")));
                    }
                }
                part.derived.push(DeriveDef { label, expr: e, unit, source: src.trim().to_string() });
            }
            other => return Err(at(format!("unknown keyword `{other}` (part summary explain tradeoffs limits pairs active category realtime port input output param state let eq flow set energy derive)"))),
        }
    }
    if part.name.is_empty() {
        return Err(format!("{file}: missing `part NAME \"Label\"` line"));
    }
    if part.ports.is_empty() && part.outputs.is_empty() {
        return Err(format!("{file}: a part needs at least one port or output"));
    }
    if part.equations.len() != part.states.len() {
        return Err(format!("{file}: {} state(s) but {} equation(s); each state needs exactly one `eq`", part.states.len(), part.equations.len()));
    }
    for (name, (line, _)) in &flows {
        if !part.ports.iter().any(|p| &p.name == name) {
            return Err(format!("{file}:{line}: no port `{name}`"));
        }
    }
    part.flows = part.ports.iter().map(|p| flows.remove(&p.name).map(|(_, e)| e)).collect();
    part.sets = part.outputs.iter().map(|o| sets.remove(&o.name).map(|(_, e)| e)).collect();
    if let Some((i, o)) = part.outputs.iter().enumerate().find(|(i, _)| part.sets[*i].is_none()) {
        return Err(format!("{file}: output `{}` (#{i}) is never set", o.name));
    }
    Ok(part)
}

/// An equation as written in the file, shown the way it reads on paper:
/// port variables as symbols (`shaft.w` → ω_shaft), `der(x)` → dx/dt,
/// `*` → ·, small powers as superscripts.
fn pretty(part: &PartDef, src: &str) -> String {
    let symbol = |port: &str, var: &str| -> Option<&'static str> {
        let kind = part.ports.iter().find(|p| p.name == port)?.kind;
        Some(match (kind, var) {
            (PortKind::Rotational, "phi") => "φ",
            (PortKind::Rotational, "w") => "ω",
            (PortKind::Translational, "s") => "x",
            (PortKind::Translational, "v") | (PortKind::Electrical, "v") => "v",
            (PortKind::Thermal, "T") => "T",
            _ => return None,
        })
    };
    let chars: Vec<char> = src.trim().chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    let ident = |i: usize| -> usize {
        let mut j = i;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '.') {
            j += 1;
        }
        j
    };
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphabetic() || c == '_' {
            let j = ident(i);
            let word: String = chars[i..j].iter().collect();
            let word = match word.split_once('.') {
                Some((p, v)) => symbol(p, v).map(|s| format!("{s}_{p}")).unwrap_or(word),
                None => word,
            };
            // der(x) → dx/dt; der(...) → d(...)/dt
            if word == "der" && chars.get(j) == Some(&'(') {
                let (mut depth, mut k) = (0, j);
                while k < chars.len() {
                    match chars[k] {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                let inner = pretty(part, &chars[j + 1..k.min(chars.len())].iter().collect::<String>());
                let simple = inner.chars().all(|c| c.is_alphanumeric() || c == '_');
                out.push_str(&if simple { format!("d{inner}/dt") } else { format!("d({inner})/dt") });
                i = k + 1;
                continue;
            }
            out.push_str(match word.as_str() {
                "sqrt" => "√",
                "pi" => "π",
                _ => &word,
            });
            i = j;
            continue;
        }
        match c {
            '*' => out.push('·'),
            '-' => out.push('−'),
            '^' if matches!(chars.get(i + 1), Some('2' | '3')) && !chars.get(i + 2).is_some_and(|d| d.is_ascii_digit() || *d == '.') => {
                out.push(if chars[i + 1] == '2' { '²' } else { '³' });
                i += 2;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn valid(name: &str) -> bool {
    !name.is_empty() && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_') && name != "t" && name != "der"
}

fn taken(part: &PartDef, lets: &BTreeMap<String, Expr>, name: &str) -> bool {
    lets.contains_key(name)
        || part.params.iter().any(|p| p.name == name)
        || part.states.iter().any(|p| p.name == name)
        || part.ports.iter().any(|p| p.name == name)
        || part.inputs.iter().any(|p| p.name == name)
        || part.outputs.iter().any(|p| p.name == name)
}

fn var_dim(part: &PartDef, v: Var) -> Dim {
    match v {
        Var::Param(i) => part.params[i].dim,
        Var::State(i) => part.states[i].dim,
        Var::StateRate(i) => part.states[i].dim.div(units::S),
        Var::Across(i) => units::parse(part.ports[i].kind.variables().0[0].2).unwrap(),
        Var::AcrossRate(i) => units::parse(part.ports[i].kind.variables().0[1].2).unwrap(),
        Var::Signal(i) => part.inputs[i].dim,
        Var::Time => units::S,
    }
}

fn dim_of(part: &PartDef, e: &Expr) -> Result<Option<Dim>, String> {
    expr::dimension(e, &|v| var_dim(part, v))
}

/// A part instance: its definition and parameter values.
pub struct PartBehavior {
    pub def: std::sync::Arc<PartDef>,
    pub params: Vec<f64>,
}

impl PartBehavior {
    fn input(v: Var) -> Option<sim_core::Input> {
        use sim_core::Input as I;
        Some(match v {
            Var::State(i) => I::State(i),
            Var::StateRate(i) => I::StateRate(i),
            Var::Across(p) => I::Across(p, 0),
            Var::AcrossRate(p) => I::AcrossRate(p, 0),
            Var::Signal(i) => I::Signal(i),
            Var::Param(_) | Var::Time => return None,
        })
    }
}

impl sim_core::Behavior for PartBehavior {
    fn states(&self) -> Vec<sim_core::StateDeclaration> {
        self.def.states.iter().map(|s| sim_core::StateDeclaration::new(s.name.clone(), s.quantity.clone(), s.initial)).collect()
    }
    fn residual(&self, ctx: &mut sim_core::Context) {
        let values = {
            let c = &*ctx;
            let read = |v: Var| match v {
                Var::Param(i) => self.params[i],
                Var::State(i) => c.state(i),
                Var::StateRate(i) => c.state_rate(i),
                Var::Across(p) => c.across(p),
                Var::AcrossRate(p) => c.across_rate(p),
                Var::Signal(i) => c.signal_in(i),
                Var::Time => c.time,
            };
            let rows: Vec<f64> = self.def.equations.iter().map(|e| expr::eval(e, &read, None).v).collect();
            let flows: Vec<Option<f64>> = self.def.flows.iter().map(|f| f.as_ref().map(|e| expr::eval(e, &read, None).v)).collect();
            let sets: Vec<f64> = self.def.sets.iter().map(|f| f.as_ref().map(|e| expr::eval(e, &read, None).v).unwrap_or(0.)).collect();
            (rows, flows, sets)
        };
        for (k, r) in values.0.into_iter().enumerate() {
            ctx.set_state_residual(k, r);
        }
        for (p, f) in values.1.into_iter().enumerate() {
            if let Some(f) = f {
                ctx.add_through(p, f);
            }
        }
        for (i, s) in values.2.into_iter().enumerate() {
            ctx.set_signal(i, s);
        }
    }
    fn energy(&self, view: &sim_core::View) -> f64 {
        let Some(e) = &self.def.energy else { return 0. };
        let read = |v: Var| match v {
            Var::Param(i) => self.params[i],
            Var::State(i) => view.state(i),
            Var::Across(p) => view.across(p),
            Var::AcrossRate(p) => view.across_rate(p),
            Var::Signal(i) => view.signal_in(i),
            Var::Time => view.time,
            Var::StateRate(_) => 0.,
        };
        expr::eval(e, &read, None).v
    }
    fn jacobian_at(&self, view: &sim_core::View, state_rates: &[f64], out: &mut sim_core::LocalJacobian) -> bool {
        use sim_core::Output;
        if std::env::var_os("SIM_PARTS_NUMERIC_JACOBIAN").is_some() {
            return false;
        }
        let read = |v: Var| match v {
            Var::Param(i) => self.params[i],
            Var::State(i) => view.state(i),
            Var::StateRate(i) => state_rates.get(i).copied().unwrap_or(0.),
            Var::Across(p) => view.across(p),
            Var::AcrossRate(p) => view.across_rate(p),
            Var::Signal(i) => view.signal_in(i),
            Var::Time => view.time,
        };
        let emit = |e: &Expr, output: Output, out: &mut sim_core::LocalJacobian| {
            let mut used = BTreeSet::new();
            expr::vars(e, &mut used);
            for v in used {
                if let Some(input) = Self::input(v) {
                    let Dual { d, .. } = expr::eval(e, &read, Some(v));
                    if d != 0. && d.is_finite() {
                        out.set(output, input, d);
                    }
                }
            }
        };
        for (k, e) in self.def.equations.iter().enumerate() {
            emit(e, Output::State(k), out);
        }
        for (p, f) in self.def.flows.iter().enumerate() {
            if let Some(e) = f {
                emit(e, Output::Through(p, 0), out);
            }
        }
        for (i, f) in self.def.sets.iter().enumerate() {
            if let Some(e) = f {
                emit(e, Output::Signal(i), out);
            }
        }
        true
    }
    fn jacobian(&self, view: &sim_core::View, out: &mut sim_core::LocalJacobian) -> bool {
        // Without the actual rates, a rate-dependent part must be differenced.
        let mut used = BTreeSet::new();
        self.def.equations.iter().chain(self.def.flows.iter().flatten()).chain(self.def.sets.iter().flatten()).for_each(|e| expr::vars(e, &mut used));
        if used.iter().any(|v| matches!(v, Var::StateRate(_))) {
            return false;
        }
        self.jacobian_at(view, &vec![0.; self.def.states.len()], out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn equations_read_like_math() {
        let src = "part m \"M\"\nport p electrical\nport n electrical\nport shaft rotational\nport case rotational\nparam r Ω = 2\nparam k N·m/A = 0.01\nparam l H = 0.001\nstate i Current = 0\nlet w = shaft.w - case.w\neq l*der(i) = p.v - n.v - r*i - k*w\nflow p = i\nflow n = -i\nflow shaft = -k*i\nflow case = k*i\nenergy 0.5*l*i^2\n";
        let def = super::parse("m.part", src).unwrap();
        assert_eq!(def.text_equations, ["w = ω_shaft − ω_case", "l·di/dt = v_p − v_n − r·i − k·w", "i_p = i", "i_n = −i", "τ_shaft = −k·i", "τ_case = k·i"]);
    }
}
