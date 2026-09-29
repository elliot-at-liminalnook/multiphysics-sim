//! Environment for virtual cameras (`sim.vision-scene/1`).
//!
//! Simple exact geometry (rooms seen from inside, boxes turned about +Z,
//! spheres, upright cylinders) with procedural textures, lit by fixed
//! ambient plus one directional light. Surfaces are Lambertian and the light
//! does not move, so a point looks the same from every view: the
//! photometric-consistency assumption that [`crate::mvs`] relies on holds
//! exactly here and only approximately in a real room.
use crate::{V3, add, dot, scale, sub, unit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub schema: String,
    #[serde(default)]
    pub description: String,
    pub lighting: Lighting,
    pub objects: Vec<Object>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    pub ambient: f64,
    /// Direction the light travels (towards the scene), world frame.
    pub sun_direction: V3,
    pub sun: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub name: String,
    pub shape: Shape,
    pub texture: Texture,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    /// An axis-aligned room seen from inside (floor, ceiling, walls).
    Room { min: V3, max: V3 },
    /// A box turned `yaw_deg` about +Z around its centre.
    Box { center: V3, size: V3, #[serde(default)] yaw_deg: f64 },
    Sphere { center: V3, radius: f64 },
    /// Upright (along +Z) capped cylinder standing on `base`.
    Cylinder { base: V3, radius: f64, height: f64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Texture {
    Solid { color: V3 },
    /// Squares of `size` m on the surface, alternating `a`/`b`.
    Checker { size: f64, a: V3, b: V3 },
    /// Fractal value noise at `scale` features per metre, blending `a`→`b`.
    Noise { scale: f64, a: V3, b: V3, #[serde(default)] seed: u32, #[serde(default = "four")] octaves: u32 },
}
fn four() -> u32 {
    4
}

pub struct Hit {
    pub t: f64,
    pub point: V3,
    pub normal: V3,
    pub object: usize,
}

impl Scene {
    pub const SCHEMA: &'static str = "sim.vision-scene/1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!("scene schema `{}` is not {}", self.schema, Self::SCHEMA));
        }
        for (i, o) in self.objects.iter().enumerate() {
            let bad = match &o.shape {
                Shape::Room { min, max } => (0..3).any(|k| max[k] <= min[k]),
                Shape::Box { size, .. } => size.iter().any(|s| *s <= 0.0),
                Shape::Sphere { radius, .. } => *radius <= 0.0,
                Shape::Cylinder { radius, height, .. } => *radius <= 0.0 || *height <= 0.0,
            };
            if bad {
                return Err(format!("objects[{i}] ({}): sizes must be positive", o.name));
            }
        }
        Ok(())
    }

    /// Nearest surface along `origin + t·dir` with `t > 1e-6`.
    pub fn hit(&self, origin: V3, dir: V3) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (i, o) in self.objects.iter().enumerate() {
            let limit = best.as_ref().map_or(f64::INFINITY, |b| b.t);
            if let Some((t, normal)) = intersect(&o.shape, origin, dir, limit) {
                best = Some(Hit { t, point: add(origin, scale(dir, t)), normal, object: i });
            }
        }
        best
    }

    /// Linear radiance (0..1) leaving a hit point.
    pub fn shade(&self, hit: &Hit) -> V3 {
        let o = &self.objects[hit.object];
        let albedo = texture(&o.texture, hit.point, hit.normal);
        let light = self.lighting.ambient + self.lighting.sun * (-dot(hit.normal, unit(self.lighting.sun_direction))).max(0.0);
        scale(albedo, light)
    }
}

fn slab(origin: V3, dir: V3, min: V3, max: V3) -> Option<(f64, usize, f64, usize)> {
    let (mut t0, mut t1, mut a0, mut a1) = (f64::NEG_INFINITY, f64::INFINITY, 0, 0);
    for k in 0..3 {
        if dir[k].abs() < 1e-15 {
            if origin[k] < min[k] || origin[k] > max[k] {
                return None;
            }
            continue;
        }
        let (mut n, mut f) = ((min[k] - origin[k]) / dir[k], (max[k] - origin[k]) / dir[k]);
        if n > f {
            std::mem::swap(&mut n, &mut f);
        }
        if n > t0 {
            t0 = n;
            a0 = k;
        }
        if f < t1 {
            t1 = f;
            a1 = k;
        }
    }
    (t0 <= t1).then_some((t0, a0, t1, a1))
}

fn intersect(shape: &Shape, origin: V3, dir: V3, limit: f64) -> Option<(f64, V3)> {
    const EPS: f64 = 1e-6;
    match *shape {
        Shape::Room { min, max } => {
            let (_, _, t1, axis) = slab(origin, dir, min, max)?;
            if t1 <= EPS || t1 >= limit {
                return None;
            }
            let mut n = [0.0; 3];
            n[axis] = -dir[axis].signum();
            Some((t1, n))
        }
        Shape::Box { center, size, yaw_deg } => {
            let (s, c) = yaw_deg.to_radians().sin_cos();
            let local = |v: V3| [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]];
            let o = local(sub(origin, center));
            let d = local(dir);
            let h = scale(size, 0.5);
            let (t0, axis, _, _) = slab(o, d, scale(h, -1.0), h)?;
            if t0 <= EPS || t0 >= limit {
                return None;
            }
            let mut n = [0.0; 3];
            n[axis] = -d[axis].signum();
            Some((t0, [c * n[0] - s * n[1], s * n[0] + c * n[1], n[2]]))
        }
        Shape::Sphere { center, radius } => {
            let oc = sub(origin, center);
            let b = dot(oc, dir);
            let cc = dot(oc, oc) - radius * radius;
            let a = dot(dir, dir);
            let disc = b * b - a * cc;
            if disc < 0.0 {
                return None;
            }
            let t = (-b - disc.sqrt()) / a;
            (t > EPS && t < limit).then(|| (t, unit(sub(add(origin, scale(dir, t)), center))))
        }
        Shape::Cylinder { base, radius, height } => {
            let mut best: Option<(f64, V3)> = None;
            let (ox, oy) = (origin[0] - base[0], origin[1] - base[1]);
            let a = dir[0] * dir[0] + dir[1] * dir[1];
            if a > 1e-15 {
                let b = ox * dir[0] + oy * dir[1];
                let cc = ox * ox + oy * oy - radius * radius;
                let disc = b * b - a * cc;
                if disc >= 0.0 {
                    let t = (-b - disc.sqrt()) / a;
                    let z = origin[2] + t * dir[2];
                    if t > EPS && t < limit && z >= base[2] && z <= base[2] + height {
                        best = Some((t, unit([ox + t * dir[0], oy + t * dir[1], 0.0])));
                    }
                }
            }
            for (zc, nz) in [(base[2] + height, 1.0), (base[2], -1.0)] {
                if dir[2].abs() > 1e-15 {
                    let t = (zc - origin[2]) / dir[2];
                    let (x, y) = (ox + t * dir[0], oy + t * dir[1]);
                    if t > EPS && t < best.map_or(limit, |b| b.0) && x * x + y * y <= radius * radius {
                        best = Some((t, [0.0, 0.0, nz]));
                    }
                }
            }
            best
        }
    }
}

