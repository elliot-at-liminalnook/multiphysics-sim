//! Kinematics and contact reuse scoped to one linearization. Changes to
//! accelerations or constraint reactions leave contact unchanged. Twist and
//! bristle changes reuse geometry but recompute forces; pose changes query
//! geometry again. Full kinematics requires unchanged motion and acceleration.
//! Complete evaluations can also be borrowed when only rates unused by the
//! force calculation change; write_residual still consumes the current rates.
//! No mutable cache is shared across workers or steps.
use super::*;

#[derive(Clone)]
pub(super) struct ContactHit {
    pub sample: usize,
    pub other: usize,
    pub depth: f64,
    pub normal: V,
}

#[derive(Clone)]
pub(super) struct ContactSample {
    pub offset: V,
    pub point: V,
    pub floor_depth: f64,
}

/// Exact fixed-pose sample positions, terrain depths and SDF query results,
/// including the absence of contacts.
/// Geometry and exclusions belong to the immutable articulated model. Contact
/// forces and bristle evolution are deliberately absent from this cache.
pub(super) struct ContactGeometry {
    inter_link: bool,
    // Exclusion metadata is shared only within this immutable model borrow.
    // Sparse adjacency avoids quadratic storage for large assemblies.
    bands: std::sync::Arc<Vec<Vec<(usize, (usize, V, f64))>>>,
    poses: Vec<(M, V)>,
    boxes: Vec<(V, V)>,
    pub samples: Vec<Vec<ContactSample>>,
    pub hits: Vec<Vec<ContactHit>>,
}
impl ContactGeometry {
    pub fn matches(&self, links: &[LinkKin]) -> bool {
        self.poses.len() == links.len()
            && self.poses.iter().zip(links).all(|((r, p), k)| {
                r.iter()
                    .zip(k.r.iter())
                    .chain(p.iter().zip(k.p.iter()))
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            })
    }

    pub fn new(art: &Articulated, links: &[LinkKin]) -> Self {
        Self::new_reusing(art, links, None)
    }

    /// Reuse only pairs whose two poses are bit-identical to the prepared
    /// point. Absence of a hit is reusable too; a moving target must always be
    /// queried, even when the sample's own link did not move.
    /// `base` belongs to the same immutably borrowed articulated model; fresh
    /// evaluations after model edits construct a new cache with `new`.
    pub fn new_reusing(art: &Articulated, links: &[LinkKin], base: Option<&Self>) -> Self {
        sim_solve::profile::CONTACT_GEOMETRY.time(|| Self::new_reusing_impl(art, links, base, !art.omit_inter_link_contact))
    }

    /// Full geometry inspection regardless of the dynamics fidelity reduction.
    pub fn new_all(art: &Articulated, links: &[LinkKin]) -> Self {
        Self::new_reusing_impl(art, links, None, true)
    }

