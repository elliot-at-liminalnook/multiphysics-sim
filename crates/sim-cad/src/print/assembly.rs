//! Assembly instructions for a split part (RoboCAD's `print_assembly.py`).
//!
//! From the split's seams (which pieces meet, with which joints): an order
//! that starts with the largest piece and adds one neighbour at a time, the
//! hardware list with the tools it needs, a self-contained HTML guide with
//! a picture of every step, and an exploded view (instances of the pieces,
//! offset along the way each one goes on; the pieces themselves are not
//! moved).
//!
//! Preparation comes first: heat-set inserts and press-fit pins go into
//! their pieces before those pieces are joined (the lesson "Screw bosses and
//! heat-set inserts" explains the insert).
use super::plates::escape;
use super::{K, V3, round, scale};
use crate::archive::ArchiveDocument;
use crate::geometry::resolved_brep;
use crate::ops::Ctx;
use base64::Engine;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The hex key for a socket head screw size.
pub fn hex_key(size: &str) -> &'static str {
    match size {
        "M2" => "1.5 mm",
        "M2.5" => "2 mm",
        "M3" => "2.5 mm",
        "M4" => "3 mm",
        "M5" => "4 mm",
        _ => "hex",
    }
}

fn name_of(doc: &ArchiveDocument, id: &str) -> String {
    doc.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
}

