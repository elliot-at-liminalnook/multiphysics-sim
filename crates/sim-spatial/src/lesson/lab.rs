//! Hardware lab worker and independent lifetime STOP. The lesson apply owns UI
//! state; jobs owns acquisition. This feature never contacts a hardware server.
use super::*;
use crate::app::actions::{Act, Origin};
use crate::robot::hardware::local::{Client, ServiceKind};
use sim_runtime::{
    hardware::protocol::{Body, calibration},
    lesson_lab,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(super) struct Control {
    stopped: AtomicBool,
    client: Mutex<Option<Client>>,
}
impl Control {
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(client) = self
            .client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            client.latch_stop();
        }
    }
    fn install(&self, client: Client) -> Result<(), String> {
        let mut slot = self.client.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::SeqCst) {
            client.latch_stop();
            return Err("Lesson lab cancelled before acquisition; nothing ran".into());
        }
        *slot = Some(client);
        Ok(())
    }
    fn check(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            Err("Lesson lab STOP requested; release readback pending".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for super::extras::LabState {
    fn drop(&mut self) {
        if let Some(control) = &self.control {
            control.stop();
        }
    }
}
/// Releases the actual client before the acquisition job returns, including errors
/// and unwinding. A latch requests release; it does not claim stationary proof.
struct Release(Arc<Control>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.stop();
        self.0
            .client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}
impl Learn {
    pub(crate) fn stop_labs(&mut self) {
        for state in self.labs.values_mut() {
            if let Some(control) = &state.control {
                control.stop();
            }
            if let Some(job) = &state.job {
                job.cancel();
            }
        }
    }
    pub(crate) fn run_lab(&mut self, id: &str) -> Result<(), String> {
        if !self.active {
            return Err("Open the lesson at the window before running its hardware lab".into());
        }
        let lab = self
            .lesson
            .as_ref()
            .and_then(|l| l.lab(id))
            .cloned()
            .ok_or("no such lab")?;
        let path = lesson_lab::bench_config()?.ok_or(
            "Set SIM_BENCH_CONFIG to a local calibration JSON file; no hardware server is used",
        )?;
        let state = self.labs.entry(id.into()).or_default();
        if state.running {
            return Err("A lab step is already running; STOP before replacing it".into());
        }
        if lab.predict.is_some() && sim_lesson::units::split_quantity(&state.prediction).is_none() {
            return Err("write your prediction first".into());
        }
        if !state.ticks.iter().all(|t| *t) {
            return Err("tick every item of the checklist first".into());
        }
        let control = Arc::new(Control::default());
        let worker_control = control.clone();
        state.job = Some(crate::jobs::Job::spawn(
            crate::jobs::Pool::Dedicated,
            0,
            "in-process lesson lab",
            move |ctx| {
                let _release = Release(worker_control.clone());
                if ctx.cancelled() {
                    return Err("Lesson lab cancelled before acquisition; nothing ran".into());
                }
                worker_control.check()?;
                let client = Client::open(&path, ServiceKind::Calibration)?;
                worker_control.install(client.clone())?;
                // No virtual lab motion authorization is added: the shared allowlist
                // deliberately refuses lab_step, just as the reference does.
                let status = client.get(calibration::STATUS).map_err(|e| e.to_string())?;
                if status["execution"]["kind"] != "physical" {
                    return Err("Lesson lab steps require the physical operator; virtual lab_step is outside the calibration scope".into());
                }
                worker_control.check()?;
                let inspected = client
                    .post(calibration::COMMAND, &calibration::inspect(1))
                    .map_err(|e| e.to_string())?;
                let id = inspected["calibration"]["axes"]
                    .as_object()
                    .and_then(|axes| {
                        axes.iter()
                            .find(|(_, axis)| axis["role"].as_str() == Some(lab.joint.as_str()))
                    })
                    .and_then(|(id, _)| id.parse::<u8>().ok())
                    .ok_or_else(|| format!("No motor has the role `{}`", lab.joint))?;
                let select_epoch = client.calibration_stop_epoch();
                worker_control.check()?;
                let selected = client
                    .post_at_epoch(
                        calibration::COMMAND,
                        &calibration::select(id, 2, true),
                        select_epoch,
                    )
                    .map_err(|e| e.to_string())?;
                if selected["enabled_id"].as_u64() != Some(id as u64) {
                    return Err("Watchdog proof/selection refused; the lab did not start".into());
                }
                let motion_epoch = client.calibration_stop_epoch();
                worker_control.check()?;
                let request = lesson_lab::bench_request(&lab, true)?;
                let body = Body(
                    request
                        .as_object()
                        .ok_or("Invalid lab request")?
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone().into()))
                        .chain([("sequence".into(), 3u64.into())])
                        .collect(),
                );
                let mut status = client
                    .post_at_epoch(calibration::COMMAND, &body, motion_epoch)
                    .map_err(|e| e.to_string())?;
                let started = std::time::Instant::now();
                loop {
                    if ctx.cancelled() {
                        worker_control.stop();
                        return Err("Lesson lab cancelled; release readback pending".into());
                    }
                    worker_control.check()?;
                    if let Some(result) = lesson_lab::bench_result(&status)? {
                        return Ok(result);
                    }
                    if started.elapsed() > std::time::Duration::from_secs(60) {
                        return Err("the bench did not finish within a minute".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    status = client.get(calibration::STATUS).map_err(|e| e.to_string())?;
                }
            },
        ));
        state.control = Some(control);
        state.running = true;
        state.result = None;
        Ok(())
    }
}
/// Always drains loss messages, even outside Lessons. The independent latch is
/// immediate; the quiet occurrence is consumed by the existing action apply.
pub(super) fn window_loss(
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut close: MessageReader<bevy::window::WindowCloseRequested>,
    learn: Option<Res<Learn>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut out: MessageWriter<Act<super::actions::LessonCommand>>,
) {
    let lost = focus.read().any(|e| !e.focused);
    let closing = close.read().next().is_some();
    if !(lost || closing || keys.just_pressed(KeyCode::Escape)) {
        return;
    }
    let Some(learn) = learn else {
        return;
    };
    if !learn.labs.values().any(|state| state.running) {
        return;
    }
    for state in learn.labs.values() {
        if let Some(control) = &state.control {
            control.stop();
        }
    }
    out.write(Act {
        action: super::actions::LessonCommand::Ui(LessonAction::LabStop),
        origin: Origin::Quiet,
    });
}
