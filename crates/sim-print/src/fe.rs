//! Linear elastic finite elements on voxels: one trilinear hexahedron per
//! voxel, a transversely isotropic material whose weak axis is the build
//! direction (+Z of the print frame), matrix-free products and Jacobi
//! preconditioned conjugate gradients.

use rayon::prelude::*;

/// Elastic constants of solid printed material (Pa), isotropic in the layer plane.
#[derive(Clone, Copy, Debug)]
pub struct Elastic {
    /// Young's modulus along the layers.
    pub e_in: f64,
    /// Young's modulus across the layers (build direction).
    pub e_across: f64,
    /// Shear modulus on planes containing the build direction (interlayer shear).
    pub g_across: f64,
    pub poisson: f64,
}

pub type D6 = [[f64; 6]; 6];

impl Elastic {
    pub fn isotropic(e: f64, nu: f64) -> Elastic {
        Elastic { e_in: e, e_across: e, g_across: e / (2. * (1. + nu)), poisson: nu }
    }
    /// Stress from strain, Voigt order xx, yy, zz, yz, xz, xy (engineering shears).
    pub fn d(&self) -> D6 {
        let (ep, et, nu) = (self.e_in, self.e_across, self.poisson);
        // Compliance of the normal block; ν between any pair, symmetric.
        let s = [[1. / ep, -nu / ep, -nu / ep], [-nu / ep, 1. / ep, -nu / ep], [-nu / ep, -nu / ep, 1. / et]];
        let c = inv3(s);
        let mut d = [[0.; 6]; 6];
        for i in 0..3 {
            for j in 0..3 {
                d[i][j] = c[i][j];
            }
        }
        d[3][3] = self.g_across;
        d[4][4] = self.g_across;
        d[5][5] = ep / (2. * (1. + nu));
        d
    }
}

fn inv3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let mut r = [[0.; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let (a, b) = ((j + 1) % 3, (j + 2) % 3);
            let (c, d) = ((i + 1) % 3, (i + 2) % 3);
            r[i][j] = (m[a][c] * m[b][d] - m[a][d] * m[b][c]) / det;
        }
    }
    r
}

/// Node `a` of a voxel sits at offset (a&1, a>>1&1, a>>2&1).
pub fn corner(a: usize) -> [usize; 3] {
    [a & 1, (a >> 1) & 1, (a >> 2) & 1]
}

/// Strain-displacement matrix (6×24) at natural coordinates, for a cube of edge `h`.
pub fn b_matrix(h: f64, xi: [f64; 3]) -> [[f64; 24]; 6] {
    let mut b = [[0.; 24]; 6];
    for a in 0..8 {
        let s = corner(a).map(|c| if c == 0 { -1. } else { 1. });
        let n = |k: usize| 1. + s[k] * xi[k];
        // dN/dx = (2/h) dN/dξ.
        let g = [s[0] * n(1) * n(2) / 8. * 2. / h, s[1] * n(0) * n(2) / 8. * 2. / h, s[2] * n(0) * n(1) / 8. * 2. / h];
        let c = 3 * a;
        b[0][c] = g[0];
        b[1][c + 1] = g[1];
        b[2][c + 2] = g[2];
        b[3][c + 1] = g[2];
        b[3][c + 2] = g[1];
        b[4][c] = g[2];
        b[4][c + 2] = g[0];
        b[5][c] = g[1];
        b[5][c + 1] = g[0];
    }
    b
}

/// Element stiffness (24×24) of a cube of edge `h` (m), 2×2×2 Gauss.
pub fn element_stiffness(d: &D6, h: f64) -> Vec<[f64; 24]> {
    let g = 1. / 3f64.sqrt();
    let det = (h / 2.).powi(3);
    let mut k = vec![[0.; 24]; 24];
    for q in 0..8 {
        let xi = corner(q).map(|c| if c == 0 { -g } else { g });
        let b = b_matrix(h, xi);
        // DB (6×24)
        let mut db = [[0.; 24]; 6];
        for i in 0..6 {
            for j in 0..24 {
                db[i][j] = (0..6).map(|m| d[i][m] * b[m][j]).sum();
            }
        }
        for i in 0..24 {
            for j in 0..24 {
                k[i][j] += (0..6).map(|m| b[m][i] * db[m][j]).sum::<f64>() * det;
            }
        }
    }
    k
}

