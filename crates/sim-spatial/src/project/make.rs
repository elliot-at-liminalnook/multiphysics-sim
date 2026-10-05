//! Make: the tested design's part files. Every printed body (a material
//! tagged `print`) of the saved design becomes a binary STL under the
//! project's `make/` (its exact-geometry tessellation, mm), listed in
//! `make/parts.json` with the CAD file's SHA-256 (the tested design's),
//! its material, mass, volume and print settings, and the bought parts (the
//! library motors). File names are the bodies' names, made distinct when two
//! bodies share one (`part_files`); files a previous export wrote that this
//! design no longer has are removed. Print studies (layer strength, splitting, plates) are
//! not ported to the in-process editor; the list says so.
use super::ProjectState;
use crate::cad::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};

pub(crate) const NOT_PORTED: &str = "print studies (layer-aware strength, splitting into printable pieces, plate layout) are not ported to the in-process editor: check printed-part strength before relying on these parts";

#[derive(Default)]
pub(crate) struct MakeRun {
    job: Option<Job<Value>>,
    pub last: Option<Result<String, String>>,
}
impl MakeRun {
    pub(crate) fn json(&self) -> Value {
        json!({"running": self.job.is_some(), "last": self.last.as_ref().map(super::msg)})
    }
}

/// The print settings the model export assumes for a body that states none
/// (`sim_cad::physical`).
pub(crate) const DEFAULT_INFILL: f64 = 0.3;
pub(crate) const DEFAULT_WALLS: u32 = 3;

/// A body's print settings as its CAD node states them, and which of them
/// it does not state (the default is then used, and named).
pub(crate) struct PrintSettings {
    pub orientation: [f64; 3],
    pub infill: f64,
    pub walls: u32,
    pub assumed: Vec<String>,
}

pub(crate) fn print_settings(node: &Value) -> PrintSettings {
    let robot = &node["robot"];
    let mut assumed = Vec::new();
    let orientation = robot["print_orientation"].as_array().filter(|a| a.len() == 3).and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])).filter(|v| v.iter().map(|c| c * c).sum::<f64>() > 1e-12).unwrap_or_else(|| {
        assumed.push("printed upright (+Z)".to_string());
        [0.0, 0.0, 1.0]
    });
    let infill = robot["infill"].as_f64().map(|i| i.clamp(0.0, 1.0)).unwrap_or_else(|| {
        assumed.push(format!("{:.0} % infill", DEFAULT_INFILL * 100.0));
        DEFAULT_INFILL
    });
    let walls = robot["walls"].as_u64().map(|w| w as u32).unwrap_or_else(|| {
        assumed.push(format!("{DEFAULT_WALLS} walls"));
        DEFAULT_WALLS
    });
    PrintSettings { orientation, infill, walls, assumed }
}

/// One distinct `.stl` file name per body, from (id, name): the name's slug,
/// with the body's id appended for every body whose slug another shares
/// (or that has none), so no part overwrites another and a name keeps its
/// file whatever order the bodies come in.
pub(crate) fn part_files(bodies: &[(String, String)]) -> Vec<String> {
    let slug = sim_runtime::robot_project::slug;
    let slugs: Vec<String> = bodies.iter().map(|(_, name)| slug(name)).collect();
    let mut files: Vec<String> = bodies.iter().zip(&slugs).map(|((id, _), s)| {
        let shared = s.is_empty() || slugs.iter().filter(|o| *o == s).count() > 1;
        match (shared, s.is_empty()) {
            (false, _) => s.clone(),
            (true, true) => format!("part-{}", slug(id)),
            (true, false) => format!("{s}-{}", slug(id)),
        }
    }).collect();
    // Ids are unique, but a suffixed name can still meet another body's plain one.
    for i in 0..files.len() {
        let mut n = 2;
        while files[..i].contains(&files[i]) {
            files[i] = format!("{}-{n}", files[i].trim_end_matches(&format!("-{}", n - 1)));
            n += 1;
        }
    }
    files.into_iter().map(|f| format!("{f}.stl")).collect()
}