/// A split group's record and its piece nodes in piece order.
fn split_of(doc: &ArchiveDocument, group: &str) -> Result<(Value, Vec<String>), String> {
    let g = doc.node(group).ok_or_else(|| format!("{group:?} is not a split group (the group Split for printing made)"))?;
    let split = g["robot"]["print_split"].clone();
    if !split.is_object() {
        return Err(format!("{} is not a split for printing (no print_split record)", name_of(doc, group)));
    }
    let mut pieces: Vec<(i64, String)> = g["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|c| doc.node(c).and_then(|n| n["robot"]["print_piece"]["index"].as_i64()).map(|i| (i, c.to_string())))
        .collect();
    pieces.sort();
    Ok((split, pieces.into_iter().map(|p| p.1).collect()))
}

fn size_of(j: &Value) -> String {
    j["spec"]["size"].as_str().unwrap_or("M3").to_string()
}

fn screw_len(j: &Value) -> String {
    format!("{} × {} mm", size_of(j), j["spec"]["screw_length_mm"].as_f64().unwrap_or(0.0))
}

fn pin_size(j: &Value) -> String {
    j["hardware"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|h| h["item"] == "steel dowel pin")
        .and_then(|h| h["size"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("Ø{} mm", j["spec"]["diameter_mm"].as_f64().unwrap_or(0.0)))
}

fn idx(v: &Value) -> usize {
    v.as_u64().unwrap_or(0) as usize
}

fn joints(s: &Value) -> impl Iterator<Item = &Value> {
    s["joints"].as_array().into_iter().flatten()
}

/// The way a piece goes on: along its dovetail tabs or rail, else straight
/// along the seam normal, away from the piece it joins.
fn direction(new: usize, links: &[&Value]) -> Value {
    for x in links {
        for j in joints(x) {
            if j["kind"] == "dovetail" {
                return j["spec"]["along"].clone();
            }
        }
    }
    let x = links[0];
    let n: Vec<f64> = x["normal"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    if new == idx(&x["plus"]) { json!(n) } else { json!(n.iter().map(|v| -v).collect::<Vec<_>>()) }
}

fn step(number: usize, title: String, text: String, piece: Option<usize>, direction: Option<Value>, hardware: Vec<Value>) -> Value {
    json!({"number": number, "title": title, "text": text, "piece": piece, "direction": direction, "hardware": hardware})
}

/// The order, steps, hardware and tools for a split group.
pub fn plan_assembly(doc: &ArchiveDocument, k: &K, group: &str) -> Result<Value, String> {
    let (split, piece_ids) = split_of(doc, group)?;
    if piece_ids.is_empty() {
        return Err(format!("{} has no pieces", name_of(doc, group)));
    }
    let mut volumes = Vec::new();
    for p in &piece_ids {
        volumes.push(k.volume_centroid(&resolved_brep(doc, p)?)?.0);
    }
    let names: Vec<String> = piece_ids.iter().map(|p| name_of(doc, p)).collect();
    let seams: Vec<Value> = split["seams"].as_array().cloned().unwrap_or_default();
    let mut steps: Vec<Value> = Vec::new();
    let mut n = 0;
    // Preparation per piece: inserts into the minus side, pins pressed into the minus side.
    let mut prep: BTreeMap<(usize, String), Vec<&Value>> = BTreeMap::new();
    for s in &seams {
        for j in joints(s) {
            let kind = j["kind"].as_str().unwrap_or("");
            if kind == "insert_screw" || kind == "dowel" {
                prep.entry((idx(&s["minus"]), kind.to_string())).or_default().push(j);
            }
        }
    }
    for ((piece, kind), js) in &prep {
        n += 1;
        let name = names.get(*piece).cloned().unwrap_or_default();
        if kind == "insert_screw" {
            let size = size_of(js[0]);
            steps.push(step(n, format!("Inserts into {name}"),
                format!("Press {} {size} heat-set insert(s) into the pockets on the cut face of {name} with a soldering iron at about 220–240 °C (PLA). Push straight and slowly until each is flush; let them cool before the next step.", js.len()),
                Some(*piece), None, vec![json!({"item": "heat-set insert", "size": size, "count": js.len()})]));
        } else {
            steps.push(step(n, format!("Pins into {name}"),
                format!("Press {} steel dowel pin(s) into the tight holes on the cut face of {name} (a vice or a gentle tap). They stand proud and locate the next piece.", js.len()),
                Some(*piece), None, vec![json!({"item": "steel dowel pin", "size": pin_size(js[0]), "count": js.len()})]));
        }
    }
    // Joining: largest piece first, then neighbours by seam, breadth first.
    let first = (0..volumes.len()).fold(0, |b, i| if volumes[i] > volumes[b] { i } else { b });
    let mut order = vec![first];
    let mut placed: BTreeSet<usize> = BTreeSet::from([first]);
    let mut todo: Vec<usize> = (0..seams.len()).collect();
    let mut joins: Vec<(usize, Vec<usize>)> = Vec::new();
    while placed.len() < piece_ids.len() {
        let mut grew = false;
        for t in todo.clone() {
            let (a, b) = (idx(&seams[t]["minus"]), idx(&seams[t]["plus"]));
            if placed.contains(&a) != placed.contains(&b) {
                let new = if placed.contains(&a) { b } else { a };
                let links: Vec<usize> = (0..seams.len())
                    .filter(|x| {
                        let (m, p) = (idx(&seams[*x]["minus"]), idx(&seams[*x]["plus"]));
                        (m == new || p == new) && [m, p].iter().all(|q| *q == new || placed.contains(q))
                    })
                    .collect();
                placed.insert(new);
                order.push(new);
                todo.retain(|x| !links.contains(x));
                joins.push((new, links));
                grew = true;
                break;
            }
        }
        if !grew {
            let missing: Vec<&str> = (0..piece_ids.len()).filter(|i| !placed.contains(i)).map(|i| names[i].as_str()).collect();
            return Err(format!("these pieces share no seam with the rest: {}", missing.join(", ")));
        }
    }
    n += 1;
    steps.push(step(n, format!("Start with {}", names[first]), format!("Lay {} down with its cut faces up or out: the other pieces join onto it.", names[first]), Some(first), None, Vec::new()));
    for (new, links) in &joins {
        n += 1;
        let links: Vec<&Value> = links.iter().map(|x| &seams[*x]).collect();
        let all: Vec<&Value> = links.iter().flat_map(|x| joints(x)).collect();
        let tabs: Vec<&&Value> = all.iter().filter(|j| j["kind"] == "dovetail").collect();
        let screws: Vec<&&Value> = all.iter().filter(|j| j["kind"] == "insert_screw").collect();
        let pins = all.iter().filter(|j| j["kind"] == "dowel").count();
        let mut how = Vec::new();
        let jigsaw = tabs.iter().filter(|j| j["notes"].as_array().into_iter().flatten().any(|n| n.as_str().is_some_and(|s| s.contains("jigsaw")))).count();
        if jigsaw > 0 {
            how.push(format!("lower it straight in so its {jigsaw} dovetail tab(s) drop into the matching slots"));
        } else if !tabs.is_empty() {
            how.push("slide it along the dovetail rail from the open end".into());
        } else {
            how.push("push it straight onto the cut face".into());
        }
        if pins > 0 {
            how.push(format!("{pins} pin(s) line it up"));
        }
        let mut text = format!("Fit {}: {}. Check the seam closes all the way, with no gap.", names[*new], how.join("; "));
        let mut hw = Vec::new();
        if !screws.is_empty() {
            let size = size_of(screws[0]);
            let lengths: BTreeSet<String> = screws.iter().map(|j| screw_len(j)).collect();
            text += &format!(
                " Then drive {} {size} screw(s) ({}) through the counterbores or side pockets into the inserts with a {} hex key. Snug, not tight: the insert holds far more than a plastic thread, but the head bears on plastic.",
                screws.len(),
                lengths.iter().cloned().collect::<Vec<_>>().join(", "),
                hex_key(&size)
            );
            for l in &lengths {
                hw.push(json!({"item": "socket head screw", "size": l, "count": screws.iter().filter(|j| screw_len(j) == *l).count()}));
            }
        }
        steps.push(step(n, format!("Add {}", names[*new]), text, Some(*new), Some(direction(*new, &links)), hw));
    }
    let mut tools: Vec<String> = Vec::new();
    let every: Vec<&Value> = seams.iter().flat_map(joints).collect();
    if every.iter().any(|j| j["kind"] == "insert_screw") {
        let sizes: BTreeSet<String> = every.iter().filter(|j| j["kind"] == "insert_screw").map(|j| size_of(j)).collect();
        tools.push("soldering iron with an insert tip (220–240 °C for PLA)".into());
        tools.extend(sizes.iter().map(|z| format!("{} hex key ({z})", hex_key(z))));
    }
    if every.iter().any(|j| j["kind"] == "dowel") {
        tools.push("vice or small hammer (pressing pins)".into());
    }
    Ok(json!({
        "group": group,
        "pieces": piece_ids.iter().enumerate().map(|(i, pid)| json!({"index": i, "node": pid, "name": names[i], "volume_mm3": round(volumes[i], 1)})).collect::<Vec<_>>(),
        "order": order, "steps": steps, "hardware": split["hardware"], "tools": tools,
    }))
}

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// Offsets for an exploded view: each added piece moves back along its way
/// on, further for later steps. Keyed by piece node.
pub fn exploded_offsets(plan: &Value, spacing: f64) -> BTreeMap<String, V3> {
    let mut offsets = BTreeMap::new();
    let mut depth = 0;
    for s in plan["steps"].as_array().into_iter().flatten() {
        if let (Some(d), Some(piece)) = (v3(&s["direction"]), s["piece"].as_u64()) {
            depth += 1;
            if let Some(pid) = plan["pieces"][piece as usize]["node"].as_str() {
                offsets.insert(pid.to_string(), scale(d, spacing * (1.0 + 0.5 * (depth - 1) as f64)));
            }
        }
    }
    for p in plan["pieces"].as_array().into_iter().flatten() {
        if let Some(pid) = p["node"].as_str() {
            offsets.entry(pid.to_string()).or_insert([0.0; 3]);
        }
    }
    offsets
}

/// Instances of the pieces, offset: an exploded view that follows the
/// pieces, added to a staged edit. The new group's id.
pub fn add_exploded_view(cx: &mut Ctx, plan: &Value, name: Option<&str>) -> Result<String, String> {
    let group = plan["group"].as_str().ok_or("plan has no group")?;
    let gname = cx.node(group)?["name"].as_str().unwrap_or(group).to_string();
    let parent = cx.node(group)?["parent"].as_str().map(str::to_string);
    let g = cx.add_node("group", name.unwrap_or(&format!("{gname}: exploded")), parent.as_deref(), Map::new())?;
    for (pid, off) in exploded_offsets(plan, 35.0) {
        let src = cx.node(&pid)?.clone();
        let mut extra = Map::new();
        extra.insert("source".into(), json!(pid));
        extra.insert("material".into(), src["material"].clone());
        extra.insert("transform".into(), json!({"translation": off, "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}));
        cx.add_node("instance", &format!("{} (exploded)", src["name"].as_str().unwrap_or(&pid)), Some(&g), extra)?;
    }
    Ok(g)
}

const PALETTE: [[f32; 3]; 6] = [[0.95, 0.6, 0.25], [0.3, 0.55, 0.9], [0.4, 0.8, 0.45], [0.85, 0.4, 0.6], [0.7, 0.65, 0.3], [0.5, 0.45, 0.85]];

/// `assembly.json` and a self-contained `assembly.html` (a picture per step); the page's path.
pub fn write_guide(doc: &ArchiveDocument, k: &K, plan: &Value, out_dir: &Path, title: Option<&str>, images: bool) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let offsets = exploded_offsets(plan, 35.0);
    let pieces: Vec<String> = plan["pieces"].as_array().into_iter().flatten().filter_map(|p| p["node"].as_str().map(str::to_string)).collect();
    let mut pics: BTreeMap<u64, String> = BTreeMap::new();
    if images {
        // Each piece tessellated once; a step moves its piece's vertices.
        let mut meshes = BTreeMap::new();
        for (i, pid) in pieces.iter().enumerate() {
            let g = k.geometry(&resolved_brep(doc, pid)?, 0.5)?;
            meshes.insert(pid.clone(), (i, g));
        }
        let mut present: Vec<String> = Vec::new();
        for s in plan["steps"].as_array().into_iter().flatten() {
            let start = s["title"].as_str().is_some_and(|t| t.starts_with("Start"));
            let Some(piece) = s["piece"].as_u64() else { continue };
            if s["direction"].is_null() && !start {
                continue;
            }
            if (k.cancelled)() {
                return Err("cancelled".into());
            }
            let pid = pieces[piece as usize].clone();
            if !present.contains(&pid) {
                present.push(pid.clone());
            }
            let moving = !s["direction"].is_null();
            let bodies: Vec<sim_render::cad::Body> = present
                .iter()
                .map(|q| {
                    let (i, g) = &meshes[q];
                    let off = if *q == pid && moving { offsets[q] } else { [0.0; 3] };
                    sim_render::cad::Body {
                        id: q.clone(),
                        name: name_of(doc, q),
                        vertices: g.vertices_mm.iter().map(|v| [v[0] + off[0], v[1] + off[1], v[2] + off[2]]).collect(),
                        triangles: g.triangles.clone(),
                        triangle_face: g.triangle_faces.clone(),
                        color: if *q == pid { PALETTE[i % PALETTE.len()] } else { [0.72, 0.72, 0.74] },
                    }
                })
                .collect();
            let o = sim_render::cad::Options { width: 900, height: 620, view: [-0.9, -1.2, 1.0], mode: "shaded".into(), section: None, highlight: Vec::new(), labels: false, edges: false, focus: Vec::new(), title: None };
            let png = sim_render::cad::render(&bodies, &o)?;
            pics.insert(s["number"].as_u64().unwrap_or(0), base64::engine::general_purpose::STANDARD.encode(png));
        }
    }
    let json_path = out_dir.join("assembly.json");
    std::fs::write(&json_path, serde_json::to_string_pretty(plan).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", json_path.display()))?;
    let group = plan["group"].as_str().unwrap_or("");
    let title = title.map(str::to_string).unwrap_or_else(|| format!("Assembling {}", name_of(doc, group)));
    let li = |items: Vec<String>| if items.is_empty() { "<li>none</li>".to_string() } else { items.into_iter().map(|t| format!("<li>{t}</li>")).collect() };
    let rows = li(plan["hardware"].as_array().into_iter().flatten().map(|h| format!("{} × {} {}", h["count"], escape(h["item"].as_str().unwrap_or("")), escape(h["size"].as_str().unwrap_or("")))).collect());
    let tools = li(plan["tools"].as_array().into_iter().flatten().filter_map(Value::as_str).map(escape).collect());
    let steps: String = plan["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let num = s["number"].as_u64().unwrap_or(0);
            let img = pics.get(&num).map_or(String::new(), |b| format!("<img alt=\"step {num}\" src=\"data:image/png;base64,{b}\">"));
            format!("<section class=\"step\"><h2>{num}. {}</h2><p>{}</p>{img}</section>", escape(s["title"].as_str().unwrap_or("")), escape(s["text"].as_str().unwrap_or("")))
        })
        .collect();
    let count = pieces.len();
    let page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t}</title><style>
:root{{--bg:#fafaf8;--ink:#1d2025;--muted:#5b6270;--card:#fff;--line:#e3e2de}}
@media (prefers-color-scheme:dark){{:root{{--bg:#16181c;--ink:#e8e8e6;--muted:#a4a9b3;--card:#1f2228;--line:#30343c}}}}
body{{margin:0;background:var(--bg);color:var(--ink);font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}}
main{{max-width:880px;margin:0 auto;padding:24px 16px}} h1{{font-size:1.6rem;margin:0 0 4px}} .muted{{color:var(--muted)}}
.cols{{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:16px;margin:16px 0}}
.card,.step{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 18px}} .step{{margin:14px 0}}
.step h2{{font-size:1.1rem;margin:0 0 6px}} img{{width:100%;height:auto;border-radius:6px;margin-top:8px;background:#20242a}}
</style></head><body><main><h1>{t}</h1>
<p class="muted">{count} printed pieces. Order: largest piece first, then each neighbour; inserts and pins go in before their piece is joined.</p>
<div class="cols"><div class="card"><h3>Hardware</h3><ul>{rows}</ul></div><div class="card"><h3>Tools</h3><ul>{tools}</ul></div></div>
{steps}</main></body></html>"#,
        t = escape(&title)
    );
    let path = out_dir.join("assembly.html");
    std::fs::write(&path, page).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_steps_explode_further() {
        let plan = json!({
            "pieces": [{"node": "a"}, {"node": "b"}, {"node": "c"}],
            "steps": [
                {"number": 1, "piece": 0, "direction": null},
                {"number": 2, "piece": 1, "direction": [1.0, 0.0, 0.0]},
                {"number": 3, "piece": 2, "direction": [0.0, 0.0, 1.0]},
            ],
        });
        let o = exploded_offsets(&plan, 10.0);
        assert_eq!(o["a"], [0.0; 3]);
        assert_eq!(o["b"], [10.0, 0.0, 0.0]);
        assert_eq!(o["c"], [0.0, 0.0, 15.0]);
    }
}