    fn new_reusing_impl(art: &Articulated, links: &[LinkKin], base: Option<&Self>, inter_link: bool) -> Self {
        let base = base.filter(|b| b.inter_link == inter_link);
        let bands = sim_solve::profile::CONTACT_TOPOLOGY.time(|| match base {
            Some(base) => base.bands.clone(),
            None if !inter_link => std::sync::Arc::new(vec![vec![]; art.links.len()]),
            None => {
                let mut rows: Vec<Vec<(usize, (usize, V, f64))>> = vec![Vec::new(); art.links.len()];
                for (a, b) in art.joints.iter().map(|j| (j.parent, j.child))
                    .chain(art.loops.iter().map(|lp| (lp.a, lp.b))) {
                    if rows[a].iter().any(|(other, _)| *other == b) { continue; }
                    // Keep the original first-tree-joint/first-loop selection
                    // including duplicate pairs. Only local metadata is cached.
                    let band = art.neighbour_band(a, b).unwrap();
                    rows[a].push((b, band));
                    if a != b { rows[b].push((a, band)); }
                }
                for row in &mut rows { row.sort_unstable_by_key(|(other, _)| *other); }
                std::sync::Arc::new(rows)
            }
        });
        let unchanged: Vec<bool> = links.iter().enumerate().map(|(i, k)| {
            base.and_then(|b| b.poses.get(i)).is_some_and(|(r, p)| {
                r.iter().zip(k.r.iter()).chain(p.iter().zip(k.p.iter()))
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            })
        }).collect();
        let boxes: Vec<(V, V)> = art
            .links
            .iter()
            .zip(links)
            .enumerate()
            .map(|(i, (l, k))| if unchanged[i] { base.unwrap().boxes[i] } else { world_box(l, k) })
            .collect();
        let samples: Vec<Vec<ContactSample>> = sim_solve::profile::CONTACT_SAMPLES.time(|| art.links.iter().zip(links).enumerate().map(|(i, (l, k))| {
            if unchanged[i] { return base.unwrap().samples[i].clone(); }
            l.contact.iter().map(|c| {
                let offset = k.r * c;
                let point = k.p + offset;
                let floor_depth = if l.grounded { 0.0 } else {
                    art.floor_height(point.x, point.y) - point.z
                };
                ContactSample { offset, point, floor_depth }
            }).collect()
        }).collect());
        let hits = sim_solve::profile::CONTACT_PAIRS.time(|| art
            .links
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if !inter_link { return Vec::new(); }
                let candidates = contact_candidates(samples[i].iter().map(|s| &s.point), &boxes);
                let mut hits: Vec<ContactHit> = if unchanged[i] {
                    base.unwrap().hits[i].iter().filter(|h| unchanged[h.other]).cloned().collect()
                } else { Vec::new() };
                // Preserve the uncached sample/pair order for force accumulation.
                for (ci, sample) in samples[i].iter().enumerate() {
                    let pt = sample.point;
                    for &j in &candidates {
                        if unchanged[i] && unchanged[j] { continue; }
                        if j == i || art.links[j].sdf.is_none() {
                            continue;
                        }
                        if l.excluded.get(&j).map(|f| f[ci]).unwrap_or(false) {
                            continue;
                        }
                        let (lo, hi) = &boxes[j];
                        if pt.x < lo.x
                            || pt.y < lo.y
                            || pt.z < lo.z
                            || pt.x > hi.x
                            || pt.y > hi.y
                            || pt.z > hi.z
                        {
                            continue;
                        }
                        if let Ok(at) = bands[i].binary_search_by_key(&j, |(other, _)| *other) {
                            let band = bands[i][at].1;
                            let anchor = &links[band.0];
                            let center = anchor.p + anchor.r * band.1;
                            if crate::sdf::inside_exclusion_band(pt, center, band.2) {
                                continue;
                            }
                        }
                        let kj = &links[j];
                        let local = kj.r.transpose() * (pt - kj.p);
                        let (phi, grad) = art.links[j].sdf.as_ref().unwrap().sample(local);
                        if phi >= 0.0 {
                            continue;
                        }
                        hits.push(ContactHit {
                            sample: ci,
                            other: j,
                            depth: -phi,
                            normal: kj.r * grad,
                        });
                    }
                }
                // Merge reused and fresh hits in the original sample/pair
                // traversal order so force summation remains identical.
                if unchanged[i] { hits.sort_unstable_by_key(|h| (h.sample, h.other)); }
                hits
            })
            .collect());
        Self {
            inter_link,
            bands,
            poses: links.iter().map(|k| (k.r, k.p)).collect(),
            boxes,
            samples,
            hits,
        }
    }
}

#[derive(Clone)]
pub(super) struct ContactForces {
    pub f_ext: Vec<V>,
    pub t_ext: Vec<V>,
    pub contacts: Vec<ContactPoint>,
    pub bristle_rates: Vec<f64>,
    pub contact_normal: Vec<f64>,
}

pub(super) type Kinematics = (Vec<LinkKin>, Vec<V>, Vec<Vec<V>>);

// Contact uses pose and velocity, not acceleration. Keep signed zeros distinct
// and cover all base/modal motion inputs before borrowing prepared link motion.
fn same_contact_motion(art: &Articulated, base: &Generalized, g: &Generalized) -> bool {
    let same = |a: &[f64], b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(a,b)| a.to_bits() == b.to_bits())
    };
    let states = |start, count| same(&g.states[start..start+count], &base.states[start..start+count]);
    same(&g.q, &base.q) && same(&g.qd, &base.qd)
        && art.bases.iter().all(|b| states(b.state, BASE_STATES))
        && art.links.iter().all(|l| l.flex.as_ref().is_none_or(|f| states(f.state, 2*f.modes)))
}

