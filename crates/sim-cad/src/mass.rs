//! Exact B-rep mass derivation. All kernel properties are already world placed;
//! neither a body's archived transform nor an instance placement is applied here.
//! Reference: document.py:430–496, physical.py:108–164, 751–766.
use crate::archive::ArchiveDocument;
use crate::geometry::{BodyGeometry, GeometryProperties};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MassResult {
    pub mass_kg: f64,
    pub centroid_m: [f64; 3],
    pub inertia_kg_m2: [[f64; 3]; 3],
    pub origin: String,
    pub source: Value,
    pub included_in: Option<String>,
    pub provenance: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MassResults {
    pub bodies: BTreeMap<String, MassResult>,
    pub assembly: MassResult,
    /// Physical.py's doc.bodies() excludes instances but includes hidden/disabled bodies.
    pub physical_assembly: MassResult,
    pub model_identity: String,
    pub derivation_identity: String,
}

fn vector(v: &Value) -> Result<[f64; 3], String> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or("expected finite 3-vector")?;
    let mut out = [0.; 3];
    for i in 0..3 {
        out[i] = a[i]
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or("expected finite 3-vector")?;
    }
    Ok(out)
}
fn tensor(v: &Value) -> Result<[[f64; 3]; 3], String> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or("expected finite 3x3 tensor")?;
    Ok([vector(&a[0])?, vector(&a[1])?, vector(&a[2])?])
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|v| v != 0.),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}
/// Jacobi diagonalization of a symmetric 3x3 tensor, independent of native handles.
fn eigenvalues(mut a: [[f64; 3]; 3]) -> [f64; 3] {
    for _ in 0..64 {
        let mut p = 0;
        let mut q = 1;
        for (i, j) in [(0, 2), (1, 2)] {
            if a[i][j].abs() > a[p][q].abs() {
                p = i;
                q = j;
            }
        }
        if a[p][q] == 0. {
            break;
        }
        let theta = 0.5 * (2. * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (s, c) = theta.sin_cos();
        let app = a[p][p];
        let aqq = a[q][q];
        let apq = a[p][q];
        a[p][p] = c * c * app - 2. * s * c * apq + s * s * aqq;
        a[q][q] = s * s * app + 2. * s * c * apq + c * c * aqq;
        a[p][q] = 0.;
        a[q][p] = 0.;
        for k in 0..3 {
            if k != p && k != q {
                let x = a[k][p];
                let y = a[k][q];
                a[k][p] = c * x - s * y;
                a[p][k] = a[k][p];
                a[k][q] = s * x + c * y;
                a[q][k] = a[k][q];
            }
        }
    }
    [a[0][0], a[1][1], a[2][2]]
}
/// NumPy reference tolerances: symmetry atol=1e-14, rtol=1e-5;
/// PSD >= -1e-14; principal triangle allowance 1e-12.
pub fn validate_inertia(a: [[f64; 3]; 3]) -> Result<(), String> {
    for i in 0..3 {
        for j in 0..3 {
            if !a[i][j].is_finite() || (a[i][j] - a[j][i]).abs() > 1e-14 + 1e-5 * a[j][i].abs() {
                return Err("inertia must be finite and symmetric".into());
            }
        }
    }
    // eigvalsh uses the lower triangle, as does this symmetrization.
    let mut symmetric = a;
    for i in 0..3 {
        for j in 0..i {
            symmetric[j][i] = a[i][j];
        }
    }
    let e = eigenvalues(symmetric);
    let largest = e.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if e.iter().any(|v| !v.is_finite() || *v < -1e-14)
        || !e.iter().sum::<f64>().is_finite()
        || largest > e.iter().sum::<f64>() - largest + 1e-12
    {
        return Err(
            "inertia is not physically positive semidefinite with physical principal moments"
                .into(),
        );
    }
    Ok(())
}
fn aggregate<'a>(
    terms: impl IntoIterator<Item = &'a MassResult>,
    source: Value,
    floor: f64,
) -> MassResult {
    let terms: Vec<_> = terms.into_iter().collect();
    let mass = terms.iter().map(|t| t.mass_kg).sum::<f64>();
    let mut com = [0.; 3];
    for t in &terms {
        for i in 0..3 {
            com[i] += t.mass_kg * t.centroid_m[i] / mass.max(floor);
        }
    }
    let mut inertia = [[0.; 3]; 3];
    for t in &terms {
        let d = std::array::from_fn::<_, 3, _>(|i| t.centroid_m[i] - com[i]);
        let d2 = d.iter().map(|x| x * x).sum::<f64>();
        for i in 0..3 {
            for j in 0..3 {
                inertia[i][j] += t.inertia_kg_m2[i][j]
                    + t.mass_kg * ((if i == j { d2 } else { 0. }) - d[i] * d[j]);
            }
        }
    }
    MassResult {
        mass_kg: mass,
        centroid_m: com,
        inertia_kg_m2: inertia,
        origin: if terms.iter().any(|t| t.origin == "provisional") {
            "provisional"
        } else {
            "derived"
        }
        .into(),
        source,
        included_in: None,
        provenance: json!({"formula":"sum full centroid tensors + parallel-axis terms", "members":terms.iter().map(|t|&t.provenance).collect::<Vec<_>>()}),
    }
}
fn density(doc: &ArchiveDocument, mid: Option<&str>) -> Result<(f64, bool), String> {
    let materials = doc.manifest["materials"].as_array();
    if let Some(m) =
        materials.and_then(|m| m.iter().find(|m| m["id"].as_str() == mid && mid.is_some()))
    {
        return Ok((
            m["density"]
                .as_f64()
                .filter(|d| d.is_finite() && *d >= 0.)
                .ok_or("material density must be finite and nonnegative")?,
            false,
        ));
    }
    // Empty/missing material tables restore document defaults on reference load.
    if materials.is_none_or(|m| m.is_empty()) {
        let d = match mid.unwrap_or("") {
            "pla" => 1.24,
            "petg" => 1.27,
            "abs" => 1.04,
            "asa" => 1.07,
            "tpu" => 1.21,
            "nylon" => 1.01,
            "resin" => 1.15,
            "al" => 2.70,
            "steel" => 7.85,
            "brass" => 8.5,
            "pcb" => 1.85,
            "rubber" => 1.1,
            "glass" => 1.18,
            _ => 1.,
        };
        return Ok((d, true));
    }
    Ok((1., true)) // document.density_of's explicit missing-material fallback.
}
fn material_exists(doc: &ArchiveDocument, mid: &str) -> bool {
    let materials = doc.manifest["materials"].as_array();
    if materials.is_none_or(|m| m.is_empty()) {
        return [
            "pla", "petg", "abs", "asa", "tpu", "nylon", "resin", "al", "steel", "brass", "pcb",
            "rubber", "glass",
        ]
        .contains(&mid);
    }
    materials.is_some_and(|m| m.iter().any(|m| m["id"].as_str() == Some(mid)))
}
fn from_geometry(p: &GeometryProperties, rho: f64, default: bool) -> Result<MassResult, String> {
    if !p.volume_mm3.is_finite()
        || p.volume_mm3 < 0.
        || p.centroid_mm.iter().any(|v| !v.is_finite())
    {
        return Err("invalid exact B-rep volume/centroid".into());
    }
    let inertia = p.inertia_mm5.map(|row| row.map(|v| v * rho * 1e-12));
    validate_inertia(inertia)?;
    Ok(MassResult {
        mass_kg: p.volume_mm3 * rho * 1e-6,
        centroid_m: p.centroid_mm.map(|v| v * 1e-3),
        inertia_kg_m2: inertia,
        origin: "provisional".into(),
        source: json!("CAD volume and material density"),
        included_in: None,
        provenance: json!({"density_g_cm3":rho,"density_default":default,"calibration_status":"provisional unless calibrated","geometry_authority":"OCCT exact B-rep","frame":"world","volume_mm3":p.volume_mm3}),
    })
}
fn visible(doc: &ArchiveDocument, node: &Value) -> Result<bool, String> {
    let mut current = node;
    let mut seen = BTreeSet::new();
    loop {
        if current["visible"].as_bool() == Some(false)
            || current["disabled"].as_bool() == Some(true)
        {
            return Ok(false);
        }
        let Some(parent) = current["parent"].as_str() else {
            return Ok(true);
        };
        if !seen.insert(parent) {
            return Err("outliner parent cycle".into());
        }
        current = doc.node(parent).ok_or("missing outliner parent")?;
    }
}

