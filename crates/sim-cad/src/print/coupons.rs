//! Test coupons: small prints you break to measure what the registry
//! estimates (RoboCAD's `print_coupons.py`).
//!
//! Material coupons are printed solid (the registry's strengths are for
//! solid material): a tensile bar lying flat (pulled along its layers) and
//! one standing on end (pulled across them). Joint coupons copy a split's
//! real joints where given: a dowel pin in a lap joint (pin bearing), a
//! heat-set insert in a boss pulled along the layers (the insert lesson's
//! test), and a dovetail tab in its slot. Every coupon has a loading hole at
//! each end for a bolt or S-hook.
//!
//! [`make_coupons`] returns coupon bodies with their print direction and
//! settings; [`write_coupon_kit`] lays them on plates (3MF), writes the
//! results template for `sim-print promote` and a short test protocol.
use super::plates::{PlanPiece, write_plates};
use super::{K, Registry, V3, add};
use serde_json::{Map, Value, json};
use std::path::Path;

/// Solid print settings for material coupons.
pub fn solid_settings() -> Value {
    json!({"walls": 3, "infill": 1.0, "pattern": "gyroid", "layer_height": 0.2, "top_bottom_layers": 5})
}

pub struct Coupon {
    pub kind: String,
    pub name: String,
    pub body: Vec<u8>,
    pub build_direction: V3,
    pub settings: Value,
    pub geometry: Value,
    pub how: String,
    pub notes: Vec<String>,
}

/// A flat outline (x, y) extruded up by `thickness` from z = 0.
fn plate_xy(k: &K, outline: &[[f64; 2]], thickness: f64) -> Result<Vec<u8>, String> {
    k.extrude(outline.iter().map(|p| [p[0], p[1], 0.0]).collect(), [0.0, 0.0, thickness])
}

/// Vertical loading holes at `xs` along y = `y`.
fn holes(k: &K, mut body: Vec<u8>, xs: &[f64], y: f64, d: f64, height: f64) -> Result<Vec<u8>, String> {
    for x in xs {
        body = k.subtract(&body, &k.cylinder([*x, y, -1.0], [0.0, 0.0, 1.0], d / 2.0, height + 2.0)?)?;
    }
    Ok(body)
}

/// A box from a corner and a size (RoboCAD's `box`).
fn bx(k: &K, corner: V3, size: V3) -> Result<Vec<u8>, String> {
    k.cuboid(corner, add(corner, size))
}

/// A flat tensile bar along x: (body, gauge area mm²).
#[allow(clippy::too_many_arguments)]
pub fn dogbone(k: &K, gauge_len: f64, gauge_w: f64, thick: f64, grip_w: f64, grip_len: f64, taper: f64, hole: f64) -> Result<(Vec<u8>, f64), String> {
    let l = 2.0 * grip_len + 2.0 * taper + gauge_len;
    let (g0, g1) = (grip_len + taper, grip_len + taper + gauge_len);
    let (hw, gw) = (grip_w / 2.0, gauge_w / 2.0);
    let outline = [[0.0, -hw], [grip_len, -hw], [g0, -gw], [g1, -gw], [l - grip_len, -hw], [l, -hw], [l, hw], [l - grip_len, hw], [g1, gw], [g0, gw], [grip_len, hw], [0.0, hw]];
    let b = plate_xy(k, &outline, thick)?;
    let b = holes(k, b, &[grip_len / 2.0, l - grip_len / 2.0], 0.0, hole, thick)?;
    Ok((b, gauge_w * thick))
}

fn joint_default_settings() -> Value {
    json!({"walls": 3, "infill": 0.15, "pattern": "gyroid", "layer_height": 0.2, "top_bottom_layers": 5})
}

