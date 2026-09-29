//! One part, one orientation, one set of print settings: voxels, material
//! fill, fixtures and loads, the solve, and a layer-aware strength check.

use crate::fe::{self, Elastic, Model};
use crate::joints::{self, SeamCheck, SeamLoad};
use crate::mesh::{Mesh, Rot, V3, cross, dot, norm, scale, sub, unit};
use crate::registry::{Material, Printer, Registry};
use crate::study::{Fixture, Load, PartStudy, Region, Section};
use crate::voxel::{self, Grid, Settings};
use serde::Serialize;

/// Failure modes, weakest first in a print.
pub const MODES: [&str; 4] = ["layer split", "interlayer shear", "in-layer", "crushing"];

#[derive(Clone, Debug, Serialize)]
pub struct AppliedLoad {
    pub name: String,
    pub newtons: f64,
    /// Unit direction, part frame.
    pub direction: V3,
    /// Where the number came from.
    pub source: String,
    pub surface_nodes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Governing {
    pub safety_factor: f64,
    pub mode: String,
    /// Part frame, mm.
    pub at: V3,
    /// Stress there (Pa) in the print frame: along layers x, y; across layers z.
    pub stress_print_frame: [f64; 6],
}

#[derive(Clone, Debug, Serialize)]
pub struct SectionResult {
    pub name: String,
    pub load: SeamLoad,
    /// Solid area the plane cuts (mm², voxel estimate).
    pub area_mm2: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartResult {
    pub name: String,
    pub build_direction: V3,
    pub settings: Settings,
    pub voxel_mm: f64,
    pub voxels: usize,
    pub nodes: usize,
    pub iterations: usize,
    pub relative_residual: f64,
    pub mass_g: f64,
    pub max_displacement_mm: f64,
    /// Least over the part: surface stresses extrapolated from the voxel centres, smoothed along the surface.
    pub safety_factor: f64,
    /// The same without smoothing (an upper bound on the local peak).
    pub safety_factor_peak: f64,
    pub governing: Governing,
    /// Least safety factor per mode.
    pub by_mode: Vec<(String, f64)>,
    pub loads: Vec<AppliedLoad>,
    /// |Σ loads + Σ reactions| ÷ Σ|loads| (should be ~0).
    pub equilibrium_error: f64,
    pub sections: Vec<SectionResult>,
    pub seams: Vec<SeamCheck>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub field: Field,
}

/// Per-voxel results for colouring the part (print frame grid).
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub origin: V3,
    pub h: f64,
    pub n: [usize; 3],
    /// Print frame → part frame.
    pub to_part: Option<Rot>,
    /// Smoothed failure index (NaN outside the part).
    pub failure_index: Vec<f32>,
    pub mode: Vec<u8>,
}

impl Field {
    pub fn header(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "sim.print-field/1",
            "origin_mm": self.origin, "voxel_mm": self.h, "n": self.n,
            "to_part": self.to_part.map(|r| r.0),
            "layout": "f32 failure index (NaN outside), then u8 mode, x fastest then y then z",
            "modes": MODES,
        })
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.failure_index.len() * 5);
        for v in &self.failure_index {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.mode);
        out
    }
}

pub struct Inputs<'a> {
    pub registry: &'a Registry,
    pub printer: &'a Printer,
    pub material: &'a Material,
    /// Part frame, mm.
    pub mesh: &'a Mesh,
    pub part: &'a PartStudy,
    /// Newtons per load (same order as `part.loads`), with where each came from.
    pub magnitudes: &'a [(f64, String)],
    pub build_direction: V3,
    pub settings: &'a Settings,
    pub voxel_mm: f64,
}

pub fn elastic(m: &Material) -> Elastic {
    Elastic { e_in: m.modulus_in_layer.value, e_across: m.modulus_across_layers.value, g_across: m.shear_modulus.value, poisson: m.poisson.value }
}

