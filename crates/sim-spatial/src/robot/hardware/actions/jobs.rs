//! The panel's job results (JobResults): the connect job, STOP answers, the
//! export, then the link's snapshot and the counters the form follows.
use super::*;

/// JobResults: the connect job (a new link, sent the form's inputs), STOP
/// answers (to the link), the export, then the link's snapshot for this
/// frame and the counters the form follows (tune/campaign confirmations
/// unchecked when one finishes, the speed back to 0 when a sweep starts).
pub(in crate::robot::hardware) fn poll_jobs(hw: Option<ResMut<Hardware>>) {
    let Some(mut hw) = hw else { return };
    let hw = &mut *hw;
    if let Some(result) = hw.connecting.as_ref().and_then(|j| j.poll()) {
        let generation = hw.connecting.take().map_or(hw.generation, |j| j.generation());
        match result {
            Ok(client) => {
                let pinned = client.calibration_execution.as_ref().is_some_and(|(identity, _)| identity.is_virtual_calibration());
                let link = link::Link::spawn(client, generation);
                // A new link counts its speed resets from 0.
                hw.form.inputs.speed_reset = 0;
                link.send(LinkCommand::Inputs(hw.form.inputs.clone()));
                hw.link = Some(link);
                hw.notice = None;
                if std::mem::take(&mut hw.replaced_virtual) && !pinned {
                    hw.notice = Some(BENCH_GONE.into());
                }
                // The suspended-leg confirmation attests the leg on the link it
                // was given for (remotely, only a virtual one): a new link
                // starts unchecked, so a confirmation never carries from a
                // virtual bench to a physical leg.
                hw.form.gait_ok = false;
                hw.form.seen_tune_done = 0;
                hw.form.seen_campaign_done = 0;
                hw.form.seen_speed_reset = 0;
                hw.ui_revision += 1;
            }
            Err(e) => hw.notice = Some(format!("Could not connect to {}: {e}", hw.configuration_label())),
        }
    }
    // An immediate STOP's answer goes to the link that posted it (a STOP
    // posted before a reconnect is answered to nobody: its link is gone).
    let mut answered = Vec::new();
    hw.stops.retain(|job| match job.poll() {
        Some(result) => {
            answered.push((job.generation(), result));
            false
        }
        None => true,
    });
    if let Some(link) = hw.link.as_ref() {
        for (_, result) in answered.into_iter().filter(|(generation, _)| *generation == link.generation) {
            link.send(LinkCommand::StopAnswered(result));
        }
    }
    if let Some(result) = hw.export.as_ref().and_then(|j| j.poll()) {
        let seq = hw.export.take().map_or(hw.export_seq, |j| j.generation());
        let line = match &result {
            Ok(exported) => format!("Saved {}", exported.path.display()),
            Err(e) => format!("Download failed: {e}"),
        };
        // Labelled when the link was pinned to a virtual bench, or the
        // server labelled the document simulated itself.
        let simulated = hw.export_virtual || result.as_ref().is_ok_and(|e| e.simulated);
        hw.export_line = Some(if simulated { format!("{} {line}", crate::robot::hardware::handlers::VIRTUAL_EXPORT) } else { line });
        hw.export_done = Some((seq, result));
    }
    hw.snapshot = hw.link.as_ref().map(|l| l.snapshot()).unwrap_or_default();
    // Remote jog presses nobody waits on (a one-way activation): a refusal
    // puts the held flag back and shows the refusal (a REST one is settled
    // as it resolves). The results are moved out and back, not cloned.
    if !hw.pending_presses.is_empty() {
        let results = std::mem::take(&mut hw.snapshot.command_results);
        crate::robot::hardware::handlers::settle_presses(hw, &results);
        hw.snapshot.command_results = results;
    }
    // Staleness revokes this generation permanently, even if an in-flight
    // request later produces a fresh-looking answer. STOP does not wait for it.
    // A status that aged only because the link thread is waiting for a
    // request that may still answer (a select proving watchdogs, a tune
    // start: the server waits up to 8 s for its hardware, the client 10 s) is
    // not a lost binding; that request's own failure revokes if it is
    // (`calibration::binding_lost`), and its deadline bounds the wait. The
    // panel still shows the status as stale, and queued remote commands still
    // need a fresh status when the link thread takes them.
    let now = Instant::now();
    let revoke = hw.snapshot.execution.is_some() && hw.snapshot.read_at.is_some()
        && ((hw.snapshot.stale(now) && !hw.snapshot.awaiting_answer(now)) || !hw.snapshot.connection_valid || hw.snapshot.authorization_revoked);
    if revoke && hw.link.as_ref().is_some_and(|link| !link.authorization.swap(true, std::sync::atomic::Ordering::SeqCst)) {
        stop_immediate(hw);
        hw.snapshot.authorization_revoked = true;
    }
    let (tune, campaign, reset) = (hw.snapshot.tune_done, hw.snapshot.campaign_done, hw.snapshot.speed_reset);
    let f = &mut hw.form;
    if tune > f.seen_tune_done {
        f.tune_ok = false;
    }
    if campaign > f.seen_campaign_done {
        f.campaign_ok = false;
    }
    let speed_reset = reset > f.seen_speed_reset;
    if speed_reset {
        // The link reset the speed (a sweep started): the slider goes to 0,
        // and the inputs say which reset they include, so the link does not
        // take an older speed sent before it for a new one.
        f.inputs.speed_percent = 0.0;
        f.inputs.speed_reset = reset;
    }
    (f.seen_tune_done, f.seen_campaign_done, f.seen_speed_reset) = (tune, campaign, reset);
    if speed_reset {
        inputs_changed(hw);
    }
}
