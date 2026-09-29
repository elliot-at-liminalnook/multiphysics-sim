//! Qualification of the voxel stress check against analytic cases, and of
//! the registry and joint checks. These run in CI (`cargo test -p sim-print`).

use sim_print::analyze::{self, Inputs};
use sim_print::joints::{self, Joint, SeamLoad};
use sim_print::mesh::Mesh;
use sim_print::registry::{self, Registry};
use sim_print::study::{Fixture, Load, Magnitude, PartStudy, Region, Section};
use sim_print::voxel::Settings;

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/printing/registry.json");
    registry::load(&path).unwrap().registry
}

/// Solid (100 % infill) so the analytic cases compare with homogeneous material.
fn solid() -> Settings {
    Settings { walls: 3, infill: 1.0, pattern: "gyroid".into(), layer_height: 0.2, top_bottom_layers: 5 }
}

fn beam_part(length: f64, load: f64, direction: [f64; 3], build: [f64; 3], sections: Vec<Section>) -> (Mesh, PartStudy) {
    let mesh = Mesh::cuboid([0., 0., 0.], [length, 10., 10.]);
    let part = PartStudy {
        name: "beam".into(),
        mesh: String::new(),
        build_direction: build,
        settings: solid(),
        fixtures: vec![Fixture { name: "root".into(), region: Region::Below { axis: [1., 0., 0.], height: 0.01 } }],
        loads: vec![Load { name: "tip".into(), region: Region::Box { min: [length - 0.01, -1., -1.], max: [length + 1., 11., 11.] }, direction, magnitude: Magnitude::Newtons(load), moment: None, about: None }],
        acceleration: None,
        sections,
        seams: vec![],
        directions: None,
    };
    (mesh, part)
}

fn run(reg: &Registry, mesh: &Mesh, part: &PartStudy, h: f64) -> analyze::PartResult {
    let printer = reg.printer("bambu-h2c").unwrap();
    let material = reg.material("pla-basic").unwrap();
    let magnitudes: Vec<(f64, String)> = part.loads.iter().map(|l| match l.magnitude { Magnitude::Newtons(n) => (n, "test".into()), _ => unreachable!() }).collect();
    let inputs = Inputs { registry: reg, printer, material, mesh, part, magnitudes: &magnitudes, build_direction: part.build_direction, settings: &part.settings, voxel_mm: h };
    analyze::analyze(&inputs, &mut |_, _| true).unwrap()
}

#[test]
fn the_registry_loads_and_its_fingerprint_is_sha256_of_the_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/printing/registry.json");
    let loaded = registry::load(&path).unwrap();
    assert_eq!(loaded.sha256, sim_print::sha256::hex(&std::fs::read(&path).unwrap()));
    assert!(loaded.registry.printers.contains_key("bambu-h2c"));
    let pla = loaded.registry.material("pla-basic").unwrap();
    assert!(pla.tensile_across_layers.value < pla.tensile_in_layer.value, "layers are the weak direction");
    // A bad value is refused with its path.
    let text = std::fs::read_to_string(&path).unwrap().replacen("\"value\": 26e6", "\"value\": -1", 1);
    let err = registry::parse(text.as_bytes()).unwrap_err();
    assert!(err.contains("materials.pla-basic.tensile_across_layers.value"), "{err}");
    // A measured value without evidence is refused.
    let text = std::fs::read_to_string(&path).unwrap().replacen("\"provenance\": \"estimated\", \"uncertainty\": 0.03, \"source\": \"Typical PLA filament density.\"", "\"provenance\": \"measured\", \"uncertainty\": 0.03, \"source\": \"x\"", 1);
    assert!(registry::parse(text.as_bytes()).unwrap_err().contains("evidence"));
}

#[test]
fn a_bar_in_tension_carries_its_load_through_every_section_and_stretches_by_fl_over_ea() {
    let reg = registry();
    let (mesh, part) = beam_part(60., 1000., [1., 0., 0.], [0., 0., 1.], vec![Section { name: "middle".into(), point: [30., 5., 5.], normal: [1., 0., 0.] }]);
    let r = run(&reg, &mesh, &part, 1.0);
    let s = &r.sections[0].load;
    assert!((s.tension_n - 1000.).abs() < 1e-3 * 1000., "tension {}", s.tension_n);
    assert!(sim_print::mesh::norm(s.shear) < 1.0 && sim_print::mesh::norm(s.bending) < 1e-3);
    // δ = F L / (E A): clamping the root face stiffens it slightly (Poisson restraint).
    let e = reg.material("pla-basic").unwrap().modulus_in_layer.value;
    let delta = 1000. * 0.06 / (e * 1e-4) * 1e3;
    let err = (r.max_displacement_mm - delta) / delta;
    assert!(err.abs() < 0.04, "stretch {} mm vs {delta} mm ({:.1} %)", r.max_displacement_mm, err * 100.);
    assert!(r.equilibrium_error < 1e-5, "{}", r.equilibrium_error);
    // σ = F/A = 10 MPa along the layers: safety factor ≈ 45/10 away from the ends.
    assert!(r.safety_factor > 2.0 && r.safety_factor < 4.6, "{}", r.safety_factor);
}

