//! The "hardware-sync" thread: `/live/open` and the newest sample every
//! [`SEND_PERIOD`] while a session is active.
use super::{SEND_PERIOD, SyncCommand, SyncShared};
use crate::robot::hardware::local::{Body, Client, bench};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The sync thread's side of a session: active, the newest sample, the last sent.
#[derive(Default)]
pub(super) struct Outbox {
    pub(super) active: bool,
    pub(super) latest: Option<bench::Sample>,
    pub(super) last_sent: u64,
}

/// Applies `first` and every queued command in order, re-checking the queue after each (a stop
/// sent during `/live/open` is seen before any sample); the newest Latest wins.
/// True when a session opened and is still active (send at once, :23).
pub(super) fn drain(rx: &mpsc::Receiver<SyncCommand>, first: Option<SyncCommand>, out: &mut Outbox, open: &mut dyn FnMut(&Body,u64) -> Result<(), String>) -> bool {
    let (mut next, mut opened) = (first, false);
    while let Some(command) = next.take().or_else(|| rx.try_recv().ok()) {
        match command {
            SyncCommand::Latest(s) => out.latest = Some(s),
            SyncCommand::Open { body, last_sent, stop_epoch } => {
                out.last_sent = last_sent;
                out.active = open(&body,stop_epoch).is_ok();
                opened = true;
            }
            SyncCommand::Deactivate => out.active = false,
        }
    }
    opened && out.active
}

/// The sync thread: `/live/open`, and `/live/sample` every 50 ms while
/// active (`send()`, :21). Exits when the channel closes.
pub(super) fn worker(client: Client, rx: mpsc::Receiver<SyncCommand>, shared: Arc<Mutex<SyncShared>>) {
    let mut out = Outbox::default();
    let mut next_send = Instant::now() + SEND_PERIOD;
    let mut open = |body: &Body, stop_epoch: u64| {
        let result = client.post_at_epoch(bench::LIVE_OPEN, body, stop_epoch).map(|_| ()).map_err(|e| e.to_string());
        lock(&shared).open = Some(result.clone());
        result
    };
    loop {
        let first = match rx.recv_timeout(next_send.saturating_duration_since(Instant::now())) {
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            received => received.ok(),
        };
        let opened = drain(&rx, first, &mut out, &mut open);
        let now = Instant::now();
        if now >= next_send {
            next_send = now + SEND_PERIOD;
        } else if !opened {
            continue;
        }
        // The newest sample, if newer than the last sent.
        let (active, last_sent) = (out.active, out.last_sent);
        let Some(sample) = out.latest.as_ref().filter(|s| active && s.sequence > last_sent) else { continue };
        match client.post(bench::LIVE_SAMPLE, &bench::sample(sample)) {
            Ok(_) => out.last_sent = sample.sequence,
            Err(e) => {
                out.active = false;
                lock(&shared).send_error = Some(e.to_string());
            }
        }
    }
}
