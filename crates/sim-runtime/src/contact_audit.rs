//! Contact impulse diagnostics on committed implicit substeps. Reporting-frame
//! force samples are insufficient around impacts; use the integrator's stages.
use crate::session::Session;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct FloorContactLinkProfile {
    pub link: String,
    pub material: String,
    pub compiled_surface_samples: usize,
    pub static_friction: f64,
    pub kinetic_friction: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct FloorContactProfile {
    pub expected_cad_sha256: String,
    pub floor_z_m: f64,
    pub stiffness_per_sample_n_m: f64,
    pub normal_dissipation_s_m: f64,
    pub friction_model: sim_domain_robot::articulated::friction::FloorFrictionModel,
    pub links: Vec<FloorContactLinkProfile>,
}
/// Inspect resolved runtime contact properties, including material-pair friction
/// and any compiled dissipation defaults/overrides. The authored world friction
/// field alone does not determine the actual link/world coefficient.
pub fn floor_contact_profile(
    art: &sim_domain_robot::Articulated,
    links: &[String],
    expected_cad_sha256: &str,
) -> Result<FloorContactProfile, String> {
    crate::tracking::compiled_surface_markers(art, links, expected_cad_sha256)?;
    Ok(FloorContactProfile {
        expected_cad_sha256: expected_cad_sha256.into(), floor_z_m: art.floor_z,
        stiffness_per_sample_n_m: art.floor_k, normal_dissipation_s_m: art.floor_dissipation_s_m,
        friction_model: art.floor_friction,
        links: links.iter().map(|name| {
            let link=art.links.iter().find(|l| &l.name==name).unwrap();
            FloorContactLinkProfile {link:name.clone(),material:link.material.clone(),
                compiled_surface_samples:link.contact.len(),static_friction:link.floor_mu.0,kinetic_friction:link.floor_mu.1}
        }).collect(),
    })
}

/// Full inter-link geometric inspection of a saved frame. Force-profile
/// reductions cannot turn this into a vacuous no-contact check.
pub fn sampled_inter_link_penetrations(
    art: &sim_domain_robot::Articulated,
    poses: &[crate::session::LinkPose],
) -> Result<Vec<sim_domain_robot::articulated::InterLinkPenetration>, String> {
    use nalgebra::{Matrix3, Vector3};
    if poses.len() != art.links.len() {
        return Err("one recorded rigid pose per articulated link required".into());
    }
    let links = art.links.iter().map(|l| {
        let found = poses.iter().filter(|p| p.name == l.name).collect::<Vec<_>>();
        if found.len() != 1 || !found[0].valid_rigid_transform() {
            return Err(format!("missing, ambiguous or invalid collision pose: {}", l.name));
        }
        let p = found[0];
        Ok(sim_domain_robot::articulated::LinkKin {
            p: Vector3::from(p.position_m),
            r: Matrix3::from_fn(|i,j| p.rotation[i][j]),
            vel: Vector3::zeros(), w: Vector3::zeros(),
            acc: Vector3::zeros(), alpha: Vector3::zeros(),
        })
    }).collect::<Result<Vec<_>, String>>()?;
    art.inter_link_penetrations(&links)
}

#[cfg(test)]
mod geometric_tests {
    use super::*;
    use sim_domain_robot::articulated::{Articulated, Options};
    #[test]
    fn recorded_overlap_remains_visible_when_forces_are_omitted() {
        let model = serde_json::from_value(serde_json::json!({
            "links":[{"name":"a"},{"name":"b"}],
            "joints":[{"name":"ab","parent":"a","child":"b","type":"revolute"}]
        })).unwrap();
        let mut art = Articulated::new(std::sync::Arc::new(model),
            &Options { flex:false, omit_inter_link_contact:true, ..Default::default() }).unwrap();
        art.links[0].contact = vec![nalgebra::Vector3::new(0.05,0.,0.)];
        art.links[0].excluded.clear(); art.links[1].contact.clear();
        art.links[1].lo = nalgebra::Vector3::repeat(-0.1);
        art.links[1].hi = nalgebra::Vector3::repeat(0.1);
        art.links[1].sdf = Some(sim_domain_robot::model::Sdf {
            origin:[-0.1;3], cell:0.2, dims:[2;3],
            values:vec![-0.11,0.09,-0.11,0.09,-0.11,0.09,-0.11,0.09],
            refinements: vec![],
        });
        let pose = |name:&str| crate::session::LinkPose { name:name.into(),
            position_m:[0.;3],rotation:[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]] };
        let mut poses=vec![pose("a"),pose("b")];
        let hits=sampled_inter_link_penetrations(&art,&poses).unwrap();
        assert_eq!(hits.len(),1);assert!((hits[0].penetration_m-0.01).abs()<1e-12);
        poses[1].position_m[0]=1.;
        assert!(sampled_inter_link_penetrations(&art,&poses).unwrap().is_empty());
        poses[1].name="a".into();
        assert!(sampled_inter_link_penetrations(&art,&poses).is_err());
        assert!(sampled_inter_link_penetrations(&art,&poses[..1]).is_err());
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FloorClearance {
    pub link: String,
    pub surface_samples: usize,
    /// Minimum world-Z separation from the runtime floor/height field.
    /// Uses compiled contact vertices, not an exact CAD surface or force test.
    pub minimum_clearance_m: f64,
}

/// Inspect recorded rigid poses against the same sampled surface vertices and
/// floor height used by the shared runtime. Caller must establish matching CAD
/// and world provenance. Positive gap is geometric, not inferred from zero force.
pub fn sampled_floor_clearances(
    art: &sim_domain_robot::Articulated,
    poses: &[crate::session::LinkPose],
    links: &[String],
) -> Result<Vec<FloorClearance>, String> {
    if links.is_empty()
        || links
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != links.len()
    {
        return Err("unique nonempty clearance links required".into());
    }
    links
        .iter()
        .map(|name| {
            let parts: Vec<_> = art.links.iter().filter(|l| &l.name == name).collect();
            let matches: Vec<_> = poses.iter().filter(|p| &p.name == name).collect();
            if parts.len() != 1 || matches.len() != 1 || !matches[0].valid_rigid_transform() {
                return Err(format!(
                    "missing, ambiguous or invalid clearance pose: {name}"
                ));
            }
            let part = parts[0];
            let pose = matches[0];
            if part.contact.is_empty() {
                return Err(format!("no compiled surface samples for {name}"));
            }
            let mut minimum = f64::INFINITY;
            for local in &part.contact {
                let world: [f64; 3] = std::array::from_fn(|i| {
                    pose.position_m[i] + (0..3).map(|j| pose.rotation[i][j] * local[j]).sum::<f64>()
                });
                let gap = world[2] - art.floor_height(world[0], world[1]);
                if !gap.is_finite() {
                    return Err("nonfinite surface/floor clearance".into());
                }
                minimum = minimum.min(gap);
            }
            Ok(FloorClearance {
                link: name.clone(),
                surface_samples: part.contact.len(),
                minimum_clearance_m: minimum,
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct ContactImpulse {
    pub link: usize,
    pub other: Option<usize>,
    /// Force on `link`, in world axes, integrated with the method's stage rule.
    pub impulse_ns: [f64; 3],
    pub peak_stage_force_n: f64,
    /// Duration of steps where this pair had at least one contact stage sample.
    /// This is not a continuously located contact-event duration.
    pub active_step_duration_s: f64,
}
#[derive(Debug, Serialize)]
pub struct ContactImpulseReport {
    pub start_time_s: f64,
    pub end_time_s: f64,
    pub committed_steps: usize,
    pub contacts: Vec<ContactImpulse>,
    pub notes: Vec<&'static str>,
}

/// Requires complete audit coverage of a window bounded by committed steps.
/// Restored/discarded trials are excluded; unknown legacy status is rejected.
/// Evaluates the unchanged articulated contact law at recorded stage states.
/// Includes linear contact impulse only, not an unreported torsional moment.
pub fn committed_contact_impulses(
    session: &Session,
    start: f64,
    end: f64,
) -> Result<ContactImpulseReport, String> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err("contact impulse window must be finite with 0 <= start < end".into());
    }
    let mut samples = Vec::new();
    let mut physical_island = None;
    for (index, island) in session.robot.runtime.islands.iter().enumerate() {
        if island.attempt_audit_limit() == 0
            || island.implicit_attempts.len() == island.attempt_audit_limit()
        {
            return Err("contact impulse audit is disabled or reached its capacity".into());
        }
        for point in &island.implicit_attempts {
            let finish = point.start_time + point.step;
            if finish <= start || point.start_time >= end {
                continue;
            }
            let Some(g) = session.robot.generalized_at_solver_point(index, point)? else {
                continue;
            };
            match point.committed {
                None => return Err("legacy attempt has unknown commit status".into()),
                Some(false) => continue,
                Some(true) => {}
            }
            if !point.solve_succeeded {
                return Err("committed attempt did not solve successfully".into());
            }
            if physical_island.is_some_and(|i| i != index) {
                return Err("robot spans multiple audited islands".into());
            }
            physical_island = Some(index);
            let evaluation = session.robot.art.evaluate(&g);
            let mut forces: BTreeMap<(usize, Option<usize>), [f64; 3]> = BTreeMap::new();
            for contact in evaluation.contacts {
                let sum = forces.entry((contact.link, contact.other)).or_default();
                for (value, force) in sum.iter_mut().zip(contact.force.iter()) {
                    *value += force;
                }
            }
            samples.push((point.start_time, point.step, forces));
        }
    }
    integrate_samples(start, end, &samples)
}

/// Integrate a complete accepted reduced-model trace using the same stage
/// quadrature and coverage checks as the detailed runtime. The motor adapter
/// emits only accepted BE endpoints; no event-search candidates belong here.
pub fn embedded_contact_impulses(
    start: f64,
    end: f64,
    steps: &[sim_domain_robot::articulated::embedding::EmbeddedContactStep],
) -> Result<ContactImpulseReport, String> {
    let samples: Vec<_> = steps
        .iter()
        .map(|step| {
            let mut forces: BTreeMap<(usize, Option<usize>), [f64; 3]> = BTreeMap::new();
            for contact in &step.contacts {
                let sum = forces.entry((contact.link, contact.other)).or_default();
                for (value, force) in sum.iter_mut().zip(contact.force_n) {
                    *value += force;
                }
            }
            (step.start_time_s, step.step_s, forces)
        })
        .collect();
    integrate_samples(start, end, &samples)
}

type Sample = (f64, f64, BTreeMap<(usize, Option<usize>), [f64; 3]>);

#[derive(Debug, Serialize)]
pub struct FloorForceRatio {
    pub link: usize,
    pub maximum_ratio: f64,
    pub endpoint_time_s: f64,
    pub step_s: f64,
    pub normal_force_n: f64,
    pub tangential_force_n: f64,
    pub qualifying_steps: usize,
}

/// Diagnostic of the current articulated floor law, whose normal is world +Z.
/// Sum all points on a body's patch before computing |F_xy| / F_z. Internal
/// body contacts are excluded. The explicit positive force cutoff avoids
/// magnifying vanishing loads; omitted bodies had no qualifying stage.
/// This reports forces, not a calibrated Coulomb bound or friction coefficient.
/// No torsional moment is present in this trace.
pub fn embedded_floor_force_ratios(
    start: f64,
    end: f64,
    steps: &[sim_domain_robot::articulated::embedding::EmbeddedContactStep],
    minimum_normal_force_n: f64,
) -> Result<Vec<FloorForceRatio>, String> {
    if !minimum_normal_force_n.is_finite() || minimum_normal_force_n <= 0.0 {
        return Err("floor force-ratio cutoff must be positive and finite".into());
    }
    // Reuse complete accepted-stage coverage and finite-force validation.
    embedded_contact_impulses(start, end, steps)?;
    let mut peaks = BTreeMap::<usize, FloorForceRatio>::new();
    for step in steps {
        let mut sums = BTreeMap::<usize, [f64; 3]>::new();
        for contact in &step.contacts {
            if contact.other.is_none() {
                let force = sums.entry(contact.link).or_default();
                for (sum, value) in force.iter_mut().zip(contact.force_n) {
                    *sum += value;
                }
            }
        }
        for (link, force) in sums {
            if force[2] < 0.0 {
                return Err("floor trace contains negative normal load".into());
            }
            if force[2] < minimum_normal_force_n {
                continue;
            }
            let tangential = force[0].hypot(force[1]);
            let ratio = tangential / force[2];
            if !ratio.is_finite() {
                return Err("nonfinite floor force ratio".into());
            }
            let peak = peaks.entry(link).or_insert(FloorForceRatio {
                link,
                maximum_ratio: -1.0,
                endpoint_time_s: 0.0,
                step_s: 0.0,
                normal_force_n: 0.0,
                tangential_force_n: 0.0,
                qualifying_steps: 0,
            });
            peak.qualifying_steps += 1;
            if ratio > peak.maximum_ratio {
                peak.maximum_ratio = ratio;
                peak.endpoint_time_s = step.start_time_s + step.step_s;
                peak.step_s = step.step_s;
                peak.normal_force_n = force[2];
                peak.tangential_force_n = tangential;
            }
        }
    }
    Ok(peaks.into_values().collect())
}

fn integrate_samples(
    start: f64,
    end: f64,
    samples: &[Sample],
) -> Result<ContactImpulseReport, String> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err("contact impulse window must be finite with 0 <= start < end".into());
    }
    let mut cursor = start;
    let mut totals: BTreeMap<(usize, Option<usize>), ContactImpulse> = BTreeMap::new();
    for (time, h, forces) in samples {
        let tolerance = 128.0 * f64::EPSILON * end.abs().max(time.abs()).max(h.abs());
        if !time.is_finite()
            || !h.is_finite()
            || *h <= 0.0
            || (time - cursor).abs() > tolerance
            || time + h > end + tolerance
        {
            return Err("committed contact stages do not partition the requested window".into());
        }
        cursor = time + h;
        for (&(link, other), force) in forces {
            if force.iter().any(|f| !f.is_finite()) {
                return Err("nonfinite committed contact force".into());
            }
            let total = totals.entry((link, other)).or_insert(ContactImpulse {
                link,
                other,
                impulse_ns: [0.0; 3],
                peak_stage_force_n: 0.0,
                active_step_duration_s: 0.0,
            });
            for (impulse, f) in total.impulse_ns.iter_mut().zip(force) {
                *impulse += h * f;
            }
            total.peak_stage_force_n = total
                .peak_stage_force_n
                .max(force.iter().map(|v| v * v).sum::<f64>().sqrt());
            total.active_step_duration_s += h;
        }
    }
    if samples.is_empty() || (cursor - end).abs() > 128.0 * f64::EPSILON * end.abs() {
        return Err("incomplete committed contact-stage coverage".into());
    }
    Ok(ContactImpulseReport {
        start_time_s: start,
        end_time_s: end,
        committed_steps: samples.len(),
        contacts: totals.into_values().collect(),
        notes: vec![
            "Linear impulse in world axes: sum of stage contact force times committed substep duration.",
            "Backward Euler uses endpoint force; implicit midpoint uses midpoint force. This is method-consistent quadrature, not an exact continuous impulse.",
            "Rejected candidates, event-search probes and superseded local commits are excluded. Unknown or incomplete coverage is rejected.",
            "No torsional contact moment or work/energy integral is included. Contact-event duration is not located continuously.",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_quadrature_integrates_linear_force_and_tracks_pair_presence() {
        // Midpoint quadrature is exact for F(t)=[2t,-3,4] over [0,1].
        let mut t = 0.0;
        let mut samples = vec![];
        for h in [0.125, 0.375, 0.5] {
            let mut forces = BTreeMap::from([((1, None), [2.0 * (t + h / 2.0), -3.0, 4.0])]);
            if t >= 0.5 {
                forces.insert((2, Some(3)), [10.0, 0.0, 0.0]);
            }
            samples.push((t, h, forces));
            t += h;
        }
        let report = integrate_samples(0.0, 1.0, &samples).unwrap();
        assert_eq!(report.contacts[0].impulse_ns, [1.0, -3.0, 4.0]);
        assert_eq!(report.contacts[0].active_step_duration_s, 1.0);
        assert_eq!(report.contacts[1].impulse_ns, [5.0, 0.0, 0.0]);
        assert_eq!(report.contacts[1].active_step_duration_s, 0.5);
        // Endpoint quadrature integrates the same force with its BE bias.
        for (t, h, f) in &mut samples {
            f.get_mut(&(1, None)).unwrap()[0] = 2.0 * (*t + *h);
        }
        assert_eq!(
            integrate_samples(0.0, 1.0, &samples).unwrap().contacts[0].impulse_ns[0],
            1.40625
        );
    }
    #[test]
    fn missing_overlapping_and_partial_windows_are_rejected() {
        let samples = vec![(0.0, 0.5, BTreeMap::new()), (0.5, 0.5, BTreeMap::new())];
        assert!(integrate_samples(0.0, 1.0, &samples).is_ok());
        assert!(integrate_samples(0.0, 1.0, &samples[..1]).is_err());
        let mut overlap = samples.clone();
        overlap[1].0 = 0.25;
        assert!(integrate_samples(0.0, 1.0, &overlap).is_err());
        assert!(integrate_samples(0.1, 1.0, &samples).is_err());
        assert!(integrate_samples(0.0, 0.9, &samples).is_err());
    }
}

#[cfg(test)]
mod embedded_tests {
    use super::*;
    use sim_domain_robot::articulated::embedding::{EmbeddedContactSample, EmbeddedContactStep};
    #[test]
    fn floor_force_ratio_uses_patch_resultant_and_explicit_load_cutoff() {
        let contact = |force, other| EmbeddedContactSample {
            link: 1,
            other,
            force_n: force,
            point_m: [0.0; 3],
            penetration_m: 0.01,
        };
        let mut steps = vec![
            EmbeddedContactStep {
                start_time_s: 0.0,
                step_s: 0.5,
                contacts: vec![
                    contact([3.0, 4.0, 1.0], None),
                    contact([-3.0, -4.0, 1.0], None),
                    contact([1e6, 0.0, 0.1], Some(2)),
                ],
            },
            EmbeddedContactStep {
                start_time_s: 0.5,
                step_s: 0.5,
                contacts: vec![contact([3.0, 4.0, 2.0], None)],
            },
        ];
        let r = embedded_floor_force_ratios(0.0, 1.0, &steps, 0.1).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].maximum_ratio, 2.5);
        assert_eq!(r[0].endpoint_time_s, 1.0);
        assert_eq!(r[0].tangential_force_n, 5.0);
        assert_eq!(r[0].qualifying_steps, 2);
        assert!(
            embedded_floor_force_ratios(0.0, 1.0, &steps, 3.0)
                .unwrap()
                .is_empty()
        );
        assert!(embedded_floor_force_ratios(0.0, 1.0, &steps, 0.0).is_err());
        assert!(embedded_floor_force_ratios(0.0, 1.0, &steps[..1], 0.1).is_err());
        steps[1].contacts[0].force_n[2] = -1.0;
        assert!(embedded_floor_force_ratios(0.0, 1.0, &steps, 0.1).is_err());
    }
    #[test]
    fn embedded_trace_aggregates_points_and_requires_all_accepted_time() {
        let contact = |force| EmbeddedContactSample {
            link: 1,
            other: None,
            force_n: force,
            point_m: [0.0; 3],
            penetration_m: 0.01,
        };
        let steps = vec![
            EmbeddedContactStep {
                start_time_s: 0.0,
                step_s: 0.25,
                contacts: vec![contact([2.0, 0.0, 4.0]), contact([0.0, 1.0, 4.0])],
            },
            EmbeddedContactStep {
                start_time_s: 0.25,
                step_s: 0.5,
                contacts: vec![],
            },
            EmbeddedContactStep {
                start_time_s: 0.75,
                step_s: 0.25,
                contacts: vec![contact([-2.0, 0.0, 4.0])],
            },
        ];
        let report = embedded_contact_impulses(0.0, 1.0, &steps).unwrap();
        assert_eq!(report.committed_steps, 3);
        assert_eq!(report.contacts.len(), 1);
        assert_eq!(report.contacts[0].impulse_ns, [0.0, 0.25, 3.0]);
        assert_eq!(report.contacts[0].active_step_duration_s, 0.5);
        let empty = embedded_contact_impulses(
            0.0,
            1.0,
            &[EmbeddedContactStep {
                start_time_s: 0.0,
                step_s: 1.0,
                contacts: vec![],
            }],
        )
        .unwrap();
        let delta = compare_impulse_reports(&report, &empty).unwrap();
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].candidate_ns, [0.0, 0.25, 3.0]);
        assert_eq!(delta[0].reference_ns, [0.0; 3]);
        assert!((delta[0].difference_norm_ns - (9.0625_f64).sqrt()).abs() < 1e-15);
        let shifted = embedded_contact_impulses(
            1.0,
            2.0,
            &[EmbeddedContactStep {
                start_time_s: 1.0,
                step_s: 1.0,
                contacts: vec![],
            }],
        )
        .unwrap();
        assert!(compare_impulse_reports(&report, &shifted).is_err());
        assert!(
            embedded_contact_impulses(0.0, 1.0, &[steps[0].clone(), steps[2].clone()]).is_err()
        );
        assert!(embedded_contact_impulses(f64::NAN, 1.0, &steps).is_err());
        let mut invalid = steps.clone();
        invalid[0].contacts[0].force_n[0] = f64::NAN;
        assert!(embedded_contact_impulses(0.0, 1.0, &invalid).is_err());
    }
}

#[derive(Debug, Serialize)]
pub struct ContactImpulseDifference {
    pub link: usize,
    pub other: Option<usize>,
    pub candidate_ns: [f64; 3],
    pub reference_ns: [f64; 3],
    pub difference_norm_ns: f64,
}

/// Compare matching time windows in shared world axes and body numbering.
/// The caller must establish matching physical source and coordinate frames.
/// An absent contact pair contributes zero impulse, rather than disappearing
/// from the comparison. No error threshold or hardware accuracy is inferred.
pub fn compare_impulse_reports(
    candidate: &ContactImpulseReport,
    reference: &ContactImpulseReport,
) -> Result<Vec<ContactImpulseDifference>, String> {
    for report in [candidate, reference] {
        if !report.start_time_s.is_finite()
            || !report.end_time_s.is_finite()
            || report.start_time_s < 0.0
            || report.end_time_s <= report.start_time_s
            || report
                .contacts
                .iter()
                .any(|c| c.impulse_ns.iter().any(|v| !v.is_finite()))
        {
            return Err("invalid contact impulse report".into());
        }
    }
    let tolerance =
        128.0 * f64::EPSILON * candidate.end_time_s.abs().max(reference.end_time_s.abs());
    if (candidate.start_time_s - reference.start_time_s).abs() > tolerance
        || (candidate.end_time_s - reference.end_time_s).abs() > tolerance
    {
        return Err("contact impulse comparison requires matching windows".into());
    }
    let a: BTreeMap<_, _> = candidate
        .contacts
        .iter()
        .map(|c| ((c.link, c.other), c.impulse_ns))
        .collect();
    let b: BTreeMap<_, _> = reference
        .contacts
        .iter()
        .map(|c| ((c.link, c.other), c.impulse_ns))
        .collect();
    let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).copied().collect();
    Ok(keys
        .into_iter()
        .map(|(link, other)| {
            let candidate_ns = a.get(&(link, other)).copied().unwrap_or_default();
            let reference_ns = b.get(&(link, other)).copied().unwrap_or_default();
            let difference_norm_ns = candidate_ns
                .iter()
                .zip(reference_ns)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt();
            ContactImpulseDifference {
                link,
                other,
                candidate_ns,
                reference_ns,
                difference_norm_ns,
            }
        })
        .collect())
}
