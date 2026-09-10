//! Force knots attached to a fixed cyclic ordering of contact transitions.
//! This is a timing parameterization, not a contact-topology search.
use super::*;
use std::borrow::Cow;

// Numerical separation in normalized phase, not a physical duration limit.
const EVENT_SEPARATION: f64 = 1e-10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContactEvent {
    Touchdown { foot: usize },
    Liftoff { foot: usize },
    AdditionalTouchdown { foot: usize, step: usize },
    AdditionalLiftoff { foot: usize, step: usize },
}
impl ContactEvent {
    pub(super) fn phase(self, motion: &ContactPhaseConfig) -> Result<f64, String> {
        let (index, step, liftoff) = match self {
            Self::Touchdown { foot } => (foot, None, false),
            Self::Liftoff { foot } => (foot, None, true),
            Self::AdditionalTouchdown { foot, step } => (foot, Some(step), false),
            Self::AdditionalLiftoff { foot, step } => (foot, Some(step), true),
        };
        let foot = motion
            .feet
            .get(index)
            .ok_or("contact event foot out of range")?;
        let (onset, duration) = if let Some(step) = step {
            let step = foot.additional_steps.get(step).ok_or("contact event stance out of range")?;
            (step.phase_offset, step.stance_fraction)
        } else { (foot.phase_offset, foot.stance_fraction) };
        Ok((onset + if liftoff { duration } else { 0. }).rem_euclid(1.))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForceKnotBinding {
    /// Index of the left event in the cyclic ordering; the right event follows
    /// it, wrapping at cycle end. A zero fraction means exactly the left event.
    pub interval: usize,
    pub fraction: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointForceTiming {
    /// Every stance's touchdown and liftoff exactly once, starting at foot 0
    /// touchdown. Coincident events and changes of this order are rejected.
    pub event_order: Vec<ContactEvent>,
    /// Clock / stance slot / knot, matching force_templates without adding variables.
    pub knots: Vec<Vec<Vec<ForceKnotBinding>>>,
}

impl JointForceTiming {
    fn phases(&self, motion: &ContactPhaseConfig) -> Result<Vec<f64>, String> {
        ContactPhaseMotion::new(motion.clone())?;
        if self.event_order.len() != 2 * force_slots(motion).len()
            || self.event_order.first() != Some(&ContactEvent::Touchdown { foot: 0 })
        {
            return Err(
                "force timing requires all contact events, anchored at foot 0 touchdown".into(),
            );
        }
        let unique = self
            .event_order
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != self.event_order.len() {
            return Err("duplicate contact event in force timing".into());
        }
        let origin = self.event_order[0].phase(motion)?;
        let phases = self
            .event_order
            .iter()
            .map(|e| Ok((e.phase(motion)? - origin).rem_euclid(1.)))
            .collect::<Result<Vec<_>, String>>()?;
        if phases.windows(2).any(|p| p[1] - p[0] <= EVENT_SEPARATION)
            || 1. - phases.last().unwrap() <= EVENT_SEPARATION
        {
            return Err("force timing contact events crossed or are numerically coincident".into());
        }
        Ok(phases)
    }
}

impl JointContactMotion {
    /// Attach each existing force knot to its surrounding contact events.
    /// Requires knots at every contact transition inside each stance (use the
    /// explicit linear refinement first). No force coefficients are changed.
    /// Subsequent phase/duty changes move these knots with the overlap windows.
    /// Only the initial strict event ordering is supported by this local solve.
    pub fn with_contact_timed_forces(&self) -> Result<Self, String> {
        if self.force_timing.is_some() {
            return Err("force timing is already bound".into());
        }
        ContactPhaseMotion::new(self.motion.clone())?;
        if self.force_templates.is_empty() {
            return Err("at least one force operating clock required".into());
        }
        let origin = self.motion.feet[0].phase_offset.rem_euclid(1.);
        let slots = force_slots(&self.motion);
        let mut events = slots.iter()
            .flat_map(|slot| [slot.touchdown(), slot.liftoff()])
            .map(|event| Ok((event, (event.phase(&self.motion)? - origin).rem_euclid(1.))))
            .collect::<Result<Vec<_>, String>>()?;
        events.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut timing = JointForceTiming {
            event_order: events.iter().map(|(e, _)| *e).collect(),
            knots: vec![],
        };
        let phases = timing.phases(&self.motion)?;
        for templates in &self.force_templates {
            PreparedForces::new(&self.motion, templates)?;
            let mut bindings = Vec::new();
            for (slot, template) in slots.iter().zip(templates)
            {
                let foot = &slot.step;
                for event in &timing.event_order {
                    let local = (event.phase(&self.motion)? - foot.phase_offset).rem_euclid(1.);
                    if local > EVENT_SEPARATION
                        && local < foot.stance_fraction - EVENT_SEPARATION
                        && !template
                            .keyframes
                            .iter()
                            .any(|k| (k.time_s * foot.stance_fraction - local).abs() <= 1e-12)
                    {
                        return Err(
                            "force knots must include every contact event inside stance".into()
                        );
                    }
                }
                let last = template.keyframes.len() - 1;
                let mut nodes = Vec::new();
                for (i, node) in template.keyframes.iter().enumerate() {
                    let endpoint = if i == 0 {
                        Some(slot.touchdown())
                    } else if i == last {
                        Some(slot.liftoff())
                    } else {
                        None
                    };
                    let phase = (foot.phase_offset + node.time_s * foot.stance_fraction - origin)
                        .rem_euclid(1.);
                    let event_index = endpoint
                        .and_then(|e| timing.event_order.iter().position(|x| *x == e))
                        .or_else(|| phases.iter().position(|p| (p - phase).abs() <= 1e-12));
                    let binding = if let Some(interval) = event_index {
                        ForceKnotBinding {
                            interval,
                            fraction: 0.,
                        }
                    } else {
                        let interval = phases.partition_point(|p| *p < phase).saturating_sub(1);
                        let right = phases.get(interval + 1).copied().unwrap_or(1.);
                        ForceKnotBinding {
                            interval,
                            fraction: (phase - phases[interval]) / (right - phases[interval]),
                        }
                    };
                    nodes.push(binding);
                }
                bindings.push(nodes);
            }
            timing.knots.push(bindings);
        }
        let mut result = self.clone();
        result.force_timing = Some(timing);
        result.materialized_force_timing()
    }

    /// Resolve stored bindings into serializable normalized stance times.
    /// Metadata remains attached for further optimization; callers exporting a
    /// fixed reference may explicitly clear it after materializing.
    pub fn materialized_force_timing(&self) -> Result<Self, String> {
        Ok(self.resolved_force_timing()?.into_owned())
    }

    pub(super) fn resolved_force_timing(&self) -> Result<Cow<'_, Self>, String> {
        let Some(timing) = &self.force_timing else {
            return Ok(Cow::Borrowed(self));
        };
        let phases = timing.phases(&self.motion)?;
        if timing.knots.len() != self.force_templates.len() || timing.knots.is_empty() {
            return Err("force timing clock count mismatch".into());
        }
        let origin = timing.event_order[0].phase(&self.motion)?;
        let mut result = self.clone();
        let slots = force_slots(&self.motion);
        for (templates, bindings) in result.force_templates.iter_mut().zip(&timing.knots) {
            if templates.len() != slots.len() || bindings.len() != templates.len() {
                return Err("force timing stance count mismatch".into());
            }
            for ((template, nodes), slot) in templates
                .iter_mut()
                .zip(bindings)
                .zip(&slots)
            {
                let foot = &slot.step;
                if nodes.len() != template.keyframes.len() || nodes.len() < 3 {
                    return Err("force timing knot count mismatch".into());
                }
                let last = nodes.len() - 1;
                for (i, (node, binding)) in template.keyframes.iter_mut().zip(nodes).enumerate() {
                    if binding.interval >= phases.len()
                        || !binding.fraction.is_finite()
                        || binding.fraction < 0.
                        || binding.fraction >= 1.
                    {
                        return Err("invalid force knot event binding".into());
                    }
                    if i == 0 || i == last {
                        let expected = if i == 0 {
                            slot.touchdown()
                        } else {
                            slot.liftoff()
                        };
                        if binding.fraction != 0.
                            || timing.event_order[binding.interval] != expected
                        {
                            return Err(
                                "force endpoint must bind to its own contact transition".into()
                            );
                        }
                        node.time_s = if i == 0 { 0. } else { 1. };
                    } else {
                        let left = phases[binding.interval];
                        let right = phases.get(binding.interval + 1).copied().unwrap_or(1.);
                        let phase = origin + left + binding.fraction * (right - left);
                        node.time_s =
                            (phase - foot.phase_offset).rem_euclid(1.) / foot.stance_fraction;
                        if node.time_s <= 0. || node.time_s >= 1. {
                            return Err("force knot moved outside stance".into());
                        }
                    }
                }
            }
            PreparedForces::new(&self.motion, templates)?;
        }
        Ok(Cow::Owned(result))
    }
}