#[test]
fn a_cantilever_matches_beam_theory_and_its_sections_carry_p_times_arm() {
    let reg = registry();
    let (length, p) = (100., 10.);
    let sections = vec![Section { name: "mid".into(), point: [50., 5., 5.], normal: [1., 0., 0.] }];
    let (mesh, part) = beam_part(length, p, [0., 0., -1.], [0., 0., 1.], sections);
    let r = run(&reg, &mesh, &part, 1.0);
    let m = reg.material("pla-basic").unwrap();
    let (e, g) = (m.modulus_in_layer.value, m.shear_modulus.value);
    let (l, b, h) = (0.1, 0.01, 0.01);
    let i = b * h * h * h / 12.;
    // Euler–Bernoulli plus Timoshenko shear (k = 5/6).
    let delta = (p * l * l * l / (3. * e * i) + p * l / (5. / 6. * g * b * h)) * 1e3;
    let err = (r.max_displacement_mm - delta) / delta;
    assert!(err.abs() < 0.06, "tip {} mm vs {delta} mm ({:.1} %)", r.max_displacement_mm, err * 100.);
    // Section at mid-span: shear P, moment P·L/2, no tension.
    let s = &r.sections[0].load;
    assert!((sim_print::mesh::norm(s.shear) - p).abs() < 1e-3 * p, "shear {:?}", s.shear);
    assert!((sim_print::mesh::norm(s.bending) - p * 0.05).abs() < 1e-3 * p * 0.05, "moment {:?}", s.bending);
    assert!(s.tension_n.abs() < 1e-3 * p);
    // Root stress M·c/I = 6 MPa along the layers; in-layer governs, and the
    // safety factor is within 10 % of 45/6 = 7.5 (surface stress extrapolated from voxel centres).
    assert_eq!(r.governing.mode, "in-layer");
    let analytic = m.tensile_in_layer.value / (p * l * (h / 2.) / i);
    assert!((r.safety_factor - analytic).abs() < 0.10 * analytic, "safety {} vs beam theory {analytic}", r.safety_factor);
}

#[test]
fn turning_the_layers_across_the_bending_stress_makes_layer_split_govern() {
    let reg = registry();
    let (mesh, flat) = beam_part(100., 10., [0., 0., -1.], [0., 0., 1.], vec![]);
    let (_, upright) = beam_part(100., 10., [0., 0., -1.], [1., 0., 0.], vec![]);
    let a = run(&reg, &mesh, &flat, 1.0);
    let b = run(&reg, &mesh, &upright, 1.0);
    assert_eq!(b.governing.mode, "layer split", "{:?}", b.by_mode);
    let m = reg.material("pla-basic").unwrap();
    let expected = m.tensile_across_layers.value / m.tensile_in_layer.value;
    let ratio = b.safety_factor / a.safety_factor;
    // Upright is weaker by about the strength ratio (the stiffness change shifts it a little).
    assert!((ratio - expected).abs() < 0.12 * expected, "ratio {ratio} vs {expected}");
}

#[test]
fn refining_the_voxels_changes_the_cantilever_little() {
    let reg = registry();
    let (mesh, part) = beam_part(100., 10., [0., 0., -1.], [0., 0., 1.], vec![]);
    let coarse = run(&reg, &mesh, &part, 2.0);
    let fine = run(&reg, &mesh, &part, 1.0);
    let d = (coarse.max_displacement_mm - fine.max_displacement_mm) / fine.max_displacement_mm;
    // Coarse hexahedra are stiffer in bending (5 across the depth); bounded and in the safe direction for strength.
    assert!(d < 0.0 && d > -0.12, "coarse is {:.1} % off", d * 100.);
    let s = (coarse.safety_factor - fine.safety_factor) / fine.safety_factor;
    assert!(s.abs() < 0.25, "safety factor moves {:.1} %", s * 100.);
}

