//! The inspector's section texts (Link, Joints, Drives, Source) and the
//! watched file's identity block.
use super::*;

/// A typed provenance label spelled as the file stores it.
fn provenance_label(p: &impl Serialize) -> String {
    serde_json::to_value(p).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}
fn v3(v: &[f64; 3]) -> String {
    format!("[{:?}, {:?}, {:?}]", v[0], v[1], v[2])
}
/// A stored JSON note as text: strings verbatim, anything else as JSON.
fn verbatim(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}
fn or_none(s: &str) -> &str {
    if s.is_empty() { "(empty in file)" } else { s }
}

/// Selected link: stored mass properties, material and the file's own text.
pub(super) fn link_text(view: &RobotView, m: &PhysicalModel, link: Option<usize>) -> String {
    let Some((i, l)) = link.and_then(|i| Some((i, m.links.get(i)?))) else {
        return format!("Select a link in the list or the 3D view.\n\n{} links · {} joints · {} motors\n\nValues are shown exactly as stored (SI units, full precision).", m.links.len(), m.joints.len(), m.motors.len());
    };
    let mut t = format!("{}   (link {} of {})\n\n", l.name, i + 1, m.links.len());
    t += &format!("mass: {:?} kg\ncom: {} m (model frame, Z up)\ninertia about com (kg·m², model axes):\n", l.mass, v3(&l.com));
    for row in &l.inertia {
        t += &format!("  {}\n", v3(row));
    }
    t += &match (l.material.as_str(), m.materials.get(&l.material)) {
        ("", _) => "material: none recorded\n".to_string(),
        (name, Some(mat)) => format!("material: {name} — density {:?} kg/m³ (file's materials map)\n", mat.density),
        (name, None) => format!("material: {name} — not in the file's materials map; no density shown\n"),
    };
    t += &format!("ground: {}\n", if l.ground { "yes" } else { "no" });
    t += &match view.triangles.get(i).copied().unwrap_or(0) {
        0 => "collision: no geometry (not drawn)\n".to_string(),
        n => format!("collision: {n} display triangles\n"),
    };
    t += &format!("\nprovenance: {UNLABELLED}\n\nFILE'S OWN TEXT (verbatim; not a provenance label)\n");
    let notes = view.notes.links.get(i).cloned().unwrap_or(Value::Null);
    let names = notes.get("member_names").and_then(|n| n.as_array());
    let sources = notes.get("mass_sources").and_then(|s| s.as_object());
    if l.members.is_empty() && sources.is_none_or(|s| s.is_empty()) {
        t += "no members or mass_sources in file\n";
    }
    for (k, id) in l.members.iter().enumerate() {
        let name = names.and_then(|n| n.get(k)).map_or("(no member_name)".to_string(), verbatim);
        let source = sources.and_then(|s| s.get(id)).map_or("(no mass_source)".to_string(), |s| format!("\"{}\"", verbatim(s)));
        t += &format!("• {name} [{id}]\n   mass_source: {source}\n");
    }
    for (id, source) in sources.into_iter().flatten().filter(|(id, _)| !l.members.contains(id)) {
        t += &format!("• [{id}] (not a listed member)\n   mass_source: \"{}\"\n", verbatim(source));
    }
    let joints = touching(m, &l.name).count();
    t += &format!("\n{joints} joint(s) touch this link — see Joints.");
    t
}

/// Joints touching the selected link (all joints when none is selected).
pub(super) fn joints_text(view: &RobotView, m: &PhysicalModel, link: Option<usize>) -> String {
    let selected = link.and_then(|i| m.links.get(i));
    let joints: Vec<_> = match selected {
        Some(l) => touching(m, &l.name).collect(),
        None => m.joints.iter().enumerate().collect(),
    };
    let mut t = match selected {
        Some(l) => format!("Joints touching {} ({} of {})\n", l.name, joints.len(), m.joints.len()),
        None => format!("All {} joints (select a link to filter)\n", m.joints.len()),
    };
    t += &format!("SI units, as stored. Values without a label: {UNLABELLED}.\n");
    if let Some(r) = &view.run {
        t += &format!("\nJOG — {JOG_LABEL}\ncontrol mode (file): {}\n{}\n", m.control.mode, JOG_NOTE);
        if let Some(e) = r.jog_error() {
            t += &format!("last jog not applied: {e}\n");
        }
    }
    for (_, j) in joints {
        let p = &j.physics;
        let f = &p.friction;
        t += &format!("\n{} — {}\n  {} → {}\n  axis {} · origin {} m\n", j.name, j.kind, j.parent.as_deref().unwrap_or("(world)"), j.child, v3(&j.axis), v3(&j.origin));
        t += &match j.limits {
            Some([lo, hi]) => format!("  limits [{lo:?}, {hi:?}] · home {:?}\n", j.home),
            None => format!("  limits: none stored · home {:?}\n", j.home),
        };
        t += &format!("  friction: coulomb {:?}, viscous {:?}, stribeck {:?}, stribeck_speed {:?}, static_ratio {:?}\n", f.coulomb, f.viscous, f.stribeck, f.stribeck_speed, f.static_ratio);
        t += &format!("  clearance {:?} m · backlash {:?} rad · wobble {:?} · damping_ratio {:?}\n", p.clearance, p.backlash, p.wobble, p.damping_ratio);
        t += &match &p.drive_backlash {
            Some(b) => format!(
                "  drive_backlash: width {} rad, uncertainty {} rad\n    provenance: {} (typed label in file)\n    reference: \"{}\"\n",
                b.width_rad.map_or("none".into(), |w| format!("{w:?}")),
                b.uncertainty_rad.map_or("none".into(), |w| format!("{w:?}")),
                provenance_label(&b.provenance),
                b.reference
            ),
            None => "  drive_backlash: none stored\n".to_string(),
        };
        t += &format!("  physics.source (file's text): \"{}\"\n  motor: {}\n", or_none(&p.source), j.motor.as_deref().unwrap_or("none"));
        if let Some(r) = &view.run {
            t += &format!("  servo: {}\n", jog_line(r, &j.name));
        }
    }
    t
}

