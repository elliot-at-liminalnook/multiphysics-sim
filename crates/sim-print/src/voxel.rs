//! Voxels from a closed triangle mesh, and what each voxel is made of once
//! printed: solid walls and skins near the surface, infill inside.

use crate::mesh::{Mesh, V3};

/// A regular grid of cubes in the print frame (+Z is the build direction).
#[derive(Clone, Debug)]
pub struct Grid {
    /// Corner of voxel (0, 0, 0), mm.
    pub origin: V3,
    /// Edge length, mm.
    pub h: f64,
    pub n: [usize; 3],
    pub solid: Vec<bool>,
}

impl Grid {
    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }
    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
    pub fn count(&self) -> usize {
        self.solid.iter().filter(|s| **s).count()
    }
    pub fn at(&self, i: isize, j: isize, k: isize) -> bool {
        if i < 0 || j < 0 || k < 0 || i as usize >= self.n[0] || j as usize >= self.n[1] || k as usize >= self.n[2] {
            return false;
        }
        self.solid[self.index(i as usize, j as usize, k as usize)]
    }
    pub fn centre(&self, i: usize, j: usize, k: usize) -> V3 {
        [self.origin[0] + (i as f64 + 0.5) * self.h, self.origin[1] + (j as f64 + 0.5) * self.h, self.origin[2] + (k as f64 + 0.5) * self.h]
    }
    /// The voxel edge that gives about `target` voxels for this mesh's volume.
    pub fn pitch_for(mesh: &Mesh, target: usize) -> f64 {
        let v = mesh.volume().abs().max(1e-9);
        (v / target.max(1) as f64).cbrt()
    }

    /// Fill voxels whose centres lie inside the mesh (ray parity along +Z).
    pub fn voxelize(mesh: &Mesh, h: f64) -> Result<Grid, String> {
        if !(h > 0.) {
            return Err("voxel size must be positive".into());
        }
        let (lo, hi) = mesh.bounds();
        if !lo.iter().chain(hi.iter()).all(|x| x.is_finite()) {
            return Err("the mesh has no finite vertices".into());
        }
        let n = [0, 1, 2].map(|k| (((hi[k] - lo[k]) / h).ceil() as usize).max(1));
        if n[0] * n[1] * n[2] > 40_000_000 {
            return Err(format!("{}×{}×{} voxels is too many: use a larger voxel size", n[0], n[1], n[2]));
        }
        // Centre the grid on the part.
        let origin = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5 - n[k] as f64 * h * 0.5);
        let mut grid = Grid { origin, h, n, solid: vec![false; n[0] * n[1] * n[2]] };
        // Bin triangles into the columns their xy bounds cover.
        let mut columns: Vec<Vec<u32>> = vec![Vec::new(); n[0] * n[1]];
        for (t, tri) in mesh.triangles.iter().enumerate() {
            let p = tri.map(|i| mesh.vertices[i as usize]);
            let (x0, x1) = (p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min), p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max));
            let (y0, y1) = (p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min), p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max));
            let i0 = (((x0 - origin[0]) / h - 0.5).floor().max(0.)) as usize;
            let i1 = ((((x1 - origin[0]) / h - 0.5).ceil()) as isize).clamp(0, n[0] as isize - 1) as usize;
            let j0 = (((y0 - origin[1]) / h - 0.5).floor().max(0.)) as usize;
            let j1 = ((((y1 - origin[1]) / h - 0.5).ceil()) as isize).clamp(0, n[1] as isize - 1) as usize;
            for j in j0..=j1.max(j0) {
                for i in i0..=i1.max(i0) {
                    if i < n[0] && j < n[1] {
                        columns[i + n[0] * j].push(t as u32);
                    }
                }
            }
        }
        // A tiny irrational offset keeps rays off edges and vertices.
        let (ex, ey) = (h * 1.234_567e-4, h * 2.718_281e-4);
        let mut hits = Vec::new();
        for j in 0..n[1] {
            for i in 0..n[0] {
                let x = origin[0] + (i as f64 + 0.5) * h + ex;
                let y = origin[1] + (j as f64 + 0.5) * h + ey;
                hits.clear();
                for &t in &columns[i + n[0] * j] {
                    let tri = mesh.triangles[t as usize];
                    let [a, b, c] = tri.map(|v| mesh.vertices[v as usize]);
                    let d = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
                    if d.abs() < 1e-18 {
                        continue;
                    }
                    let u = ((x - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (y - a[1])) / d;
                    let v = ((b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1])) / d;
                    if u >= 0. && v >= 0. && u + v <= 1. {
                        hits.push(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]));
                    }
                }
                if hits.len() < 2 {
                    continue;
                }
                hits.sort_by(|a, b| a.total_cmp(b));
                for pair in hits.chunks(2) {
                    let [z0, z1] = [pair[0], *pair.get(1).unwrap_or(&pair[0])];
                    let k0 = ((z0 - origin[2]) / h - 0.5).ceil().max(0.) as usize;
                    let k1 = ((z1 - origin[2]) / h - 0.5).floor();
                    if k1 < 0. {
                        continue;
                    }
                    for k in k0..=(k1 as usize).min(n[2] - 1) {
                        let idx = grid.index(i, j, k);
                        grid.solid[idx] = true;
                    }
                }
            }
        }
        if grid.is_empty() {
            return Err(format!("no voxel centre lies inside the mesh at {h} mm: the part is thinner than a voxel, or the mesh is open"));
        }
        Ok(grid)
    }
}