/// Immutable contact-only preparation. It never computes inverse dynamics or
/// retains finished forces. Every history query evaluates the original contact
/// law at the supplied history; changed motion refreshes link kinematics and
/// changed poses refresh geometry through ContactGeometry's exact checks.
pub(super) struct ContactHistoryPreparation<'a> {
    art: &'a Articulated,
    base: Generalized,
    links: Vec<LinkKin>,
    geometry: ContactGeometry,
}
impl<'a> ContactHistoryPreparation<'a> {
    pub fn new(art: &'a Articulated, base: Generalized) -> Self {
        let links = art.kinematics(&base).0;
        let geometry = ContactGeometry::new(art, &links);
        Self { art, base, links, geometry }
    }

    pub fn rates(&self, g: &Generalized) -> Vec<f64> {
        let fresh;
        let links = if same_contact_motion(self.art, &self.base, g) {
            &self.links
        } else {
            fresh = self.art.kinematics(g).0;
            &fresh
        };
        self.art.contact_forces(g, links, true, self.art.gravity, Some(&self.geometry)).bristle_rates
    }
}

pub(super) struct ContactLinearization<'a> {
    art: &'a Articulated,
    base: Generalized,
    contact: ContactForces,
    geometry: ContactGeometry,
    kinematics: Kinematics,
    evaluation: Evaluation,
}
impl<'a> ContactLinearization<'a> {
    pub fn new(art: &'a Articulated, view: &View, rates: &[f64]) -> Self {
        let mut base = art.read_view(view);
        // Match the rates consumed by kinematics through read_ctx. Grounded
        // base rates are algebraic and remain zero, even if the supplied slice
        // contains unused nonzero entries. Other rates do not affect kinematics.
        for b in &art.bases {
            if !b.grounded {
                base.rates[b.state + 7..b.state + 13]
                    .copy_from_slice(&rates[b.state + 7..b.state + 13]);
            }
        }
        for (i, (_, d)) in art.dofs().enumerate() {
            base.qdd[i] = rates[d.qd_state];
        }
        for l in &art.links {
            if let Some(f) = &l.flex {
                let start = f.state + f.modes;
                base.rates[start..start + f.modes].copy_from_slice(&rates[start..start + f.modes]);
            }
        }
        Self::at(art, base)
    }

    pub(super) fn at(art: &'a Articulated, base: Generalized) -> Self {
        let kinematics = art.kinematics(&base);
        let geometry = ContactGeometry::new(art, &kinematics.0);
        let contact = art.contact_forces(&base, &kinematics.0, true, art.gravity, Some(&geometry));
        let evaluation = art.evaluate_reusing_contacts(&base, true, false,
            Some(&contact), Some(&geometry), Some(&kinematics));
        Self {
            art,
            base,
            contact,
            geometry,
            kinematics,
            evaluation,
        }
    }

    fn matches_motion(&self, g: &Generalized) -> bool {
        same_contact_motion(self.art, &self.base, g)
    }

    /// Share the same dependency checks with hybrid derivative sampling.
    pub fn evaluate(&self, g: &Generalized) -> Evaluation {
        self.evaluate_shared(g).into_owned()
    }

