//! Triangle meshes in millimetres (as CAD exports them): STL in and out,
//! bounds, and the rotation that puts a build direction up.

pub type V3 = [f64; 3];

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0. { scale(a, 1. / n) } else { [0., 0., 1.] }
}

/// A rotation as a 3×3 matrix (rows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rot(pub [[f64; 3]; 3]);

impl Rot {
    pub const IDENTITY: Rot = Rot([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
    pub fn apply(&self, v: V3) -> V3 {
        let m = &self.0;
        [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
    }
    pub fn transpose(&self) -> Rot {
        let m = &self.0;
        Rot([[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]])
    }
    /// The rotation taking unit vector `d` to +Z (the build direction up).
    pub fn build_up(d: V3) -> Rot {
        let d = unit(d);
        let z = [0., 0., 1.];
        let c = dot(d, z);
        if c > 1. - 1e-12 {
            return Rot::IDENTITY;
        }
        if c < -1. + 1e-12 {
            // Upside down: turn half a turn about X.
            return Rot([[1., 0., 0.], [0., -1., 0.], [0., 0., -1.]]);
        }
        let k = unit(cross(d, z));
        let s = (1. - c * c).sqrt();
        let (kx, ky, kz) = (k[0], k[1], k[2]);
        let t = 1. - c;
        Rot([
            [c + kx * kx * t, kx * ky * t - kz * s, kx * kz * t + ky * s],
            [ky * kx * t + kz * s, c + ky * ky * t, ky * kz * t - kx * s],
            [kz * kx * t - ky * s, kz * ky * t + kx * s, c + kz * kz * t],
        ])
    }
}

impl Mesh {
    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &self.vertices {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        (lo, hi)
    }
    pub fn rotated(&self, r: &Rot) -> Mesh {
        Mesh { vertices: self.vertices.iter().map(|v| r.apply(*v)).collect(), triangles: self.triangles.clone() }
    }
    /// Enclosed volume (mm³) by the divergence theorem; negative if inside out.
    pub fn volume(&self) -> f64 {
        self.triangles.iter().map(|t| {
            let (a, b, c) = (self.vertices[t[0] as usize], self.vertices[t[1] as usize], self.vertices[t[2] as usize]);
            dot(a, cross(b, c)) / 6.
        }).sum()
    }
    pub fn area(&self) -> f64 {
        self.triangles.iter().map(|t| {
            let (a, b, c) = (self.vertices[t[0] as usize], self.vertices[t[1] as usize], self.vertices[t[2] as usize]);
            0.5 * norm(cross(sub(b, a), sub(c, a)))
        }).sum()
    }

    /// Read binary or ASCII STL (millimetres).
    pub fn read_stl(path: &std::path::Path) -> Result<Mesh, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse_stl(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
    pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, String> {
        let binary = bytes.len() >= 84 && {
            let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
            84 + n * 50 == bytes.len()
        };
        let mut raw: Vec<[V3; 3]> = Vec::new();
        if binary {
            let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
            for i in 0..n {
                let o = 84 + i * 50 + 12;
                let f = |k: usize| f32::from_le_bytes([bytes[o + 4 * k], bytes[o + 4 * k + 1], bytes[o + 4 * k + 2], bytes[o + 4 * k + 3]]) as f64;
                raw.push([[f(0), f(1), f(2)], [f(3), f(4), f(5)], [f(6), f(7), f(8)]]);
            }
        } else {
            let text = std::str::from_utf8(bytes).map_err(|_| "neither binary STL nor UTF-8 ASCII STL")?;
            let mut tri = Vec::new();
            for (line_no, line) in text.lines().enumerate() {
                let mut words = line.split_whitespace();
                if words.next() == Some("vertex") {
                    let v: Vec<f64> = words.map(|w| w.parse::<f64>().map_err(|_| format!("line {}: bad vertex", line_no + 1))).collect::<Result<_, _>>()?;
                    if v.len() != 3 {
                        return Err(format!("line {}: a vertex needs 3 numbers", line_no + 1));
                    }
                    tri.push([v[0], v[1], v[2]]);
                    if tri.len() == 3 {
                        raw.push([tri[0], tri[1], tri[2]]);
                        tri.clear();
                    }
                }
            }
        }
        if raw.is_empty() {
            return Err("the STL has no triangles".into());
        }
        Ok(Self::weld(&raw))
    }
    /// Share vertices closer than 1 µm.
    pub fn weld(raw: &[[V3; 3]]) -> Mesh {
        let mut index = std::collections::HashMap::new();
        let mut mesh = Mesh::default();
        for t in raw {
            let mut ids = [0u32; 3];
            for (k, v) in t.iter().enumerate() {
                let key = ((v[0] * 1e3).round() as i64, (v[1] * 1e3).round() as i64, (v[2] * 1e3).round() as i64);
                ids[k] = *index.entry(key).or_insert_with(|| {
                    mesh.vertices.push(*v);
                    (mesh.vertices.len() - 1) as u32
                });
            }
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                mesh.triangles.push(ids);
            }
        }
        mesh
    }
    pub fn to_stl(&self) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(self.triangles.len() as u32).to_le_bytes());
        for t in &self.triangles {
            let (a, b, c) = (self.vertices[t[0] as usize], self.vertices[t[1] as usize], self.vertices[t[2] as usize]);
            let n = unit(cross(sub(b, a), sub(c, a)));
            for v in [n, a, b, c] {
                for x in v {
                    out.extend_from_slice(&(x as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }
    /// An axis-aligned box (for tests and coupons).
    pub fn cuboid(lo: V3, hi: V3) -> Mesh {
        let v = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
        let vertices = (0..8).map(v).collect();
        // Outward-facing triangles.
        let triangles = vec![[0, 2, 3], [0, 3, 1], [4, 5, 7], [4, 7, 6], [0, 1, 5], [0, 5, 4], [2, 6, 7], [2, 7, 3], [0, 4, 6], [0, 6, 2], [1, 3, 7], [1, 7, 5]];
        Mesh { vertices, triangles }
    }
}