pub fn analyze(inp: &Inputs, progress: &mut dyn FnMut(usize, f64) -> bool) -> Result<PartResult, String> {
    let part = inp.part;
    let rot = Rot::build_up(inp.build_direction);
    let back = rot.transpose();
    let mesh = inp.mesh.rotated(&rot);
    let grid = Grid::voxelize(&mesh, inp.voxel_mm)?;
    let law = inp.registry.infill_law(&inp.settings.pattern)?;
    let fill = voxel::fill(&grid, inp.settings, inp.printer.line_width_mm.value, law.modulus_exponent.value, law.strength_exponent.value);
    let d = elastic(inp.material).d();
    let h_m = grid.h * 1e-3;
    let model = Model::new(grid.n, h_m, fill.stiffness.clone(), &d);
    let mut warnings = Vec::new();
    let node_mm = |p: [usize; 3]| -> V3 { [grid.origin[0] + p[0] as f64 * grid.h, grid.origin[1] + p[1] as f64 * grid.h, grid.origin[2] + p[2] as f64 * grid.h] };
    // Surface nodes: touching an empty voxel.
    let surface: Vec<bool> = model.nodes.iter().map(|&[i, j, k]| {
        (0..8).any(|e| {
            let c = fe::corner(e);
            let (a, b, cc) = (i as isize - c[0] as isize, j as isize - c[1] as isize, k as isize - c[2] as isize);
            !grid.at(a, b, cc)
        })
    }).collect();
    // Surface nodes a region covers, weighted by the exposed voxel-face area
    // around each (faces whose centres lie in the region): a load spreads like
    // a pressure, not as equal point forces.
    let pick = |region: &Region| -> Vec<(usize, f64)> {
        // Face samples from CAD sit up to about a voxel from the nearest voxel face;
        // boxes and half-spaces are exact, so they widen only a little.
        let slack = match region {
            Region::Points { .. } => 0.75,
            Region::Sphere { .. } | Region::Cylinder { .. } => 0.4,
            Region::Box { .. } | Region::Below { .. } | Region::Slab { .. } => 0.25,
        } * grid.h;
        let r = region.rotated(&rot).inflated(slack);
        let mut weight: std::collections::BTreeMap<usize, f64> = Default::default();
        for k in 0..grid.n[2] {
            for j in 0..grid.n[1] {
                for i in 0..grid.n[0] {
                    if !grid.solid[grid.index(i, j, k)] {
                        continue;
                    }
                    let c = grid.centre(i, j, k);
                    for axis in 0..3 {
                        for side in [-1isize, 1] {
                            let mut o = [i as isize, j as isize, k as isize];
                            o[axis] += side;
                            if grid.at(o[0], o[1], o[2]) {
                                continue;
                            }
                            let mut fc = c;
                            fc[axis] += side as f64 * grid.h * 0.5;
                            if !r.contains(fc) {
                                continue;
                            }
                            // The face's four corner nodes.
                            let ids = model.element_nodes(i, j, k);
                            for (a, id) in ids.iter().enumerate() {
                                let cc = fe::corner(a);
                                if (cc[axis] == 1) == (side == 1) {
                                    *weight.entry(*id as usize).or_default() += 0.25;
                                }
                            }
                        }
                    }
                }
            }
        }
        weight.into_iter().collect()
    };
    let nearest = |region: &Region| -> f64 {
        // Distance from the region's reference point to the nearest surface node (for error messages).
        let reference = match region {
            Region::Sphere { center, .. } => *center,
            Region::Box { min, max } => scale(crate::mesh::add(*min, *max), 0.5),
            Region::Cylinder { base, .. } => *base,
            Region::Points { points, .. } => points.first().copied().unwrap_or([0.; 3]),
            Region::Below { .. } => return f64::NAN,
            Region::Slab { point, .. } => *point,
        };
        let r = rot.apply(reference);
        (0..model.nodes.len()).filter(|&p| surface[p]).map(|p| norm(sub(node_mm(model.nodes[p]), r))).fold(f64::INFINITY, f64::min)
    };
    let dofs = model.dofs();
    let mut fixed = vec![false; dofs];
    let mut fixed_nodes = Vec::new();
    for Fixture { name, region } in &part.fixtures {
        let nodes = pick(region);
        if nodes.is_empty() {
            return Err(format!("{}: fixture `{name}` touches no surface ({:.1} mm from the nearest surface node): widen its region", part.name, nearest(region)));
        }
        for (p, _) in nodes {
            fixed_nodes.push(p);
            for r in 0..3 {
                fixed[3 * p + r] = true;
            }
        }
    }
    // At least three fixed nodes not on one line, or the part can spin.
    if !spans_plane(&fixed_nodes.iter().map(|p| node_mm(model.nodes[*p])).collect::<Vec<_>>()) {
        return Err(format!("{}: the fixtures hold the part only along a line or at a point, so it can still turn: hold a face", part.name));
    }
    let mut f = vec![0.; dofs];
    let mut applied = Vec::new();
    for (k, (Load { name, region, direction, moment, about, .. }, (newtons, source))) in part.loads.iter().zip(inp.magnitudes).enumerate() {
        let nodes = pick(region);
        if nodes.is_empty() {
            return Err(format!("{}: load `{name}` (loads[{k}]) touches no surface ({:.1} mm from the nearest surface node): widen its region", part.name, nearest(region)));
        }
        let dir = unit(*direction);
        let dp = rot.apply(dir);
        let total: f64 = nodes.iter().map(|(_, w)| w).sum();
        let mut on_fixed = 0;
        for (p, w) in &nodes {
            if fixed[3 * p] {
                on_fixed += 1;
            }
            for r in 0..3 {
                f[3 * p + r] += newtons * w / total * dp[r];
            }
        }
        if let Some(m) = moment {
            // Linear traction f_i = w_i (ω × r_i) with I ω = M, I = Σ w_i (|r|² 1 − r rᵀ): zero net force, moment M.
            let pts: Vec<V3> = nodes.iter().map(|(p, _)| node_mm(model.nodes[*p])).collect();
            let c = match about {
                Some(a) => rot.apply(*a),
                None => scale(pts.iter().zip(&nodes).fold([0.; 3], |acc, (x, (_, w))| crate::mesh::add(acc, scale(*x, *w))), 1. / total),
            };
            let mut inertia = [[0.; 3]; 3];
            let rs: Vec<V3> = pts.iter().map(|x| scale(sub(*x, c), 1e-3)).collect();
            for (r, (_, w)) in rs.iter().zip(&nodes) {
                let r2 = dot(*r, *r);
                for a in 0..3 {
                    for b in 0..3 {
                        inertia[a][b] += w * ((if a == b { r2 } else { 0. }) - r[a] * r[b]);
                    }
                }
            }
            let mp = rot.apply(*m);
            let omega = solve3(inertia, mp).ok_or_else(|| format!("{}: load `{name}`: a moment needs a region spread over an area, not a line", part.name))?;
            for (r, (p, w)) in rs.iter().zip(&nodes) {
                let f_i = scale(cross(omega, *r), *w);
                for q in 0..3 {
                    f[3 * p + q] += f_i[q];
                }
            }
        }
        if on_fixed == nodes.len() {
            warnings.push(format!("load `{name}` lies wholly on a fixture: it goes straight into the support and stresses nothing"));
        }
        applied.push(AppliedLoad { name: name.clone(), newtons: *newtons, direction: dir, source: source.clone(), surface_nodes: nodes.len() });
    }
    // Own weight or inertia.
    let density = inp.material.density.value;
    let mut mass_kg = 0.;
    let acc = part.acceleration.map(|a| rot.apply(a));
    for k in 0..grid.n[2] {
        for j in 0..grid.n[1] {
            for i in 0..grid.n[0] {
                let v = grid.index(i, j, k);
                if !grid.solid[v] {
                    continue;
                }
                let m = density * fill.mass[v] * h_m.powi(3);
                mass_kg += m;
                if let Some(a) = acc {
                    for id in model.element_nodes(i, j, k) {
                        for r in 0..3 {
                            f[3 * id as usize + r] += m * a[r] / 8.;
                        }
                    }
                }
            }
        }
    }
    let solution = fe::solve(&model, &f, &fixed, 1e-7, 40_000, progress)?;
    let u = &solution.u;
    let max_disp = u.chunks(3).map(|c| (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()).fold(0., f64::max) * 1e3;
    // Reactions: K u − f is the support force at fixed nodes (≈0 elsewhere).
    let mut ku = vec![0.; dofs];
    model.apply(u, &mut ku);
    // Recomputed out of balance at the free nodes (independent of the solver's recurrences).
    let (mut out, mut fnorm) = (0., 0.);
    for i in 0..dofs {
        fnorm += f[i] * f[i];
        if !fixed[i] {
            out += (ku[i] - f[i]).powi(2);
        }
    }
    let equilibrium_error = (out / fnorm.max(1e-300)).sqrt();
    // Stress and failure index per voxel.
    let m = inp.material;
    let (st, sc, sils, sin) = (m.tensile_across_layers.value, m.compressive.value, m.interlayer_shear.value, m.tensile_in_layer.value);
    let n_vox = grid.len();
    let mut fi = vec![f32::NAN; n_vox];
    let mut mode = vec![255u8; n_vox];
    let mut stress = vec![[0.; 6]; n_vox];
    for k in 0..grid.n[2] {
        for j in 0..grid.n[1] {
            for i in 0..grid.n[0] {
                let v = grid.index(i, j, k);
                if !grid.solid[v] {
                    continue;
                }
                let s = fe::stress_at(&model, &d, u, i, j, k);
                let q = fill.strength[v].max(1e-6);
                let (value, which) = failure_index(&s, st * q, sc * q, sils * q, sin * q);
                fi[v] = value as f32;
                mode[v] = which;
                stress[v] = s;
            }
        }
    }
    // Surface stresses: a surface voxel's centre is half a voxel inside, where
    // bending stress is lower; extrapolate outward from it and its inward
    // neighbour. Then smooth only along the surface (a voxel staircase
    // overstates corner peaks), never with the interior.
    let dirs: [(isize, isize, isize); 6] = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)];
    let at = |i: usize, j: usize, k: usize, d: (isize, isize, isize)| (i as isize + d.0, j as isize + d.1, k as isize + d.2);
    let mut extrapolated = fi.clone();
    let mut is_surface = vec![false; n_vox];
    for k in 0..grid.n[2] {
        for j in 0..grid.n[1] {
            for i in 0..grid.n[0] {
                let v = grid.index(i, j, k);
                if !grid.solid[v] {
                    continue;
                }
                let mut best = fi[v];
                for d in dirs {
                    let (a, b, c) = at(i, j, k, d);
                    if grid.at(a, b, c) {
                        continue;
                    }
                    is_surface[v] = true;
                    // Opposite neighbour, one voxel in.
                    let (a, b, c) = at(i, j, k, (-d.0, -d.1, -d.2));
                    if grid.at(a, b, c) {
                        let inner = fi[grid.index(a as usize, b as usize, c as usize)];
                        if fi[v] > inner {
                            best = best.max(fi[v] + 0.5 * (fi[v] - inner));
                        }
                    }
                }
                extrapolated[v] = best;
            }
        }
    }
    let mut smooth = vec![f32::NAN; n_vox];
    let mut peak = (0f32, 0usize);
    let mut worst = (0f32, 0usize);
    let mut by_mode = [0f32; 4];
    for k in 0..grid.n[2] {
        for j in 0..grid.n[1] {
            for i in 0..grid.n[0] {
                let v = grid.index(i, j, k);
                if !grid.solid[v] {
                    continue;
                }
                let (mut sum, mut count) = (extrapolated[v], 1.);
                for d in dirs {
                    let (a, b, c) = at(i, j, k, d);
                    if grid.at(a, b, c) {
                        let o = grid.index(a as usize, b as usize, c as usize);
                        if is_surface[o] == is_surface[v] {
                            sum += extrapolated[o];
                            count += 1.;
                        }
                    }
                }
                let sm = sum / count;
                smooth[v] = sm;
                if extrapolated[v] > peak.0 {
                    peak = (extrapolated[v], v);
                }
                if sm > worst.0 {
                    worst = (sm, v);
                }
                let md = mode[v] as usize;
                if md < 4 {
                    by_mode[md] = by_mode[md].max(sm);
                }
            }
        }
    }
    let voxel_centre = |v: usize| {
        let i = v % grid.n[0];
        let j = (v / grid.n[0]) % grid.n[1];
        let k = v / (grid.n[0] * grid.n[1]);
        grid.centre(i, j, k)
    };
    let sf = |x: f32| if x > 0. { 1. / x as f64 } else { f64::INFINITY };
    let governing = Governing { safety_factor: sf(worst.0), mode: MODES.get(mode[worst.1] as usize).unwrap_or(&"none").to_string(), at: back.apply(voxel_centre(worst.1)), stress_print_frame: stress[worst.1] };
    // Loads through each section and seam (part frame in and out).
    let section_load = |point: V3, normal: V3| -> (SeamLoad, f64) {
        let (pp, nn) = (rot.apply(point), unit(rot.apply(normal)));
        // Centroid and area of the cut.
        let (mut csum, mut count) = ([0.; 3], 0usize);
        for v in 0..n_vox {
            if grid.solid[v] {
                let c = voxel_centre(v);
                if dot(sub(c, pp), nn).abs() <= grid.h * 0.5 {
                    csum = crate::mesh::add(csum, c);
                    count += 1;
                }
            }
        }
        let centroid = if count > 0 { scale(csum, 1. / count as f64) } else { pp };
        let mut force = [0.; 3];
        let mut moment = [0.; 3];
        for (p, node) in model.nodes.iter().enumerate() {
            let x = node_mm(*node);
            if dot(sub(x, pp), nn) > 1e-9 {
                let fp = [ku[3 * p], ku[3 * p + 1], ku[3 * p + 2]];
                force = crate::mesh::add(force, fp);
                moment = crate::mesh::add(moment, cross(scale(sub(x, centroid), 1e-3), fp));
            }
        }
        // Internal force and moment on the plus side from the minus side.
        let fi_ = scale(force, -1.);
        let mi = scale(moment, -1.);
        let tension = -dot(fi_, nn);
        let shear = sub(fi_, scale(nn, dot(fi_, nn)));
        let torsion = dot(mi, nn);
        let bending = sub(mi, scale(nn, torsion));
        let area = count as f64 * grid.h * grid.h;
        (SeamLoad { tension_n: tension, shear: back.apply(shear), bending: back.apply(bending), torsion_nm: torsion, centroid: back.apply(centroid), normal: unit(normal) }, area)
    };
    let sections = part.sections.iter().map(|Section { name, point, normal }| {
        let (load, area_mm2) = section_load(*point, *normal);
        SectionResult { name: name.clone(), load, area_mm2 }
    }).collect();
    let mut seams = Vec::new();
    for s in &part.seams {
        let (load, _) = section_load(s.point, s.normal);
        seams.push(joints::check_seam(&s.name, &load, &s.joints, inp.registry, inp.material, 1.0)?);
    }
    if solution.iterations > 20_000 {
        warnings.push(format!("the solver needed {} iterations: a barely held part, or very thin features", solution.iterations));
    }
    if grid.h > inp.settings.walls as f64 * inp.printer.line_width_mm.value * 3. {
        warnings.push(format!("voxels ({:.2} mm) are coarse next to the walls ({:.2} mm): wall strength is averaged into them", grid.h, inp.settings.walls as f64 * inp.printer.line_width_mm.value));
    }
    Ok(PartResult {
        name: part.name.clone(),
        build_direction: unit(inp.build_direction),
        settings: inp.settings.clone(),
        voxel_mm: grid.h,
        voxels: grid.count(),
        nodes: model.nodes.len(),
        iterations: solution.iterations,
        relative_residual: solution.relative_residual,
        mass_g: mass_kg * 1e3,
        max_displacement_mm: max_disp,
        safety_factor: sf(worst.0),
        safety_factor_peak: sf(peak.0),
        governing,
        by_mode: MODES.iter().zip(by_mode).map(|(m, x)| (m.to_string(), sf(x))).collect(),
        loads: applied,
        equilibrium_error,
        sections,
        seams,
        warnings,
        field: Field { origin: grid.origin, h: grid.h, n: grid.n, to_part: Some(back), failure_index: smooth, mode },
    })
}