/// The coupons: material bars, then pin, insert and dovetail coupons (from
/// the split's joints where given).
pub fn make_coupons(k: &K, reg: &Registry, split: Option<&Value>, part_settings: Option<&Value>, copies: Option<usize>) -> Result<Vec<Coupon>, String> {
    let copies = copies.unwrap_or_else(|| reg.loaded.registry.test_plan.get("min_samples").and_then(Value::as_u64).unwrap_or(3) as usize);
    let joint_settings = part_settings.cloned().unwrap_or_else(joint_default_settings);
    let mut out = Vec::new();
    let coupon = |kind: &str, name: String, body: Vec<u8>, settings: &Value, geometry: Value, how: String, notes: Vec<String>| Coupon {
        kind: kind.into(),
        name,
        body,
        build_direction: [0.0, 0.0, 1.0],
        settings: settings.clone(),
        geometry,
        how,
        notes,
    };
    // Material: in-layer (flat) and across-layer (standing) tensile bars.
    let (bar, area) = dogbone(k, 60.0, 10.0, 4.0, 20.0, 30.0, 12.0, 6.5)?;
    for i in 0..copies {
        out.push(coupon("tensile_in_layer", format!("tensile flat {}", i + 1), bar.clone(), &solid_settings(), json!({"gauge_area_mm2": area}),
            "Hang it from one hole and load the other until it breaks; the break should be in the narrow middle.".into(), Vec::new()));
    }
    let (tall, area_t) = dogbone(k, 50.0, 10.0, 6.0, 22.0, 25.0, 10.0, 6.5)?;
    for i in 0..copies {
        let mut c = coupon("tensile_across_layers", format!("tensile standing {}", i + 1), tall.clone(), &solid_settings(), json!({"gauge_area_mm2": area_t}),
            "Printed standing on one end (use a brim). Load it like the flat bar; it breaks between two layers.".into(),
            vec!["stands 120 mm tall on a 22 × 6 mm end: add a brim in the slicer".into()]);
        c.build_direction = [1.0, 0.0, 0.0];
        out.push(c);
    }
    // Joints: from the split's joints where given, else registry defaults.
    let joints: Vec<&Value> = split.and_then(|s| s["seams"].as_array()).into_iter().flatten().flat_map(|s| s["joints"].as_array().into_iter().flatten()).collect();
    let find = |kind: &str| joints.iter().find(|j| j["kind"] == kind).copied();
    let dowel = find("dowel");
    let d = dowel.and_then(|j| j["spec"]["diameter_mm"].as_f64()).unwrap_or(4.0);
    let engaged = dowel.map_or(6.0, |j| j["spec"]["depth_minus_mm"].as_f64().unwrap_or(6.0).min(j["spec"]["depth_plus_mm"].as_f64().unwrap_or(6.0)));
    let slip = reg.number("dowel_pin.slip_clearance_mm")?;
    let press = reg.number("dowel_pin.press_clearance_mm")?;
    for i in 0..copies {
        if (k.cancelled)() {
            return Err("cancelled".into());
        }
        // Two lapped straps: the pin crosses the lap; pulling the straps apart loads it in bearing.
        let a = bx(k, [0.0, -10.0, 0.0], [50.0, 20.0, engaged])?;
        let a = k.subtract(&a, &k.cylinder([40.0, 0.0, -1.0], [0.0, 0.0, 1.0], (d + press) / 2.0, engaged + 2.0)?)?;
        let a = holes(k, a, &[8.0], 0.0, 6.5, engaged)?;
        let b = bx(k, [30.0, -10.0, engaged + 0.4], [50.0, 20.0, engaged])?;
        let b = k.subtract(&b, &k.cylinder([40.0, 0.0, engaged - 1.0], [0.0, 0.0, 1.0], (d + slip) / 2.0, engaged + 2.0)?)?;
        let b = holes(k, k.translate(&b, [0.0, 0.0, -(engaged + 0.4)])?, &[72.0], 0.0, 6.5, engaged)?;
        let b = k.translate(&b, [0.0, 30.0, 0.0])?;
        out.push(coupon("pin_shear", format!("pin lap {} (two straps)", i + 1), k.join(&[&a, &b])?, &joint_settings, json!({"pin_diameter_mm": d, "engaged_mm": engaged}),
            format!("Press a Ø{d} pin into the tight hole of the short strap, lay the long strap over it, and pull the straps apart along their length."), Vec::new()));
    }
    let size = find("insert_screw").and_then(|j| j["spec"]["size"].as_str()).unwrap_or("M3").to_string();
    let geo = reg.insert(&size)?;
    for i in 0..copies {
        if (k.cancelled)() {
            return Err("cancelled".into());
        }
        let base = bx(k, [-20.0, -20.0, 0.0], [40.0, 40.0, 5.0])?;
        let base = holes(k, base, &[-13.0, 13.0], 0.0, 3.4, 5.0)?;
        let boss = k.cylinder([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], geo.knurl_mm, 15.0)?;
        let b = k.union(&base, &boss)?;
        let b = k.subtract(&b, &k.cylinder([0.0, 0.0, 20.1], [0.0, 0.0, -1.0], geo.hole_mm / 2.0, geo.depth_mm + 0.1)?)?;
        out.push(coupon("insert_pullout", format!("insert boss {size} {}", i + 1), b, &joint_settings, json!({"knurl_mm": geo.knurl_mm, "insert_length_mm": geo.length_mm, "size": size}),
            format!("Heat-set a {size} insert, screw the base down through its two holes, and pull a screw in the insert straight up (along the layers, as in the insert lesson)."), Vec::new()));
    }
    let tab = find("dovetail");
    let neck = tab.and_then(|j| j["spec"]["neck_mm"].as_f64()).unwrap_or(10.0);
    let depth = tab.and_then(|j| j["spec"]["depth_mm"].as_f64()).unwrap_or(9.0);
    let thick = tab.and_then(|j| j["spec"]["rail_length_mm"].as_f64()).map_or(5.0, |r| r.min(8.0));
    let angle = reg.number("dovetail.angle_deg")?.to_radians();
    let c = reg.number("dovetail.sliding_clearance_mm")?;
    for i in 0..copies {
        if (k.cancelled)() {
            return Err("cancelled".into());
        }
        let (w0, w1) = (neck / 2.0, neck / 2.0 + depth * angle.tan());
        // Tab strap along +x ending in the tail; slot strap receives it.
        let tab_outline = [[0.0, -12.0], [40.0, -12.0], [40.0, -w0], [40.0 + depth, -w1], [40.0 + depth, w1], [40.0, w0], [40.0, 12.0], [0.0, 12.0]];
        let t = holes(k, plate_xy(k, &tab_outline, thick)?, &[8.0], 0.0, 6.5, thick)?;
        let slot_outline = [[40.0 - 0.01, -12.0], [90.0, -12.0], [90.0, 12.0], [40.0 - 0.01, 12.0], [40.0 - 0.01, w0 + c], [40.0 + depth + c, w1 + c], [40.0 + depth + c, -w1 - c], [40.0 - 0.01, -w0 - c]];
        let sl = holes(k, plate_xy(k, &slot_outline, thick)?, &[82.0], 0.0, 6.5, thick)?;
        let sl = k.translate(&sl, [0.0, 30.0, 0.0])?;
        out.push(coupon("dovetail_pull", format!("dovetail tab {} (two straps)", i + 1), k.join(&[&t, &sl])?, &joint_settings,
            json!({"neck_mm": neck, "depth_mm": depth, "thickness_mm": thick, "angle_deg": angle.to_degrees()}),
            "Drop the tab into the slot and pull the straps apart along their length until the tab or slot breaks.".into(), Vec::new()));
    }
    Ok(out)
}