/// The assembled problem, matrix-free.
pub struct Model {
    pub n: [usize; 3],
    /// Voxel edge (m).
    pub h: f64,
    /// Stiffness factor per voxel (0 = empty).
    pub scale: Vec<f64>,
    pub ke: Vec<[f64; 24]>,
    /// Grid node → active node number (u32::MAX if no solid voxel touches it).
    pub node_id: Vec<u32>,
    /// Active node → grid (i, j, k).
    pub nodes: Vec<[usize; 3]>,
}

impl Model {
    pub fn new(n: [usize; 3], h: f64, scale: Vec<f64>, d: &D6) -> Model {
        let (nx, ny, nz) = (n[0] + 1, n[1] + 1, n[2] + 1);
        let mut node_id = vec![u32::MAX; nx * ny * nz];
        let mut nodes = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let touched = (0..8).any(|e| {
                        let c = corner(e);
                        let (a, b, cc) = (i as isize - c[0] as isize, j as isize - c[1] as isize, k as isize - c[2] as isize);
                        a >= 0 && b >= 0 && cc >= 0 && (a as usize) < n[0] && (b as usize) < n[1] && (cc as usize) < n[2] && scale[a as usize + n[0] * (b as usize + n[1] * cc as usize)] > 0.
                    });
                    if touched {
                        node_id[i + nx * (j + ny * k)] = nodes.len() as u32;
                        nodes.push([i, j, k]);
                    }
                }
            }
        }
        Model { n, h, scale, ke: element_stiffness(d, h), node_id, nodes }
    }
    pub fn dofs(&self) -> usize {
        3 * self.nodes.len()
    }
    pub fn voxel(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }
    pub fn node(&self, i: usize, j: usize, k: usize) -> u32 {
        self.node_id[i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)]
    }
    /// Active node numbers of a voxel's eight corners.
    pub fn element_nodes(&self, i: usize, j: usize, k: usize) -> [u32; 8] {
        std::array::from_fn(|a| {
            let c = corner(a);
            self.node(i + c[0], j + c[1], k + c[2])
        })
    }
    /// y = K x.
    pub fn apply(&self, x: &[f64], y: &mut [f64]) {
        let n = self.n;
        y.par_chunks_mut(3).enumerate().for_each(|(p, out)| {
            let [i, j, k] = self.nodes[p];
            let mut acc = [0.; 3];
            for e in 0..8 {
                let c = corner(e);
                // Voxel whose corner `e`... is this node: voxel = node − corner(e).
                if i < c[0] || j < c[1] || k < c[2] {
                    continue;
                }
                let (vi, vj, vk) = (i - c[0], j - c[1], k - c[2]);
                if vi >= n[0] || vj >= n[1] || vk >= n[2] {
                    continue;
                }
                let s = self.scale[self.voxel(vi, vj, vk)];
                if s == 0. {
                    continue;
                }
                let ids = self.element_nodes(vi, vj, vk);
                for (b, id) in ids.iter().enumerate() {
                    let xb = &x[3 * *id as usize..3 * *id as usize + 3];
                    for r in 0..3 {
                        let row = &self.ke[3 * e + r];
                        acc[r] += s * (row[3 * b] * xb[0] + row[3 * b + 1] * xb[1] + row[3 * b + 2] * xb[2]);
                    }
                }
            }
            out.copy_from_slice(&acc);
        });
    }
    pub fn diagonal(&self) -> Vec<f64> {
        let mut diag = vec![0.; self.dofs()];
        for k in 0..self.n[2] {
            for j in 0..self.n[1] {
                for i in 0..self.n[0] {
                    let s = self.scale[self.voxel(i, j, k)];
                    if s == 0. {
                        continue;
                    }
                    for (a, id) in self.element_nodes(i, j, k).iter().enumerate() {
                        for r in 0..3 {
                            diag[3 * *id as usize + r] += s * self.ke[3 * a + r][3 * a + r];
                        }
                    }
                }
            }
        }
        diag
    }
}

pub struct Solution {
    pub u: Vec<f64>,
    pub iterations: usize,
    pub relative_residual: f64,
}

