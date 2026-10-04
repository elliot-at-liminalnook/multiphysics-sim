//! Bounded reconstruction of the archive's component definition catalogue.
//! This is archive interpretation, not a modelling UI or editable feature tree.
use crate::{
    archive::ArchiveDocument,
    geometry::{transform, vector},
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

// Match components.py's Python truthiness for JSON declarations at the
// regeneration gate only. Physical metadata is preserved unchanged and still
// validated by mass.rs: an empty material map is valid, but an empty mass
// object (or a non-object material value) is not a valid mass declaration.
fn declaration_is_substantive(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Recipe {
    pub content: String,
    pub primitive: i32,
    pub args: [f64; 8],
    pub matrices: Vec<[f64; 12]>,
}
#[derive(Clone, Copy)]
struct Quantity {
    v: f64,
    d: [i32; 4],
}
fn unit(s: &str) -> Result<Quantity, String> {
    let (v, d) = match s {
        "1" => (1., [0; 4]),
        "mm" => (1., [1, 0, 0, 0]),
        "cm" => (10., [1, 0, 0, 0]),
        "m" => (1000., [1, 0, 0, 0]),
        "deg" => (1., [0, 1, 0, 0]),
        "rad" => (180. / std::f64::consts::PI, [0, 1, 0, 0]),
        "kg" => (1., [0, 0, 1, 0]),
        "g" => (0.001, [0, 0, 1, 0]),
        "s" => (1., [0, 0, 0, 1]),
        _ => return Err(format!("unsupported component unit {s}")),
    };
    Ok(Quantity { v, d })
}
struct Parser<'a> {
    s: &'a [u8],
    p: usize,
    n: usize,
    typed: bool,
    vars: &'a HashMap<String, Quantity>,
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self.p < self.s.len() && self.s[self.p].is_ascii_whitespace() {
            self.p += 1;
        }
    }
    fn expr(&mut self, min: u8) -> Result<Quantity, String> {
        self.n += 1;
        if self.n > 100 {
            return Err("component expression exceeds 100 nodes".into());
        }
        self.ws();
        let ch = *self.s.get(self.p).ok_or("incomplete expression")?;
        let mut a = if ch == b'+' || ch == b'-' {
            self.p += 1;
            let mut v = self.expr(3)?;
            if ch == b'-' {
                v.v = -v.v;
            }
            v
        } else if ch == b'(' {
            self.p += 1;
            let v = self.expr(0)?;
            self.ws();
            if self.s.get(self.p) != Some(&b')') {
                return Err("missing closing parenthesis".into());
            }
            self.p += 1;
            v
        } else if ch.is_ascii_digit() || ch == b'.' {
            let start = self.p;
            self.p += 1;
            while self.p < self.s.len() {
                let c = self.s[self.p];
                if c.is_ascii_digit()
                    || c == b'.'
                    || c == b'e'
                    || c == b'E'
                    || ((c == b'+' || c == b'-') && matches!(self.s[self.p - 1], b'e' | b'E'))
                {
                    self.p += 1;
                } else {
                    break;
                }
            }
            let v = std::str::from_utf8(&self.s[start..self.p])
                .map_err(|_| "invalid expression")?
                .parse::<f64>()
                .map_err(|_| "invalid numeric literal")?;
            Quantity { v, d: [0; 4] }
        } else if ch.is_ascii_alphabetic() || ch == b'_' {
            let start = self.p;
            self.p += 1;
            while self.p < self.s.len()
                && (self.s[self.p].is_ascii_alphanumeric() || self.s[self.p] == b'_')
            {
                self.p += 1;
            }
            let name = std::str::from_utf8(&self.s[start..self.p])
                .map_err(|_| "invalid parameter name")?;
            if let Some(v) = self.vars.get(name) {
                self.typed = true;
                *v
            } else if name == "pi" {
                Quantity {
                    v: std::f64::consts::PI,
                    d: [0; 4],
                }
            } else if name == "tau" {
                Quantity {
                    v: std::f64::consts::TAU,
                    d: [0; 4],
                }
            } else {
                self.typed = true;
                unit(name)?
            }
        } else {
            return Err("only arithmetic component expressions are supported".into());
        };
        loop {
            self.ws();
            let Some(&op) = self.s.get(self.p) else {
                break;
            };
            let pow = op == b'*' && self.s.get(self.p + 1) == Some(&b'*');
            let implicit = op.is_ascii_alphabetic();
            let prec = if pow {
                3
            } else if matches!(op, b'*' | b'/') || implicit {
                2
            } else if matches!(op, b'+' | b'-') {
                1
            } else {
                break;
            };
            if prec < min {
                break;
            }
            if !implicit {
                self.p += if pow { 2 } else { 1 };
            }
            let b = self.expr(if pow { prec } else { prec + 1 })?;
            if pow {
                if b.d != [0; 4] || b.v.fract() != 0. || b.v.abs() > 4. {
                    return Err("exponent must be integer -4..4".into());
                }
                a = Quantity {
                    v: a.v.powi(b.v as i32),
                    d: a.d.map(|x| x * b.v as i32),
                };
            } else if matches!(op, b'+' | b'-') && !implicit {
                let mut b = b;
                if a.d != b.d {
                    if a.v == 0. && a.d == [0; 4] {
                        a.d = b.d;
                    } else if b.v == 0. && b.d == [0; 4] {
                        b.d = a.d;
                    } else {
                        return Err("cannot add different component dimensions".into());
                    }
                }
                a.v += if op == b'+' { b.v } else { -b.v };
            } else {
                if op == b'/' && b.v == 0. {
                    return Err("component division by zero".into());
                }
                let sign = if op == b'/' { -1 } else { 1 };
                a = Quantity {
                    v: if sign == 1 { a.v * b.v } else { a.v / b.v },
                    d: std::array::from_fn(|i| a.d[i] + sign * b.d[i]),
                };
            }
        }
        if !a.v.is_finite() {
            return Err("component expression must be finite".into());
        }
        Ok(a)
    }
}
fn expression(v: &Value, u: &str, vars: &HashMap<String, Quantity>) -> Result<f64, String> {
    if let Some(n) = v.as_f64() {
        return if n.is_finite() {
            Ok(n)
        } else {
            Err("component value must be finite".into())
        };
    }
    let text = v
        .as_str()
        .filter(|s| s.len() <= 1024)
        .ok_or("expected component number or expression")?;
    let mut p = Parser {
        s: text.as_bytes(),
        p: 0,
        n: 0,
        typed: false,
        vars,
    };
    let q = p.expr(0)?;
    p.ws();
    if p.p != p.s.len() {
        return Err("unsupported expression suffix".into());
    }
    let target = unit(u)?;
    if !p.typed && q.d == [0; 4] {
        return Ok(q.v);
    }
    if q.d != target.d {
        if q.v == 0. && q.d == [0; 4] {
            return Ok(0.);
        }
        return Err(format!("expression does not have units {u}"));
    }
    Ok(q.v / target.v)
}
fn parameters(d: &Value, overrides: &Value) -> Result<HashMap<String, Quantity>, String> {
    let empty = serde_json::Map::new();
    let specs = d["parameters"].as_object().unwrap_or(&empty);
    let overrides = overrides.as_object().unwrap_or(&empty);
    for k in overrides.keys() {
        if !specs.contains_key(k) {
            return Err(format!("unknown component override {k}"));
        }
    }
    let mut vars = HashMap::new();
    for (name, s) in specs {
        let u = s["unit"].as_str().ok_or("parameter unit missing")?;
        let scale = unit(u)?;
        let blank = HashMap::new();
        let default = expression(&s["value"], u, &blank)?;
        let v = expression(overrides.get(name).unwrap_or(&s["value"]), u, &blank)?;
        let lo = if s["min"].is_null() {
            f64::NEG_INFINITY
        } else {
            expression(&s["min"], u, &blank)?
        };
        let hi = if s["max"].is_null() {
            f64::INFINITY
        } else {
            expression(&s["max"], u, &blank)?
        };
        if !(lo <= default && default <= hi && lo <= v && v <= hi) {
            return Err(format!("parameter {name} outside [{lo},{hi}] {u}"));
        }
        if !matches!(
            s["provenance"].as_str(),
            Some("measured" | "derived" | "estimated")
        ) {
            return Err(format!("parameter {name} requires provenance"));
        }
        vars.insert(
            name.clone(),
            Quantity {
                v: v * scale.v,
                d: scale.d,
            },
        );
    }
    Ok(vars)
}
fn arguments(f: &Value, vars: &HashMap<String, Quantity>) -> Result<Value, String> {
    let kind = f["kind"].as_str().ok_or("feature kind missing")?;
    let fields: &[(&str, &str, usize)] = match kind {
        "box" => &[("corner", "mm", 3), ("size", "mm", 3)],
        "cylinder" => &[
            ("base", "mm", 3),
            ("axis", "1", 3),
            ("radius", "mm", 1),
            ("height", "mm", 1),
        ],
        "placement" | "assembly_placement" => &[
            ("translation", "mm", 3),
            ("axis", "1", 3),
            ("angle_deg", "deg", 1),
        ],
        "joint_ratio" => &[("ratio", "1", 1)],
        "joint_home" => &[("angle_deg", "deg", 1)],
        "joint_frame" => &[("pivot", "mm", 3), ("axis", "1", 3)],
        _ => return Err(format!("component feature {kind} awaits Rust migration")),
    };
    let object = f["arguments"]
        .as_object()
        .ok_or("feature arguments must be object")?;
    if object.len() != fields.len() {
        return Err(format!("{kind}: wrong argument fields"));
    }
    let mut out = serde_json::Map::new();
    for (name, u, count) in fields {
        let v = object
            .get(*name)
            .ok_or_else(|| format!("{kind}: missing {name}"))?;
        let value = if *count == 1 {
            json!(expression(v, u, vars)?)
        } else {
            let a = v
                .as_array()
                .filter(|a| a.len() == *count)
                .ok_or("feature vector length")?;
            json!(
                a.iter()
                    .map(|v| expression(v, u, vars))
                    .collect::<Result<Vec<_>, _>>()?
            )
        };
        out.insert((*name).into(), value);
    }
    Ok(Value::Object(out))
}
#[derive(Clone)]
struct Part {
    node: Value,
    recipe: Option<Recipe>,
}
fn placement(v: &Value) -> Result<[f64; 12], String> {
    if let Some(a) = v["_resolved_matrix"].as_array() {
        if a.len() != 12 {
            return Err("resolved component matrix must have 12 coefficients".into());
        }
        let mut m = [0.; 12];
        for i in 0..12 {
            m[i] = a[i]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or("invalid resolved matrix")?;
        }
        Ok(m)
    } else {
        transform(v, true)
    }
}
fn apply(part: &mut Part, m: [f64; 12]) -> Result<(), String> {
    if let Some(r) = &mut part.recipe {
        r.matrices.push(m);
    }
    physical_transform(&mut part.node["robot"], m)?;
    if !part.node["component_instance"].is_null() {
        let spec = &mut part.node["component_instance"];
        let n = placement(&spec["placement"])?;
        let mut composed = [0.; 12];
        for i in 0..3 {
            for j in 0..3 {
                composed[i * 4 + j] = (0..3).map(|k| m[i * 4 + k] * n[k * 4 + j]).sum();
            }
            composed[i * 4 + 3] =
                m[i * 4 + 3] + (0..3).map(|k| m[i * 4 + k] * n[k * 4 + 3]).sum::<f64>();
        }
        spec["placement"]["_resolved_matrix"] = json!(composed);
    }
    Ok(())
}

