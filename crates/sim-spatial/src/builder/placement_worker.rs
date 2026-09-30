//! A latest-position validator on its own `jobs::RunThread`. Pointer moves
//! are commands; the worker drains the channel and validates only the newest
//! position (the latest request wins), so slow validation never works through
//! pointer history and never holds the lock while doing model work. Dropping
//! it closes the channel and never waits (join bound zero): a release or a
//! cancel never joins a validation in progress, and a position still queued
//! is never started.
use crate::jobs::RunThread;
use std::sync::mpsc;
use std::time::Duration;

pub(super) struct Latest<T, R>(RunThread<T, Option<(T, R)>>);
impl<T: Send + 'static, R: Send + 'static> Latest<T, R> {
    pub fn new(mut compute: impl FnMut(&T) -> R + Send + 'static) -> Self {
        let worker = RunThread::spawn("placement-validator", None, move |inputs: mpsc::Receiver<T>, out| {
            while let Ok(mut input) = inputs.recv() {
                loop {
                    match inputs.try_recv() {
                        Ok(newer) => input = newer,
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                let result = compute(&input);
                *out.lock().unwrap_or_else(|p| p.into_inner()) = Some((input, result));
            }
        });
        Self(worker.join_bound(Duration::ZERO))
    }
    pub fn submit(&self, input: T) {
        let _ = self.0.send(input);
    }
    pub fn take(&self) -> Option<(T, R)> {
        self.0.lock().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn slow_validation_coalesces_ten_thousand_moves_to_latest_position() {
        let (entered, enter) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = Latest::new(move |p: &usize| {
            entered.send(*p).unwrap();
            if *p == 0 {
                gate.recv().unwrap();
            }
            *p
        });
        worker.submit(0);
        assert_eq!(enter.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
        // The validator remains blocked throughout every pointer update.
        for i in 1..=10_000 {
            worker.submit(i);
        }
        release.send(()).unwrap();
        // Only the newest position is validated next.
        assert_eq!(enter.recv_timeout(Duration::from_secs(2)).unwrap(), 10_000);
        assert!(enter.recv_timeout(Duration::from_millis(20)).is_err());
        assert_eq!(worker.take(), Some((10_000, 10_000)));
    }
    #[test]
    fn cancelling_does_not_join_slow_validation_or_start_pending_work() {
        let (entered, enter) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = Latest::new(move |p: &usize| {
            entered.send(*p).unwrap();
            gate.recv().unwrap();
        });
        worker.submit(1);
        enter.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.submit(2);
        let started = Instant::now();
        drop(worker);
        let promptly = started.elapsed() < Duration::from_millis(250);
        release.send(()).unwrap();
        assert!(promptly, "cancelling a drag waited for validation");
        assert!(enter.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
