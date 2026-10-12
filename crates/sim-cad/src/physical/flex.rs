//! Flexible links (RoboCAD's `flex.py`): a voxel finite-element model of
//! a printed part reduced to a handful of modes the simulator integrates
//! as generalised coordinates.
//!
//! The solid is coarsened to at most ~1200 cubic 8-node hexahedra
//! (trilinear, 2×2×2 Gauss), the material is orthotropic with the build
//! direction weaker by the material's `anisotropy_z`, and each joint or
//! attachment on the link is a rigid patch (RBE2) with six degrees of
//! freedom. The link is clamped at its parent joint ("root") frame; the
//! lowest fixed-root modes are the flexible coordinates. Per mode the
//! export carries the frequency, the motion of every other frame, the
//! modal participation in the link's rigid acceleration, and the stress
//! tensor at element centroids. The static sag under 1 g is reported as
//! a sanity number.
//!
//! Two stated differences from RoboCAD's implementation, same model:
//! voxel membership is an exact ray-parity test on the members' solid
//! tessellations where RoboCAD samples its signed-distance grid (itself
//! tessellation-derived), and the lowest modes come from subspace
//! iteration with conjugate-gradient solves where RoboCAD calls scipy's
//! shift-invert `eigsh`. Simplifications, as RoboCAD states them:
//! trilinear hexes are a few percent stiff in bending at 4–5 elements
//! through a wall; infill is homogenised (the modulus is scaled by the
//! infill-and-walls fraction); the FE density is scaled so the voxel
//! mass matches the exact link mass.
use serde_json::{Value, json};
use std::collections::HashMap;

const MAX_ELEMENTS: usize = 1200;
const MAX_STRESS_CELLS: usize = 2000;
const MIN_MODES: usize = 1;
/// RoboCAD's `_MAX_SDF_DIM` (physical.py), which sizes the first voxel grid.
const MAX_SDF_DIM: f64 = 48.0;
/// Voigt order used throughout (matches the schema): xx, yy, zz, xy, yz, xz.
const VOIGT: [(usize, usize); 6] = [(0, 0), (1, 1), (2, 2), (0, 1), (1, 2), (0, 2)];

/// One boundary frame on the link (metres, link frame): a joint or an
/// attachment point, with its rigid-patch radius when declared.
#[derive(Clone, Debug)]
pub struct Frame {
    pub id: Value,
    pub name: String,
    pub point: [f64; 3],
    pub role: Option<String>,
    pub radius: Option<f64>,
    pub radius_source: Option<String>,
}

/// 6×6 inverse by Gauss–Jordan with partial pivoting.
fn inverse6(mut a: [[f64; 6]; 6]) -> Option<[[f64; 6]; 6]> {
    let mut inv = [[0.0; 6]; 6];
    for i in 0..6 {
        inv[i][i] = 1.0;
    }
    for col in 0..6 {
        let pivot = (col..6).max_by(|&r1, &r2| a[r1][col].abs().partial_cmp(&a[r2][col].abs()).unwrap_or(std::cmp::Ordering::Equal))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        inv.swap(col, pivot);
        let d = a[col][col];
        for j in 0..6 {
            a[col][j] /= d;
            inv[col][j] /= d;
        }
        for row in 0..6 {
            if row != col && a[row][col] != 0.0 {
                let f = a[row][col];
                for j in 0..6 {
                    a[row][j] -= f * a[col][j];
                    inv[row][j] -= f * inv[col][j];
                }
            }
        }
    }
    Some(inv)
}