fn physical_transform(v: &mut Value, m: [f64; 12]) -> Result<(), String> {
    let Some(o) = v.as_object_mut() else {
        return Ok(());
    };
    for (k, v) in o {
        if matches!(
            k.as_str(),
            "mount_point"
                | "shaft_tip"
                | "point"
                | "point_mm"
                | "com_mm"
                | "pivot"
                | "origin_mm"
                | "axis_point"
                | "shaft_axis"
                | "axis"
                | "axis_dir"
                | "normal"
        ) && v.is_array()
        {
            let p = vector(v, [0.; 3])?;
            let point = matches!(
                k.as_str(),
                "mount_point"
                    | "shaft_tip"
                    | "point"
                    | "point_mm"
                    | "com_mm"
                    | "pivot"
                    | "origin_mm"
                    | "axis_point"
            );
            *v = json!(std::array::from_fn::<_, 3, _>(|i| (0..3)
                .map(|j| m[i * 4 + j] * p[j])
                .sum::<f64>()
                + if point { m[i * 4 + 3] } else { 0. }));
        } else if k == "inertia_kg_m2" {
            let rows = v
                .as_array()
                .filter(|v| v.len() == 3)
                .ok_or("declared inertia must be 3x3")?;
            let mut tensor = [[0.; 3]; 3];
            for i in 0..3 {
                tensor[i] = vector(&rows[i], [0.; 3])?;
            }
            let mut out = [[0.; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    for a in 0..3 {
                        for b in 0..3 {
                            out[i][j] += m[i * 4 + a] * tensor[a][b] * m[j * 4 + b];
                        }
                    }
                }
            }
            *v = json!(out);
        } else if v.is_object() {
            physical_transform(v, m)?;
        } else if let Some(a) = v.as_array_mut() {
            for v in a {
                if v.is_object() {
                    physical_transform(v, m)?;
                }
            }
        }
    }
    Ok(())
}
fn bind_ports(
    doc: &ArchiveDocument,
    target: &str,
    spec: &Value,
    map: &mut serde_json::Map<String, Value>,
    local: &HashMap<String, Part>,
) -> Result<(), String> {
    let mut d = &doc.manifest["component_definitions"][target];
    let mut seen = HashSet::new();
    while d["variants"].as_object().is_some_and(|v| !v.is_empty()) {
        let id = d["id"].as_str().ok_or("family id missing")?;
        if !seen.insert(id) {
            return Err("family cycle".into());
        }
        let choice = spec["variant"]
            .as_str()
            .or_else(|| d["default_variant"].as_str())
            .ok_or("family choice missing")?;
        let next = d["variants"][choice]["definition_id"]
            .as_str()
            .ok_or("family target missing")?;
        d = &doc.manifest["component_definitions"][next];
    }
    let empty = serde_json::Map::new();
    let ports = d["ports"].as_object().unwrap_or(&empty);
    let bindings = spec["bindings"].as_object().unwrap_or(&empty);
    if ports.len() != bindings.len() || ports.keys().any(|k| !bindings.contains_key(k)) {
        return Err("component bindings differ from declared ports".into());
    }
    for (k, port) in ports {
        let dest = bindings[k].as_str().ok_or("port binding must be node id")?;
        let n = local
            .get(dest)
            .map(|p| &p.node)
            .or_else(|| doc.node(dest))
            .ok_or_else(|| format!("port {k}: missing binding {dest}"))?;
        if n["kind"] != port["kind"] {
            return Err(format!(
                "port {k}: binding {dest} requires {}",
                port["kind"]
            ));
        }
        let source = port["source_id"].as_str().ok_or("port source_id missing")?;
        map.insert(source.into(), json!(dest));
    }
    Ok(())
}
fn remap(v: &mut Value, map: &serde_json::Map<String, Value>) {
    match v {
        Value::String(s) => {
            if let Some(n) = map.get(s).and_then(Value::as_str) {
                *s = n.into();
            }
        }
        Value::Array(a) => {
            for v in a {
                remap(v, map)
            }
        }
        Value::Object(o) => {
            let old = std::mem::take(o);
            for (k, mut v) in old {
                remap(&mut v, map);
                o.insert(map.get(&k).and_then(Value::as_str).unwrap_or(&k).into(), v);
            }
        }
        _ => {}
    }
}
fn remap_node(n: &mut Value, map: &serde_json::Map<String, Value>) {
    for k in ["id", "parent", "children", "source", "robot", "joint"] {
        remap(&mut n[k], map);
    }
    if let Some(member) = n["component_member"].as_object_mut() {
        if let Some(v) = member.get_mut("instance_id") {
            remap(v, map);
        }
    }
    if let Some(spec) = n["component_instance"].as_object_mut() {
        for k in ["node_map", "bindings"] {
            if let Some(o) = spec.get_mut(k).and_then(Value::as_object_mut) {
                for v in o.values_mut() {
                    remap(v, map);
                }
            }
        }
    }
}
fn variant(
    doc: &ArchiveDocument,
    id: &str,
    overrides: &Value,
    nested: &Value,
    choice: Option<&str>,
    stack: &mut HashSet<String>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<HashMap<String, Part>, String> {
    if cancelled() {
        return Err("component reconstruction cancelled".into());
    }
    progress(&format!("Resolving embedded component definition {id}"));
    if stack.len() > 64 || !stack.insert(id.into()) {
        return Err(format!("component definition cycle/depth {id}"));
    }
    let d = &doc.manifest["component_definitions"][id];
    if d.is_null() {
        return Err(format!("missing definition {id}"));
    }
    if d["version"] != 1 {
        return Err(format!("unsupported component schema in {id}"));
    }
    let vars = parameters(d, overrides)?;
    if let Some(variants) = d["variants"].as_object().filter(|o| !o.is_empty()) {
        let choice = choice
            .or_else(|| d["default_variant"].as_str())
            .ok_or("missing component variant")?;
        let selected = variants.get(choice).ok_or("unknown variant")?;
        let target = selected["definition_id"]
            .as_str()
            .ok_or("variant target missing")?;
        let td = &doc.manifest["component_definitions"][target];
        let mut mapped = json!({});
        if let Some(bindings) = selected["parameter_bindings"].as_object() {
            for (k, v) in bindings {
                mapped[k] = json!(expression(
                    v,
                    td["parameters"][k]["unit"]
                        .as_str()
                        .ok_or("mapped parameter unit")?,
                    &vars
                )?);
            }
        }
        let result = variant(
            doc, target, &mapped, nested, None, stack, cancelled, progress,
        );
        stack.remove(id);
        return result;
    }
    let mut parts: HashMap<String, Part> = HashMap::new();
    for n in d["nodes"].as_array().ok_or("definition nodes missing")? {
        let nid = n["id"].as_str().ok_or("definition node id missing")?;
        let recipe = if n["component_member"].is_null() && !n["body_kind"].is_null() {
            Some(Recipe {
                content: format!("components/{id}/{nid}.brep"),
                primitive: 0,
                args: [0.; 8],
                matrices: vec![],
            })
        } else {
            None
        };
        parts.insert(
            nid.into(),
            Part {
                node: n.clone(),
                recipe,
            },
        );
    }
    for f in d["features"].as_array().into_iter().flatten() {
        if cancelled() {
            return Err("component reconstruction cancelled between features".into());
        }
        let kind = f["kind"].as_str().ok_or("component feature kind")?;
        let args = arguments(f, &vars)?;
        let target = f["node"].as_str().ok_or("feature node missing")?;
        if kind == "assembly_placement" {
            let m = transform(&args, true)?;
            for p in parts.values_mut() {
                if p.node["component_member"].is_null() {
                    apply(p, m)?;
                }
            }
            continue;
        }
        let p = parts
            .get_mut(target)
            .ok_or_else(|| format!("feature missing node {target}"))?;
        match kind {
            "box" | "cylinder" => {
                if !matches!(p.node["kind"].as_str(), Some("body" | "sheet")) {
                    return Err(format!("{target}: geometry generation must target a body"));
                }
                if declaration_is_substantive(&p.node["robot"]["mass_properties"])
                    || declaration_is_substantive(&p.node["robot"]["solid_materials"])
                {
                    return Err(format!(
                        "{target}: geometry regeneration requires cleared mass/material declarations"
                    ));
                }
                let mut data = [0.; 8];
                if kind == "box" {
                    data[..3].copy_from_slice(&vector(&args["corner"], [0.; 3])?);
                    let size = vector(&args["size"], [0.; 3])?;
                    if size.iter().any(|x| *x <= 0.) {
                        return Err("box size must be positive".into());
                    }
                    data[3..6].copy_from_slice(&size);
                } else {
                    data[..3].copy_from_slice(&vector(&args["base"], [0.; 3])?);
                    data[3..6].copy_from_slice(&vector(&args["axis"], [0., 0., 1.])?);
                    data[6] = args["radius"].as_f64().ok_or("radius")?;
                    data[7] = args["height"].as_f64().ok_or("height")?;
                    if data[6] <= 0. || data[7] <= 0. {
                        return Err("cylinder radius/height must be positive".into());
                    }
                }
                p.recipe = Some(Recipe {
                    content: format!("components/{id}/{target}.brep"),
                    primitive: if kind == "box" { 1 } else { 2 },
                    args: data,
                    matrices: vec![],
                });
                p.node["body_kind"] = json!("solid");
            }
            "placement" => {
                apply(p, transform(&args, true)?)?;
            }
            "joint_ratio" | "joint_home" | "joint_frame" => {}
            _ => return Err(format!("feature {kind} migration missing")),
        }
    }
    let roots: Vec<String> = parts
        .iter()
        .filter(|(_, p)| {
            !p.node["component_instance"].is_null() && p.node["component_member"].is_null()
        })
        .map(|(k, _)| k.clone())
        .collect();
    for nid in roots {
        let root = parts.get(&nid).ok_or("nested root missing")?.clone();
        let mut spec = root.node["component_instance"].clone();
        let target = spec["definition_id"]
            .as_str()
            .ok_or("nested definition missing")?
            .to_owned();
        let td = &doc.manifest["component_definitions"][&target];
        let mut child_values = spec["overrides"].as_object().cloned().unwrap_or_default();
        if let Some(bindings) = spec["parameter_bindings"].as_object() {
            for (k, v) in bindings {
                child_values.insert(
                    k.clone(),
                    json!(expression(
                        v,
                        td["parameters"][k]["unit"]
                            .as_str()
                            .ok_or("child unit missing")?,
                        &vars
                    )?),
                );
            }
        }
        if let Some(extra) = nested[&nid].as_object() {
            child_values.extend(extra.clone());
        }
        spec["overrides"] = Value::Object(child_values.clone());
        let mut map = spec["node_map"]
            .as_object()
            .ok_or("nested node_map missing")?
            .clone();
        let mut child_nested = spec["nested_overrides"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (k, v) in &map {
            if let Some(extra) = v.as_str().and_then(|v| nested.get(v)) {
                child_nested.insert(k.clone(), extra.clone());
            }
        }
        let child = variant(
            doc,
            &target,
            &Value::Object(child_values),
            &Value::Object(child_nested),
            spec["variant"].as_str(),
            stack,
            cancelled,
            progress,
        )?;
        let m = placement(&spec["placement"])?;
        bind_ports(doc, &target, &spec, &mut map, &parts)?;
        parts.get_mut(&nid).ok_or("nested root missing")?.node["component_instance"] = spec.clone();
        for (source, mut p) in child {
            let dest = map
                .get(&source)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("nested identity missing for {source}"))?;
            apply(&mut p, m)?;
            remap_node(&mut p.node, &map);
            p.node["id"] = json!(dest);
            if p.node["parent"].is_null() {
                p.node["parent"] = json!(nid);
            }
            if p.node["component_member"].is_null() {
                p.node["component_member"] = json!({"instance_id":nid,"source_node":source});
            }
            parts.insert(dest.into(), p);
        }
    }
    stack.remove(id);
    Ok(parts)
}

fn structural_roots(
    doc: &ArchiveDocument,
    id: &str,
    choice: Option<&str>,
) -> Result<Vec<String>, String> {
    let definition = &doc.manifest["component_definitions"][id];
    if let Some(variants) = definition["variants"].as_object().filter(|v| !v.is_empty()) {
        let name = choice
            .or_else(|| definition["default_variant"].as_str())
            .ok_or("missing component variant")?;
        let selected = variants.get(name).ok_or("unknown component variant")?;
        let target = selected["definition_id"]
            .as_str()
            .ok_or("variant definition missing")?;
        // Reference family targets must be an assembly, never another family.
        let target_definition = &doc.manifest["component_definitions"][target];
        if target_definition["variants"]
            .as_object()
            .is_some_and(|v| !v.is_empty())
        {
            return Err("A family variant must be an assembly definition".into());
        }
        return structural_roots(doc, target, None);
    }
    definition["roots"]
        .as_array()
        .ok_or("component roots missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "component root must be a node ID".into())
        })
        .collect()
}