fn hash(x: i64, y: i64, z: i64, seed: u32) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9) ^ (seed as u64).wrapping_mul(0x27D4_EB2F_1656_67C5);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn value_noise(p: V3, seed: u32) -> f64 {
    let f = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = [p[0] - f[0], p[1] - f[1], p[2] - f[2]];
    let s = t.map(|x| x * x * (3.0 - 2.0 * x));
    let (i, j, k) = (f[0] as i64, f[1] as i64, f[2] as i64);
    let mut v = 0.0;
    for (dx, wx) in [(0, 1.0 - s[0]), (1, s[0])] {
        for (dy, wy) in [(0, 1.0 - s[1]), (1, s[1])] {
            for (dz, wz) in [(0, 1.0 - s[2]), (1, s[2])] {
                v += wx * wy * wz * hash(i + dx, j + dy, k + dz, seed);
            }
        }
    }
    v
}

fn texture(t: &Texture, p: V3, n: V3) -> V3 {
    match *t {
        Texture::Solid { color } => color,
        Texture::Checker { size, a, b } => {
            // The two surface axes least aligned with the normal.
            let k = (0..3).max_by(|x, y| n[*x].abs().total_cmp(&n[*y].abs())).unwrap_or(2);
            let s: i64 = (0..3).filter(|j| *j != k).map(|j| (p[j] / size).floor() as i64).sum();
            if s.rem_euclid(2) == 0 { a } else { b }
        }
        Texture::Noise { scale: sc, a, b, seed, octaves } => {
            let (mut v, mut amp, mut freq, mut total) = (0.0, 1.0, sc, 0.0);
            for o in 0..octaves.max(1) {
                v += amp * value_noise(scale(p, freq), seed.wrapping_add(o));
                total += amp;
                amp *= 0.5;
                freq *= 2.0;
            }
            let x = (v / total).clamp(0.0, 1.0);
            let x = x * x * (3.0 - 2.0 * x);
            add(scale(a, 1.0 - x), scale(b, x))
        }
    }
}