#[test]
fn a_seam_shares_tension_and_bending_as_a_bolt_group() {
    let reg = registry();
    let m = reg.material("pla-basic").unwrap();
    let screw = |x: f64, y: f64| Joint::InsertScrew { at: [x, y, 0.], size: "M3".into(), screw_length_mm: 12., clamp_mm: 6. };
    let joints = vec![screw(-20., 0.), screw(20., 0.), Joint::Dowel { at: [0., 10., 0.], diameter_mm: 4., depth_minus_mm: 6., depth_plus_mm: 6. }];
    // 200 N pulling apart, 2 N·m bending about y (opens the +x side), 30 N sliding.
    let load = SeamLoad { tension_n: 200., shear: [30., 0., 0.], bending: [0., 2., 0.], torsion_nm: 0., centroid: [0., 0., 0.], normal: [0., 0., 1.] };
    let check = joints::check_seam("test", &load, &joints, &reg, m, 1.0).unwrap();
    let t: Vec<f64> = check.joints.iter().map(|j| j.tension_n).collect();
    // Equal share 100 N each, ± M/(2·0.02 m) = ±50 N; the pin takes none.
    assert!((t[0] + t[1] - 200.).abs() < 1e-6, "{t:?}");
    assert!(((t[0] - t[1]).abs() - 100.).abs() < 1e-6, "{t:?}");
    assert_eq!(t[2], 0.);
    // M3 in PLA: the head's bearing (45 MPa × π/4 × (5.5² − 3.4²) mm² ≈ 660 N) limits before
    // insert pull-out (17 MPa × π × 5.6 × 5.7 mm² × 0.5 ≈ 852 N, the lesson's number).
    let cap = &check.joints[0].capacity;
    assert!((cap.tension_n - 660.).abs() < 5. && cap.tension_limit.contains("head"), "{cap:?}");
    let mut strong_head = m.clone();
    strong_head.bearing.value = 1e9;
    let pull = joints::capacity(&joints[0], &reg, &strong_head, 1.0).unwrap();
    assert!((pull.tension_n - 852.).abs() < 5. && pull.tension_limit.contains("pull-out"), "{pull:?}");
    assert!(check.safety_factor > 1. && check.safety_factor.is_finite());
}

#[test]
fn the_planner_picks_stronger_settings_for_a_heavier_load_and_meets_the_target() {
    use sim_print::plan::{self, PlanInputs};
    use sim_print::study::PlanSpace;
    let reg = registry();
    let printer = reg.printer("bambu-h2c").unwrap();
    let material = reg.material("pla-basic").unwrap();
    let space = PlanSpace { walls: vec![2, 4], infill: vec![0.15, 0.6, 1.0], layer_heights: None, pattern: "gyroid".into(), search_voxels: 4000 };
    let run_with = |load: f64| {
        let (mesh, mut part) = beam_part(100., load, [0., 0., -1.], [0., 0., 1.], vec![]);
        part.directions = Some(vec![[0., 0., 1.], [1., 0., 0.]]);
        let magnitudes = vec![(load, "test".to_string())];
        let inputs = PlanInputs { registry: &reg, printer, material, mesh: &mesh, part: &part, magnitudes: &magnitudes, space: &space, safety_target: 2.0, final_voxel_mm: 1.0 };
        plan::plan(&inputs, &mut |_, _| true).unwrap()
    };
    let light = run_with(5.0);
    // 40 N: root stress M·c/I = 24 MPa, so solid PLA (45 MPa) just exceeds a safety factor of 1.8.
    let heavy = run_with(40.0);
    let (a, b) = (light.chosen.clone().unwrap(), heavy.chosen.clone().unwrap());
    assert_eq!(a.passes, Some(true));
    assert_eq!(b.passes, Some(true), "{:?}", heavy.notes);
    // The heavier load needs more material, so more time.
    assert!(b.estimate.print_hours > a.estimate.print_hours, "{} vs {}", b.estimate.print_hours, a.estimate.print_hours);
    assert!(b.settings.walls > a.settings.walls || b.settings.infill > a.settings.infill);
    // Laid flat (layers along the beam), not standing on end (layers across the bending stress).
    assert_eq!(b.build_direction, [0., 0., 1.]);
    // Re-checked at full resolution.
    let v = heavy.verified.clone().unwrap();
    assert!(v.safety_factor >= 1.5, "{}", v.safety_factor);
    // Candidates cheaper than the pick were checked and failed; slower ones were not solved.
    assert!(heavy.candidates.iter().any(|c| c.safety_factor.is_none()));
}

#[test]
fn a_moment_load_bends_a_cantilever_like_its_force_would_at_the_same_section() {
    // Tip force P at L versus a pure moment P·L/2 at the tip: at mid-span both give moment P·L/2.
    let reg = registry();
    let sections = vec![Section { name: "mid".into(), point: [50., 5., 5.], normal: [1., 0., 0.] }];
    let (mesh, mut part) = beam_part(100., 0.0, [0., 0., -1.], [0., 0., 1.], sections);
    part.loads[0].moment = Some([0., 0.5, 0.]);
    let r = run(&reg, &mesh, &part, 1.0);
    let s = &r.sections[0].load;
    assert!((sim_print::mesh::norm(s.bending) - 0.5).abs() < 1e-3 * 0.5, "{:?}", s.bending);
    assert!(sim_print::mesh::norm(s.shear) < 1e-6 && s.tension_n.abs() < 1e-6);
}

