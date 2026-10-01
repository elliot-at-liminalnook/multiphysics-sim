//! The orbit's math: poses, eased glides and spin (moved from `view.rs`,
//! which Lessons and Build used), presets, fit, pan, zoom (toward a point),
//! turntable and trackball rotation, and the step that places the camera
//! ([`place`]). Pure methods on [`Orbit`], so the headless inspect server
//! and the tests use them without a window.
use super::{Framing, Orbit, OrbitRules, RadiusLimits, ViewPreset};
use bevy::camera::ScalingMode;
use bevy::prelude::*;

/// A camera position around its focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub focus: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// An eased move between two poses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    from: Pose,
    to: Pose,
    t: f32,
    duration: f32,
}

/// Default glide time: long enough to follow where the view goes (about a
/// second), short enough not to feel like waiting.
pub const GLIDE_S: f32 = 1.0;

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// An angle in [−π, π).
pub(super) fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Orbit {
    pub fn pose(&self) -> Pose {
        Pose { focus: self.focus, radius: self.radius, yaw: self.yaw, pitch: self.pitch }
    }
    fn set_pose(&mut self, p: Pose) {
        self.focus = p.focus;
        self.radius = p.radius;
        self.yaw = p.yaw;
        self.pitch = p.pitch;
    }
    /// Ease to `to` over `seconds`; 0 (or no previous pose) cuts. Clears a
    /// pending home request and returns to the turntable.
    pub fn glide_to(&mut self, to: Pose, seconds: f32) {
        self.home = false;
        self.trackball = None;
        if seconds <= 0. || self.radius <= 0. {
            self.glide = None;
            self.set_pose(to);
            return;
        }
        // Turn the short way round.
        let to = Pose { yaw: self.yaw + wrap(to.yaw - self.yaw), ..to };
        self.glide = Some(Glide { from: self.pose(), to, t: 0., duration: seconds });
    }
    /// The direction the camera is settling on: a glide's destination, else
    /// where it is now. A zoom that starts during a glide keeps that heading.
    pub fn heading(&self) -> (f32, f32) {
        self.glide.map_or((self.yaw, self.pitch), |g| (g.to.yaw, g.to.pitch))
    }
    /// Advance any glide and spin by `dt` wall seconds.
    pub fn step(&mut self, dt: f32) {
        if let Some(mut g) = self.glide {
            g.t += dt / g.duration.max(1e-3);
            let s = ease(g.t);
            let lerp = |a: f32, b: f32| a + (b - a) * s;
            // Radius eases in log space so zooms feel even.
            let radius = (g.from.radius.max(1e-6).ln() + (g.to.radius.max(1e-6).ln() - g.from.radius.max(1e-6).ln()) * s).exp();
            self.set_pose(Pose { focus: g.from.focus.lerp(g.to.focus, s), radius, yaw: lerp(g.from.yaw, g.to.yaw), pitch: lerp(g.from.pitch, g.to.pitch) });
            self.glide = (g.t < 1.).then_some(g);
        }
        if self.spin != 0. && self.trackball.is_none() {
            self.yaw += self.spin * dt;
        }
    }
    /// Jump to where a glide was going (reduced motion: cuts, not glides).
    pub fn finish_glide(&mut self) {
        if let Some(g) = self.glide.take() {
            self.set_pose(g.to);
        }
    }
    /// The user took over: stop scripted motion.
    pub fn interrupt(&mut self) {
        self.glide = None;
        self.spin = 0.;
    }

    /// The camera's rotation: the trackball's, else the turntable's
    /// (`Ry(yaw) · Rx(−pitch)`, which equals `looking_at(focus, Y)` for
    /// |pitch| < 90° and stays defined at the poles).
    pub fn rotation(&self) -> Quat {
        self.trackball.unwrap_or_else(|| Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch))
    }
    /// The eye position.
    pub fn eye(&self) -> Vec3 {
        self.focus + self.rotation() * Vec3::new(0.0, 0.0, self.radius)
    }
    /// The camera transform this state places.
    pub fn transform(&self) -> Transform {
        Transform { translation: self.eye(), rotation: self.rotation(), scale: Vec3::ONE }
    }

    /// The mode's zoom limits, as distances.
    pub fn limits(&self, rules: &OrbitRules) -> (f32, f32) {
        match rules.radius {
            RadiusLimits::Extent { min, max } => (self.extent * min, self.extent * max),
            RadiusLimits::Absolute { min, max } => (min, max),
        }
    }

    /// A named view: RoboCAD's yaw and pitch (turntable), within the
    /// mode's pitch limit; focus and distance kept.
    pub fn preset(&mut self, view: ViewPreset, rules: &OrbitRules, seconds: f32) {
        let (yaw, pitch) = view.yaw_pitch();
        let pitch = pitch.clamp(-rules.pitch_limit, rules.pitch_limit);
        let to = Pose { yaw, pitch, ..self.pose() };
        self.interrupt();
        self.glide_to(to, seconds);
    }
    /// RoboCAD's `opposite`: yaw + 180°, pitch negated (turntable).
    pub fn opposite(&mut self, rules: &OrbitRules) {
        self.interrupt();
        self.sync_turntable(rules);
        self.yaw = wrap(self.yaw + std::f32::consts::PI);
        self.pitch = (-self.pitch).clamp(-rules.pitch_limit, rules.pitch_limit);
    }

    /// The pose a home request (or a fit, `keep_heading`) frames, for a view
    /// of aspect ratio `aspect` (width / height).
    pub fn framing(&self, rules: &OrbitRules, aspect: f32, keep_heading: bool) -> Pose {
        match rules.framing {
            Framing::Fixed(pose) if !keep_heading => pose,
            Framing::Fixed(pose) => Pose { yaw: self.heading().0, pitch: self.heading().1, ..pose },
            Framing::Bounds { scale, aspect: by_aspect, view } => {
                let (yaw, pitch) = match view {
                    Some(v) if !keep_heading => v,
                    _ => self.heading(),
                };
                let mut radius = self.extent.max(1e-3) * scale;
                if by_aspect {
                    radius /= aspect.clamp(0.1, 1.0);
                }
                Pose { focus: self.centre, radius, yaw, pitch }
            }
        }
    }
    /// Frame the bounds: `home` uses the mode's framing (its fixed view),
    /// else the current heading. Glides when the rules ask for it and the
    /// view was placed before (and motion is not reduced).
    pub fn frame(&mut self, rules: &OrbitRules, aspect: f32, home: bool) {
        let pose = self.framing(rules, aspect, !home);
        let placed = self.radius > 0. && self.focus != Vec3::ZERO;
        let seconds = if rules.glide_home && placed && !rules.reduced_motion { GLIDE_S } else { 0. };
        // A fit keeps the view's direction: in the trackball that is its
        // rotation (the turntable's yaw and pitch are stale there), so only
        // the focus and distance move. Home returns to the turntable.
        let keep = if home { None } else { self.trackball };
        self.glide_to(pose, seconds);
        if keep.is_some() {
            self.trackball = keep;
        }
    }

    /// Pan by a drag of `delta` window pixels (right is +x, down is +y), at
    /// the mode's rate: the focus moves along the view's right and up.
    pub fn pan(&mut self, delta: Vec2, rules: &OrbitRules) {
        let r = self.rotation();
        self.focus += (r * Vec3::X * -delta.x + r * Vec3::Y * delta.y) * self.radius * rules.pan_rate;
    }
    /// Orbit by a drag of `delta` window pixels: the turntable's yaw and
    /// pitch (pitch within the limit), or the trackball's rotation about
    /// the view's own up and right axes.
    pub fn rotate(&mut self, delta: Vec2, rules: &OrbitRules) {
        match self.trackball {
            Some(q) => {
                let q = q * Quat::from_rotation_y(-delta.x * rules.rate) * Quat::from_rotation_x(-delta.y * rules.rate);
                self.trackball = Some(q.normalize());
            }
            None => {
                self.yaw -= delta.x * rules.rate;
                self.pitch = (self.pitch + delta.y * rules.rate).clamp(-rules.pitch_limit, rules.pitch_limit);
            }
        }
    }
    /// Zoom by `factor` (< 1 closer) within the limits; with `anchor` (a
    /// display-frame point), the focus moves toward it by the same ratio,
    /// so the anchor keeps its place on screen (RoboCAD's `Camera.zoom`:
    /// the eye scales about the anchor). Returns the factor applied.
    pub fn zoom(&mut self, factor: f32, anchor: Option<Vec3>, rules: &OrbitRules) -> f32 {
        if !(factor.is_finite() && factor > 0.0) || self.radius <= 0.0 {
            return 1.0;
        }
        let (lo, hi) = self.limits(rules);
        let radius = (self.radius * factor).clamp(lo.min(hi), hi.max(lo));
        let applied = radius / self.radius;
        if let Some(anchor) = anchor.filter(|a| a.is_finite()) {
            self.focus += (anchor - self.focus) * (1.0 - applied);
        }
        self.radius = radius;
        applied
    }
    /// Turntable or trackball. Entering the trackball starts from the
    /// current view; leaving it returns to the nearest turntable heading.
    pub fn set_trackball(&mut self, on: bool, rules: &OrbitRules) {
        match (on, self.trackball) {
            (true, None) => self.trackball = Some(self.rotation()),
            (false, Some(_)) => self.sync_turntable(rules),
            _ => {}
        }
    }
    /// Leave the trackball for the turntable heading closest to it.
    fn sync_turntable(&mut self, rules: &OrbitRules) {
        let Some(q) = self.trackball.take() else { return };
        let back = q * Vec3::Z;
        self.pitch = back.y.clamp(-1.0, 1.0).asin().clamp(-rules.pitch_limit, rules.pitch_limit);
        self.yaw = back.x.atan2(back.z);
    }

    /// The display-frame point on the plane through the focus facing the
    /// view, under a ray (origin, unit direction); None when the ray runs
    /// parallel to it or away from it.
    pub fn focus_plane_hit(&self, origin: Vec3, direction: Vec3) -> Option<Vec3> {
        let normal = self.rotation() * Vec3::Z;
        let denom = direction.dot(normal);
        if denom.abs() < 1e-9 {
            return None;
        }
        let t = (self.focus - origin).dot(normal) / denom;
        (t > 0.0).then(|| origin + direction * t)
    }

    /// The projection this state asks for, given the camera's current one
    /// (a perspective keeps its near plane and clip settings; ortho is
    /// sized to `2 × radius × tan(fov / 2)` as RoboCAD's).
    pub fn projection(&self, current: &Projection) -> Option<Projection> {
        match (self.orthographic, current) {
            (false, Projection::Perspective(p)) if (p.fov - self.fov).abs() <= 1e-6 => None,
            (false, Projection::Perspective(p)) => Some(Projection::Perspective(PerspectiveProjection { fov: self.fov, ..p.clone() })),
            // Bevy's default clip plane sits at its default near (0.1); keep the two together.
            (false, _) => Some(Projection::Perspective(PerspectiveProjection { fov: self.fov, near: 0.001, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.001), ..default() })),
            (true, current) => {
                let height = 2.0 * self.radius.max(1e-6) * (self.fov * 0.5).tan();
                // Clip planes reach far behind and before the eye: an ortho camera sees the whole model.
                let depth = (self.radius.max(self.extent) * 50.0).max(1.0);
                let want = OrthographicProjection { near: -depth, far: depth, scaling_mode: ScalingMode::FixedVertical { viewport_height: height }, ..OrthographicProjection::default_3d() };
                match current {
                    // `ScalingMode` has no PartialEq in 0.19.1.
                    Projection::Orthographic(o) if o.near == want.near && o.far == want.far && matches!(o.scaling_mode, ScalingMode::FixedVertical { viewport_height } if viewport_height == height) => None,
                    _ => Some(Projection::Orthographic(want)),
                }
            }
        }
    }
}