/// Failure index (stress ÷ strength, 1 = failure) and mode, print frame
/// (layers in xy). Layer interface: quadratic in normal tension and
/// interlayer shear; in-layer: von Mises against in-layer tensile strength;
/// crushing across the layers against compressive strength.
pub fn failure_index(s: &[f64; 6], across: f64, compressive: f64, interlayer_shear: f64, in_layer: f64) -> (f64, u8) {
    let (sx, sy, sz, tyz, txz, txy) = (s[0], s[1], s[2], s[3], s[4], s[5]);
    let open = sz.max(0.) / across;
    let slide = (tyz * tyz + txz * txz).sqrt() / interlayer_shear;
    let layer = (open * open + slide * slide).sqrt();
    let vm = (0.5 * ((sx - sy).powi(2) + (sy - sz).powi(2) + (sz - sx).powi(2)) + 3. * (tyz * tyz + txz * txz + txy * txy)).sqrt();
    let inl = vm / in_layer;
    let crush = (-sz).max(0.) / compressive;
    let mut best = (layer, if open >= slide { 0 } else { 1 });
    if inl > best.0 {
        best = (inl, 2);
    }
    if crush > best.0 {
        best = (crush, 3);
    }
    best
}

fn spans_plane(points: &[V3]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let a = points[0];
    let Some(b) = points.iter().copied().max_by(|p, q| norm(sub(*p, a)).total_cmp(&norm(sub(*q, a)))) else { return false };
    let ab = sub(b, a);
    if norm(ab) < 1e-9 {
        return false;
    }
    points.iter().any(|p| norm(cross(ab, sub(*p, a))) > 1e-6 * norm(ab) * norm(ab).max(1.))
}

/// Solve a 3×3 system (None if singular).
fn solve3(m: [[f64; 3]; 3], b: V3) -> Option<V3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let scale_ = m.iter().flatten().map(|x| x.abs()).fold(0., f64::max);
    if det.abs() <= 1e-12 * scale_.powi(3) {
        return None;
    }
    let col = |k: usize| -> f64 {
        let mut a = m;
        for r in 0..3 {
            a[r][k] = b[r];
        }
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    Some([col(0) / det, col(1) / det, col(2) / det])
}