pub fn derive_document(
    doc: &ArchiveDocument,
    bodies: &[BodyGeometry],
) -> Result<MassResults, String> {
    derive_document_with(doc, bodies, &|| false, &|_| {})
}
pub fn derive_document_with(
    doc: &ArchiveDocument,
    bodies: &[BodyGeometry],
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<MassResults, String> {
    // Document.bodies() follows Document.walk(), not all stored node records.
    // Orphan records remain owned and inspectable but never become assembly mass.
    let mut reachable = BTreeSet::new();
    let mut pending: Vec<String> = doc.manifest["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    while let Some(id) = pending.pop() {
        if cancelled() {
            return Err(format!("{}: assembly walk cancelled", doc.path.display()));
        }
        if !reachable.insert(id.clone()) {
            return Err(format!(
                "{}: node {id}: repeated assembly tree identity",
                doc.path.display()
            ));
        }
        // Reference load filters missing root IDs; walk skips missing children.
        if let Some(node) = doc.node(&id) {
            pending.extend(
                node["children"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned)),
            );
        }
    }
    let mut results = BTreeMap::new();
    let mut included = BTreeSet::new();
    for body in bodies {
        let id = &body.node_id;
        if doc.node(id).is_some_and(|node| node["kind"] == "curve") {
            continue;
        }
        if cancelled() {
            return Err(format!(
                "{}: node {id}: mass derivation cancelled",
                doc.path.display()
            ));
        }
        progress(&format!("Deriving mass: {id}"));
        let run = || -> Result<MassResult, String> {
            let node = doc.node(id).ok_or("geometry has no manifest node")?;
            let meta = &node["robot"];
            let regions = &meta["solid_materials"];
            if !regions.is_null() && !regions.is_object() {
                return Err("solid_materials must be an object".into());
            }
            if let Some(regions) = regions.as_object() {
                for (index, mid) in regions {
                    if index.is_empty()
                        || !index.bytes().all(|b| b.is_ascii_digit())
                        || mid.as_str().is_none()
                    {
                        return Err("solid_materials requires nonnegative indices and existing material IDs".into());
                    }
                    if !material_exists(doc, mid.as_str().ok_or("material ID must be a string")?) {
                        return Err(format!("solid_materials[{index}] missing material {mid}"));
                    }
                }
            }
            let declared = &meta["mass_properties"];
            if !declared.is_null() {
                let mass = declared["mass_kg"]
                    .as_f64()
                    .filter(|m| m.is_finite() && *m >= 0.)
                    .ok_or("mass_kg must be finite and nonnegative")?;
                let com = vector(&declared["com_mm"])?;
                let inertia = tensor(&declared["inertia_kg_m2"])?;
                validate_inertia(inertia)?;
                let source = &declared["source"];
                if !truthy(source) {
                    return Err("mass declaration requires source".into());
                }
                let owner = if declared["included_in"].is_null() {
                    None
                } else {
                    Some(
                        declared["included_in"]
                            .as_str()
                            .ok_or("included_in must be a node ID")?
                            .to_owned(),
                    )
                };
                if let Some(owner) = &owner {
                    if doc.node(owner).is_none() {
                        return Err(format!("included_in references missing node {owner}"));
                    }
                }
                if mass == 0.
                    && (owner.is_none() || inertia.iter().flatten().any(|v| v.abs() > 1e-15))
                {
                    return Err("zero mass requires zero inertia and included_in".into());
                }
                return Ok(MassResult {
                    mass_kg: mass,
                    centroid_m: com.map(|v| v * 1e-3),
                    inertia_kg_m2: inertia,
                    // A declaration may combine specified mass with estimated
                    // COM/inertia. Never upgrade arbitrary source text to measured.
                    origin: declared["origin"]
                        .as_str()
                        .filter(|s| {
                            matches!(*s, "measured" | "derived" | "estimated" | "provisional")
                        })
                        .unwrap_or("declared")
                        .into(),
                    source: source.clone(),
                    included_in: owner,
                    provenance: json!({"declaration":declared,"frame":"world","archive_saved":doc.manifest["saved"]}),
                });
            }
            if let Some(regions) = regions.as_object().filter(|r| !r.is_empty()) {
                for index in regions.keys() {
                    if index.parse::<usize>().map_err(|_| "solid index overflow")?
                        >= body.solids.len()
                    {
                        return Err(format!(
                            "solid_materials[{index}] is stale for {} solids",
                            body.solids.len()
                        ));
                    }
                }
                let mut terms = Vec::new();
                for (i, solid) in body.solids.iter().enumerate() {
                    if cancelled() {
                        return Err(format!("solid {i}: mass derivation cancelled"));
                    }
                    let mid = regions
                        .get(&i.to_string())
                        .and_then(Value::as_str)
                        .or(node["material"].as_str())
                        .ok_or_else(|| format!("solid {i}: missing inherited body material"))?;
                    if !material_exists(doc, mid) {
                        return Err(format!("solid {i}: missing material {mid}"));
                    }
                    let (rho, default) = density(doc, Some(mid))?;
                    let mut term = from_geometry(&solid.properties, rho, default)?;
                    term.provenance["solid_index"] = json!(i);
                    term.provenance["material_id"] = json!(mid);
                    term.provenance["material_assignment"] =
                        json!(if regions.contains_key(&i.to_string()) {
                            "solid override"
                        } else {
                            "inherited body material"
                        });
                    terms.push(term);
                }
                return Ok(aggregate(
                    &terms,
                    json!(
                        "CAD volumes with per-solid material densities (provisional unless calibrated)"
                    ),
                    1e-15,
                ));
            }
            let (rho, default) = density(doc, node["material"].as_str())?;
            let mut result = from_geometry(&body.properties, rho, default)?;
            result.provenance["material_id"] = node["material"].clone();
            Ok(result)
        };
        let mut result = run().map_err(|e| format!("{}: node {id}: {e}", doc.path.display()))?;
        result.provenance["node_id"] = json!(id);
        let node = doc
            .node(id)
            .ok_or_else(|| format!("{}: node {id}: missing node", doc.path.display()))?;
        if reachable.contains(id)
            && visible(doc, node).map_err(|e| format!("{}: node {id}: {e}", doc.path.display()))?
        {
            included.insert(id.clone());
        }
        results.insert(id.clone(), result);
    }
    let mut assembly = aggregate(
        included.iter().filter_map(|id| results.get(id)),
        json!("Visible non-disabled body/sheet/instance aggregation; world full tensors"),
        1e-12,
    );
    assembly.provenance["included_nodes"] = json!(included);
    let physical_ids: Vec<_> = results
        .keys()
        .filter(|id| {
            reachable.contains(*id)
                && doc
                    .node(id)
                    .is_some_and(|n| matches!(n["kind"].as_str(), Some("body" | "sheet")))
        })
        .cloned()
        .collect();
    let mut physical_assembly = aggregate(
        physical_ids.iter().filter_map(|id| results.get(id)),
        json!(
            "Physical reference doc.bodies(): all body/sheet geometry, including hidden/disabled; instances excluded"
        ),
        1e-12,
    );
    physical_assembly.provenance["included_nodes"] = json!(physical_ids);
    let kernel_identity = crate::geometry::kernel_identity_with(cancelled).map_err(|e| {
        format!(
            "{}: mass derivation kernel identity: {e}",
            doc.path.display()
        )
    })?;
    if cancelled() {
        return Err(format!(
            "{}: cancelled after kernel identity",
            doc.path.display()
        ));
    }
    let derivation_identity = derivation_identity_for(&kernel_identity);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("{}: derivation timestamp: {e}", doc.path.display()))?
        .as_secs_f64();
    for result in results
        .values_mut()
        .chain([&mut assembly, &mut physical_assembly])
    {
        let id = result.provenance["node_id"].as_str().unwrap_or("assembly");
        if !result.mass_kg.is_finite() || result.centroid_m.iter().any(|x| !x.is_finite()) {
            return Err(format!(
                "{}: node {id}: mass/centroid overflow during aggregation",
                doc.path.display()
            ));
        }
        validate_inertia(result.inertia_kg_m2)
            .map_err(|e| format!("{}: node {id}: derived result: {e}", doc.path.display()))?;
        result.provenance["model_identity"] = json!(doc.model_identity);
        result.provenance["derivation_identity"] = json!(derivation_identity);
        result.provenance["derived_at_unix"] = json!(timestamp);
        result.provenance["schema_version"] = json!(1);
        result.provenance["kernel_header_version"] = json!(crate::geometry::kernel_version());
        result.provenance["kernel_identity"] = json!(kernel_identity);
    }
    Ok(MassResults {
        bodies: results,
        assembly,
        physical_assembly,
        model_identity: doc.model_identity.clone(),
        derivation_identity,
    })
}

/// Production identity covers exact archive/kernel/bridge/build implementation,
/// these arithmetic rules, result schema, and actual loaded OCCT library bytes.
pub fn derivation_identity() -> Result<String, String> {
    Ok(derivation_identity_for(&crate::geometry::kernel_identity()?))
}
fn derivation_identity_for(kernel: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let production = crate::production_source_identity();
    for bytes in [
        production.as_bytes(),
        include_bytes!("mass.rs").as_slice(),
        include_bytes!("lib.rs").as_slice(),
        b"mass-result-schema:1",
        kernel.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    format!("sha256:{:x}", hash.finalize())
}

// Unexecuted acceptance cases: 10 mm cube at 1 g/cm³ => 1e-3 kg,
// diagonal inertia 1.666666666666667e-8 kg m²; two equal translated cubes
// verify full parallel-axis off diagonals; rotated anisotropic box verifies
// R I Rᵀ at kernel boundary; material compound checks weighted COM; measured
// zero-mass member preserves included_in; stale indices and invalid symmetric,
// PSD/principal moments reject contextually. Relative analytic tolerance 1e-9,
// B-rep/reference parity 1e-7 (absolute mass 1e-12 kg, COM 1e-9 m,
// inertia 1e-14 kg m²); no execution is claimed.