    fn evaluate_shared(&self, g: &Generalized) -> std::borrow::Cow<'_, Evaluation> {
        let same = |a: &[f64], b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
        };
        let motion_matches = self.matches_motion(g);
        let contact_matches = motion_matches
            && self.art.links.iter().all(|l| {
                l.grounded
                    || same(
                        &g.states[l.bristle_state..l.bristle_state + 3],
                        &self.base.states[l.bristle_state..l.bristle_state + 3],
                    )
            });
        let kinematics_matches = motion_matches
            && same(&g.qdd, &self.base.qdd)
            && self.art.bases.iter().all(|b| {
                same(
                    &g.rates[b.state + 7..b.state + 13],
                    &self.base.rates[b.state + 7..b.state + 13],
                )
            })
            && self.art.links.iter().all(|l| {
                l.flex.as_ref().is_none_or(|f| {
                    let start = f.state + f.modes;
                    same(
                        &g.rates[start..start + f.modes],
                        &self.base.rates[start..start + f.modes],
                    )
                })
            });
        // evaluate_reusing_contacts consumes rates only through kinematics
        // (base/joint/modal accelerations). Position, bristle and sensor rates
        // enter write_residual separately and must still be evaluated there.
        // Compare all states and temperature inputs conservatively, including
        // unused ones, so reactions and every constitutive input remain covered.
        if kinematics_matches && same(&g.states, &self.base.states)
            && same(&g.temperatures, &self.base.temperatures) {
            return std::borrow::Cow::Borrowed(&self.evaluation);
        }
        std::borrow::Cow::Owned(self.art.evaluate_reusing_contacts(
            g,
            true,
            false,
            contact_matches.then_some(&self.contact),
            Some(&self.geometry),
            kinematics_matches.then_some(&self.kinematics),
        ))
    }
}
impl sim_core::PreparedResidual for ContactLinearization<'_> {
    fn residual(&self, ctx: &mut Context) {
        let g = self.art.read_ctx(ctx);
        let e = self.evaluate_shared(&g);
        self.art.write_residual(ctx, &g, &e);
    }
}

#[cfg(test)]
mod exclusion_tests {
    use super::*;

    #[test]
    fn joint_exclusion_follows_translation_rotation_and_preserves_outer_contacts() {
        for kind in ["revolute", "loop_revolute"] {
            let model = serde_json::from_value(serde_json::json!({
                "links": [{"name":"a"}, {"name":"b"}],
                "joints": [{"name":"ab", "parent":"a", "child":"b",
                    "origin":[0.02,0.0,0.0], "type":kind}]
            })).unwrap();
            let mut art = Articulated::new(std::sync::Arc::new(model),
                &Options { flex: false, ..Default::default() }).unwrap();
            // Three samples within the target: inside, on, and beyond the
            // existing 10 mm joint-region radius. The boundary remains active.
            art.links[0].contact = vec![V::new(0.024,0.0,0.0), V::new(0.03,0.0,0.0), V::new(0.05,0.0,0.0)];
            art.links[0].excluded.clear();
            art.links[1].contact.clear();
            art.links[1].lo = V::repeat(-0.1);
            art.links[1].hi = V::repeat(0.1);
            // Exact signed distance to the plane z=0.01 over the query box.
            art.links[1].sdf = Some(Sdf { origin:[-0.1;3], cell:0.2,
                dims:[2;3], values:vec![-0.11,0.09,-0.11,0.09,-0.11,0.09,-0.11,0.09] });
            let pose = |r, p| LinkKin {r,p,w:V::zeros(),vel:V::zeros(),alpha:V::zeros(),acc:V::zeros()};
            let mut original = vec![pose(M::identity(),V::zeros());2];
            let first = ContactGeometry::new(&art, &original);
            assert_eq!(first.hits[0].iter().map(|h| h.sample).collect::<Vec<_>>(),vec![1,2]);
            // A dynamics reduction must not disable independent geometric checks.
            art.floor_z = 0.005;
            original[0].vel.x = 0.02;
            let states = art.states().iter().map(|s| s.initial).collect();
            let g = art.generalized(states, vec![0.;art.state_count],
                &vec![0.;art.port_names.len()+1], vec![]);
            let full_forces = art.contact_forces(&g, &original, true, V::zeros(), None);
            assert!(full_forces.contacts.iter().any(|c| c.other.is_some()));
            assert!(full_forces.contacts.iter().any(|c| c.other.is_none() && c.force.z > 0.));
            art.omit_inter_link_contact = true;
            let reduced = art.contact_forces(&g, &original, true, V::zeros(), None);
            assert!(reduced.contacts.iter().all(|c| c.other.is_none()));
            let floor = |e: &ContactForces| e.contacts.iter().filter(|c| c.other.is_none())
                .map(|c| (c.link,c.point,c.force,c.penetration)).collect::<Vec<_>>();
            assert_eq!(floor(&full_forces), floor(&reduced));
            assert_eq!(full_forces.bristle_rates, reduced.bristle_rates);
            assert!(reduced.bristle_rates.iter().any(|v| v.abs() > 1e-8));
            let inspected = art.inter_link_penetrations(&original).unwrap();
            assert_eq!(inspected.len(),2);
            assert!(inspected.iter().all(|h| (h.penetration_m-0.01).abs()<1e-12));
            assert!(art.inter_link_penetrations(&original[..1]).is_err());
            let mut invalid = original.clone(); invalid[0].r[(0,0)] = 2.;
            assert!(art.inter_link_penetrations(&invalid).is_err());
            art.omit_inter_link_contact = false;
            let rotation = nalgebra::Rotation3::from_euler_angles(0.3,-0.6,1.1).into_inner();
            let moved = vec![pose(rotation,V::new(2.0,-3.0,4.0));2];
            let fresh = ContactGeometry::new(&art, &moved);
            let reused = ContactGeometry::new_reusing(&art, &moved, Some(&first));
            assert!(std::sync::Arc::ptr_eq(&first.bands,&reused.bands));
            for cache in [&fresh,&reused] {
                assert_eq!(cache.hits[0].iter().map(|h| h.sample).collect::<Vec<_>>(),vec![1,2]);
                for hit in &cache.hits[0] {
                    assert!((hit.depth-0.01).abs()<1e-12);
                    assert!((hit.normal-rotation*V::z()).norm()<1e-12);
                }
            }
        }
    }

