//! CAD mode's view snapshot: the 3D camera and the Z-up millimetre root as
//! plain matrices, refreshed every frame in SimSync after the shared
//! camera places it (`crate::camera::CameraSet::Place`; [`update`]). Picking, box select, snapping, the gizmo and the overlays
//! project RoboCAD's model points (mm, Z up) to window pixels and cast
//! cursor rays into the model through it, so they agree with what is drawn
//! and need no camera query of their own (the apply system and jobs get a
//! copy). Display only: nothing here changes geometry.
use super::mesh::CadRoot;
use bevy::camera::{CameraProjection, SubCameraView};
use bevy::math::Affine3A;
use bevy::prelude::*;

/// The camera as last drawn. `valid` is false until the camera has a
/// viewport size (its first frame) and while CAD mode has no 3D view.
#[derive(Resource, Clone, Debug)]
pub struct CadView {
    pub valid: bool,
    /// Model (RoboCAD's mm, Z up) → Bevy world: the root's transform.
    pub world_from_model: Affine3A,
    pub model_from_world: Affine3A,
    /// Bevy world → the camera's view space, and back.
    pub view_from_world: Affine3A,
    pub world_from_view: Affine3A,
    /// The camera's projection (Bevy's reverse-Z perspective).
    pub clip_from_view: Mat4,
    pub view_from_clip: Mat4,
    /// The 3D view's rectangle in window logical pixels (origin top left).
    pub min: Vec2,
    pub size: Vec2,
}

impl Default for CadView {
    fn default() -> Self {
        Self {
            valid: false,
            world_from_model: Affine3A::IDENTITY,
            model_from_world: Affine3A::IDENTITY,
            view_from_world: Affine3A::IDENTITY,
            world_from_view: Affine3A::IDENTITY,
            clip_from_view: Mat4::IDENTITY,
            view_from_clip: Mat4::IDENTITY,
            min: Vec2::ZERO,
            size: Vec2::ONE,
        }
    }
}

impl CadView {
    /// A model point (mm) in window logical pixels, or None when it is
    /// behind the camera (or the view is not ready).
    pub fn project(&self, model: Vec3) -> Option<Vec2> {
        if !self.valid {
            return None;
        }
        let view = self.view_from_world.transform_point3(self.world_from_model.transform_point3(model));
        let clip = self.clip_from_view * view.extend(1.0);
        if clip.w <= 1e-9 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new(self.min.x + (ndc.x + 1.0) * 0.5 * self.size.x, self.min.y + (1.0 - ndc.y) * 0.5 * self.size.y))
    }

    /// The ray under window pixel `cursor` in the model frame: (origin on the
    /// near plane, unit direction), mm. None outside a ready view.
    pub fn ray(&self, cursor: Vec2) -> Option<(Vec3, Vec3)> {
        if !self.valid || self.size.x <= 0.0 || self.size.y <= 0.0 {
            return None;
        }
        let ndc = Vec2::new((cursor.x - self.min.x) / self.size.x * 2.0 - 1.0, 1.0 - (cursor.y - self.min.y) / self.size.y * 2.0);
        // As `Camera::viewport_to_world`: z = 1 is the near plane, a tiny z far away.
        let near = self.view_from_clip.project_point3(ndc.extend(1.0));
        let far = self.view_from_clip.project_point3(ndc.extend(f32::EPSILON));
        let origin = self.model_from_world.transform_point3(self.world_from_view.transform_point3(near));
        let direction = self.model_from_world.transform_vector3(self.world_from_view.transform_vector3(far - near));
        let direction = direction.try_normalize()?;
        origin.is_finite().then_some((origin, direction))
    }

    /// Whether window pixel `p` is inside the 3D view.
    pub fn contains(&self, p: Vec2) -> bool {
        self.valid && p.cmpge(self.min).all() && p.cmple(self.min + self.size).all()
    }

    /// Model millimetres per window pixel at model point `at` (for sizing
    /// handles and snap radii in screen terms); None when not visible.
    pub fn mm_per_pixel(&self, at: Vec3) -> Option<f32> {
        let a = self.project(at)?;
        let (_, dir) = self.ray(a)?;
        // A step across the view direction, 1 mm long, measured on screen.
        let side = dir.any_orthonormal_vector();
        let b = self.project(at + side)?;
        let pixels = (b - a).length();
        (pixels > 1e-6).then(|| 1.0 / pixels)
    }
}

/// The intersection of a model-frame ray with the plane through `point`
/// with normal `normal` (both mm), if it hits in front of the origin.
pub fn ray_plane(origin: Vec3, direction: Vec3, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = direction.dot(normal);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = (point - origin).dot(normal) / denom;
    (t >= 0.0).then(|| origin + direction * t)
}

/// The clip-from-view matrix `projection` gives over a viewport of
/// `size` logical pixels (and a card's sub-view), as Bevy's
/// `camera_system` computes `Camera::clip_from_view`: `update` with the
/// logical size, then the whole or sub-view matrix.
pub(super) fn clip_from_view(projection: &Projection, size: Vec2, sub: Option<&SubCameraView>) -> Mat4 {
    let mut projection = projection.clone();
    projection.update(size.x, size.y);
    match sub {
        Some(sub) => projection.get_clip_from_view_for_sub(sub),
        None => projection.get_clip_from_view(),
    }
}