const JOG_NOTE: &str = "Jogging while paused sets the target; it takes effect when running or stepping. Before the first build it is queued and applied after the build. Reset returns every target to the file's control targets. Nothing is written.";

/// A joint's servo state from the latest accepted frame, or why it has none.
pub(super) fn jog_line(r: &RunController, joint: &str) -> String {
    match crate::robot::run::servo(r.model(), joint) {
        Err(e) => format!("none — {e}"),
        Ok(s) => {
            let latest = r.frame().and_then(|f| f.servo(joint));
            let now = latest.map_or("target — · measured — (no frame yet)".to_string(), |(t, a)| format!("target {t:.3} · measured {a:.3} {}", s.unit));
            let requested = r.requested_target(&s);
            let pending = if latest.is_none_or(|(t, _)| t != requested) { format!(" · requested {requested:.3}") } else { String::new() };
            format!("{now}{pending} · {}", s.limit_text())
        }
    }
}

/// Motors, transmissions, battery and actuator profiles.
pub(super) fn drives_text(view: &RobotView, m: &PhysicalModel) -> String {
    let mut t = format!("SI units, as stored. Motor, transmission and battery values: {UNLABELLED}.\n\nMOTORS ({})\n", m.motors.len());
    for (i, x) in m.motors.iter().enumerate() {
        let (e, g, fw) = (&x.electrical, &x.gearbox, &x.firmware);
        t += &format!("• {} — spec {} · joint {} · gear_ratio {:?}\n", x.name, or_none(&x.spec), x.joint.as_deref().unwrap_or("none"), x.gear_ratio);
        t += &format!("   R {:?} Ω · L {:?} H · kt {:?} · ke {:?} · supply {:?} V · limit {:?} A\n", e.resistance, e.inductance, e.torque_constant, e.back_emf_constant, e.supply_voltage, e.current_limit);
        t += &format!("   gearbox ratio {:?} · efficiency {:?} · backlash {:?} rad · max torque {:?} · max speed {:?}\n", g.ratio, g.efficiency, g.backlash_rad, g.max_output_torque, g.max_output_speed);
        t += &format!("   firmware {} · {:?} Hz · latency {:?} s · kp {:?} ki {:?} kd {:?}\n", fw.kind, fw.loop_rate_hz, fw.latency_s, fw.kp, fw.ki, fw.kd);
        if let Some(n) = view.notes.motors.get(i).filter(|n| !n.is_null()) {
            t += &format!("   notes (file's text): \"{}\"\n", verbatim(n));
        }
    }
    t += &format!("\nTRANSMISSIONS ({})\n", m.transmissions.len());
    for x in &m.transmissions {
        t += &format!("• {}: {} = {:?} × {}\n", x.name, x.driver_joint, x.ratio, x.driven_joint);
    }
    t += &match &m.battery {
        Some(b) => format!("\nBATTERY\n  cells {:?} · nominal {:?} V · R {:?} Ω · {:?} Ah · soc {:?} · cutoff {:?} V\n", b.cells, b.nominal_voltage, b.internal_resistance, b.capacity_ah, b.initial_soc, b.cutoff_voltage),
        None => "\nBATTERY: none in file\n".to_string(),
    };
    match &m.actuator_profiles {
        None => t += "\nACTUATOR PROFILES: none in file\n",
        Some(p) => {
            t += &format!("\nACTUATOR PROFILES (v{}) — {} bindings\n", p.version, p.bindings.len());
            for (key, fam) in &p.families {
                t += &format!("• {key} v{} — content hash {}\n  \"{}\"\n", fam.version, fam.content_hash(), fam.description);
                let bound = p.bindings.values().filter(|b| &b.family == key).count();
                t += &format!("  bound to {bound} motor(s); provenance per parameter (typed label in file):\n");
                for (group, params) in [("motor", &fam.motor), ("driver", &fam.driver)] {
                    for (name, x) in params {
                        let u = x.uncertainty.map_or("unknown".into(), |u| format!("{u:?}"));
                        t += &format!("   {group}.{name} = {:?} {} — {}, ± {u}\n", x.value, x.unit, provenance_label(&x.provenance));
                    }
                }
                for l in &fam.limitations {
                    t += &format!("  limitation (file's text): \"{l}\"\n");
                }
            }
        }
    }
    t
}