    #[test]
    fn exclusion_cache_preserves_pair_priority_and_refreshes_after_model_edits() {
        let model = serde_json::from_value(serde_json::json!({
            "links": [{"name":"a"}, {"name":"b"}, {"name":"c"}],
            "joints": [
                {"name":"ab", "parent":"a", "child":"b", "type":"revolute"},
                {"name":"loop ab", "parent":"a", "child":"b", "type":"loop_revolute"},
                {"name":"loop bc", "parent":"b", "child":"c", "type":"loop_revolute"}
            ]
        })).unwrap();
        let mut art = Articulated::new(std::sync::Arc::new(model), &Options { flex: false, ..Default::default() }).unwrap();
        let g = art.generalized(art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count], &[], vec![]);
        let links = art.kinematics(&g).0;
        // Deliberate duplicate/reversed pairs with different metadata. First
        // tree joint wins over later tree joints and all loop definitions.
        art.joints[0].band = -0.0;
        let mut duplicate = art.joints[0].clone();
        std::mem::swap(&mut duplicate.parent, &mut duplicate.child);
        duplicate.r_pj = V::new(8.0, 9.0, 10.0);
        duplicate.band = 2.0;
        art.joints.push(duplicate);
        let mut duplicate_loop = art.loops[1].clone();
        duplicate_loop.r_a = V::new(3.0, 4.0, 5.0);
        art.loops.push(duplicate_loop);
        let bits = |b: Option<(usize, V, f64)>| b.map(|(anchor, p, r)| (anchor, [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), r.to_bits()]));
        let check = |art: &Articulated, cache: &ContactGeometry| {
            for i in 0..art.links.len() {
                for j in 0..art.links.len() {
                    let cached = cache.bands[i].iter().find(|(other, _)| *other == j).map(|(_, band)| *band);
                    assert_eq!(bits(cached), bits(art.neighbour_band(i, j)), "pair {i},{j}");
                }
            }
        };
        let first = ContactGeometry::new(&art, &links);
        check(&art, &first);
        let reused = ContactGeometry::new_reusing(&art, &links, Some(&first));
        assert!(std::sync::Arc::ptr_eq(&first.bands, &reused.bands));
        check(&art, &reused);
        // A later evaluation must observe edits, including removal of the tree
        // definitions so that the former lower-priority loop now supplies a pair.
        art.joints.clear();
        art.loops[0].r_a = V::new(0.2, 0.3, 0.4);
        let fresh = ContactGeometry::new(&art, &links);
        check(&art, &fresh);
        assert!(!std::sync::Arc::ptr_eq(&first.bands, &fresh.bands));
        assert_ne!(bits(Some(first.bands[0][0].1)), bits(Some(fresh.bands[0][0].1)));
    }
}
