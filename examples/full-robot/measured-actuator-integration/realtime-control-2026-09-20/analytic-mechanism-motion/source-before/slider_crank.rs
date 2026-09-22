//! Analytic local coordinates for a structurally checked rigid slider-crank.
//! This component does not replace dynamics, infer limits, or mutate a model.
use super::super::{Articulated, DofKind, JointC};
use crate::math::V;
use nalgebra::{Rotation3, Unit};
use std::collections::BTreeSet;

/// Internal compiled coverage certificate; callers cannot mutate its geometry.
pub(super) struct AnalyticPositions {
    knees: Vec<AnalyticSliderCrank>,
}
impl AnalyticPositions {
    pub(super) fn new(
        art: &Articulated,
        independent: &[usize],
        dependent: &[usize],
    ) -> Result<Self, String> {
        let mut covered = BTreeSet::new();
        let mut knees = Vec::new();
        for audit in art.audit_slider_cranks() {
            let c = audit.candidate.ok_or_else(|| {
                format!(
                    "analytic positions: {}: {}",
                    audit.loop_name,
                    audit.rejection.unwrap_or_default()
                )
            })?;
            if !independent.contains(&c.dof_indices[0]) {
                return Err(format!(
                    "analytic positions: {} requires an independent crank",
                    c.loop_name
                ));
            }
            for i in &c.dof_indices[1..] {
                if !dependent.contains(i) || !covered.insert(*i) {
                    return Err(
                        "analytic positions require disjoint dependent mechanism coordinates"
                            .into(),
                    );
                }
            }
            knees.push(c);
        }
        for t in &art.transmissions {
            if !independent.contains(&t.driver)
                || !dependent.contains(&t.driven)
                || !covered.insert(t.driven)
                || !t.ratio.is_finite()
                || t.ratio == 0.0
            {
                return Err(
                    "analytic positions require disjoint independent-to-dependent transmissions"
                        .into(),
                );
            }
        }
        if covered != dependent.iter().copied().collect() {
            return Err("analytic positions do not cover all dependent coordinates".into());
        }
        Ok(Self { knees })
    }
    pub(super) fn apply(
        &self,
        art: &Articulated,
        seed: &super::Generalized,
        g: &mut super::Generalized,
    ) -> Result<(), String> {
        for t in &art.transmissions {
            g.q[t.driven] = g.q[t.driver] / t.ratio;
        }
        for c in &self.knees {
            let [crank, coupler, slider] = c.dof_indices;
            // Select the seed's physical assembly branch, including the branch
            // opposite the export pose. Angle unwrapping preserves winding.
            let n = V::from(c.axis);
            let rod = Rotation3::from_axis_angle(
                &Unit::new_normalize(n),
                seed.q[crank] + c.coupler_axis_sign * seed.q[coupler],
            ) * V::from(c.rod_offset_m);
            let projection = V::from(c.slider_axis).dot(&rod);
            if !projection.is_finite() || projection.abs() <= c.certification_length_tolerance_m {
                return Err(format!(
                    "analytic positions: {} seed is at a toggle",
                    c.loop_name
                ));
            }
            let x = c.coordinates(
                g.q[crank],
                if projection > 0.0 { 1 } else { -1 },
                seed.q[coupler],
            )?;
            g.q[coupler] = x.coupler_rad;
            g.q[slider] = x.slider_m;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AnalyticSliderCrank {
    pub loop_name: String,
    pub carrier: String,
    /// Crank, coupler rotation, slider translation in the full joint vector.
    pub dof_indices: [usize; 3],
    pub coordinate_names: [String; 3],
    /// All geometry below is in the common carrier's local COM frame, metres.
    origin_m: [f64; 3],
    crank_offset_m: [f64; 3],
    rod_offset_m: [f64; 3],
    slider_point_m: [f64; 3],
    axis: [f64; 3],
    slider_axis: [f64; 3],
    coupler_axis_sign: f64,
    pub reference_branch: i8,
    pub crank_radius_m: f64,
    pub projected_rod_length_m: f64,
    pub export_closure_error_m: f64,
    pub maximum_axis_error: f64,
    pub certification_length_tolerance_m: f64,
    pub certification_axis_tolerance: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SliderCrankCoordinates {
    pub coupler_rad: f64,
    pub slider_m: f64,
    /// Derivatives with respect to the crank coordinate (radians).
    pub first_derivative: [f64; 2],
    pub second_derivative: [f64; 2],
    /// Signed projection of the rod on the slider direction; zero is a toggle.
    pub rod_projection_m: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SliderCrankAudit {
    pub loop_name: String,
    pub candidate: Option<AnalyticSliderCrank>,
    pub rejection: Option<String>,
}

impl Articulated {
    /// Audit every authored loop from compiled topology and local joint frames.
    /// No part names, pose-local numerical sparsity or sampled rank select a
    /// mechanism. Unsupported loops are reported individually, never omitted.
    pub fn audit_slider_cranks(&self) -> Vec<SliderCrankAudit> {
        self.loops
            .iter()
            .enumerate()
            .map(|(i, lp)| match self.compile_slider_crank(i) {
                Ok(candidate) => SliderCrankAudit {
                    loop_name: lp.name.clone(),
                    candidate: Some(candidate),
                    rejection: None,
                },
                Err(rejection) => SliderCrankAudit {
                    loop_name: lp.name.clone(),
                    candidate: None,
                    rejection: Some(rejection),
                },
            })
            .collect()
    }

    fn compile_slider_crank(&self, index: usize) -> Result<AnalyticSliderCrank, String> {
        let lp = &self.loops[index];
        let axis_tol = 1e-12;
        let length_tol = 1e-12;
        let axis_rows = lp.axis.as_ref().ok_or("requires a revolute closure")?;
        let single = |j: &JointC, kind: DofKind| {
            j.dofs.len() == 1 && j.dofs[0].kind == kind && j.flex_boundary.is_none()
        };
        let mut roles = None;
        for (coupler, slider, rc, rs) in
            [(lp.a, lp.b, lp.r_a, lp.r_b), (lp.b, lp.a, lp.r_b, lp.r_a)]
        {
            let Some(ci) = self.links[coupler].parent_joint else {
                continue;
            };
            let Some(si) = self.links[slider].parent_joint else {
                continue;
            };
            let cj = &self.joints[ci];
            let sj = &self.joints[si];
            if !single(cj, DofKind::Revolute) || !single(sj, DofKind::Prismatic) {
                continue;
            }
            let Some(pi) = self.links[cj.parent].parent_joint else {
                continue;
            };
            let pj = &self.joints[pi];
            if single(pj, DofKind::Revolute) && pj.parent == sj.parent {
                roles = Some((pi, ci, si, coupler, slider, rc, rs));
                break;
            }
        }
        let (pi, ci, si, coupler, slider, rc, rs) = roles.ok_or(
            "requires two serial revolute joints and one prismatic joint with a common carrier",
        )?;
        let (p, c, s) = (&self.joints[pi], &self.joints[ci], &self.joints[si]);
        if [p.parent, p.child, coupler, slider]
            .iter()
            .any(|i| self.links[*i].flex.is_some())
        {
            return Err("modal flexibility is not discarded".into());
        }
        let n = p.r_j.column(p.dofs[0].axis).into_owned();
        let crank_rotation = p.r_j * p.r_jc_rot;
        let rod_joint_rotation = crank_rotation * c.r_j;
        let rod_rotation = rod_joint_rotation * c.r_jc_rot;
        let slider_rotation = s.r_j * s.r_jc_rot;
        let cn = rod_joint_rotation.column(c.dofs[0].axis).into_owned();
        let u = s.r_j.column(s.dofs[0].axis).into_owned();
        let (ra, rb) = if lp.a == coupler {
            (rod_rotation, slider_rotation)
        } else {
            (slider_rotation, rod_rotation)
        };
        let (e1, e2, baxis) = axis_rows;
        let axis_error = [
            (n.norm() - 1.0).abs(),
            (cn.norm() - 1.0).abs(),
            (u.norm() - 1.0).abs(),
            n.cross(&cn).norm(),
            n.dot(&u).abs(),
            n.cross(&(rb * baxis)).norm(),
            n.dot(&(ra * e1)).abs(),
            n.dot(&(ra * e2)).abs(),
        ]
        .into_iter()
        .fold(0.0_f64, f64::max);
        if !axis_error.is_finite() || axis_error > axis_tol {
            return Err(format!(
                "axes do not define an invariant slider-crank plane: {axis_error:e}"
            ));
        }
        let origin = p.r_pj;
        let crank_offset = p.r_j * p.r_jc + crank_rotation * c.r_pj;
        let rod_offset = rod_joint_rotation * c.r_jc + rod_rotation * rc;
        let slider_point = s.r_pj + s.r_j * s.r_jc + slider_rotation * rs;
        let closure_error = (slider_point - origin - crank_offset - rod_offset).norm();
        if !closure_error.is_finite() || closure_error > length_tol {
            return Err(format!(
                "exported zero-coordinate closure is inconsistent: {closure_error:e} m"
            ));
        }
        let rod_planar = rod_offset - n * n.dot(&rod_offset);
        let crank_planar = crank_offset - n * n.dot(&crank_offset);
        if rod_planar.norm() <= length_tol || crank_planar.norm() <= length_tol {
            return Err("degenerate projected rod or crank radius".into());
        }
        let dof_index = |ji| self.joints[..ji].iter().map(|j| j.dofs.len()).sum();
        Ok(AnalyticSliderCrank {
            loop_name: lp.name.clone(),
            carrier: self.links[p.parent].name.clone(),
            dof_indices: [dof_index(pi), dof_index(ci), dof_index(si)],
            coordinate_names: [
                p.dofs[0].name.clone(),
                c.dofs[0].name.clone(),
                s.dofs[0].name.clone(),
            ],
            origin_m: origin.into(),
            crank_offset_m: crank_offset.into(),
            rod_offset_m: rod_offset.into(),
            slider_point_m: slider_point.into(),
            axis: n.into(),
            slider_axis: u.into(),
            coupler_axis_sign: if n.dot(&cn) > 0.0 { 1.0 } else { -1.0 },
            reference_branch: if u.dot(&rod_offset) >= 0.0 { 1 } else { -1 },
            crank_radius_m: crank_planar.norm(),
            projected_rod_length_m: rod_planar.norm(),
            export_closure_error_m: closure_error,
            maximum_axis_error: axis_error,
            certification_length_tolerance_m: length_tol,
            certification_axis_tolerance: axis_tol,
        })
    }
}

impl AnalyticSliderCrank {
    /// Evaluate one explicitly selected assembly branch. `previous_coupler_rad`
    /// selects the nearest equivalent 2π angle, not another geometric branch.
    /// No clamping of an unreachable crank angle or a toggle is permitted.
    pub fn coordinates(
        &self,
        theta: f64,
        branch: i8,
        previous_coupler_rad: f64,
    ) -> Result<SliderCrankCoordinates, String> {
        if !theta.is_finite() || !previous_coupler_rad.is_finite() || ![-1, 1].contains(&branch) {
            return Err("finite angles and explicit branch -1 or +1 required".into());
        }
        let n = V::from(self.axis);
        let u = V::from(self.slider_axis);
        let v = n.cross(&u);
        let rot = Rotation3::from_axis_angle(&Unit::new_normalize(n), theta);
        let r = rot * V::from(self.crank_offset_m);
        let center = V::from(self.origin_m) + r;
        let dc = n.cross(&r);
        let ddc = n.cross(&dc);
        let delta = V::from(self.slider_point_m) - center;
        let a = u.dot(&delta);
        let b = v.dot(&delta);
        let da = -u.dot(&dc);
        let db = -v.dot(&dc);
        let dda = -u.dot(&ddc);
        let ddb = -v.dot(&ddc);
        let radius2 = self.projected_rod_length_m.powi(2);
        let disc = radius2 - b * b;
        if !disc.is_finite() || disc <= self.certification_length_tolerance_m.powi(2) {
            return Err("unreachable or singular slider-crank branch".into());
        }
        let root = disc.sqrt();
        let sign = f64::from(branch);
        let ds = sign * (-b * db / root) - da;
        let dds = sign * (-(db * db + b * ddb) / root - b * b * db * db / root.powi(3)) - dda;
        let slider = sign * root - a;
        let d = delta + u * slider;
        let dd = u * ds - dc;
        let ddd = u * dds - ddc;
        let d0 = V::from(self.rod_offset_m);
        let d0p = d0 - n * n.dot(&d0);
        let dp = d - n * n.dot(&d);
        let mut angle = n.dot(&d0p.cross(&dp)).atan2(d0p.dot(&dp));
        let expected = theta + self.coupler_axis_sign * previous_coupler_rad;
        angle += std::f64::consts::TAU * ((expected - angle) / std::f64::consts::TAU).round();
        let dangle = n.dot(&d.cross(&dd)) / radius2;
        let ddangle = n.dot(&d.cross(&ddd)) / radius2;
        let result = SliderCrankCoordinates {
            coupler_rad: (angle - theta) / self.coupler_axis_sign,
            slider_m: slider,
            first_derivative: [(dangle - 1.0) / self.coupler_axis_sign, ds],
            second_derivative: [ddangle / self.coupler_axis_sign, dds],
            rod_projection_m: sign * root,
        };
        if [
            result.coupler_rad,
            result.slider_m,
            result.first_derivative[0],
            result.first_derivative[1],
            result.second_derivative[0],
            result.second_derivative[1],
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("nonfinite analytic slider-crank coordinates".into());
        }
        Ok(result)
    }
}
