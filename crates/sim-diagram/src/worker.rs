//! Cancellable layout jobs. A stale revision never replaces edited positions.
use crate::{Layout, layout};
use sim_inspect::{DiagramState, Point, SystemDescription};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
type ResultMessage = (u64, BTreeMap<String, Point>, Layout);
pub struct LayoutJob {
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<ResultMessage>,
}
impl LayoutJob {
    pub fn start(
        description: Arc<SystemDescription>,
        mut state: DiagramState,
        arrange: bool,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            if arrange || state.positions.len() != description.components.len() {
                let Some(initial) = layout::initial_state_cancellable(&description, &token) else {
                    return;
                };
                let mut positions = initial.positions;
                let fixed: std::collections::BTreeSet<_> = if arrange {
                    state.pinned.clone()
                } else {
                    state.positions.keys().cloned().collect()
                };
                for id in &fixed {
                    if let Some(p) = state.positions.get(id) {
                        positions.insert(id.clone(), *p);
                    }
                }
                // User-pinned positions take precedence; move other cards clear
                // of them instead of silently stacking cards after Arrange.
                if !fixed.is_empty() {
                    let mut counts = BTreeMap::new();
                    for port in description.ports.values() {
                        *counts.entry(&port.component).or_insert(0usize) += 1;
                    }
                    let height = |id: &String| {
                        layout::HEADER
                            + counts.get(id).copied().unwrap_or(0).max(1) as f32 * layout::ROW
                            + 32.
                    };
                    let mut placed: Vec<_> = fixed
                        .iter()
                        .filter_map(|id| positions.get(id).map(|p| (*p, height(id))))
                        .collect();
                    for (id, p) in &mut positions {
                        if !fixed.contains(id) {
                            let h = height(id);
                            loop {
                                let collision = placed.iter().find(|(q, qh)| {
                                    p.x < q.x + layout::WIDTH + 32.
                                        && p.x + layout::WIDTH + 32. > q.x
                                        && p.y < q.y + qh + 32.
                                        && p.y + h + 32. > q.y
                                });
                                if let Some((q, qh)) = collision {
                                    p.y = ((q.y + qh + 64.) / 16.).ceil() * 16.;
                                } else {
                                    break;
                                }
                            }
                            placed.push((*p, h));
                        }
                    }
                }
                state.positions = positions;
            }
            if token.load(Ordering::Relaxed) {
                return;
            }
            if let Some(layout) = layout::route_cancellable(&description, &state, &token) {
                let _ = sender.send((state.revision, state.positions, layout));
            }
        });
        Self {
            cancelled,
            receiver,
        }
    }
    pub fn result(&self) -> Result<Option<ResultMessage>, String> {
        match self.receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Layout worker stopped without a result".into())
            }
        }
    }
}
impl Drop for LayoutJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_stops_full_assembly_computation() {
        let d: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/full-robot.description.json"
        ))
        .unwrap();
        let state = DiagramState::new(&d);
        let job = LayoutJob::start(Arc::new(d), state, true);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let start = std::time::Instant::now();
        job.cancelled.store(true, Ordering::Relaxed);
        assert!(matches!(
            job.receiver.recv_timeout(std::time::Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        eprintln!("full assembly layout cancellation {:?}", start.elapsed());
    }
}
