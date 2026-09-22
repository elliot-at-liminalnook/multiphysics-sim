//! Source-based graph discovery and bounded live display history. No physics or UI.
use crate::{selection::SelectionTarget, *};
use std::collections::VecDeque;

/// A physical net exposes each terminal's quantities, not an invented net flow.
/// Component states are offered only for component selections, not adjacent nets.
pub fn options<'a>(
    d: &'a SystemDescription,
    target: &SelectionTarget,
) -> Result<Vec<&'a ObservableDescriptor>, InspectionError> {
    let details = target.resolve(d)?;
    Ok(d.observables
        .values()
        .filter(|o| match &o.location {
            ObservationLocation::Across { port, .. }
            | ObservationLocation::Through { port, .. }
            | ObservationLocation::Signal { port } => details.ports.contains(port),
            ObservationLocation::State { component, .. }
            | ObservationLocation::Diagnostic {
                component: Some(component),
                ..
            } => {
                matches!(target, SelectionTarget::Components { .. })
                    && details.components.contains(component)
            }
            _ => false,
        })
        .collect())
}

pub fn unit<'a>(d: &'a SystemDescription, o: &ObservableDescriptor) -> &'a str {
    d.definitions
        .quantities
        .iter()
        .find(|q| q.id == o.quantity)
        .map(|q| q.canonical_unit.as_str())
        .unwrap_or("unknown unit")
}
pub fn label(d: &SystemDescription, o: &ObservableDescriptor) -> String {
    let owner = match &o.location {
        ObservationLocation::Across { port, .. }
        | ObservationLocation::Through { port, .. }
        | ObservationLocation::Signal { port } => {
            let p = &d.ports[port];
            format!("{}.{}", d.components[&p.component].label, p.name)
        }
        ObservationLocation::State { component, .. }
        | ObservationLocation::Diagnostic {
            component: Some(component),
            ..
        } => d.components[component].label.clone(),
        _ => "System".into(),
    };
    format!("{owner} · {} [{}]", o.label, unit(d, o))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotPoint {
    pub time: f64,
    pub value: f64,
    pub accepted_stage: bool,
}
/// Display samples may be coalesced by transport. This is not a solver recording.
/// A new history is required for every run/reset generation.
#[derive(Debug)]
pub struct History {
    gate: FrameGate,
    frames: VecDeque<SampleFrame>,
    capacity: usize,
}
impl History {
    pub fn new(run_id: String, generation: u64, capacity: usize) -> Self {
        Self {
            gate: FrameGate::new(run_id, generation),
            frames: VecDeque::new(),
            capacity: capacity.clamp(1, 10_000),
        }
    }
    pub fn push(
        &mut self,
        d: &SystemDescription,
        frame: SampleFrame,
    ) -> Result<(), InspectionError> {
        self.gate.accept(d, &frame)?;
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
        Ok(())
    }
    pub fn frames(&self) -> &VecDeque<SampleFrame> {
        &self.frames
    }
    /// Missing/unavailable samples break the curve. X uses the actual evaluation
    /// time, including accepted solver stages, never the frame's endpoint time.
    pub fn series(&self, id: &str) -> Vec<Option<PlotPoint>> {
        self.frames
            .iter()
            .map(|f| match f.values.get(id) {
                Some(SampleValue::Committed { value, sample_time }) => Some(PlotPoint {
                    time: *sample_time,
                    value: *value,
                    accepted_stage: false,
                }),
                Some(SampleValue::AcceptedStage {
                    value, sample_time, ..
                }) => Some(PlotPoint {
                    time: *sample_time,
                    value: *value,
                    accepted_stage: true,
                }),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> SystemDescription {
        serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
        ))
        .unwrap()
    }
    #[test]
    fn thermal_net_offers_all_terminals_but_no_unrelated_state() {
        let d = fixture();
        let net = d.nets.values().find(|n| n.ports.len() == 3).unwrap();
        let choices = options(&d, &SelectionTarget::net(&net.id)).unwrap();
        assert_eq!(choices.len(), 6);
        for port in &net.ports {
            assert_eq!(choices.iter().filter(|o| matches!(&o.location,
                ObservationLocation::Across { port: p, .. } | ObservationLocation::Through { port: p, .. } if p == port)).count(), 2);
        }
        assert!(
            choices
                .iter()
                .all(|o| !matches!(o.location, ObservationLocation::State { .. }))
        );
        assert!(choices.iter().all(|o| ["K", "W"].contains(&unit(&d, o))));
    }
    #[test]
    fn history_preserves_sample_time_gaps_bounds_and_rejects_stale_frames() {
        let mut d = fixture();
        let id = d.observables.keys().next().unwrap().clone();
        d.observables.get_mut(&id).unwrap().availability = Availability::Available;
        d.seal().unwrap();
        let mut h = History::new("test".into(), 0, 3);
        let frame = |seq, sample| SampleFrame {
            version: SAMPLE_FRAME_VERSION,
            description_id: d.id.clone(),
            model_revision: d.model_revision,
            run_id: "test".into(),
            generation: 0,
            sequence: seq,
            step: seq,
            time: seq as f64,
            values: BTreeMap::from([(id.clone(), sample)]),
        };
        let first = frame(
            1,
            SampleValue::AcceptedStage {
                value: 2.,
                sample_time: 0.5,
                step_start: 0.,
                step_end: 1.,
            },
        );
        h.push(&d, first.clone()).unwrap();
        assert_eq!(h.series(&id)[0].unwrap().time, 0.5);
        assert!(h.push(&d, first).is_err());
        h.push(
            &d,
            frame(
                2,
                SampleValue::Unavailable {
                    reason: "not observed".into(),
                },
            ),
        )
        .unwrap();
        h.push(
            &d,
            frame(
                3,
                SampleValue::Committed {
                    value: 4.,
                    sample_time: 3.,
                },
            ),
        )
        .unwrap();
        assert!(h.series(&id)[1].is_none());
        let mut wrong = frame(
            4,
            SampleValue::Committed {
                value: 5.,
                sample_time: 4.,
            },
        );
        wrong.generation = 1;
        assert!(h.push(&d, wrong.clone()).is_err());
        wrong.generation = 0;
        h.push(&d, wrong).unwrap();
        assert_eq!(h.frames().len(), 3);
        assert!(h.series(&id)[0].is_none());
        assert!(h.series("absent").iter().all(Option::is_none));
    }
}