#[test]
fn a_seam_held_only_by_pins_fails_when_it_is_pulled_or_bent() {
    let reg = registry();
    let m = reg.material("pla-basic").unwrap();
    let pins = vec![Joint::Dowel { at: [-10., 0., 0.], diameter_mm: 4., depth_minus_mm: 6., depth_plus_mm: 6. }, Joint::Dowel { at: [10., 0., 0.], diameter_mm: 4., depth_minus_mm: 6., depth_plus_mm: 6. }];
    let shear_only = SeamLoad { tension_n: -5., shear: [20., 0., 0.], bending: [0.; 3], torsion_nm: 0., centroid: [0.; 3], normal: [0., 0., 1.] };
    assert!(joints::check_seam("pressed", &shear_only, &pins, &reg, m, 1.0).unwrap().safety_factor > 10.);
    let bent = SeamLoad { bending: [0., 1.0, 0.], ..shear_only };
    let c = joints::check_seam("bent", &bent, &pins, &reg, m, 1.0).unwrap();
    assert_eq!(c.safety_factor, 0.);
    assert!(c.notes.iter().any(|n| n.contains("holds the seam closed")));
}

#[test]
fn coupon_breaks_promote_to_measured_values_with_evidence_and_history() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/printing/registry.json");
    let loaded = registry::load(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let results = serde_json::json!({
        "schema": "sim.print-test/1", "material": "pla-basic", "printer": "bambu-h2c", "registry_sha256": loaded.sha256,
        "settings": {"walls": 3, "infill": 1.0}, "printed": "2026-09-29", "tested": "2026-09-30",
        "tests": [
            {"coupon": "tensile_across_layers", "failure_n": [1100.0, 1200.0, 1150.0], "geometry": {"gauge_area_mm2": 40.0}},
            {"coupon": "pin_shear", "failure_n": [900.0, null], "geometry": {"pin_diameter_mm": 4.0, "engaged_mm": 6.0}}
        ]
    });
    let bytes = serde_json::to_vec(&results).unwrap();
    let out = sim_print::promote::promote(&raw, &loaded.registry, &loaded.sha256, &bytes, "2026-09-30").unwrap();
    // Three breaks promote: 1150 N / 40 mm² = 28.75 MPa.
    assert_eq!(out.promotions.len(), 1);
    let p = &out.promotions[0];
    assert!((p.mean - 28.75e6).abs() < 1.0 && p.samples == 3 && p.design_value < p.mean);
    // One break is not enough.
    assert!(out.refused.iter().any(|r| r.starts_with("pin_shear")));
    let new = out.registry.unwrap();
    let parsed = registry::parse(serde_json::to_string(&new).unwrap().as_bytes()).unwrap();
    let q = &parsed.materials["pla-basic"].tensile_across_layers;
    assert_eq!(q.provenance, registry::Provenance::Measured);
    let ev = q.evidence.as_ref().unwrap();
    assert_eq!(ev["results_sha256"], sim_print::sha256::hex(&bytes));
    assert_eq!(ev["previous"]["value"], 26e6);
    assert_eq!(parsed.revision, loaded.registry.revision + 1);
    assert!(parsed.history.last().unwrap()["change"].as_str().unwrap().contains("was 2.6000e7"));
}

#[test]
fn a_measured_dovetail_capacity_replaces_the_model_scaled_by_neck_area() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/printing/registry.json");
    let loaded = registry::load(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let results = serde_json::json!({"schema": "sim.print-test/1", "material": "pla-basic", "printer": "bambu-h2c", "registry_sha256": loaded.sha256,
        "tests": [{"coupon": "dovetail_pull", "failure_n": [300.0, 320.0, 310.0], "geometry": {"neck_mm": 10.0, "thickness_mm": 5.0, "depth_mm": 9.0}}]});
    let out = sim_print::promote::promote(&raw, &loaded.registry, &loaded.sha256, &serde_json::to_vec(&results).unwrap(), "2026-09-30").unwrap();
    let reg = registry::parse(serde_json::to_string(&out.registry.unwrap()).unwrap().as_bytes()).unwrap();
    let m = reg.material("pla-basic").unwrap();
    // A tab twice as thick with the same neck holds twice the measured 310 N.
    let tab = Joint::Dovetail { at: [0.; 3], along: [0., 0., 1.], rail_length_mm: 10.0, neck_mm: 10.0, depth_mm: 9.0 };
    let cap = joints::capacity(&tab, &reg, m, 1.0).unwrap();
    assert!((cap.tension_n - 620.).abs() < 1e-6, "{cap:?}");
    assert!(cap.tension_limit.contains("measured"));
}