/// The results file `sim-print promote` reads (`sim.print-test/1`), with an
/// empty slot per coupon.
pub fn results_template(coupons: &[Coupon], material: &str, printer: &str, registry_sha256: &str, settings_note: Value) -> Value {
    let mut tests: Vec<(String, Value)> = Vec::new();
    for c in coupons {
        let i = match tests.iter().position(|(k, _)| *k == c.kind) {
            Some(i) => i,
            None => {
                let geometry: Map<String, Value> = c.geometry.as_object().into_iter().flatten().filter(|(k, _)| *k != "size").map(|(k, v)| (k.clone(), v.clone())).collect();
                tests.push((c.kind.clone(), json!({"coupon": c.kind, "failure_n": [], "geometry": geometry, "how_it_broke": [], "notes": ""})));
                tests.len() - 1
            }
        };
        let t = &mut tests[i].1;
        t["failure_n"].as_array_mut().expect("array").push(Value::Null);
        t["how_it_broke"].as_array_mut().expect("array").push(json!(""));
    }
    json!({"schema": "sim.print-test/1", "material": material, "printer": printer, "registry_sha256": registry_sha256, "settings": settings_note,
        "printed": "", "tested": "", "operator": "", "tests": tests.into_iter().map(|t| t.1).collect::<Vec<_>>()})
}