/// Solve K u = f with `fixed` dofs held at zero. `progress` gets the residual ratio now and then.
pub fn solve(model: &Model, f: &[f64], fixed: &[bool], tolerance: f64, max_iterations: usize, progress: &mut dyn FnMut(usize, f64) -> bool) -> Result<Solution, String> {
    let n = model.dofs();
    let diag = model.diagonal();
    let inv: Vec<f64> = diag.iter().zip(fixed).map(|(d, fx)| if *fx || *d <= 0. { 0. } else { 1. / d }).collect();
    let mut u = vec![0.; n];
    let mut r: Vec<f64> = f.iter().zip(fixed).map(|(v, fx)| if *fx { 0. } else { *v }).collect();
    let norm_f = r.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_f == 0. {
        return Ok(Solution { u, iterations: 0, relative_residual: 0. });
    }
    let mut z: Vec<f64> = r.iter().zip(&inv).map(|(a, b)| a * b).collect();
    let mut p = z.clone();
    let mut rz: f64 = r.par_iter().zip(&z).map(|(a, b)| a * b).sum();
    let mut q = vec![0.; n];
    for it in 1..=max_iterations {
        model.apply(&p, &mut q);
        q.par_iter_mut().zip(fixed.par_iter()).for_each(|(v, fx)| if *fx { *v = 0. });
        let pq: f64 = p.par_iter().zip(&q).map(|(a, b)| a * b).sum();
        if pq <= 0. {
            return Err("the stiffness matrix is not positive definite: the part is free to move (add or widen a fixture)".into());
        }
        let alpha = rz / pq;
        u.par_iter_mut().zip(&p).for_each(|(a, b)| *a += alpha * b);
        r.par_iter_mut().zip(&q).for_each(|(a, b)| *a -= alpha * b);
        let res = r.par_iter().map(|x| x * x).sum::<f64>().sqrt() / norm_f;
        if it % 50 == 0 && !progress(it, res) {
            return Err("cancelled".into());
        }
        if res < tolerance {
            return Ok(Solution { u, iterations: it, relative_residual: res });
        }
        z.par_iter_mut().zip(r.par_iter().zip(&inv)).for_each(|(zz, (a, b))| *zz = a * b);
        let rz_new: f64 = r.par_iter().zip(&z).map(|(a, b)| a * b).sum();
        let beta = rz_new / rz;
        rz = rz_new;
        p.par_iter_mut().zip(&z).for_each(|(pp, zz)| *pp = zz + beta * *pp);
    }
    let res = r.iter().map(|x| x * x).sum::<f64>().sqrt() / norm_f;
    Err(format!("the solver did not converge in {max_iterations} iterations (residual {res:.2e}): the part may be barely held, or use a coarser voxel size"))
}

/// Stress (Pa, Voigt order) at a voxel's centre.
pub fn stress_at(model: &Model, d: &D6, u: &[f64], i: usize, j: usize, k: usize) -> [f64; 6] {
    let s = model.scale[model.voxel(i, j, k)];
    let b = b_matrix(model.h, [0., 0., 0.]);
    let ids = model.element_nodes(i, j, k);
    let mut ue = [0.; 24];
    for (a, id) in ids.iter().enumerate() {
        for r in 0..3 {
            ue[3 * a + r] = u[3 * *id as usize + r];
        }
    }
    let eps: [f64; 6] = std::array::from_fn(|m| (0..24).map(|c| b[m][c] * ue[c]).sum());
    std::array::from_fn(|m| s * (0..6).map(|c| d[m][c] * eps[c]).sum::<f64>())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn element_stiffness_is_symmetric_with_six_rigid_modes() {
        let d = Elastic::isotropic(3e9, 0.35).d();
        let k = element_stiffness(&d, 0.002);
        for i in 0..24 {
            for j in 0..24 {
                assert!((k[i][j] - k[j][i]).abs() <= 1e-9 * k[i][i].abs());
            }
        }
        // A rigid translation produces no force.
        for axis in 0..3 {
            for i in 0..24 {
                let f: f64 = (0..8).map(|a| k[i][3 * a + axis]).sum();
                assert!(f.abs() < 1e-6 * k[i][i].abs());
            }
        }
    }
}