pub(crate) fn restore(
    doc: &mut ArchiveDocument,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<(), String> {
    let roots: Vec<Value> = doc
        .effective_nodes
        .values()
        .filter(|n| !n["component_instance"].is_null() && n["component_member"].is_null())
        .cloned()
        .collect();
    let mut claimed = HashSet::new();
    for root in roots {
        if cancelled() {
            return Err("component reconstruction cancelled".into());
        }
        let rid = root["id"].as_str().ok_or("occurrence id missing")?;
        let spec = &root["component_instance"];
        let did = spec["definition_id"]
            .as_str()
            .ok_or("occurrence definition missing")?;
        let d = &doc.manifest["component_definitions"][did];
        if spec["revision"] != d["revision"] {
            return Err(format!(
                "occurrence {rid}: definition {did} revision mismatch"
            ));
        }
        let parts = variant(
            doc,
            did,
            &spec["overrides"],
            &spec["nested_overrides"],
            spec["variant"].as_str(),
            &mut HashSet::new(),
            cancelled,
            progress,
        )
        .map_err(|e| format!("occurrence {rid}: definition {did}: {e}"))?;
        let mut map = spec["node_map"]
            .as_object()
            .ok_or("occurrence node_map missing")?
            .clone();
        if parts.len() != map.len() {
            return Err(format!(
                "occurrence {rid}: node identities differ from definition"
            ));
        }
        let expected_children: Vec<Value> = structural_roots(doc, did, spec["variant"].as_str())?
            .iter()
            .map(|id| {
                map.get(id)
                    .cloned()
                    .ok_or_else(|| format!("occurrence {rid}: missing root identity {id}"))
            })
            .collect::<Result<_, _>>()?;
        if root["children"] != Value::Array(expected_children) {
            return Err(format!(
                "occurrence {rid}: hierarchy differs from its definition"
            ));
        }
        bind_ports(doc, did, spec, &mut map, &HashMap::new())?;
        let placement = transform(&spec["placement"], true)?;
        for (source, mut p) in parts {
            if cancelled() {
                return Err("component reconstruction cancelled between members".into());
            }
            let dest = map
                .get(&source)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("occurrence {rid}: missing identity {source}"))?;
            if !claimed.insert(dest.to_owned()) {
                return Err(format!("occurrence {rid}: shared member {dest}"));
            }
            let saved = doc
                .effective_nodes
                .get(dest)
                .ok_or_else(|| format!("occurrence {rid}: saved member {dest} missing"))?
                .clone();
            apply(&mut p, placement)?;
            remap_node(&mut p.node, &map);
            if p.node["parent"].is_null() {
                p.node["parent"] = json!(rid);
            }
            if p.node["component_member"].is_null() {
                p.node["component_member"] = json!({"instance_id":rid,"source_node":source});
            }
            if saved["component_member"] != p.node["component_member"] {
                return Err(format!(
                    "occurrence {rid}: member {dest}: source identity mismatch"
                ));
            }
            if saved["parent"] != p.node["parent"] || saved["children"] != p.node["children"] {
                return Err(format!(
                    "occurrence {rid}: member {dest}: hierarchy mismatch"
                ));
            }
            let mut effective = saved;
            effective["robot"] = p.node["robot"].clone();
            effective["body_kind"] = p.node["body_kind"].clone();
            effective["joint"] = p.node["joint"].clone();
            effective["component_instance"] = p.node["component_instance"].clone();
            if let Some(recipe) = p.recipe {
                doc.recipes.insert(dest.into(), recipe);
            }
            doc.effective_nodes.insert(dest.into(), effective);
        }
    }
    for (id, n) in &doc.effective_nodes {
        if !n["component_member"].is_null() && !claimed.contains(id) {
            return Err(format!("orphan component member {id}"));
        }
    }
    Ok(())
}