pub const PROTOCOL: &str = "# Coupon tests

Print the plates in this folder, then break each coupon and write its
breaking load (N) into `results.json` (one number per coupon, in order).

## What you need
- A luggage scale (to 50 kg / 500 N), or a bucket you fill with water and then weigh.
- An M6 bolt or S-hooks through the loading holes, and something solid to hang from.
- The hardware the joint coupons use (a dowel pin, a heat-set insert and screw).

## How
1. Hang the coupon from one loading hole. Hook the scale (or bucket) to the other.
2. Load slowly, over 10–30 s, until it breaks. Read the peak (the scale's max-hold, or weigh the bucket: 1 kg ≈ 9.81 N).
3. Write the load in `failure_n` and a word in `how_it_broke` (for example \"middle\", \"at a grip\", \"insert pulled out\", \"tab snapped\").
   Leave out breaks at a grip or hole: they measure the grip, not the material.
4. Break at least three of each kind. More gives a tighter design value (mean − 2·std).

## Recording
    target/release/sim-print promote results.json            # dry run: what would change
    target/release/sim-print promote results.json --write    # a new registry revision

The registry then says `measured`, with your breaks, their spread and the value it
replaced; every later strength check and plan uses the measured value.

## Safety
Breaking plastic can fling pieces: wear glasses, keep hands clear of the load path,
and keep the load low over the floor.
";

/// Coupons on plates, `results.json` and `PROTOCOL.md` in `out_dir`.
#[allow(clippy::too_many_arguments)]
pub fn write_coupon_kit(k: &K, reg: &Registry, out_dir: &Path, material: &str, printer: &str, split: Option<&Value>, part_settings: Option<&Value>, copies: Option<usize>) -> Result<Value, String> {
    let coupons = make_coupons(k, reg, split, part_settings, copies)?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let pieces: Vec<PlanPiece> = coupons
        .iter()
        .map(|c| PlanPiece { name: c.name.clone(), body: c.body.clone(), build_direction: c.build_direction, settings: c.settings.clone(), estimate: json!({}), safety_factor: None, source: Some(c.kind.clone()) })
        .collect();
    let plates = write_plates(k, reg, &pieces, out_dir, printer, material, None)?;
    let template = results_template(&coupons, material, printer, reg.sha256(), json!({"material_coupons": solid_settings(), "joint_coupons": part_settings.cloned().unwrap_or_else(|| json!("registry defaults"))}));
    let results = out_dir.join("results.json");
    std::fs::write(&results, serde_json::to_string_pretty(&template).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", results.display()))?;
    let mut protocol = PROTOCOL.to_string();
    protocol += "\n## Coupons\n";
    for c in &coupons {
        let notes = if c.notes.is_empty() { String::new() } else { format!(" _{}_", c.notes.join("; ")) };
        protocol += &format!("- **{}** ({}): {}{notes}\n", c.name, c.kind, c.how);
    }
    let protocol_path = out_dir.join("PROTOCOL.md");
    std::fs::write(&protocol_path, protocol).map_err(|e| format!("{}: {e}", protocol_path.display()))?;
    Ok(json!({
        "coupons": coupons.iter().map(|c| json!({"kind": c.kind, "name": c.name, "geometry": c.geometry, "build_direction": c.build_direction, "settings": c.settings})).collect::<Vec<_>>(),
        "plates": plates["plates"].as_array().into_iter().flatten().map(|p| p["file"].clone()).collect::<Vec<_>>(),
        "results_template": results.display().to_string(),
        "protocol": protocol_path.display().to_string(),
    }))
}