/// 6×6 stiffness (engineering shear) of a print: isotropic in the layer
/// plane, `anisotropy·E` across layers, rotated so the material's 3-axis
/// is the build direction `orientation` (link frame).
pub fn orthotropic_stiffness(e: f64, nu: f64, anisotropy: f64, orientation: [f64; 3]) -> [[f64; 6]; 6] {
    let (e1, e2) = (e, e);
    let e3 = anisotropy.max(0.05) * e;
    let g12 = e / (2.0 * (1.0 + nu));
    let (g13, g23) = (anisotropy.max(0.05) * g12, anisotropy.max(0.05) * g12);
    let (nu12, nu13, nu23) = (nu, nu, nu);
    let nu31 = nu13 * e3 / e1;
    let nu32 = nu23 * e3 / e2;
    let nu21 = nu12 * e2 / e1;
    let mut s = [[0.0; 6]; 6];
    s[0][0] = 1.0 / e1;
    s[1][1] = 1.0 / e2;
    s[2][2] = 1.0 / e3;
    s[0][1] = -nu21 / e2;
    s[1][0] = s[0][1];
    s[0][2] = -nu31 / e3;
    s[2][0] = s[0][2];
    s[1][2] = -nu32 / e3;
    s[2][1] = s[1][2];
    s[3][3] = 1.0 / g12;
    s[4][4] = 1.0 / g23;
    s[5][5] = 1.0 / g13;
    let c = inverse6(s).unwrap_or([[0.0; 6]; 6]);
    // Rotate: tensor form, R maps material axes to link axes.
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let mut o = orientation;
    if norm(o) < 1e-9 {
        o = [0.0, 0.0, 1.0];
    }
    let no = norm(o);
    o = [o[0] / no, o[1] / no, o[2] / no];
    let helper = if o[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let a1_raw = cross(helper, o);
    let na = norm(a1_raw);
    let a1 = [a1_raw[0] / na, a1_raw[1] / na, a1_raw[2] / na];
    let a2 = cross(o, a1);
    // R columns are the material axes in the link frame: R[i][a].
    let r = [[a1[0], a2[0], o[0]], [a1[1], a2[1], o[1]], [a1[2], a2[2], o[2]]];
    let mut t4 = [[[[0.0; 3]; 3]; 3]; 3];
    for (big_i, &(i, j)) in VOIGT.iter().enumerate() {
        for (big_j, &(k, l)) in VOIGT.iter().enumerate() {
            t4[i][j][k][l] = c[big_i][big_j];
            t4[j][i][k][l] = c[big_i][big_j];
            t4[i][j][l][k] = c[big_i][big_j];
            t4[j][i][l][k] = c[big_i][big_j];
        }
    }
    let mut cr = [[0.0; 6]; 6];
    for (big_i, &(i, j)) in VOIGT.iter().enumerate() {
        for (big_j, &(k, l)) in VOIGT.iter().enumerate() {
            let mut sum = 0.0;
            for a in 0..3 {
                for b in 0..3 {
                    for cc in 0..3 {
                        for d in 0..3 {
                            sum += r[i][a] * r[j][b] * r[k][cc] * r[l][d] * t4[a][b][cc][d];
                        }
                    }
                }
            }
            cr[big_i][big_j] = sum;
        }
    }
    cr
}

/// Stiffness (24×24) of a cube of side `h` and the strain matrix at its
/// centre (6×24), node order (−−−, +−−, ++−, −+−, −−+, +−+, +++, −++).
fn hex_element(h: f64, c: &[[f64; 6]; 6]) -> ([[f64; 24]; 24], [[f64; 24]; 6]) {
    const CORNERS: [[f64; 3]; 8] = [
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];
    let bmat = |xi: f64, eta: f64, zeta: f64| -> [[f64; 24]; 6] {
        let mut dn = [[0.0; 3]; 8];
        for (a, corner) in CORNERS.iter().enumerate() {
            let (cx, cy, cz) = (corner[0], corner[1], corner[2]);
            dn[a][0] = 0.125 * cx * (1.0 + cy * eta) * (1.0 + cz * zeta);
            dn[a][1] = 0.125 * cy * (1.0 + cx * xi) * (1.0 + cz * zeta);
            dn[a][2] = 0.125 * cz * (1.0 + cx * xi) * (1.0 + cy * eta);
        }
        let scale = 2.0 / h;
        let mut b = [[0.0; 24]; 6];
        for a in 0..8 {
            let (bx, by, bz) = (dn[a][0] * scale, dn[a][1] * scale, dn[a][2] * scale);
            b[0][3 * a] = bx;
            b[1][3 * a + 1] = by;
            b[2][3 * a + 2] = bz;
            b[3][3 * a] = by;
            b[3][3 * a + 1] = bx;
            b[4][3 * a + 1] = bz;
            b[4][3 * a + 2] = by;
            b[5][3 * a] = bz;
            b[5][3 * a + 2] = bx;
        }
        b
    };
    let g = 1.0 / 3.0f64.sqrt();
    let w = (h / 2.0).powi(3);
    let mut k = [[0.0; 24]; 24];
    for &xi in &[-g, g] {
        for &eta in &[-g, g] {
            for &zeta in &[-g, g] {
                let b = bmat(xi, eta, zeta);
                // K += Bᵀ C B · w
                let mut cb = [[0.0; 24]; 6];
                for i in 0..6 {
                    for j in 0..24 {
                        cb[i][j] = (0..6).map(|q| c[i][q] * b[q][j]).sum();
                    }
                }
                for i in 0..24 {
                    for j in 0..24 {
                        k[i][j] += (0..6).map(|q| b[q][i] * cb[q][j]).sum::<f64>() * w;
                    }
                }
            }
        }
    }
    (k, bmat(0.0, 0.0, 0.0))
}

/// A member tessellation in the link frame (metres).
pub type Mesh = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// Element centres on a cubic grid of spacing h that lie inside the solid
/// union of `solids` (ray parity down each (x, y) column), in the x-major
/// order RoboCAD's meshgrid ravel gives. `h` grows by 1.2× until at most
/// `max_elements` centres are inside, as `flex.py`'s `_voxelize`.
/// Returns (centres, h, origin).
fn voxelize(solids: &[Mesh], all: &[Mesh], max_elements: usize) -> (Vec<[f64; 3]>, f64, [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (verts, _) in all {
        for v in verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    if !lo[0].is_finite() {
        return (Vec::new(), 1.0, [0.0; 3]);
    }
    let extent_max = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f64, f64::max);
    // RoboCAD's SDF cell (collision_block), grid bounds padded by two cells.
    let mut cell = (extent_max / (MAX_SDF_DIM - 5.0)).max(1.0e-3);
    if extent_max / 2.0e-3 <= MAX_SDF_DIM - 5.0 {
        cell = cell.min(2.0e-3);
    }
    let origin = [lo[0] - 2.0 * cell, lo[1] - 2.0 * cell, lo[2] - 2.0 * cell];
    let far = [hi[0] + 2.0 * cell, hi[1] + 2.0 * cell, hi[2] + 2.0 * cell];
    let dims = [0, 1, 2].map(|k| (((far[k] - origin[k]) / cell).ceil() as i64 + 1).max(2));
    let top = [0, 1, 2].map(|k| origin[k] + cell * (dims[k] - 1) as f64);
    let mut h = cell;
    let mut centres: Vec<[f64; 3]> = Vec::new();
    for _ in 0..20 {
        let counts = [0, 1, 2].map(|k| ((top[k] - origin[k]) / h).floor().max(1.0) as usize);
        let axes: [Vec<f64>; 3] = [0, 1, 2].map(|k| (0..counts[k]).map(|i| origin[k] + h * (i as f64 + 0.5)).collect());
        let mut inside = vec![false; counts[0] * counts[1] * counts[2]];
        let at = |ix: usize, iy: usize, iz: usize| (ix * counts[1] + iy) * counts[2] + iz;
        for (verts, tris) in solids {
            for ix in 0..counts[0] {
                for iy in 0..counts[1] {
                    let (cx, cy) = (axes[0][ix], axes[1][iy]);
                    let mut zs: Vec<f64> = Vec::new();
                    for t in tris {
                        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
                        let denom = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                        if denom.abs() < 1e-300 {
                            continue;
                        }
                        let u = ((b[1] - c[1]) * (cx - c[0]) + (c[0] - b[0]) * (cy - c[1])) / denom;
                        let v = ((c[1] - a[1]) * (cx - c[0]) + (a[0] - c[0]) * (cy - c[1])) / denom;
                        let w = 1.0 - u - v;
                        if u >= 0.0 && v >= 0.0 && w >= 0.0 {
                            zs.push(u * a[2] + v * b[2] + w * c[2]);
                        }
                    }
                    zs.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
                    for pair in zs.chunks_exact(2) {
                        for (iz, &cz) in axes[2].iter().enumerate() {
                            if pair[0] < cz && cz < pair[1] {
                                inside[at(ix, iy, iz)] = true;
                            }
                        }
                    }
                }
            }
        }
        centres = Vec::new();
        for ix in 0..counts[0] {
            for iy in 0..counts[1] {
                for iz in 0..counts[2] {
                    if inside[at(ix, iy, iz)] {
                        centres.push([axes[0][ix], axes[1][iy], axes[2][iz]]);
                    }
                }
            }
        }
        if centres.len() <= max_elements || h > 0.5 * (0..3).map(|k| top[k] - origin[k]).fold(0.0f64, f64::max) {
            return (centres, h, origin);
        }
        h *= 1.2;
    }
    (centres, h, origin)
}

/// Lower Cholesky factor of a small symmetric positive definite matrix.
fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = a[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if s <= 0.0 {
                    return None;
                }
                s = s.sqrt();
            } else {
                s /= l[j][j];
            }
            l[i][j] = s;
        }
    }
    Some(l)
}

