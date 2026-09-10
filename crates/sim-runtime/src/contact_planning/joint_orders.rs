//! Discrete contact-order neighbors for an outer search around joint local solves.
//! Preparation failures describe a start and its numerical box, not a gait family.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct ContactOrderNeighbor {
    pub edge: usize,
    pub swapped: [ContactEvent; 2],
    /// Common timing rotation preserves the original foot-0 touchdown anchor.
    pub phase_shift: f64,
    pub candidate: Option<JointContactMotion>,
    pub variables: Option<Vec<JointContactVariable>>,
    pub preparation_error: Option<String>,
}

impl JointContactMotion {
    /// Recover explicit uniform per-stance/axis force-node boxes before changing
    /// knot layout. Missing, duplicate or unequal node bounds require the caller
    /// to supply a new policy explicitly; no physical bound is inferred.
    pub fn uniform_force_bounds(
        &self,
        variables: &[JointContactVariable],
    ) -> Result<Vec<Vec<[VariableBound; 3]>>, String> {
        let resolved = self.resolved_force_timing()?;
        let mut known = BTreeMap::<(usize, usize, usize), VariableBound>::new();
        let mut nodes = BTreeSet::new();
        for v in variables {
            if let Some(ForceNode {
                clock,
                slot,
                node,
                axis,
                ..
            }) = v.decision.force_node(&self.motion)?
            {
                self.force_node_value(&v.decision)?;
                if !v.bound.lower.is_finite()
                    || !v.bound.upper.is_finite()
                    || v.bound.lower > v.bound.upper
                {
                    return Err("finite ordered explicit force bounds required".into());
                }
                if !nodes.insert((clock, slot, node, axis)) {
                    return Err("duplicate force variable".into());
                }
                if let Some(old) = known.insert((clock, slot, axis), v.bound.clone()) {
                    if old.lower != v.bound.lower || old.upper != v.bound.upper {
                        return Err("changing force knots requires uniform node bounds per stance/axis or an explicit new bounds policy".into());
                    }
                }
            }
        }
        let mut bounds = Vec::new();
        for (clock, templates) in resolved.force_templates.iter().enumerate() {
            PreparedForces::new(&resolved.motion, templates)?;
            let mut row = Vec::new();
            for (slot, template) in templates.iter().enumerate() {
                for node in 1..template.keyframes.len() - 1 {
                    for axis in 0..3 {
                        if !nodes.contains(&(clock, slot, node, axis)) {
                            return Err("every interior force component needs an explicit bound before knot refinement".into());
                        }
                    }
                }
                let axis = (0..3)
                    .map(|axis| {
                        known
                            .get(&(clock, slot, axis))
                            .cloned()
                            .ok_or("missing stance force bound".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                row.push([axis[0].clone(), axis[1].clone(), axis[2].clone()]);
            }
            bounds.push(row);
        }
        Ok(bounds)
    }

    /// Exchange every adjacent pair of cyclic contact events, including the
    /// wrap pair. Event time slots are retained, then all phases rotate to keep
    /// foot-0 touchdown fixed. Body/placement/swing parameters and physical boxes
    /// are unchanged; each valid neighbor receives re-aligned independent forces
    /// and a fresh local event ordering. Every attempted edge retains its error.
    pub fn adjacent_contact_order_neighbors(
        &self,
        variables: &[JointContactVariable],
        direction: [f64; 3],
    ) -> Result<Vec<ContactOrderNeighbor>, String> {
        let bounds = self.uniform_force_bounds(variables)?;
        joint_values(self, variables, direction)?;
        let mut base = self.resolved_force_timing()?.into_owned();
        base.force_timing = None;
        base = base
            .with_event_aligned_linear_forces()?
            .with_contact_timed_forces()?;
        let events = &base.force_timing.as_ref().unwrap().event_order;
        let times = events
            .iter()
            .map(|e| e.phase(&base.motion))
            .collect::<Result<Vec<_>, _>>()?;
        let slots = force_slots(&base.motion);
        let mut output = Vec::new();
        for edge in 0..events.len() {
            let next = (edge + 1) % events.len();
            let swapped = [events[edge], events[next]];
            let mut assigned: BTreeMap<_, _> =
                events.iter().copied().zip(times.iter().copied()).collect();
            assigned.insert(events[edge], times[next]);
            assigned.insert(events[next], times[edge]);
            let phase_shift = (times[0] - assigned[&events[0]]).rem_euclid(1.0);
            let attempt = (|| {
                let mut candidate = base.clone();
                candidate.force_timing = None;
                for slot in &slots {
                    let touchdown = if slot.foot == 0 && slot.additional_step.is_none() {
                        base.motion.feet[0].phase_offset
                    } else {
                        (assigned[&slot.touchdown()] + phase_shift).rem_euclid(1.0)
                    };
                    let liftoff = (assigned[&slot.liftoff()] + phase_shift).rem_euclid(1.0);
                    let duration = (liftoff - touchdown).rem_euclid(1.0);
                    let foot = &mut candidate.motion.feet[slot.foot];
                    if let Some(index) = slot.additional_step {
                        foot.additional_steps[index].phase_offset = touchdown;
                        foot.additional_steps[index].stance_fraction = duration;
                    } else {
                        foot.phase_offset = touchdown;
                        foot.stance_fraction = duration;
                    }
                }
                ContactPhaseMotion::new(candidate.motion.clone())?;
                candidate = candidate
                    .with_event_aligned_linear_forces()?
                    .with_contact_timed_forces()?;
                let mut new_variables = variables
                    .iter()
                    .filter(|v| matches!(v.decision, JointContactDecision::Motion { .. }))
                    .cloned()
                    .collect::<Vec<_>>();
                new_variables.extend(candidate.force_variables_for_bounds(&bounds)?);
                let values = joint_values(&candidate, &new_variables, direction)?;
                for (i, (value, v)) in values.iter().zip(&new_variables).enumerate() {
                    if !value.is_finite()
                        || !v.bound.lower.is_finite()
                        || !v.bound.upper.is_finite()
                        || v.bound.lower > v.bound.upper
                        || *value < v.bound.lower
                        || *value > v.bound.upper
                    {
                        return Err(format!(
                            "neighbor initializer outside existing search box at variable {i}; this is not a physical exclusion"
                        ));
                    }
                }
                Ok((candidate, new_variables))
            })();
            let (candidate, variables, preparation_error) = match attempt {
                Ok((c, v)) => (Some(c), Some(v), None),
                Err(error) => (None, None, Some(error)),
            };
            output.push(ContactOrderNeighbor {
                edge,
                swapped,
                phase_shift,
                candidate,
                variables,
                preparation_error,
            });
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (JointContactMotion, Vec<JointContactVariable>) {
        let motion=serde_json::from_value(serde_json::json!({
            "period_s":1,"displacement_world_m":[0.1,0,0],
            "body":{"interpolation":"periodic_cubic_b_spline","keyframes":
                (0..=4).map(|i|serde_json::json!({"time_s":i as f64/4.0,"values":[0,0,0,0,0,0]})).collect::<Vec<_>>()},
            "feet":[
                {"phase_offset":0.05,"stance_fraction":0.3,"center_world_m":[0,0,0],"swing_offset_world_m":[0,0,0.02]},
                {"phase_offset":0.4,"stance_fraction":0.3,"center_world_m":[0,0.2,0],"swing_offset_world_m":[0,0,0.02]}
            ]
        })).unwrap();
        let template = TrajectoryConfig {
            interpolation: Interpolation::Linear,
            keyframes: [0.0, 0.5, 1.0]
                .into_iter()
                .map(|time_s| sim_domain_control::trajectory::Keyframe {
                    time_s,
                    values: vec![0.0, 0.0, if time_s == 0.5 { 10.0 } else { 0.0 }],
                })
                .collect(),
        };
        let candidate = JointContactMotion {
            motion,
            force_templates: vec![vec![template.clone(), template]],
            force_timing: None,
        };
        let bounds = vec![vec![
            [
                VariableBound {
                    lower: -30.0,
                    upper: 30.0
                },
                VariableBound {
                    lower: -30.0,
                    upper: 30.0
                },
                VariableBound {
                    lower: 0.0,
                    upper: 30.0
                }
            ];
            2
        ]];
        let mut variables = candidate.force_variables_for_bounds(&bounds).unwrap();
        for foot in 0..2 {
            variables.push(JointContactVariable {
                decision: JointContactDecision::Motion {
                    decision: ContactDecision::FootStance { foot },
                },
                bound: VariableBound {
                    lower: 0.05,
                    upper: 0.95,
                },
            });
        }
        variables.push(JointContactVariable {
            decision: JointContactDecision::Motion {
                decision: ContactDecision::FootPhase { foot: 1 },
            },
            bound: VariableBound {
                lower: 0.0,
                upper: 0.999,
            },
        });
        (candidate, variables)
    }
    #[test]
    fn initializer_period_edit_uses_optimizer_timestamps_and_rejects_bad_bounds() {
        let (candidate, mut variables) = fixture();
        let index = variables.len();
        variables.push(JointContactVariable {
            decision: JointContactDecision::Motion {
                decision: ContactDecision::Period,
            },
            bound: VariableBound {
                lower: 0.5,
                upper: 2.0,
            },
        });
        let candidate = candidate.with_contact_timed_forces().unwrap();
        let edited = candidate
            .with_bounded_overrides(&variables, [1.0, 0.0, 0.0], &[(index, 0.75)])
            .unwrap();
        assert_eq!(edited.motion.period_s, 0.75);
        assert_eq!(edited.motion.body.keyframes.last().unwrap().time_s, 0.75);
        ContactPhaseMotion::new(edited.motion.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&edited.motion.feet).unwrap(),
            serde_json::to_value(&candidate.motion.feet).unwrap()
        );
        for phase in [0.01, 0.1, 0.25, 0.5, 0.8] {
            let a = candidate.resolved_force_timing().unwrap();
            let b = edited.resolved_force_timing().unwrap();
            let a = PreparedForces::new(&a.motion, &a.force_templates[0]).unwrap();
            let b = PreparedForces::new(&b.motion, &b.force_templates[0]).unwrap();
            assert_eq!(
                a.sample(&candidate.motion, phase).unwrap(),
                b.sample(&edited.motion, phase).unwrap()
            );
        }
        assert!(
            candidate
                .with_bounded_overrides(&variables, [1.0, 0.0, 0.0], &[(index, 0.4)])
                .is_err()
        );
        assert!(
            candidate
                .with_bounded_overrides(&variables, [1.0, 0.0, 0.0], &[(index, 0.75), (index, 1.0)])
                .is_err()
        );
    }
    #[test]
    fn adjacent_swaps_cross_orders_and_keep_anchor_geometry_and_explicit_boxes() {
        let (source, variables) = fixture();
        let neighbors = source
            .adjacent_contact_order_neighbors(&variables, [1.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(neighbors.len(), 4);
        let original = source
            .with_event_aligned_linear_forces()
            .unwrap()
            .with_contact_timed_forces()
            .unwrap();
        for neighbor in &neighbors {
            let candidate = neighbor.candidate.as_ref().unwrap();
            assert_eq!(
                candidate.motion.feet[0].phase_offset,
                source.motion.feet[0].phase_offset
            );
            assert_eq!(
                serde_json::to_value(&candidate.motion.body).unwrap(),
                serde_json::to_value(&source.motion.body).unwrap()
            );
            assert_eq!(candidate.motion.period_s, source.motion.period_s);
            assert_eq!(
                candidate.motion.displacement_world_m,
                source.motion.displacement_world_m
            );
            for (old, new) in source.motion.feet.iter().zip(&candidate.motion.feet) {
                assert_eq!(old.center_world_m, new.center_world_m);
                assert_eq!(old.swing_offset_world_m, new.swing_offset_world_m);
            }
            let actual = candidate
                .uniform_force_bounds(neighbor.variables.as_ref().unwrap())
                .unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(source.uniform_force_bounds(&variables).unwrap()).unwrap()
            );
            assert_ne!(
                candidate.force_timing.as_ref().unwrap().event_order,
                original.force_timing.as_ref().unwrap().event_order
            );
        }
        let cross = neighbors[1].candidate.as_ref().unwrap();
        assert_eq!(
            cross.force_timing.as_ref().unwrap().event_order,
            vec![
                ContactEvent::Touchdown { foot: 0 },
                ContactEvent::Touchdown { foot: 1 },
                ContactEvent::Liftoff { foot: 0 },
                ContactEvent::Liftoff { foot: 1 }
            ]
        );
        assert!((cross.motion.feet[0].stance_fraction - 0.35).abs() < 1e-14);
        assert!((cross.motion.feet[1].phase_offset - 0.35).abs() < 1e-14);
        let mut stale = cross.clone();
        stale.force_timing = original.force_timing;
        assert!(stale.materialized_force_timing().is_err());
    }
    #[test]
    fn failures_remain_edges_and_nonuniform_or_incomplete_boxes_are_not_inferred() {
        let (source, mut variables) = fixture();
        let mut missing = variables.clone();
        missing.remove(0);
        assert!(source.uniform_force_bounds(&missing).is_err());
        let mut duplicate = variables.clone();
        duplicate.push(variables[0].clone());
        assert!(source.uniform_force_bounds(&duplicate).is_err());
        for v in &mut variables {
            if matches!(
                v.decision,
                JointContactDecision::Motion {
                    decision: ContactDecision::FootStance { .. }
                }
            ) {
                v.bound = VariableBound {
                    lower: 0.29,
                    upper: 0.31,
                };
            }
        }
        let neighbors = source
            .adjacent_contact_order_neighbors(&variables, [1.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(neighbors.len(), 4);
        assert!(neighbors.iter().all(|n| n.candidate.is_none()
            && n.preparation_error.as_ref().unwrap().contains("search box")));
        let (source, variables) = fixture();
        let (double, variables) = source
            .repeated_cycle_with_variables(&variables, 2, [1.0, 0.0, 0.0])
            .unwrap();
        let neighbors = double
            .adjacent_contact_order_neighbors(&variables, [1.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(neighbors.len(), 8);
        assert!(neighbors.iter().any(|n| n.candidate.is_some()));
        assert!(neighbors.iter().any(|n| n.preparation_error.is_some()));
    }
}
