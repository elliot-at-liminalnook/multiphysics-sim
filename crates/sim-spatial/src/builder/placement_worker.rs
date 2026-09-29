//! A latest-position mailbox. Slow validation never queues pointer history and
//! never holds the mutex while doing model work. Dropping it never joins on UI.
use std::sync::{Arc, Condvar, Mutex};

struct State<T, R> {
    pending: Option<T>,
    result: Option<(T, R)>,
    closed: bool,
}
pub(super) struct Latest<T, R>(Arc<(Mutex<State<T, R>>, Condvar)>);
impl<T: Clone + Send + 'static, R: Send + 'static> Latest<T, R> {
    pub fn new(mut compute: impl FnMut(&T) -> R + Send + 'static) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                pending: None::<T>,
                result: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::spawn(move || {
            loop {
                let (lock, wake) = &*worker;
                let mut state = lock.lock().unwrap();
                while state.pending.is_none() && !state.closed {
                    state = wake.wait(state).unwrap();
                }
                if state.closed {
                    return;
                }
                let input = state.pending.take().unwrap();
                drop(state);
                let result = compute(&input);
                let mut state = lock.lock().unwrap();
                if state.closed {
                    return;
                }
                state.result = Some((input, result));
            }
        });
        Self(shared)
    }
    pub fn submit(&self, input: T) {
        self.0.0.lock().unwrap().pending = Some(input);
        self.0.1.notify_one();
    }
    pub fn take(&self) -> Option<(T, R)> {
        self.0.0.lock().unwrap().result.take()
    }
}
impl<T, R> Drop for Latest<T, R> {
    fn drop(&mut self) {
        self.0.0.lock().unwrap().closed = true;
        self.0.1.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};
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
        assert_eq!(worker.0.0.lock().unwrap().pending, Some(10_000));
        release.send(()).unwrap();
        assert_eq!(enter.recv_timeout(Duration::from_secs(2)).unwrap(), 10_000);
        assert!(enter.recv_timeout(Duration::from_millis(20)).is_err());
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
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            drop(worker);
            done.send(()).unwrap();
        });
        let promptly = finished.recv_timeout(Duration::from_millis(250));
        release.send(()).unwrap();
        assert!(promptly.is_ok(), "cancelling a drag waited for validation");
        assert!(enter.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