/// Eigendecomposition of a small symmetric matrix by cyclic Jacobi
/// rotations: eigenvalues (unsorted) and eigenvectors as columns.
fn jacobi_eigen(a_in: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a_in.len();
    let mut a = a_in;
    let mut v = vec![vec![0.0; n]; n];
    for i in 0..n {
        v[i][i] = 1.0;
    }
    for _ in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        let scale: f64 = (0..n).map(|i| a[i][i] * a[i][i]).sum();
        if off <= 1e-28 * scale.max(1e-300) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q] == 0.0 {
                    continue;
                }
                let theta = 0.5 * (a[q][q] - a[p][p]) / a[p][q];
                let t = if theta >= 0.0 { 1.0 / (theta + (theta * theta + 1.0).sqrt()) } else { -1.0 / ((theta * theta + 1.0).sqrt() - theta) };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k][p], v[k][q]);
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i][i]).collect(), v)
}

/// The small generalised symmetric eigenproblem `H w = λ G w` (G SPD):
/// eigenvalues ascending, eigenvectors as columns.
fn generalised_eigen(h: &[Vec<f64>], g: &[Vec<f64>]) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = h.len();
    let l = cholesky(g)?;
    // Â = L⁻¹ H L⁻ᵀ by triangular solves.
    let solve_lower = |b: &[Vec<f64>]| -> Vec<Vec<f64>> {
        let mut x = vec![vec![0.0; n]; n];
        for col in 0..n {
            for i in 0..n {
                x[i][col] = (b[i][col] - (0..i).map(|k| l[i][k] * x[k][col]).sum::<f64>()) / l[i][i];
            }
        }
        x
    };
    let y = solve_lower(h);
    // Â = (L⁻¹ (L⁻¹ H)ᵀ)ᵀ
    let yt: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| y[j][i]).collect()).collect();
    let z = solve_lower(&yt);
    let mut ahat: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| z[j][i]).collect()).collect();
    for i in 0..n {
        for j in i + 1..n {
            let avg = 0.5 * (ahat[i][j] + ahat[j][i]);
            ahat[i][j] = avg;
            ahat[j][i] = avg;
        }
    }
    let (vals, vecs) = jacobi_eigen(ahat);
    // W = L⁻ᵀ V by back substitution per column.
    let mut w = vec![vec![0.0; n]; n];
    for col in 0..n {
        for i in (0..n).rev() {
            w[i][col] = (vecs[i][col] - (i + 1..n).map(|k| l[k][i] * w[k][col]).sum::<f64>()) / l[i][i];
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| vals[a].partial_cmp(&vals[b]).unwrap_or(std::cmp::Ordering::Equal));
    Some((order.iter().map(|&i| vals[i]).collect(), (0..n).map(|i| order.iter().map(|&c| w[i][c]).collect()).collect()))
}