/// The aspect ratio (width / height) a camera draws at: a lesson card's
/// whole rectangle, else its viewport, else 1.
pub(super) fn aspect(camera: &Camera) -> f32 {
    if let Some(sub) = &camera.sub_camera_view {
        let s = sub.full_size.as_vec2();
        return if s.y > 0.0 { (s.x / s.y).max(0.1) } else { 1.0 };
    }
    camera.logical_viewport_size().filter(|s| s.y > 0.0).map_or(1.0, |s| (s.x / s.y).max(0.1))
}

/// SimSync (CameraSet::Place): a pending home request, reduced motion, the
/// glide and spin step, then the transform and projection, each written
/// only on a change (so `Changed<Transform>` means the view moved).
pub(super) fn place(time: Res<Time>, mut cameras: Query<(&mut Orbit, &OrbitRules, &Camera, &mut Transform, &mut Projection)>) {
    for (mut orbit, rules, camera, mut transform, mut projection) in &mut cameras {
        if orbit.home {
            orbit.frame(rules, aspect(camera), true);
        }
        if rules.reduced_motion && (orbit.glide.is_some() || orbit.spin != 0.0) {
            orbit.finish_glide();
            orbit.spin = 0.;
        }
        if orbit.glide.is_some() || orbit.spin != 0.0 {
            orbit.step(time.delta_secs().min(0.1));
        }
        let target = orbit.transform();
        if *transform != target {
            *transform = target;
        }
        if let Some(next) = orbit.projection(&projection) {
            *projection = next;
        }
    }
}