/// SimSync, after `CameraSet::Place` (the viewport, the camera's transform
/// and its projection are this frame's): refresh the snapshot. The camera
/// and the root are top-level entities, so their `Transform` is their world
/// transform; it is read rather than `GlobalTransform`, which Bevy
/// propagates only in PostUpdate and would be last frame's (overlays and
/// picks would trail the drawn view by a frame while orbiting). Likewise
/// the projection matrix is built from the `Projection` that `place` wrote
/// this frame ([`clip_from_view`]), not `Camera::clip_from_view`, which
/// Bevy recomputes only in PostUpdate: an ortho zoom, a field-of-view
/// change or a projection toggle would otherwise pair this frame's
/// transform with last frame's projection.
#[allow(clippy::type_complexity)]
pub(super) fn update(camera: Option<Single<(&Camera, &Transform, &Projection), With<crate::camera::Orbit>>>, root: Option<Single<&Transform, With<CadRoot>>>, view: Option<ResMut<CadView>>) {
    let Some(mut view) = view else { return };
    let (Some(camera), Some(root)) = (camera, root) else {
        if view.valid {
            view.valid = false;
        }
        return;
    };
    let (camera, camera_transform, projection) = *camera;
    let (Some(rect), Some(size)) = (camera.logical_viewport_rect(), camera.logical_viewport_size()) else { return };
    let world_from_model = root.compute_affine();
    let world_from_view = camera_transform.compute_affine();
    let clip_from_view = if size.x > 0.0 && size.y > 0.0 { clip_from_view(projection, size, camera.sub_camera_view.as_ref()) } else { camera.clip_from_view() };
    let next = CadView {
        valid: rect.size().x > 0.0 && rect.size().y > 0.0,
        world_from_model,
        model_from_world: world_from_model.inverse(),
        view_from_world: world_from_view.inverse(),
        world_from_view,
        clip_from_view,
        view_from_clip: clip_from_view.inverse(),
        min: rect.min,
        size: rect.size(),
    };
    // Written only on change, so `Res<CadView>::is_changed` means the view moved.
    let same = view.valid == next.valid && view.min == next.min && view.size == next.size && view.world_from_view == next.world_from_view && view.world_from_model == next.world_from_model && view.clip_from_view == next.clip_from_view;
    if !same {
        *view = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::CameraProjection;

    /// A camera at (0, 0, 5) m looking down −Z with Bevy's default perspective,
    /// over a 200 × 100 px view at (10, 20), and the CAD root's transform.
    fn view() -> CadView {
        let projection = PerspectiveProjection { aspect_ratio: 2.0, ..default() };
        let clip_from_view = projection.get_clip_from_view();
        let world_from_view = Affine3A::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let root = super::super::mesh::root_transform();
        let world_from_model = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
        CadView {
            valid: true,
            world_from_model,
            model_from_world: world_from_model.inverse(),
            view_from_world: world_from_view.inverse(),
            world_from_view,
            clip_from_view,
            view_from_clip: clip_from_view.inverse(),
            min: Vec2::new(10.0, 20.0),
            size: Vec2::new(200.0, 100.0),
        }
    }

    #[test]
    fn projects_the_model_origin_to_the_view_centre_and_rays_come_back() {
        let v = view();
        let centre = v.project(Vec3::ZERO).unwrap();
        assert!((centre - Vec2::new(110.0, 70.0)).length() < 1e-3, "{centre:?}");
        // Model +Z (up) is Bevy +Y: above the centre on screen.
        assert!(v.project(Vec3::new(0.0, 0.0, 100.0)).unwrap().y < centre.y);
        let (origin, dir) = v.ray(centre).unwrap();
        // The camera looks down Bevy −Z, which is model +Y.
        assert!((dir - Vec3::Y).length() < 1e-4, "{dir:?}");
        let hit = ray_plane(origin, dir, Vec3::ZERO, Vec3::Y).unwrap();
        assert!(hit.length() < 1e-2, "{hit:?}");
        // Behind the camera (model −Y beyond 5 m is Bevy +Z past the eye).
        assert!(v.project(Vec3::new(0.0, -6000.0, 0.0)).is_none());
        assert!(v.mm_per_pixel(Vec3::ZERO).is_some_and(|m| m > 0.0));
    }

    #[test]
    fn the_projection_matrix_is_built_from_this_frames_projection() {
        // An orthographic view 2 m high over 200 × 100 px is 4 m wide: x = 2 m is its right edge.
        let ortho = Projection::Orthographic(OrthographicProjection { scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 2.0 }, ..OrthographicProjection::default_3d() });
        let edge = clip_from_view(&ortho, Vec2::new(200.0, 100.0), None) * Vec4::new(2.0, 1.0, -1.0, 1.0);
        assert!((edge.x / edge.w - 1.0).abs() < 1e-5 && (edge.y / edge.w - 1.0).abs() < 1e-5, "{edge:?}");
        // A perspective takes the viewport's aspect, as Bevy's camera_system gives it.
        let expected = PerspectiveProjection { aspect_ratio: 2.0, ..default() }.get_clip_from_view();
        assert_eq!(clip_from_view(&Projection::Perspective(PerspectiveProjection::default()), Vec2::new(200.0, 100.0), None), expected);
    }
}