/// Preconditioned conjugate gradient for `A x = b` (A SPD, matrix-free).
fn cg(apply: &dyn Fn(&[f64], &mut [f64]), diag: &[f64], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let pre = |v: &[f64]| -> Vec<f64> { (0..n).map(|i| if diag[i] > 0.0 { v[i] / diag[i] } else { v[i] }).collect() };
    let mut z = pre(&r);
    let mut p = z.clone();
    let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
    let bnorm = b.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-300);
    let mut ap = vec![0.0; n];
    for _ in 0..5000 {
        if r.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-10 * bnorm {
            break;
        }
        apply(&p, &mut ap);
        let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
        if !(pap > 0.0) {
            break;
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        z = pre(&r);
        let rz_new: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let beta = rz_new / rz;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        rz = rz_new;
    }
    x
}

fn columns_apply(apply: &dyn Fn(&[f64], &mut [f64]), cols: &[Vec<f64>], n: usize) -> Vec<Vec<f64>> {
    cols.iter()
        .map(|c| {
            let mut out = vec![0.0; n];
            apply(c, &mut out);
            out
        })
        .collect()
}
fn gram(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    a.iter().map(|x| b.iter().map(|y| x.iter().zip(y).map(|(u, v)| u * v).sum()).collect()).collect()
}
fn combine(z: &[Vec<f64>], w: &[Vec<f64>], n: usize) -> Vec<Vec<f64>> {
    // Columns of Z·W (z is a column list, w is row-major p×p).
    (0..z.len())
        .map(|col| (0..n).map(|i| (0..z.len()).map(|k| z[k][i] * w[k][col]).sum()).collect())
        .collect()
}

/// The lowest `m` eigenpairs of `K φ = λ M φ` (both matrix-free, K SPD)
/// by subspace iteration on K⁻¹M: eigenvalues ascending (clamped at 0),
/// mass-normalised eigenvectors as columns.
fn lowest_modes(n: usize, m: usize, kc: &dyn Fn(&[f64], &mut [f64]), mc: &dyn Fn(&[f64], &mut [f64]), kc_diag: &[f64]) -> Result<(Vec<f64>, Vec<Vec<f64>>), String> {
    let p = (m + 3).min(n).max(m);
    // Deterministic pseudo-random start (an LCG), so exports are reproducible.
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) as f64) / ((1u64 << 31) as f64) - 1.0
    };
    let mut x: Vec<Vec<f64>> = (0..p).map(|_| (0..n).map(|_| next()).collect()).collect();
    let mut lambdas = vec![0.0; m];
    for _ in 0..200 {
        let y = columns_apply(mc, &x, n);
        let z: Vec<Vec<f64>> = y.iter().map(|col| cg(kc, kc_diag, col)).collect();
        let mz = columns_apply(mc, &z, n);
        let mut g = gram(&z, &mz);
        let mut h = gram(&z, &y);
        for mat in [&mut g, &mut h] {
            for i in 0..p {
                for j in i + 1..p {
                    let avg = 0.5 * (mat[i][j] + mat[j][i]);
                    mat[i][j] = avg;
                    mat[j][i] = avg;
                }
            }
        }
        let (vals, w) = generalised_eigen(&h, &g).ok_or("flex eigensolve failed (the projected mass matrix is not positive definite)")?;
        let ritz = combine(&z, &w, n);
        let kry = combine(&y, &w, n);
        let mry = combine(&mz, &w, n);
        lambdas = vals[..m].iter().map(|v| v.max(0.0)).collect();
        let converged = (0..m).all(|i| {
            let residual: f64 = (0..n).map(|k| (kry[i][k] - vals[i] * mry[i][k]).powi(2)).sum::<f64>().sqrt();
            let scale: f64 = kry[i].iter().map(|v| v * v).sum::<f64>().sqrt();
            residual <= 1e-6 * scale.max(1e-300)
        });
        x = ritz;
        if converged {
            break;
        }
    }
    let mut phi: Vec<Vec<f64>> = x.into_iter().take(m).collect();
    for col in &mut phi {
        let mut mc_col = vec![0.0; n];
        mc(col, &mut mc_col);
        let s = col.iter().zip(&mc_col).map(|(a, b)| a * b).sum::<f64>().max(1e-30).sqrt();
        for v in col.iter_mut() {
            *v /= s;
        }
    }
    Ok((lambdas, phi))
}