/// A binary STL of `triangles` over `vertices` (mm).
pub(crate) fn stl(name: &str, vertices: &[[f64; 3]], triangles: &[[u32; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + triangles.len() * 50);
    let mut header = format!("sim-spatial part {name}").into_bytes();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for t in triangles {
        let [a, b, c] = t.map(|i| vertices[i as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        for x in n.map(|c| c / len).iter().chain(a.iter()).chain(b.iter()).chain(c.iter()) {
            out.extend_from_slice(&(*x as f32).to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Start exporting (CAD is on the project's saved, tested file).
pub(crate) fn start(st: &mut ProjectState, doc: &CadDocument) -> Result<Value, String> {
    let project = st.open.clone().ok_or("no project is open")?;
    if st.make.job.is_some() {
        return Err("the part files are already being written".into());
    }
    if crate::cad::for_project::unsaved(doc) {
        return Err("Make needs the saved, tested design: save it, make the model and test it first".into());
    }
    if st.step(super::Step::Test).is_none_or(|s| s.state != super::status::State::Done) {
        return Err("Make needs a passed test of the current design (step 3)".into());
    }
    let local = doc.local.clone().ok_or("no CAD document is open")?;
    let (make, cad) = (project.make(), project.cad());
    let report = st.files.as_ref().and_then(|f| f.report_path.clone());
    st.make.job = Some(Job::spawn(Pool::Io, 0, "make: part files", move |_| {
        let archive = &local.archive;
        std::fs::create_dir_all(&make).map_err(|e| format!("{}: {e}", make.display()))?;
        let printed = |mid: &str| sim_cad::edit::material(&archive.manifest, mid).is_some_and(|m| m["tags"].as_array().is_some_and(|t| t.iter().any(|x| x == "print")));
        let mut parts = Vec::new();
        let mut bought = Vec::new();
        let mut printed_bodies = Vec::new();
        for g in &local.geometry {
            let Some(n) = archive.node(&g.node_id) else { continue };
            let name = n["name"].as_str().unwrap_or(&g.node_id).to_string();
            if n["robot"]["kind"] == "motor" {
                bought.push(json!({"name": name, "spec": n["robot"]["spec"], "kind": "motor"}));
                continue;
            }
            let Some(mid) = n["material"].as_str().filter(|m| printed(m)) else { continue };
            printed_bodies.push((g, n, name, mid.to_string()));
        }
        let files = part_files(&printed_bodies.iter().map(|(g, _, name, _)| (g.node_id.clone(), name.clone())).collect::<Vec<_>>());
        for ((g, n, name, mid), file) in printed_bodies.into_iter().zip(&files) {
            std::fs::write(make.join(file), stl(&name, &g.vertices_mm, &g.triangles)).map_err(|e| format!("{file}: {e}"))?;
            let mass = local.masses.bodies.get(&g.node_id);
            let print = print_settings(n);
            parts.push(json!({"name": name, "id": g.node_id, "file": file, "material": mid, "mass_kg": mass.map(|m| m.mass_kg), "volume_cm3": g.properties.volume_mm3 / 1000.0,
                "print": {"orientation": print.orientation, "infill": print.infill, "walls": print.walls, "assumed": print.assumed}, "triangles": g.triangles.len()}));
        }
        if parts.is_empty() {
            return Err("the design has no printed body (a material tagged print, such as PLA or PETG)".into());
        }
        let sha = sim_domain_robot::cad_link::sha256_file(&cad).map_err(|e| format!("{}: {e}", cad.display()))?;
        if archive.identity().trim_start_matches("sha256:") != sha {
            return Err(format!("{} changed on disk since it was opened: save or reopen it", cad.display()));
        }
        let manifest = json!({
            "schema": "sim.robot-make/1", "cad": cad, "cad_sha256": sha, "cad_revision": archive.manifest["revision"],
            "tested_report": report, "made_at": crate::robot::recording::stamp(crate::robot::recording::now_ms()),
            "parts": parts, "bought": bought, "not_ported": NOT_PORTED,
            "units": "STL in millimetres, the design's frame (Z up); the CAD display tessellation of the exact geometry",
        });
        // Part files an earlier export listed that this design no longer has.
        let earlier: Option<Value> = std::fs::read(make.join("parts.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        for old in earlier.iter().flat_map(|m| m["parts"].as_array().into_iter().flatten()).filter_map(|p| p["file"].as_str()) {
            if !files.iter().any(|f| f == old) && old.ends_with(".stl") && !old.contains(['/', '\\']) {
                let _ = std::fs::remove_file(make.join(old));
            }
        }
        std::fs::write(make.join("parts.json"), serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| format!("parts.json: {e}"))?;
        Ok(manifest)
    }));
    st.say(Ok("Writing the part files…".into()));
    Ok(json!({"started": true}))
}

/// JobResults: the export lands.
pub(super) fn tick(mut st: ResMut<ProjectState>) {
    let Some(result) = st.make.job.as_ref().and_then(|j| j.poll()) else { return };
    let st = &mut *st;
    st.make.job = None;
    let outcome = result.map(|m| format!("{} part file(s) written to {}", m["parts"].as_array().map_or(0, Vec::len), st.open.as_ref().map(|p| p.make().display().to_string()).unwrap_or_default()));
    st.say(outcome.clone());
    st.make.last = Some(outcome);
    st.panel.view = Some(super::Step::Make);
}

#[cfg(test)]
mod tests {
    #[test]
    fn bodies_that_share_a_name_get_their_own_files() {
        let b = |id: &str, name: &str| (id.to_string(), name.to_string());
        let files = super::part_files(&[b("n1", "Arm"), b("n2", "Base Plate"), b("n3", "arm"), b("n4", "!!"), b("n5", "Arm n1")]);
        assert_eq!(files, ["arm-n1.stl", "base-plate.stl", "arm-n3.stl", "part-n4.stl", "arm-n1-2.stl"]);
        let mut distinct = files.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), files.len());
        // The same bodies in another order keep their files.
        let again = super::part_files(&[b("n3", "arm"), b("n1", "Arm"), b("n2", "Base Plate")]);
        assert_eq!(again, ["arm-n3.stl", "arm-n1.stl", "base-plate.stl"]);
    }

    #[test]
    fn unstated_print_settings_are_named() {
        let p = super::print_settings(&serde_json::json!({"robot": {"infill": 0.5}}));
        assert_eq!((p.infill, p.walls, p.orientation), (0.5, super::DEFAULT_WALLS, [0.0, 0.0, 1.0]));
        assert_eq!(p.assumed, ["printed upright (+Z)", "3 walls"]);
        assert!(super::print_settings(&serde_json::json!({"robot": {"infill": 0.2, "walls": 4, "print_orientation": [1, 0, 0]}})).assumed.is_empty());
    }

    #[test]
    fn a_binary_stl_has_its_header_count_and_records() {
        let b = super::stl("cube", &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], &[[0, 1, 2]]);
        assert_eq!(b.len(), 84 + 50);
        assert_eq!(u32::from_le_bytes(b[80..84].try_into().unwrap()), 1);
        // The normal of a counter-clockwise triangle in the XY plane is +Z.
        assert_eq!(f32::from_le_bytes(b[92..96].try_into().unwrap()), 1.0);
    }
}
