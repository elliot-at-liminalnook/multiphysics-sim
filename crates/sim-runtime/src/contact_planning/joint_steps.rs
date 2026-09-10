//! Stable mapping between physical feet, independent stances and force slots.
use super::*;
use sim_domain_control::contact_phase::FootStep;

#[derive(Clone)]
pub(super) struct ForceSlot {
    pub foot: usize,
    pub additional_step: Option<usize>,
    pub step: FootStep,
}
pub(super) fn force_slots(motion: &ContactPhaseConfig) -> Vec<ForceSlot> {
    motion
        .feet
        .iter()
        .enumerate()
        .flat_map(|(foot, f)| {
            f.steps().enumerate().map(move |(i, step)| ForceSlot {
                foot,
                additional_step: i.checked_sub(1),
                step,
            })
        })
        .collect()
}
impl ForceSlot {
    pub fn touchdown(&self) -> ContactEvent {
        match self.additional_step {
            None => ContactEvent::Touchdown { foot: self.foot },
            Some(step) => ContactEvent::AdditionalTouchdown {
                foot: self.foot,
                step,
            },
        }
    }
    pub fn liftoff(&self) -> ContactEvent {
        match self.additional_step {
            None => ContactEvent::Liftoff { foot: self.foot },
            Some(step) => ContactEvent::AdditionalLiftoff {
                foot: self.foot,
                step,
            },
        }
    }
    pub fn decision(&self, clock: usize, node: usize, axis: usize) -> JointContactDecision {
        match self.additional_step {
            None => JointContactDecision::Force {
                clock,
                foot: self.foot,
                node,
                axis,
            },
            Some(step) => JointContactDecision::AdditionalForce {
                clock,
                foot: self.foot,
                step,
                node,
                axis,
            },
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct ForceNode {
    pub clock: usize,
    pub slot: usize,
    pub foot: usize,
    pub node: usize,
    pub axis: usize,
}
impl JointContactDecision {
    pub(super) fn force_node(
        &self,
        motion: &ContactPhaseConfig,
    ) -> Result<Option<ForceNode>, String> {
        let (clock, foot, step, node, axis) = match *self {
            Self::Motion { .. } => return Ok(None),
            Self::Force {
                clock,
                foot,
                node,
                axis,
            } => (clock, foot, 0, node, axis),
            Self::AdditionalForce {
                clock,
                foot,
                step,
                node,
                axis,
            } => (
                clock,
                foot,
                step.checked_add(1).ok_or("force step index overflow")?,
                node,
                axis,
            ),
        };
        let f = motion
            .feet
            .get(foot)
            .ok_or("force physical foot out of range")?;
        if step > f.additional_steps.len() {
            return Err("force stance out of range".into());
        }
        let slot = motion.feet[..foot]
            .iter()
            .map(|f| 1 + f.additional_steps.len())
            .sum::<usize>()
            + step;
        Ok(Some(ForceNode {
            clock,
            slot,
            foot,
            node,
            axis,
        }))
    }
}

impl JointContactMotion {
    pub fn force_node_value(&self, decision: &JointContactDecision) -> Result<f64, String> {
        let ForceNode {
            clock,
            slot,
            node,
            axis,
            ..
        } = decision
            .force_node(&self.motion)?
            .ok_or("force decision required")?;
        self.force_templates
            .get(clock)
            .and_then(|c| c.get(slot))
            .filter(|t| node > 0 && node < t.keyframes.len().saturating_sub(1))
            .and_then(|t| t.keyframes.get(node))
            .and_then(|k| k.values.get(axis))
            .copied()
            .ok_or("force node out of range or zero endpoint".into())
    }
    pub fn set_force_node(
        &mut self,
        decision: &JointContactDecision,
        value: f64,
    ) -> Result<(), String> {
        if !value.is_finite() {
            return Err("finite force node value required".into());
        }
        if decision.force_node(&self.motion)?.is_none() {
            return Err("force decision required".into());
        }
        *joint_decision(self, decision)? = value;
        Ok(())
    }
    /// Preserve the motion and materialized force curves over a longer cycle,
    /// creating independently variable force templates for every new stance.
    /// Timing bindings are explicitly cleared and can be rebuilt after alignment.
    pub fn repeated_cycle(&self, count: usize) -> Result<Self, String> {
        let resolved = self.resolved_force_timing()?;
        if resolved.force_templates.is_empty() {
            return Err("at least one force clock required".into());
        }
        ContactPhaseMotion::new(self.motion.clone())?;
        for templates in &resolved.force_templates {
            PreparedForces::new(&self.motion, templates)?;
        }
        if count == 1 {
            return Ok(self.clone());
        }
        let mut result = resolved.as_ref().clone();
        result.motion = self.motion.repeated_cycle(count)?;
        for (input, output) in resolved
            .force_templates
            .iter()
            .zip(&mut result.force_templates)
        {
            PreparedForces::new(&self.motion, input)?;
            output.clear();
            let mut offset = 0;
            for foot in &self.motion.feet {
                let length = 1 + foot.additional_steps.len();
                for _ in 0..count {
                    output.extend_from_slice(&input[offset..offset + length]);
                }
                offset += length;
            }
            PreparedForces::new(&result.motion, output)?;
        }
        result.force_timing = None;
        Ok(result)
    }

    /// Lift a bounded search into an equivalent repeated cycle, then permit
    /// each copied step, force curve and body control to vary independently.
    /// Timing/displacement boxes transform with their coordinates; physical
    /// force/position/angle boxes are copied unchanged.
    pub fn repeated_cycle_with_variables(
        &self,
        variables: &[JointContactVariable],
        count: usize,
        direction_world: [f64; 3],
    ) -> Result<(Self, Vec<JointContactVariable>), String> {
        let candidate = self.repeated_cycle(count)?;
        joint_values(self, variables, direction_world)?;
        if count == 1 {
            return Ok((candidate, variables.to_vec()));
        }
        let factor = count as f64;
        let controls = self.motion.body.keyframes.len() - 1;
        let mut expanded = Vec::new();
        for variable in variables {
            if !variable.bound.lower.is_finite()
                || !variable.bound.upper.is_finite()
                || variable.bound.lower > variable.bound.upper
            {
                return Err("finite ordered search bounds required for cycle expansion".into());
            }
            let copies = match &variable.decision {
                JointContactDecision::Motion {
                    decision:
                        ContactDecision::Period
                        | ContactDecision::Displacement { .. }
                        | ContactDecision::DisplacementAlongDirection,
                } => 1,
                _ => count,
            };
            for repeat in 0..copies {
                let mut v = variable.clone();
                let mut scale = 1.0;
                let mut offset = 0.0;
                v.decision = match &variable.decision {
                    JointContactDecision::Force {
                        clock,
                        foot,
                        node,
                        axis,
                    } => {
                        let index = repeat * (1 + self.motion.feet[*foot].additional_steps.len());
                        if index == 0 {
                            variable.decision.clone()
                        } else {
                            JointContactDecision::AdditionalForce {
                                clock: *clock,
                                foot: *foot,
                                step: index - 1,
                                node: *node,
                                axis: *axis,
                            }
                        }
                    }
                    JointContactDecision::AdditionalForce {
                        clock,
                        foot,
                        step,
                        node,
                        axis,
                    } => JointContactDecision::AdditionalForce {
                        clock: *clock,
                        foot: *foot,
                        step: repeat * (1 + self.motion.feet[*foot].additional_steps.len()) + step,
                        node: *node,
                        axis: *axis,
                    },
                    JointContactDecision::Motion { decision } => {
                        let d = match *decision {
                            ContactDecision::Period
                            | ContactDecision::Displacement { .. }
                            | ContactDecision::DisplacementAlongDirection => {
                                scale = factor;
                                decision.clone()
                            }
                            ContactDecision::BodyControl { control, channel } => {
                                ContactDecision::BodyControl {
                                    control: control + repeat * controls,
                                    channel,
                                }
                            }
                            _ => {
                                let (foot, old_step, kind, axis) = match *decision {
                                    ContactDecision::FootPhase { foot } => (foot, 0, 0, 0),
                                    ContactDecision::FootStance { foot } => (foot, 0, 1, 0),
                                    ContactDecision::FootCenter { foot, axis } => {
                                        (foot, 0, 2, axis)
                                    }
                                    ContactDecision::FootSwing { foot, axis } => (foot, 0, 3, axis),
                                    ContactDecision::AdditionalStepPhase { foot, step } => {
                                        (foot, step + 1, 0, 0)
                                    }
                                    ContactDecision::AdditionalStepStance { foot, step } => {
                                        (foot, step + 1, 1, 0)
                                    }
                                    ContactDecision::AdditionalStepCenter { foot, step, axis } => {
                                        (foot, step + 1, 2, axis)
                                    }
                                    ContactDecision::AdditionalStepSwing { foot, step, axis } => {
                                        (foot, step + 1, 3, axis)
                                    }
                                    _ => unreachable!(),
                                };
                                let index = repeat
                                    * (1 + self.motion.feet[foot].additional_steps.len())
                                    + old_step;
                                if kind < 2 {
                                    scale = 1.0 / factor;
                                }
                                if kind == 0 {
                                    offset = repeat as f64 / factor;
                                }
                                match (index, kind) {
                                    (0, 0) => ContactDecision::FootPhase { foot },
                                    (0, 1) => ContactDecision::FootStance { foot },
                                    (0, 2) => ContactDecision::FootCenter { foot, axis },
                                    (0, 3) => ContactDecision::FootSwing { foot, axis },
                                    (_, 0) => ContactDecision::AdditionalStepPhase {
                                        foot,
                                        step: index - 1,
                                    },
                                    (_, 1) => ContactDecision::AdditionalStepStance {
                                        foot,
                                        step: index - 1,
                                    },
                                    (_, 2) => ContactDecision::AdditionalStepCenter {
                                        foot,
                                        step: index - 1,
                                        axis,
                                    },
                                    (_, 3) => ContactDecision::AdditionalStepSwing {
                                        foot,
                                        step: index - 1,
                                        axis,
                                    },
                                    _ => unreachable!(),
                                }
                            }
                        };
                        JointContactDecision::Motion { decision: d }
                    }
                };
                v.bound.lower = v.bound.lower * scale + offset;
                v.bound.upper = v.bound.upper * scale + offset;
                expanded.push(v);
            }
        }
        joint_values(&candidate, &expanded, direction_world)?;
        Ok((candidate, expanded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> JointContactMotion {
        let motion = serde_json::from_value(serde_json::json!({
            "period_s":1.0,"displacement_world_m":[0.1,0.05,0.0],
            "body":{"interpolation":"periodic_cubic_b_spline","keyframes":
                (0..=4).map(|i|serde_json::json!({"time_s":i as f64/4.0,"values":[0,0,0,0,0,0]})).collect::<Vec<_>>()},
            "feet":[
                {"center_world_m":[0,0,0],"phase_offset":0.02,"stance_fraction":0.18,"swing_offset_world_m":[0,0,0.02]},
                {"center_world_m":[0,0.1,0],"phase_offset":0.27,"stance_fraction":0.18,"swing_offset_world_m":[0,0,0.02]}
            ]
        })).unwrap();
        let template = |load| TrajectoryConfig {
            interpolation: Interpolation::Linear,
            keyframes: [0.0, 0.5, 1.0]
                .into_iter()
                .map(|time_s| sim_domain_control::trajectory::Keyframe {
                    time_s,
                    values: vec![0.0, 0.0, if time_s == 0.5 { load } else { 0.0 }],
                })
                .collect(),
        };
        JointContactMotion {
            motion,
            force_templates: vec![vec![template(10.0), template(20.0)]],
            force_timing: None,
        }
    }

    #[test]
    fn expanded_forces_preserve_loads_and_independent_physical_foot_identity() {
        let original = fixture();
        let mut expanded = original.repeated_cycle(2).unwrap();
        let a = PreparedForces::new(&original.motion, &original.force_templates[0]).unwrap();
        let b = PreparedForces::new(&expanded.motion, &expanded.force_templates[0]).unwrap();
        for i in -200..1200 {
            let t = i as f64 * 0.002731;
            let old = a.sample(&original.motion, t).unwrap();
            let new = b.sample(&expanded.motion, t / 2.0).unwrap();
            for (x, y) in old.iter().flatten().zip(new.iter().flatten()) {
                assert!((x - y).abs() < 1e-11);
            }
        }
        *joint_decision(
            &mut expanded,
            &JointContactDecision::AdditionalForce {
                clock: 0,
                foot: 0,
                step: 0,
                node: 1,
                axis: 2,
            },
        )
        .unwrap() += 7.0;
        *joint_decision(
            &mut expanded,
            &JointContactDecision::Force {
                clock: 0,
                foot: 1,
                node: 1,
                axis: 2,
            },
        )
        .unwrap() += 3.0;
        let changed = PreparedForces::new(&expanded.motion, &expanded.force_templates[0]).unwrap();
        for (phase, expected) in [
            (0.055, [10.0, 0.0]),
            (0.555, [17.0, 0.0]),
            (0.18, [0.0, 23.0]),
            (0.68, [0.0, 20.0]),
        ] {
            let force = changed.sample(&expanded.motion, phase).unwrap();
            for i in 0..2 {
                assert!((force[i][2] - expected[i]).abs() < 1e-10);
            }
        }
        assert!(changed.sample(&expanded.motion, f64::NAN).is_err());
        assert!(
            joint_decision(
                &mut expanded,
                &JointContactDecision::AdditionalForce {
                    clock: 0,
                    foot: 0,
                    step: 1,
                    node: 1,
                    axis: 2
                }
            )
            .is_err()
        );
        assert!(
            joint_decision(
                &mut expanded,
                &JointContactDecision::AdditionalForce {
                    clock: 0,
                    foot: 0,
                    step: 0,
                    node: 0,
                    axis: 2
                }
            )
            .is_err()
        );
    }

    #[test]
    fn per_stance_events_follow_independent_timing_and_reject_wrong_endpoints() {
        let original = fixture()
            .repeated_cycle(2)
            .unwrap()
            .with_event_aligned_linear_forces()
            .unwrap();
        let mut timed = original.with_contact_timed_forces().unwrap();
        assert_eq!(timed.force_timing.as_ref().unwrap().event_order.len(), 8);
        timed.motion.feet[0].additional_steps[0].phase_offset += 0.007;
        timed.motion.feet[0].additional_steps[0].stance_fraction -= 0.003;
        let shifted = timed.materialized_force_timing().unwrap();
        let force = PreparedForces::new(&shifted.motion, &shifted.force_templates[0]).unwrap();
        let motion = ContactPhaseMotion::new(shifted.motion.clone()).unwrap();
        let mut counts = [0usize; 2];
        for i in 0..1300 {
            let phase = (i as f64 + 0.31) / 1300.0;
            let sample = motion.sample(phase * shifted.motion.period_s).unwrap();
            let loads = force.sample(&shifted.motion, phase).unwrap();
            for foot in 0..2 {
                if sample.feet[foot].in_contact {
                    counts[foot] += 1;
                } else {
                    assert_eq!(loads[foot], [0.0; 3]);
                }
            }
        }
        assert!(counts.iter().all(|n| *n > 0));
        let mut wrong = shifted.clone();
        wrong.force_timing.as_mut().unwrap().knots[0][1][0] =
            wrong.force_timing.as_ref().unwrap().knots[0][0][0].clone();
        assert!(wrong.materialized_force_timing().is_err());
        let mut duplicate = shifted;
        let timing = duplicate.force_timing.as_mut().unwrap();
        timing.event_order[1] = timing.event_order[0];
        assert!(duplicate.materialized_force_timing().is_err());
    }

    #[test]
    fn expanded_search_box_preserves_corresponding_motion_and_force_perturbations() {
        let original = fixture();
        let direction = [2.0 / 5.0_f64.sqrt(), 1.0 / 5.0_f64.sqrt(), 0.0];
        let mut variables = vec![];
        for (decision, lower, upper) in [
            (ContactDecision::Period, 0.9, 1.1),
            (ContactDecision::DisplacementAlongDirection, 0.10, 0.12),
            (ContactDecision::FootPhase { foot: 1 }, 0.25, 0.28),
            (ContactDecision::FootStance { foot: 1 }, 0.15, 0.2),
            (
                ContactDecision::FootCenter { foot: 0, axis: 0 },
                -0.01,
                0.03,
            ),
            (ContactDecision::FootSwing { foot: 0, axis: 2 }, 0.01, 0.05),
            (
                ContactDecision::BodyControl {
                    control: 1,
                    channel: 2,
                },
                -0.01,
                0.02,
            ),
        ] {
            variables.push(JointContactVariable {
                decision: JointContactDecision::Motion { decision },
                bound: VariableBound { lower, upper },
            });
        }
        for foot in 0..2 {
            variables.push(JointContactVariable {
                decision: JointContactDecision::Force {
                    clock: 0,
                    foot,
                    node: 1,
                    axis: 2,
                },
                bound: VariableBound {
                    lower: 1.0,
                    upper: 40.0,
                },
            });
        }
        let (expanded, expanded_variables) = original
            .repeated_cycle_with_variables(&variables, 2, direction)
            .unwrap();
        let midpoint = |vs: &[JointContactVariable]| {
            vs.iter()
                .map(|v| 0.5 * (v.bound.lower + v.bound.upper))
                .collect::<Vec<_>>()
        };
        let changed =
            decode_joint_values(&original, &variables, direction, &midpoint(&variables)).unwrap();
        let copied = decode_joint_values(
            &expanded,
            &expanded_variables,
            direction,
            &midpoint(&expanded_variables),
        )
        .unwrap();
        let a = ContactPhaseMotion::new(changed.motion.clone()).unwrap();
        let b = ContactPhaseMotion::new(copied.motion.clone()).unwrap();
        let fa = PreparedForces::new(&changed.motion, &changed.force_templates[0]).unwrap();
        let fb = PreparedForces::new(&copied.motion, &copied.force_templates[0]).unwrap();
        for i in -20..301 {
            let t = i as f64 * 0.0073;
            let x = a.sample(t).unwrap();
            let y = b.sample(t).unwrap();
            for (x, y) in x
                .body
                .values
                .iter()
                .chain(x.feet.iter().flat_map(|f| f.position_world_m.iter()))
                .zip(
                    y.body
                        .values
                        .iter()
                        .chain(y.feet.iter().flat_map(|f| f.position_world_m.iter())),
                )
            {
                assert!((x - y).abs() < 1e-12);
            }
            let x = fa
                .sample(&changed.motion, t / changed.motion.period_s)
                .unwrap();
            let y = fb
                .sample(&copied.motion, t / copied.motion.period_s)
                .unwrap();
            for (x, y) in x.iter().flatten().zip(y.iter().flatten()) {
                assert!((x - y).abs() < 1e-10);
            }
        }
    }
}
