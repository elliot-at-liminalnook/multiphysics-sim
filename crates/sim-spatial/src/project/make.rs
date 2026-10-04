//! Make: the tested design's part files. Every printed body (a material
//! tagged `print`) of the saved design becomes a binary STL under the
//! project's `make/` (its exact-geometry tessellation, mm), listed in
//! `make/parts.json` with the CAD file's SHA-256 (the tested design's),
//! its material, mass, volume and print settings, and the bought parts (the
//! library motors). Print studies (layer strength, splitting, plates) are
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
        for g in &local.geometry {
            let Some(n) = archive.node(&g.node_id) else { continue };
            let name = n["name"].as_str().unwrap_or(&g.node_id).to_string();
            if n["robot"]["kind"] == "motor" {
                bought.push(json!({"name": name, "spec": n["robot"]["spec"], "kind": "motor"}));
                continue;
            }
            let Some(mid) = n["material"].as_str().filter(|m| printed(m)) else { continue };
            let file = format!("{}.stl", sim_runtime::robot_project::slug(&name));
            std::fs::write(make.join(&file), stl(&name, &g.vertices_mm, &g.triangles)).map_err(|e| format!("{file}: {e}"))?;
            let mass = local.masses.bodies.get(&g.node_id);
            parts.push(json!({"name": name, "id": g.node_id, "file": file, "material": mid, "mass_kg": mass.map(|m| m.mass_kg), "volume_cm3": g.properties.volume_mm3 / 1000.0,
                "print": {"orientation": n["robot"]["print_orientation"], "infill": n["robot"]["infill"], "walls": n["robot"]["walls"]}, "triangles": g.triangles.len()}));
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
    fn a_binary_stl_has_its_header_count_and_records() {
        let b = super::stl("cube", &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], &[[0, 1, 2]]);
        assert_eq!(b.len(), 84 + 50);
        assert_eq!(u32::from_le_bytes(b[80..84].try_into().unwrap()), 1);
        // The normal of a counter-clockwise triangle in the XY plane is +Z.
        assert_eq!(f32::from_le_bytes(b[92..96].try_into().unwrap()), 1.0);
    }
}
