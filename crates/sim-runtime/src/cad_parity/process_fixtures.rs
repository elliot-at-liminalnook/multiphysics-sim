//! Unexecuted regression models: virtual time, no processes or signals launched.
use super::*;
use std::collections::VecDeque;
#[derive(Default)]
struct Model {
    time: Duration,
    events: Vec<String>,
    exited: bool,
    observation_error: bool,
    reaps: VecDeque<Result<Option<ExitStatus>, String>>,
    signal_error: bool,
    observation_error_after_term: bool,
}
impl Backend for Model {
    fn observe(&mut self) -> Result<bool, String> {
        self.events.push("observe-without-reap".into());
        if self.observation_error
            || (self.observation_error_after_term
                && self.events.iter().any(|event| event == "signal:15"))
        {
            Err("ECHILD: identity released externally".into())
        } else {
            Ok(self.exited)
        }
    }
    fn signal(&mut self, signal: i32) -> Result<(), String> {
        self.events.push(format!("signal:{signal}"));
        if self.signal_error {
            Err("signal refused".into())
        } else {
            Ok(())
        }
    }
    fn reap(&mut self) -> Result<Option<ExitStatus>, String> {
        self.events.push("reap".into());
        self.reaps.pop_front().unwrap_or(Ok(None))
    }
    fn now(&self) -> Duration {
        self.time
    }
    fn pause(&mut self) {
        self.time += Duration::from_millis(250);
    }
}
#[cfg(unix)]
fn success() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0)
}
fn owner(model: Model) -> Lifecycle<Model> {
    Lifecycle {
        backend: model,
        phase: Phase::Owned,
        exit: None,
        error: None,
    }
}
#[test]
#[cfg(unix)]
fn natural_exit_retains_zombie_until_descendants_receive_final_signal() {
    let mut model = Model {
        exited: true,
        ..Default::default()
    };
    model.reaps.push_back(Ok(Some(success())));
    let mut owned = owner(model);
    assert!(owned.observe().unwrap());
    assert!(!owned.backend.events.iter().any(|event| event == "reap"));
    let completion = owned.completion(Duration::from_secs(10), &|| false);
    assert!(completion.interruption.is_none());
    assert!(completion.exit.unwrap().success());
    let events = owned.backend.events.clone();
    let term = events
        .iter()
        .position(|event| event == "signal:15")
        .unwrap();
    let kill = events.iter().position(|event| event == "signal:9").unwrap();
    let reap = events.iter().position(|event| event == "reap").unwrap();
    assert!(term < kill && kill < reap);
    assert!(owned.backend.time >= Duration::from_secs(2));
    assert_eq!(owned.phase, Phase::Released);
    owned.cleanup().unwrap(); // repeated stop and Drop perform no syscalls
    assert_eq!(owned.backend.events, events);
}
#[test]
#[cfg(unix)]
fn cancellation_and_timeout_reach_the_completion_loop() {
    for cancellation in [true, false] {
        let mut model = Model::default();
        model.reaps.push_back(Ok(Some(success())));
        let mut owned = owner(model);
        let completion = owned.completion(Duration::from_secs(1), &|| cancellation);
        assert_eq!(
            completion.interruption.as_deref(),
            Some(if cancellation {
                "cancelled"
            } else {
                "bounded child wait expired"
            })
        );
        assert!(completion.exit.unwrap().success());
        assert!(completion.cleanup_error.is_none());
        assert_eq!(owned.phase, Phase::Released);
        assert_eq!(
            owned.backend.time,
            Duration::from_secs(if cancellation { 2 } else { 3 })
        );
    }
}
#[test]
#[cfg(unix)]
fn failed_startup_stops_a_live_child_and_drop_is_idempotent() {
    let mut model = Model::default();
    model.reaps.push_back(Ok(Some(success())));
    let mut owned = owner(model);
    assert!(!owned.observe().unwrap()); // startup probe has no ready service
    owned.cleanup().unwrap(); // same path as startup-error owner Drop
    let events = owned.backend.events.clone();
    owned.cleanup().unwrap();
    assert_eq!(events, owned.backend.events);
}

#[test]
fn lost_wait_identity_never_signals_cached_group() {
    let mut owned = owner(Model {
        observation_error: true,
        ..Default::default()
    });
    assert!(owned.cleanup().is_err());
    assert_eq!(owned.phase, Phase::Lost);
    let events = owned.backend.events.clone();
    assert!(events.iter().all(|event| event == "observe-without-reap"));
    assert!(owned.cleanup().is_err());
    assert_eq!(events, owned.backend.events);
}
#[test]
fn reap_timeout_retries_only_reaping_never_group_signals() {
    let mut owned = owner(Model::default());
    assert!(owned.cleanup().is_err());
    assert_eq!(owned.phase, Phase::SignalsFinished);
    let signals = owned
        .backend
        .events
        .iter()
        .filter(|event| event.starts_with("signal:"))
        .count();
    assert!(owned.cleanup().is_err());
    assert_eq!(
        signals,
        owned
            .backend
            .events
            .iter()
            .filter(|event| event.starts_with("signal:"))
            .count()
    );
    assert!(owned.backend.time <= Duration::from_secs(6));
}
#[test]
fn reap_error_releases_signal_capability_and_preserves_error() {
    let mut model = Model::default();
    model.reaps.push_back(Err("ECHILD at reap".into()));
    let mut owned = owner(model);
    assert!(owned.cleanup().unwrap_err().contains("ECHILD"));
    assert_eq!(owned.phase, Phase::Lost);
    let events = owned.backend.events.clone();
    assert!(owned.cleanup().is_err());
    assert_eq!(events, owned.backend.events);
}
#[test]
#[cfg(unix)]
fn refused_signals_never_claim_clean_shutdown() {
    let mut model = Model {
        signal_error: true,
        ..Default::default()
    };
    model.reaps.push_back(Ok(Some(success())));
    let mut owned = owner(model);
    assert!(owned.cleanup().is_err());
    assert_eq!(owned.phase, Phase::Released);
    assert!(owned.error.as_deref().unwrap().contains("signal refused"));
}

#[test]
fn term_failure_and_later_identity_loss_both_survive_completion() {
    let model = Model {
        signal_error: true,
        observation_error_after_term: true,
        ..Default::default()
    };
    let mut owned = owner(model);
    let completion = owned.completion(Duration::from_secs(1), &|| true);
    let error = completion.cleanup_error.unwrap();
    assert!(error.contains("signal refused"));
    assert!(error.contains("ECHILD"));
    assert_eq!(owned.phase, Phase::Lost);
    assert!(
        !owned
            .backend
            .events
            .iter()
            .any(|event| event == "signal:9" || event == "reap")
    );
    let events = owned.backend.events.clone();
    assert!(owned.cleanup().is_err());
    assert_eq!(events, owned.backend.events);
}