/// How a piece is printed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Perimeter loops.
    pub walls: u32,
    /// Sparse infill density, 0–1.
    pub infill: f64,
    #[serde(default = "default_pattern")]
    pub pattern: String,
    /// mm.
    pub layer_height: f64,
    #[serde(default = "default_skin")]
    pub top_bottom_layers: u32,
}
fn default_pattern() -> String {
    "gyroid".into()
}
fn default_skin() -> u32 {
    5
}
impl Default for Settings {
    fn default() -> Self {
        Settings { walls: 3, infill: 0.15, pattern: default_pattern(), layer_height: 0.2, top_bottom_layers: 5 }
    }
}
impl std::fmt::Display for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} walls, {:.0}% {}, {} mm layers, {} top/bottom", self.walls, self.infill * 100., self.pattern, self.layer_height, self.top_bottom_layers)
    }
}

/// Per voxel: the share of printed solid (walls, skins), and the resulting
/// stiffness, strength and mass factors relative to solid material.
#[derive(Clone, Debug)]
pub struct Fill {
    pub solid_fraction: Vec<f32>,
    pub stiffness: Vec<f64>,
    pub strength: Vec<f64>,
    pub mass: Vec<f64>,
}

/// Walls reach `walls × line_width` in from the side surfaces; skins
/// `top_bottom_layers × layer_height` from surfaces facing up or down. A
/// voxel coarser than the wall is part wall, part infill (rule of mixtures).
pub fn fill(grid: &Grid, settings: &Settings, line_width_mm: f64, modulus_exponent: f64, strength_exponent: f64) -> Fill {
    let [nx, ny, nz] = grid.n;
    let h = grid.h;
    let rho = settings.infill.clamp(0., 1.);
    let wall = settings.walls as f64 * line_width_mm;
    let skin = settings.top_bottom_layers as f64 * settings.layer_height;
    let big = f64::INFINITY;
    // In-plane chamfer distance to the outside, per layer (voxel steps; 1 = on the surface).
    let mut dxy = vec![big; grid.len()];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let idx = grid.index(i, j, k);
                if !grid.solid[idx] {
                    continue;
                }
                let (ii, jj, kk) = (i as isize, j as isize, k as isize);
                if !grid.at(ii - 1, jj, kk) || !grid.at(ii + 1, jj, kk) || !grid.at(ii, jj - 1, kk) || !grid.at(ii, jj + 1, kk) {
                    dxy[idx] = 1.;
                }
            }
        }
        let s2 = std::f64::consts::SQRT_2;
        for pass in 0..2 {
            let order: Vec<(usize, usize)> = if pass == 0 { (0..ny).flat_map(|j| (0..nx).map(move |i| (i, j))).collect() } else { (0..ny).rev().flat_map(|j| (0..nx).rev().map(move |i| (i, j))).collect() };
            for (i, j) in order {
                let idx = grid.index(i, j, k);
                if !grid.solid[idx] {
                    continue;
                }
                let mut best = dxy[idx];
                let sign: isize = if pass == 0 { -1 } else { 1 };
                for (di, dj, w) in [(sign, 0, 1.), (0, sign, 1.), (sign, sign, s2), (-sign, sign, s2)] {
                    let (a, b) = (i as isize + di, j as isize + dj);
                    if a >= 0 && b >= 0 && (a as usize) < nx && (b as usize) < ny {
                        let o = grid.index(a as usize, b as usize, k);
                        if grid.solid[o] {
                            best = best.min(dxy[o] + w);
                        }
                    }
                }
                dxy[idx] = best;
            }
        }
    }
    // Vertical distance to the outside (up or down), per column.
    let mut dz = vec![big; grid.len()];
    for j in 0..ny {
        for i in 0..nx {
            let mut run = 0.;
            for k in 0..nz {
                let idx = grid.index(i, j, k);
                run = if grid.solid[idx] { run + 1. } else { 0. };
                if grid.solid[idx] {
                    dz[idx] = run;
                }
            }
            run = 0.;
            for k in (0..nz).rev() {
                let idx = grid.index(i, j, k);
                run = if grid.solid[idx] { run + 1. } else { 0. };
                if grid.solid[idx] {
                    dz[idx] = dz[idx].min(run);
                }
            }
        }
    }
    let n = grid.len();
    let mut out = Fill { solid_fraction: vec![0.; n], stiffness: vec![0.; n], strength: vec![0.; n], mass: vec![0.; n] };
    let (e_inf, s_inf) = (rho.powf(modulus_exponent), rho.powf(strength_exponent));
    for idx in 0..n {
        if !grid.solid[idx] {
            continue;
        }
        let fxy = ((wall - (dxy[idx] - 1.) * h) / h).clamp(0., 1.);
        let fz = ((skin - (dz[idx] - 1.) * h) / h).clamp(0., 1.);
        let f = if rho >= 0.999 { 1. } else { fxy.max(fz) };
        out.solid_fraction[idx] = f as f32;
        out.stiffness[idx] = f + (1. - f) * e_inf;
        out.strength[idx] = f + (1. - f) * s_inf;
        out.mass[idx] = f + (1. - f) * rho;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_box_fills_exactly_and_walls_ring_the_infill() {
        let mesh = Mesh::cuboid([0., 0., 0.], [20., 10., 6.]);
        assert!((mesh.volume() - 1200.).abs() < 1e-9);
        let g = Grid::voxelize(&mesh, 1.0).unwrap();
        assert_eq!(g.n, [20, 10, 6]);
        assert_eq!(g.count(), 1200);
        let s = Settings { walls: 2, infill: 0.2, pattern: "gyroid".into(), layer_height: 0.2, top_bottom_layers: 5 };
        let f = fill(&g, &s, 0.5, 1.6, 1.5);
        // 2 walls × 0.5 mm = 1 mm: the outer ring of voxels is solid wall.
        assert_eq!(f.solid_fraction[g.index(0, 5, 3)], 1.0);
        // Skins of 5 × 0.2 mm = 1 mm: the bottom and top layers are solid.
        assert_eq!(f.solid_fraction[g.index(10, 5, 0)], 1.0);
        // Inside: infill only.
        let inner = g.index(10, 5, 3);
        assert_eq!(f.solid_fraction[inner], 0.0);
        assert!((f.mass[inner] - 0.2).abs() < 1e-12);
    }
}