/// The source block verbatim, the CAD link status, uncertainty and identification.
pub(super) fn source_text(view: &RobotView, m: &PhysicalModel) -> String {
    let src = &m.source;
    let field = |k: &str| src.get(k).map_or("not recorded".to_string(), verbatim);
    let mut t = view.source.as_ref().map(file_watch_text).unwrap_or_default();
    t += &format!("SOURCE BLOCK (verbatim)\nfile: {}\nexported: {}\ncad_sha256: {}\ncollision_ray_backend: {}\n", field("file"), field("exported"), field("cad_sha256"), field("collision_ray_backend"));
    t += "\nNOTES — benchmark_assumptions (verbatim)\n";
    match src.get("benchmark_assumptions") {
        None => t += "none recorded\n",
        Some(Value::Object(map)) => map.iter().for_each(|(k, v)| t += &format!("• {k}: {}\n", verbatim(v))),
        Some(Value::Array(items)) => items.iter().for_each(|v| t += &format!("• {}\n", verbatim(v))),
        Some(v) => t += &format!("{}\n", verbatim(v)),
    }
    let shown = ["file", "exported", "cad_sha256", "collision_ray_backend", "benchmark_assumptions"];
    for (k, v) in src.as_object().into_iter().flatten().filter(|(k, _)| !shown.contains(&k.as_str())) {
        t += &format!("{k}: {}\n", verbatim(v));
    }
    t += "\nCAD LINK\n";
    t += &match &view.cad_link {
        None => "not computed".to_string(),
        Some(CadLinkStatus::Current { path, sha256, .. }) => format!("current — {} matches the recorded sha256\n  {sha256}", path.display()),
        Some(CadLinkStatus::Stale { path, recorded_sha256, on_disk_sha256, .. }) => format!("stale — {} changed since export\n  recorded {recorded_sha256}\n  on disk  {on_disk_sha256}", path.display()),
        Some(CadLinkStatus::Missing { file, .. }) => format!("missing — no file found for \"{file}\""),
        Some(CadLinkStatus::NoRecordedHash { path, on_disk_sha256, .. }) => format!("no recorded hash — {} exists, but the export has no cad_sha256, so it cannot be compared\n  on disk {on_disk_sha256}", path.display()),
        Some(CadLinkStatus::NoSourceFile) => "no source file recorded in the export".to_string(),
        Some(CadLinkStatus::Unreadable { path, error, .. }) => format!("unreadable — {}: {error}", path.display()),
    };
    let tried = match &view.cad_link {
        Some(CadLinkStatus::Current { tried, .. } | CadLinkStatus::Stale { tried, .. } | CadLinkStatus::Missing { tried, .. } | CadLinkStatus::NoRecordedHash { tried, .. } | CadLinkStatus::Unreadable { tried, .. }) => tried.as_slice(),
        _ => &[],
    };
    if !tried.is_empty() {
        t += "\n  tried:";
        for p in tried {
            t += &format!("\n   {}", p.display());
        }
    }
    t += &format!("\n  rule: {}\n", cad_link::RESOLUTION_RULE);
    t += "\nUNCERTAINTY (as stored)\n";
    match &view.notes.uncertainty {
        Value::Object(map) => map.iter().for_each(|(k, v)| t += &format!("  {k}: {v}\n")),
        Value::Null => t += "none in file\n",
        v => t += &format!("  {v}\n"),
    }
    t += &format!("\nIDENTIFICATION ({})\n", m.identification.len());
    if m.identification.is_empty() {
        t += "none in file\n";
    }
    for (k, x) in &m.identification {
        t += &format!("• {k}: rms {:?} rad · fitted {} · log {}\n   {}\n", x.rms_error_rad, or_none(&x.fitted_at), or_none(&x.source_log), serde_json::to_string(x).unwrap_or_default());
    }
    t += &format!("\nPROVENANCE RULE\n{PROVENANCE_RULE}\n");
    t
}

/// The opened file's identity and last reload (robot_state.source_file).
pub(super) fn file_watch_text(s: &SourceWatch) -> String {
    let mut t = format!("FILE (watched every {:.1} s; Reload re-reads it)\n{}\nsha256 {}\nloaded at {} · {} reload(s)\n", source::POLL.as_secs_f64(), s.path.display(), s.hash.as_deref().unwrap_or("—"), s.loaded_at.as_deref().unwrap_or("—"), s.reload_count);
    if let Some(r) = &s.last {
        t += &format!("last reload: {:?} · {} at {}\n", r.trigger, r.outcome, r.at).to_lowercase();
    }
    if let Some(e) = &s.failing {
        t += &format!("SHOWING THE LAST GOOD MODEL — the file on disk does not load:\n{e}\n");
    }
    t.push('\n');
    t
}