/// The reduced flexible model of a link (see the module doc). `solids` are
/// the solid members' tessellations and `all` every member's (link frame,
/// metres); `link` is the export's link block (mass, bbox, print);
/// `props` is the material's engineering properties; `frames` are the
/// link's boundary frames. `Ok(None)` when there is nothing to reduce
/// (no geometry, no frames, or fewer than eight elements), as RoboCAD.
pub fn flexible_link(solids: &[Mesh], all: &[Mesh], link: &Value, props: &Value, frames: &[Frame], modes: usize) -> Result<Option<Value>, String> {
    if all.is_empty() || frames.is_empty() {
        return Ok(None);
    }
    let (centres, h, origin) = voxelize(solids, all, MAX_ELEMENTS);
    let ne = centres.len();
    if ne < 8 {
        return Ok(None);
    }
    // Nodes: element corners on the lattice.
    const OFFSETS: [[i64; 3]; 8] = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
    let mut node_index: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut nodes: Vec<[f64; 3]> = Vec::new();
    let mut conn: Vec<[usize; 8]> = Vec::with_capacity(ne);
    for centre in &centres {
        let key = [0, 1, 2].map(|k| ((centre[k] - origin[k]) / h - 0.5).round() as i64);
        let mut element = [0usize; 8];
        for (slot, off) in OFFSETS.iter().enumerate() {
            let corner = (key[0] + off[0], key[1] + off[1], key[2] + off[2]);
            element[slot] = *node_index.entry(corner).or_insert_with(|| {
                nodes.push([origin[0] + h * corner.0 as f64, origin[1] + h * corner.1 as f64, origin[2] + h * corner.2 as f64]);
                nodes.len() - 1
            });
        }
        conn.push(element);
    }
    let nn = nodes.len();
    let ndof = 3 * nn;
    let dof_map: Vec<[usize; 24]> = conn
        .iter()
        .map(|element| {
            let mut d = [0usize; 24];
            for a in 0..8 {
                for k in 0..3 {
                    d[3 * a + k] = 3 * element[a] + k;
                }
            }
            d
        })
        .collect();
    // Material: homogenised infill and walls.
    let mass = link["mass"].as_f64().ok_or("flex needs the link mass")?;
    let print_info = &link["print"];
    let infill = print_info["infill"].as_f64().unwrap_or(0.3);
    let walls = print_info["walls"].as_f64().unwrap_or(3.0);
    let bbox_lo = [0, 1, 2].map(|k| link["bbox"][0][k].as_f64().unwrap_or(0.0));
    let bbox_hi = [0, 1, 2].map(|k| link["bbox"][1][k].as_f64().unwrap_or(0.0));
    let extent = (0..3).map(|k| bbox_hi[k] - bbox_lo[k]).fold(0.0f64, f64::max);
    let wall_fraction = (2.0 * walls * 0.4e-3 / (extent * 0.3).max(1e-3)).min(1.0);
    let fill = if props["print"].is_null() { 1.0 } else { (wall_fraction + (1.0 - wall_fraction) * infill).min(1.0) };
    let youngs = props["youngs_modulus"].as_f64().ok_or("flex needs the material's youngs_modulus")?;
    let e_mod = youngs * (0.3 + 0.7 * fill);
    let aniso = props["print"]["anisotropy_z"].as_f64().unwrap_or(1.0);
    let orientation = print_info["orientation"].as_array().filter(|a| a.len() == 3).map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(1.0)]).unwrap_or([0.0, 0.0, 1.0]);
    let c_mat = orthotropic_stiffness(e_mod, props["poisson"].as_f64().ok_or("flex needs the material's poisson")?, aniso, orientation);
    let (ke, b0) = hex_element(h, &c_mat);
    // Lumped mass per node, scaled to the exact link mass.
    let density = mass / (ne as f64 * h.powi(3));
    let mut m_node = vec![0.0; nn];
    for element in &conn {
        for &node in element {
            m_node[node] += density * h.powi(3) / 8.0;
        }
    }
    let apply_k = |u: &[f64], out: &mut [f64]| {
        out.fill(0.0);
        for dmap in &dof_map {
            let ue: [f64; 24] = std::array::from_fn(|i| u[dmap[i]]);
            for i in 0..24 {
                out[dmap[i]] += (0..24).map(|j| ke[i][j] * ue[j]).sum::<f64>();
            }
        }
    };
    // Frames: rigid patches.
    let nf = frames.len();
    let root_idx = frames
        .iter()
        .position(|f| f.role.as_deref() == Some("root"))
        .unwrap_or_else(|| {
            frames
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let na: f64 = a.point.iter().map(|v| v * v).sum();
                    let nb: f64 = b.point.iter().map(|v| v * v).sum();
                    na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        });
    let mut taken = vec![false; nn];
    let mut patches: Vec<Vec<usize>> = Vec::with_capacity(nf);
    let mut patch_evidence: Vec<Value> = Vec::with_capacity(nf);
    for frame in frames {
        let p = frame.point;
        let r = frame.radius.unwrap_or((2.2 * h).max(4.0e-3));
        let dist = |n: usize| (0..3).map(|k| (nodes[n][k] - p[k]).powi(2)).sum::<f64>().sqrt();
        let mut idx: Vec<usize> = (0..nn).filter(|&n| !taken[n] && dist(n) <= r).collect();
        let mut selection = "within_radius";
        if idx.len() < 4 {
            selection = "nearest_available_nodes";
            let mut available: Vec<usize> = (0..nn).filter(|&n| !taken[n]).collect();
            if available.len() < 4 {
                return Err(format!("Flex boundary {} has fewer than four available mesh nodes", frame.name));
            }
            available.sort_by(|&a, &b| dist(a).partial_cmp(&dist(b)).unwrap_or(std::cmp::Ordering::Equal));
            idx = available.into_iter().take(8).collect();
        }
        for &n in &idx {
            taken[n] = true;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &n in &idx {
            for k in 0..3 {
                lo[k] = lo[k].min(nodes[n][k]);
                hi[k] = hi[k].max(nodes[n][k]);
            }
        }
        patch_evidence.push(json!({
            "radius_m": r,
            "radius_source": frame.radius_source.clone().unwrap_or_else(|| if frame.radius.is_some() { "declared".into() } else { "mesh_default".into() }),
            "selection": selection, "node_count": idx.len(), "bounds_m": [lo, hi],
        }));
        patches.push(idx);
    }
    // Transformation T: q = [6 per frame | 3 per free node] → full u, as
    // sparse rows over the full coordinates.
    let free_nodes: Vec<usize> = (0..nn).filter(|&n| !taken[n]).collect();
    let nq = 6 * nf + 3 * free_nodes.len();
    let mut t_rows: Vec<Vec<(usize, f64)>> = vec![Vec::new(); ndof];
    for (fi, (frame, idx)) in frames.iter().zip(&patches).enumerate() {
        let base = 6 * fi;
        for &n in idx {
            let r = [nodes[n][0] - frame.point[0], nodes[n][1] - frame.point[1], nodes[n][2] - frame.point[2]];
            for a in 0..3 {
                t_rows[3 * n + a].push((base + a, 1.0));
            }
            // u = θ × r.
            let cross = [(1usize, 2usize, r[2], -r[1]), (2, 0, r[0], -r[2]), (0, 1, r[1], -r[0])];
            for (a, (i1, i2, c1, c2)) in cross.into_iter().enumerate() {
                t_rows[3 * n + a].push((base + 3 + i1, c1));
                t_rows[3 * n + a].push((base + 3 + i2, c2));
            }
        }
    }
    for (k, &n) in free_nodes.iter().enumerate() {
        for a in 0..3 {
            t_rows[3 * n + a].push((6 * nf + 3 * k + a, 1.0));
        }
    }
    let apply_t = |q: &[f64]| -> Vec<f64> { t_rows.iter().map(|row| row.iter().map(|&(col, v)| v * q[col]).sum()).collect() };
    let apply_tt = |u: &[f64]| -> Vec<f64> {
        let mut q = vec![0.0; nq];
        for (row, entries) in t_rows.iter().enumerate() {
            for &(col, v) in entries {
                q[col] += v * u[row];
            }
        }
        q
    };
    // The clamped (root-fixed) reduced operators.
    let keep: Vec<usize> = (0..nq).filter(|&i| !(6 * root_idx..6 * root_idx + 6).contains(&i)).collect();
    let nk = keep.len();
    if nk < 2 {
        return Err("flex reduction needs at least two free coordinates beside the root frame".into());
    }
    let expand = |v: &[f64]| -> Vec<f64> {
        let mut q = vec![0.0; nq];
        for (slot, &col) in keep.iter().enumerate() {
            q[col] = v[slot];
        }
        q
    };
    let apply_kc = |v: &[f64], out: &mut [f64]| {
        let u = apply_t(&expand(v));
        let mut ku = vec![0.0; ndof];
        apply_k(&u, &mut ku);
        let q = apply_tt(&ku);
        for (slot, &col) in keep.iter().enumerate() {
            out[slot] = q[col];
        }
    };
    let apply_mc = |v: &[f64], out: &mut [f64]| {
        let u = apply_t(&expand(v));
        let mu: Vec<f64> = (0..nn).flat_map(|n| (0..3).map(move |a| m_node[n] * u[3 * n + a])).collect();
        let q = apply_tt(&mu);
        for (slot, &col) in keep.iter().enumerate() {
            out[slot] = q[col] + 1e-12 * mass * v[slot];
        }
    };
    // Kc's diagonal (the CG preconditioner), from the elements through T.
    let mut kc_diag_full = vec![0.0; nq];
    for dmap in &dof_map {
        for a in 0..24 {
            for b in 0..24 {
                for &(ca, va) in &t_rows[dmap[a]] {
                    for &(cb, vb) in &t_rows[dmap[b]] {
                        if ca == cb {
                            kc_diag_full[ca] += va * ke[a][b] * vb;
                        }
                    }
                }
            }
        }
    }
    let kc_diag: Vec<f64> = keep.iter().map(|&col| kc_diag_full[col]).collect();
    let m_modes = modes.min(nk - 1).max(MIN_MODES);
    let (lam, phi) = lowest_modes(nk, m_modes, &apply_kc, &apply_mc, &kc_diag)?;
    let freqs: Vec<f64> = lam.iter().map(|l| l.sqrt() / (2.0 * std::f64::consts::PI)).collect();
    // Back to full coordinates.
    let q_full: Vec<Vec<f64>> = phi.iter().map(|col| expand(col)).collect();
    let u_modes: Vec<Vec<f64>> = q_full.iter().map(|q| apply_t(q)).collect();
    // Participation in the rigid acceleration about the link COM (the link
    // frame's origin): Uᵀ M Rb.
    let participation: Vec<Vec<f64>> = u_modes
        .iter()
        .map(|u| {
            let mut row = [0.0f64; 6];
            for n in 0..nn {
                let r = nodes[n];
                let rot = [[0.0, r[2], -r[1]], [-r[2], 0.0, r[0]], [r[1], -r[0], 0.0]];
                for a in 0..3 {
                    row[a] += u[3 * n + a] * m_node[n];
                    for b in 0..3 {
                        row[3 + b] += u[3 * n + a] * m_node[n] * rot[a][b];
                    }
                }
            }
            row.to_vec()
        })
        .collect();
    let boundary_shapes: Vec<Vec<Vec<f64>>> = (0..m_modes)
        .map(|i| (0..nf).map(|fi| q_full[i][6 * fi..6 * fi + 6].to_vec()).collect())
        .collect();
    // Stress per mode at element centroids (subsampled).
    let sel: Vec<usize> = if ne <= MAX_STRESS_CELLS {
        (0..ne).collect()
    } else {
        (0..MAX_STRESS_CELLS)
            .map(|i| (i as f64 * (ne - 1) as f64 / (MAX_STRESS_CELLS - 1) as f64) as usize)
            .collect()
    };
    let stress: Vec<Vec<Vec<f64>>> = u_modes
        .iter()
        .map(|u| {
            sel.iter()
                .map(|&e| {
                    let ue: [f64; 24] = std::array::from_fn(|i| u[dof_map[e][i]]);
                    let strain: [f64; 6] = std::array::from_fn(|i| (0..24).map(|j| b0[i][j] * ue[j]).sum());
                    (0..6).map(|i| (0..6).map(|j| c_mat[i][j] * strain[j]).sum()).collect()
                })
                .collect()
        })
        .collect();
    // Gravity sag: static response at 1 g along each axis, worst case.
    let mut sag = 0.0f64;
    for axis in 0..3 {
        let mut force_full = vec![0.0; ndof];
        for n in 0..nn {
            force_full[3 * n + axis] = m_node[n] * 9.81;
        }
        let fq = apply_tt(&force_full);
        let f_keep: Vec<f64> = keep.iter().map(|&col| fq[col]).collect();
        let solved = cg(&apply_kc, &kc_diag, &f_keep);
        let full = apply_t(&expand(&solved));
        for n in 0..nn {
            let norm = (0..3).map(|a| full[3 * n + a].powi(2)).sum::<f64>().sqrt();
            sag = sag.max(norm);
        }
    }
    let tg = props["glass_transition_c"].as_f64().unwrap_or(60.0);
    Ok(Some(json!({
        "normalization": "mass_normalized",
        "modes": m_modes, "frequencies_hz": freqs, "damping_ratio": 0.03,
        "boundary_frames": frames.iter().enumerate().map(|(i, f)| json!({
            "id": f.id, "name": f.name, "point": f.point,
            "role": if i == root_idx { "root".to_string() } else { f.role.clone().unwrap_or_else(|| "outboard".into()) },
            "patch": patch_evidence[i].clone(),
        })).collect::<Vec<_>>(),
        "modal_stiffness": lam, "modal_mass": vec![1.0; m_modes],
        "boundary_shapes": boundary_shapes,
        "participation": participation,
        "stress_cells": sel.iter().map(|&j| centres[j]).collect::<Vec<_>>(),
        "stress_per_mode": stress,
        "gravity_sag_m": sag,
        "softening": {"tg_c": tg, "width_c": 10.0, "ratio_above": 0.05},
        "fe": {"elements": ne, "nodes": nn, "spacing": h, "modulus": e_mod, "fill_fraction": fill, "anisotropy_z": aniso},
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mesh(lo: [f64; 3], hi: [f64; 3]) -> Mesh {
        let v = vec![
            [lo[0], lo[1], lo[2]],
            [hi[0], lo[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [lo[0], hi[1], lo[2]],
            [lo[0], lo[1], hi[2]],
            [hi[0], lo[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ];
        let t = vec![
            [0, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [1, 5, 6],
            [1, 6, 2],
            [2, 6, 7],
            [2, 7, 3],
            [3, 7, 4],
            [3, 4, 0],
        ];
        (v, t)
    }

    #[test]
    fn isotropic_stiffness_matches_the_closed_form() {
        let (e, nu) = (3.5e9, 0.36);
        let c = orthotropic_stiffness(e, nu, 1.0, [0.0, 0.0, 1.0]);
        let d = (1.0 + nu) * (1.0 - 2.0 * nu);
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs();
        assert!(close(c[0][0], e * (1.0 - nu) / d));
        assert!(close(c[0][1], e * nu / d));
        assert!(close(c[3][3], e / (2.0 * (1.0 + nu))));
        assert!(close(c[0][0], c[1][1]) && close(c[0][0], c[2][2]));
    }

    #[test]
    fn a_hex_element_strains_nothing_under_rigid_motion() {
        let c = orthotropic_stiffness(3.5e9, 0.36, 0.6, [0.0, 0.0, 1.0]);
        let (ke, _) = hex_element(0.01, &c);
        let corners = [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
        let scale: f64 = ke.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
        for mode in 0..2 {
            let mut u = [0.0; 24];
            for a in 0..8 {
                let x = [corners[a][0] * 0.005, corners[a][1] * 0.005, corners[a][2] * 0.005];
                let d = if mode == 0 { [1.0, 2.0, 3.0] } else { [x[2] - x[1], x[0] - x[2], x[1] - x[0]] };
                u[3 * a..3 * a + 3].copy_from_slice(&d);
            }
            let worst: f64 = (0..24).map(|i| (0..24).map(|j| ke[i][j] * u[j]).sum::<f64>().abs()).fold(0.0, f64::max);
            assert!(worst <= 1e-9 * scale, "rigid mode {mode} force {worst} vs element scale {scale}");
        }
    }

    #[test]
    fn a_cantilever_beam_reduces_to_positive_ordered_modes() {
        let beam = box_mesh([-0.03, -0.005, -0.005], [0.03, 0.005, 0.005]);
        let meshes = vec![beam];
        let link = json!({
            "name": "beam", "mass": 1240.0 * 6.0e-6, "bbox": [[-0.03, -0.005, -0.005], [0.03, 0.005, 0.005]],
            "print": {"orientation": [0, 0, 1], "infill": 0.3, "walls": 3},
        });
        let props = json!({"youngs_modulus": 3.5e9, "poisson": 0.36, "print": {"anisotropy_z": 0.6}, "glass_transition_c": 60.0});
        let frames = vec![
            Frame { id: json!("j1"), name: "root joint".into(), point: [-0.03, 0.0, 0.0], role: Some("root".into()), radius: Some(0.004), radius_source: None },
            Frame { id: json!("j2"), name: "tip joint".into(), point: [0.03, 0.0, 0.0], role: Some("outboard".into()), radius: Some(0.004), radius_source: None },
        ];
        let flex = flexible_link(&meshes, &meshes, &link, &props, &frames, 6).unwrap().unwrap();
        let freqs: Vec<f64> = flex["frequencies_hz"].as_array().unwrap().iter().map(|f| f.as_f64().unwrap()).collect();
        assert!(freqs.len() >= 2 && freqs.windows(2).all(|w| w[0] <= w[1] + 1e-9));
        // A 60 mm printed beam's first bending mode is in the hundreds of Hz.
        assert!((50.0..5000.0).contains(&freqs[0]), "first frequency {} Hz", freqs[0]);
        assert_eq!(flex["boundary_frames"][0]["role"], "root");
        assert_eq!(flex["modal_mass"], json!(vec![1.0; freqs.len()]));
        let sag = flex["gravity_sag_m"].as_f64().unwrap();
        assert!((0.0..1e-3).contains(&sag), "sag {sag} m");
    }
}
