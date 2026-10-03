use super::*;
use std::io::Read;
#[test]
fn lab_results_join_the_gait_list() {
    let base = std::env::temp_dir().join(format!("lab-catalog-{}", std::process::id()));
    let results = base.join("gait-lab-test/results");
    let entry = |name: &str, report: &str, governor: bool| {
        let d = results.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("report.yaml"), report).unwrap();
        fs::write(d.join("compiled.json"), "{}").unwrap();
        if governor {
            fs::write(d.join("spec-identity.json"), "{}").unwrap();
        }
    };
    entry("slow", "gait: slow\nstatus: passed\nspeed_m_s: 0.1\n", true);
    entry("fast", "gait: fast\nstatus: passed\nspeed_m_s: 0.2\n", true);
    entry(
        "fell",
        "gait: fell\nstatus: rejected\nspeed_m_s: null\n",
        true,
    );
    entry(
        "no-governor",
        "gait: old\nstatus: passed\nspeed_m_s: 0.3\n",
        false,
    );
    entry(
        "crouch",
        "kind: pose_sequence\nsequence: crouch\nstatus: ready\n",
        false,
    );
    entry(
        "blocked",
        "kind: pose_sequence\nsequence: over\nstatus: blocked\n",
        false,
    );
    let rows = lab_catalog(&base);
    fs::remove_dir_all(&base).ok();
    let names: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["kind"].as_str().unwrap(), r["trial"].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        [
            ("lab_gait", "fast"),
            ("lab_gait", "slow"),
            ("pose_sequence", "crouch")
        ],
        "passed gaits fastest first, then ready poses"
    );
}
fn tuning() -> SweepTuning {
    serde_json::from_str::<Config>(include_str!(
        "../../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json"
    ))
    .unwrap()
    .sweep_tuning
}
#[test]
fn sweep_lease_cannot_be_revived_or_updated_by_another_run() {
    let mut lease = BrowserSweep {
        owner: "owner".into(),
        id: 2,
        run_id: 4,
        sequence: 1,
        input: SweepInput {
            speed_counts_s: 5.,
            pwm_limit: 100,
        },
        last_seen: Instant::now(),
        motion: MotionCommand::Sweep,
        teaching: false,
        capture: None,
    };
    let mut request:Request=serde_json::from_value(json!({"action":"sweep_update","id":2,"run_id":4,"sequence":2,"speed_counts_s":10.,"drive_pwm":200})).unwrap();
    assert!(lease.update("other", &request, &tuning()).is_err());
    request.run_id = 3;
    assert!(lease.update("owner", &request, &tuning()).is_err());
    request.run_id = 4;
    lease.update("owner", &request, &tuning()).unwrap();
    assert_eq!(lease.input.speed_counts_s, 10.);
    assert!(lease.update("owner", &request, &tuning()).is_err());
    request.sequence = 3;
    lease.last_seen = Instant::now() - Duration::from_secs(2);
    assert!(lease.update("owner", &request, &tuning()).is_err());
}
fn identity() -> ExecutionIdentity {
    ExecutionIdentity {
        schema_version: 1,
        kind: "virtual_calibration".into(),
        server_instance: crate::hardware::protocol::new_client_id(),
        bench_instance: crate::hardware::protocol::new_client_id(),
    }
}
fn app_fixture(execution: ExecutionIdentity) -> (Arc<App>, mpsc::Receiver<Job>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let app = Arc::new(App {
        execution: execution.clone(),
        shutdown: AtomicBool::new(false),
        handles: std::sync::atomic::AtomicUsize::new(1),
        generations: Mutex::new(std::collections::BTreeMap::new()),
        safety: Mutex::new(0),
        state: Mutex::new(
            json!({"execution":execution,"campaign":{"result":{"completed":3}},"tuning":{"result":{"kp":7}},"calibration":{"axes":{"1":{"lower":100,"upper":200}}}}),
        ),
        jobs: tx,
        stop: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        cancel_sequence: AtomicU64::new(0),
        sweep: Mutex::new(None),
        gait: Mutex::new(None),
        sweep_tuning: tuning(),
    });
    (app, rx)
}
fn wire_post(
    app: Arc<App>,
    client: &str,
    body: Value,
    pin: Option<(&ExecutionIdentity, u64)>,
) -> String {
    // Exercise shared application validation; HTTP only maps its result.
    let handle = std::mem::ManuallyDrop::new(Handle { app });
    match handle.command(
        client,
        body,
        pin.map(|(identity, generation)| (identity.clone(), generation)),
    ) {
        Ok(value) => format!("HTTP/1.1 200 Response\r\n\r\n{value}"),
        Err(error) => format!(
            "HTTP/1.1 {} Response\r\n\r\n{}",
            if error.starts_with(BINDING_REFUSED) {
                409
            } else {
                400
            },
            json!({"error":error})
        ),
    }
}
#[test]
fn authoritative_handler_refuses_unbound_replaced_physical_and_out_of_scope() {
    let pin = identity();
    let client = crate::hardware::protocol::new_client_id();
    for kind in ["virtual_calibration", "physical"] {
        let mut active = pin.clone();
        active.kind = kind.into();
        let (app, rx) = app_fixture(active);
        let mut replaced = pin.clone();
        replaced.server_instance = crate::hardware::protocol::new_client_id();
        for proposed in [&pin, &replaced] {
            let answer = wire_post(
                app.clone(),
                &client,
                json!({"action":"jog","id":1,"delta":1}),
                Some((proposed, 1)),
            );
            if kind == "virtual_calibration" && proposed == &pin {
                // Valid binding, out-of-scope action: an ordinary refusal that keeps the pin.
                assert!(
                    answer.starts_with("HTTP/1.1 400")
                        && !answer.contains(BINDING_REFUSED)
                        && answer.contains(&out_of_scope("jog")),
                    "{answer}"
                );
            } else {
                assert!(
                    answer.starts_with("HTTP/1.1 409") && answer.contains(BINDING_REFUSED),
                    "binding refusal, not a business 400"
                );
            }
        }
        let answer = wire_post(
            app.clone(),
            &client,
            json!({"action":"select","id":1}),
            Some((&replaced, 1)),
        );
        assert!(answer.starts_with("HTTP/1.1 409") && answer.contains(BINDING_REFUSED));
        assert!(
            rx.try_recv().is_err(),
            "refused requests never reach acquisition"
        );
    }
    let (app, _) = app_fixture(pin);
    assert!(
        wire_post(app, &client, json!({"action":"inspect"}), None).starts_with("HTTP/1.1 409"),
        "virtual identity required is a binding refusal"
    );
}
#[test]
fn newer_generation_revokes_queued_work_and_stop_bypasses_full_queue_preserving_records() {
    let pin = identity();
    let client = crate::hardware::protocol::new_client_id();
    let (app, rx) = app_fixture(pin.clone());
    app.generations.lock().unwrap().insert(client.clone(), 1);
    let (reply, _) = mpsc::channel();
    app.jobs
        .try_send(Job {
            queued: Instant::now(),
            execution: Some((pin.clone(), 1)),
            stop_epoch: 0,
            request: serde_json::from_value(json!({"action":"select","id":1})).unwrap(),
            client: client.clone(),
            reply,
        })
        .unwrap();
    // This real inline consumer registers generation 2 before rejecting a
    // missing hold session, proving queued generation 1 is now invalid.
    assert!(
        wire_post(
            app.clone(),
            &client,
            json!({"action":"capture_hold","id":1}),
            Some((&pin, 2))
        )
        .starts_with("HTTP/1.1 400")
    );
    let queued = rx.try_recv().unwrap();
    assert!(
        app.check_execution(
            &queued.request.action,
            &queued.client,
            queued.execution.as_ref()
        )
        .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
    app.jobs.try_send(queued).unwrap();
    let records = app.state.lock().unwrap().clone();
    let epoch = *app.safety.lock().unwrap();
    let response = wire_post(
        app.clone(),
        &client,
        json!({"action":"stop","id":null}),
        None,
    );
    assert!(response.starts_with("HTTP/1.1 200"));
    let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(
        body["enabled_id"].is_null()
            && body["busy"] == json!(false)
            && body["stop_latched"] == json!(true),
        "early reply shows the axis disabled at once"
    );
    assert_eq!(body["message"], json!(STOP_LATCHED_MESSAGE));
    assert!(app.stop.load(Ordering::SeqCst) && app.cancel.load(Ordering::SeqCst));
    assert!(
        app.arm(epoch, true).is_err(),
        "STOP after early reply cannot be cleared"
    );
    assert_eq!(
        *app.state.lock().unwrap(),
        records,
        "STOP latch keeps taught/tune/campaign records"
    );
}
fn physical() -> ExecutionIdentity {
    let mut identity = identity();
    identity.kind = "physical".into();
    identity
}
fn config_fixture(output: PathBuf) -> Config {
    let mut cfg: Config = serde_json::from_str(include_str!(
        "../../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json"
    ))
    .unwrap();
    // Never the real adapter path in server.json: nothing here may open hardware.
    cfg.serial = "/nonexistent/test-serial".into();
    cfg.output = output;
    cfg.campaign_plan = None;
    cfg
}
#[test]
fn out_of_scope_with_a_valid_pin_is_a_400_that_never_runs_and_keeps_the_binding() {
    let pin = identity();
    let client = crate::hardware::protocol::new_client_id();
    let (app, rx) = app_fixture(pin.clone());
    for action in ["jog", "step", "flip", "direction", "lab_step"] {
        let answer = wire_post(
            app.clone(),
            &client,
            json!({"action":action,"id":1}),
            Some((&pin, 1)),
        );
        assert!(
            answer.starts_with("HTTP/1.1 400")
                && !answer.contains(BINDING_REFUSED)
                && answer.contains(&out_of_scope(action)),
            "{action}: {answer}"
        );
        assert!(rx.try_recv().is_err(), "{action} never reaches acquisition");
        assert_eq!(
            app.check_execution(action, &client, Some(&(pin.clone(), 1))),
            Err(out_of_scope(action))
        );
    }
    assert_eq!(
        app.generations.lock().unwrap().get(&client),
        Some(&1),
        "the binding is unchanged"
    );
    assert!(
        app.check_execution("select", &client, Some(&(pin.clone(), 1)))
            .is_ok(),
        "in-scope commands still pass"
    );
    // A wrong identity is a binding refusal whatever the action.
    let mut replaced = pin.clone();
    replaced.bench_instance = crate::hardware::protocol::new_client_id();
    assert!(
        app.check_execution("jog", &client, Some(&(replaced, 1)))
            .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
    assert!(
        app.check_execution("jog", &client, Some(&(pin, 2)))
            .is_err_and(|e| e.starts_with(BINDING_REFUSED)),
        "generation mismatch first"
    );
}
#[test]
fn virtual_gait_is_in_scope_and_keeps_every_other_refusal() {
    let pin = identity();
    let client = crate::hardware::protocol::new_client_id();
    let (app, rx) = app_fixture(pin.clone());
    app.generations.lock().unwrap().insert(client.clone(), 1);
    // A pinned virtual gait passes the binding and scope checks (never
    // posted here: it would queue to the absent worker).
    for action in ["gait_start", "gait_update"] {
        assert_eq!(
            app.check_execution(action, &client, Some(&(pin.clone(), 1))),
            Ok(()),
            "{action}"
        );
    }
    // The lease heartbeat is handled inline: with no gait playing it is an
    // ordinary refusal, not a scope or binding one.
    let answer = wire_post(
        app.clone(),
        &client,
        json!({"action":"gait_update","speed_scale":0.5,"playing":true}),
        Some((&pin, 1)),
    );
    assert!(
        answer.starts_with("HTTP/1.1 400")
            && answer.contains("No gait is playing on the leg")
            && !answer.contains(&out_of_scope("gait_update")),
        "{answer}"
    );
    // Raw step stays out of scope: a 400 that keeps the binding.
    let answer = wire_post(
        app.clone(),
        &client,
        json!({"action":"step","id":1}),
        Some((&pin, 1)),
    );
    assert!(
        answer.starts_with("HTTP/1.1 400")
            && !answer.contains(BINDING_REFUSED)
            && answer.contains(&out_of_scope("step")),
        "{answer}"
    );
    assert_eq!(
        app.generations.lock().unwrap().get(&client),
        Some(&1),
        "the binding is unchanged"
    );
    // A wrong identity or generation is still a binding refusal for a gait.
    let mut replaced = pin.clone();
    replaced.server_instance = crate::hardware::protocol::new_client_id();
    assert!(
        app.check_execution("gait_start", &client, Some(&(replaced.clone(), 1)))
            .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
    assert!(
        app.check_execution("gait_start", &client, Some(&(pin.clone(), 2)))
            .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
    let answer = wire_post(
        app.clone(),
        &client,
        json!({"action":"gait_start","id":1}),
        Some((&replaced, 1)),
    );
    assert!(
        answer.starts_with("HTTP/1.1 409") && answer.contains(BINDING_REFUSED),
        "{answer}"
    );
    // Unpinned on a virtual server: a binding refusal; STOP passes unpinned.
    assert!(
        app.check_execution("gait_start", &client, None)
            .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
    assert_eq!(app.check_execution("stop", &client, None), Ok(()));
    assert!(rx.try_recv().is_err(), "nothing reached acquisition");
    // Physical: no remote pin is accepted for a gait either.
    let (physical_app, _rx) = app_fixture(physical());
    assert!(
        physical_app
            .check_execution("gait_start", &client, Some(&(pin, 1)))
            .is_err_and(|e| e.starts_with(BINDING_REFUSED))
    );
}
#[test]
fn gait_records_events_and_status_are_labelled_and_old_records_are_not_simulated() {
    let (app, _rx) = app_fixture(identity());
    let event = labelled(&app, json!({"event":"gait_start","gait":"g"}));
    assert_eq!(
        event["execution"],
        serde_json::to_value(&app.execution).unwrap()
    );
    assert_eq!(event["simulated"], json!(true));
    assert_eq!(event["gait"], json!("g"), "the event itself is kept");
    let (physical_app, _rx) = app_fixture(physical());
    let event = labelled(&physical_app, json!({"event":"gait_end"}));
    assert_eq!(event["simulated"], json!(false));
    assert_eq!(event["execution"]["kind"], json!("physical"));
    assert!(virtual_gait_limits(None, 12.).contains("assumed 12.0 V"));
    assert!(virtual_gait_limits(Some(11.75), 11.75).contains("reported supply, 11.75 V"));

    let output =
        std::env::temp_dir().join(format!("gait-history-{}-{}", std::process::id(), stamp()));
    let runs = output.join("gait-runs");
    fs::create_dir_all(&runs).unwrap();
    let labelled_run = labelled(&app, json!({"gait":"virtual","outcome":"stopped"}));
    fs::write(runs.join("run-1.json"), labelled_run.to_string()).unwrap();
    fs::write(
        runs.join("run-2.json"),
        json!({"gait":"older","outcome":"stopped"}).to_string(),
    )
    .unwrap();
    let rows = gait_run_history(&config_fixture(output.clone()));
    fs::remove_dir_all(&output).ok();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            &rows[0]["gait"],
            &rows[0]["simulated"],
            &rows[0]["execution"]
        ),
        (&json!("older"), &json!(false), &Value::Null),
        "newest first; unlabelled is not simulated"
    );
    assert_eq!(
        (&rows[1]["gait"], &rows[1]["simulated"]),
        (&json!("virtual"), &json!(true))
    );
    assert_eq!(
        rows[1]["execution"],
        serde_json::to_value(&app.execution).unwrap()
    );
}
/// A hold session (`teaching`, holding) owned by `client` on motor 1, run 7.
fn holding_session(app: &App, client: &str) {
    *app.sweep.lock().unwrap() = Some(BrowserSweep {
        owner: client.into(),
        id: 1,
        run_id: 7,
        sequence: 1,
        input: SweepInput {
            speed_counts_s: 10.,
            pwm_limit: 200,
        },
        last_seen: Instant::now(),
        motion: MotionCommand::Hold,
        teaching: true,
        capture: None,
    });
}
fn capture_hold_body(sequence: u64) -> Value {
    json!({"action":"capture_hold","id":1,"sequence":sequence,"boundary":"upper","run_id":7,"speed_counts_s":10.,"drive_pwm":200,"motion":"hold"})
}
/// Plays the hold session's side once: waits for the pending capture,
/// takes it and answers with `outcome` (None: the session ends instead).
fn take_capture(app: Arc<App>, outcome: Option<R<()>>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let taken = app
                .sweep
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|c| c.capture.take());
            if let Some(capture) = taken {
                match outcome {
                    Some(outcome) => {
                        if outcome.is_ok() {
                            app.state.lock().unwrap()["capture_message"] =
                                json!("Saved upper pose");
                        }
                        capture.done.try_send(outcome).unwrap();
                    }
                    None => {
                        app.state.lock().unwrap()["message"] =
                            json!("Browser heartbeat lost; sweep stopped");
                        *app.sweep.lock().unwrap() = None;
                        drop(capture);
                    }
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no capture was pending");
    })
}
#[test]
fn capture_hold_answers_only_after_the_hold_session_saves_or_refuses() {
    let pin = identity();
    let client = crate::hardware::protocol::new_client_id();
    let (app, rx) = app_fixture(pin.clone());
    holding_session(&app, &client);
    // Never taken: the 1 s timeout, and the capture is withdrawn.
    let started = Instant::now();
    let answer = wire_post(app.clone(), &client, capture_hold_body(2), Some((&pin, 1)));
    assert!(
        answer.starts_with("HTTP/1.1 400")
            && answer
                .contains("Pose not saved: the hold session did not take the capture within 1 s"),
        "{answer}"
    );
    assert!(started.elapsed() >= CAPTURE_WAIT);
    assert!(
        app.sweep
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .capture
            .is_none(),
        "a withdrawn capture can never be saved later"
    );
    // Saved: 200 with the whole state, read after the save.
    let session = take_capture(app.clone(), Some(Ok(())));
    let answer = wire_post(app.clone(), &client, capture_hold_body(3), Some((&pin, 1)));
    session.join().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let body: Value = serde_json::from_str(answer.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["capture_message"], json!("Saved upper pose"));
    assert_eq!(
        body["execution"],
        serde_json::to_value(&pin).unwrap(),
        "the full state, as capture answers"
    );
    // Refused by the session: an ordinary 400 with its reason.
    let session = take_capture(
        app.clone(),
        Some(Err(
            "Still settling. Release Q/A, wait for Holding, then save the pose.".into(),
        )),
    );
    let answer = wire_post(app.clone(), &client, capture_hold_body(4), Some((&pin, 1)));
    session.join().unwrap();
    assert!(
        answer.starts_with("HTTP/1.1 400")
            && answer.contains("Still settling")
            && !answer.contains(BINDING_REFUSED),
        "{answer}"
    );
    // The session ended before saving: the sender is dropped.
    let session = take_capture(app.clone(), None);
    let answer = wire_post(app.clone(), &client, capture_hold_body(5), Some((&pin, 1)));
    session.join().unwrap();
    assert!(
        answer.starts_with("HTTP/1.1 400")
            && answer.contains("Pose not saved: the hold session ended before the capture")
            && answer.contains("Browser heartbeat lost"),
        "{answer}"
    );
    assert!(rx.try_recv().is_err(), "capture_hold never queues a job");
}
#[test]
fn session_binding_follows_the_live_coordinate_session() {
    let mut a = AxisCalibration::default();
    assert!(!bound_to_session(&a, Some("1")), "no multi-turn poses");
    a.coordinate_session = Some("1".into());
    assert!(bound_to_session(&a, Some("1")));
    assert!(
        !bound_to_session(&a, Some("2")),
        "already unusable after a re-stamp; no second re-stamp"
    );
    assert!(
        usable(
            &AxisCalibration {
                lower: Some(5000),
                ..a.clone()
            },
            Some("2")
        )
        .lower
        .is_none(),
        "a re-stamp drops the stale poses"
    );
    a.coordinate_session = None;
    a.reference_session = Some("2".into());
    assert!(bound_to_session(&a, Some("2")));
    assert!(!bound_to_session(&a, None));
}
#[test]
fn stop_outcome_separates_absent_disabled_motors_from_unverified_torque_off() {
    const WRAP: &str = "Stop readback unverified after bounded retries: ";
    let timeout = |id: u8| format!("ID {id}: serial reply timeout (0 reply bytes received)");
    // (id, disabled, error, reset_turns, failure, disconnect, link_lost)
    let cases = [
            (1, false, format!("{WRAP}{}", timeout(1)), true, true, true, true),
            (1, false, format!("{WRAP}Stop not verified: cut motor supply power"), false, true, false, false),
            (1, false, format!("{WRAP}ID 1: device error 32"), false, true, false, false),
            (1, false, format!("{WRAP}bad checksum"), true, true, true, true),
            (1, false, "Device not configured (os error 6)".into(), true, true, true, false),
            (1, false, "Broken pipe (os error 32)".into(), false, true, false, true),
            // The one note: the disabled motor itself never answered.
            (2, true, timeout(2), true, false, false, false),
            (2, true, timeout(254), true, true, true, true),
            (2, true, timeout(3), true, true, true, true),
            (2, true, "Stop not verified: cut motor supply power".into(), false, true, false, false),
            (2, true, "ID 2: device error 32".into(), false, true, false, false),
            (2, true, "Broken pipe (os error 32)".into(), false, true, false, true),
            (2, true, "Device not configured (os error 6)".into(), true, true, true, false),
            (2, true, "Ambiguous half-turn encoder jump; reference must be re-established".into(), true, true, true, true),
            (2, true, "STOP sent; receive stream fault prevents physical verification. Cut motor power, then reconnect.".into(), true, true, true, true),
        ];
    for (id, disabled, error, reset_turns, failure, disconnect, link_lost) in cases {
        assert_eq!(
            stop_outcome(id, disabled, &error),
            StopOutcome {
                reset_turns,
                failure,
                disconnect,
                link_lost
            },
            "{id} {disabled} {error}"
        );
    }
}
#[test]
fn device_loss_and_corrupt_frames_count_as_lost_readback() {
    // Exact texts from servo_bus.rs, calibration.rs and actuator_sweep.rs.
    let corrupt = [
        "bad framing or length",
        "foreign reply ID",
        "bad checksum",
        "unexpected payload width",
        "Invalid bridge stream frame header or length",
        "Invalid bridge stream frame checksum",
        "Ambiguous half-turn encoder jump; reference must be re-established",
        "ambiguous encoder wrap: observation gap exceeds speed bound",
    ];
    for e in corrupt {
        assert!(
            readback_lost(e) && transport_lost(e),
            "{e}: turns reset, a virtual bus is dropped"
        );
    }
    for e in [
        "Device not configured (os error 6)",
        "No such device or address (os error 6)",
        "Input/output error (os error 5)",
    ] {
        assert!(
            readback_lost(e) && !transport_lost(e),
            "{e}: turns reset; never drops a virtual socket"
        );
    }
    for e in [
        "Unknown motor ID",
        "ID 1: device error 32",
        "No space left on device (os error 28)",
        "Permission denied (os error 13)",
    ] {
        assert!(!readback_lost(e), "{e}");
    }
}
#[test]
fn virtual_export_is_labelled_and_physical_export_unchanged() {
    let (app, _rx) = app_fixture(identity());
    let doc = export_document(&app);
    assert_eq!(
        doc["execution"],
        serde_json::to_value(&app.execution).unwrap(),
        "the identity the viewer pins"
    );
    assert_eq!(doc["simulated"], json!(true));
    assert_eq!(
        doc["axes"],
        app.state.lock().unwrap()["calibration"]["axes"]
    );
    let (app, _rx) = app_fixture(physical());
    assert_eq!(
        export_document(&app),
        app.state.lock().unwrap()["calibration"],
        "no new keys on a physical export"
    );
}
/// A bench on a Unix socket (never a serial device): completes the
/// handshake, then answers each request frame with `respond`.
fn fake_bench(
    respond: fn(&[u8]) -> Option<Vec<u8>>,
) -> (CalibrationBus, std::thread::JoinHandle<()>, PathBuf) {
    use std::os::unix::net::UnixListener;
    // Short absolute base: macOS $TMPDIR exceeds the 104-byte sun_path limit.
    let dir = PathBuf::from(format!(
        "/tmp/fb-{}-{}",
        std::process::id(),
        &crate::hardware::protocol::new_client_id()[..8]
    ));
    fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("bench.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let instance = crate::hardware::protocol::new_client_id();
    let peer_instance = instance.clone();
    let peer = std::thread::spawn(move || {
        let mut peer = listener.accept().unwrap().0;
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut greeting = [0; 25];
        peer.read_exact(&mut greeting).unwrap();
        assert_eq!(&greeting, b"HX-VIRTUAL-CALIBRATION/1\n");
        writeln!(
            peer,
            "{}",
            json!({"schema_version":1,"kind":"virtual_calibration","bench_instance":peer_instance})
        )
        .unwrap();
        // Stay connected until the client closes, like the real bench.
        let (mut pending, mut buf) = (Vec::new(), [0u8; 256]);
        loop {
            match peer.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => pending.extend_from_slice(&buf[..n]),
            }
            while pending.len() >= 4 && pending.len() >= pending[3] as usize + 4 {
                let total = pending[3] as usize + 4;
                let frame: Vec<u8> = pending.drain(..total).collect();
                if let Some(reply) = respond(&frame) {
                    if peer.write_all(&reply).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let (bus, opened) =
        CalibrationBus::open_virtual(&socket, Some(&instance), &dir.join("serial.jsonl")).unwrap();
    assert_eq!(opened, instance);
    (bus, peer, dir)
}
/// The FPGA (ID 254) answers every request with a valid calibration
/// profile status; motors never answer.
fn fpga_reply(frame: &[u8]) -> Option<Vec<u8>> {
    (frame[2] == 254).then(|| {
        crate::acquisition::servo_bus::packet(
            254,
            0,
            &[5, 0, 0, 0, 0, 0, 0, 0, 60, 90, 126, 208, 7],
        )
        .unwrap()
    })
}
fn run_observe_stop(app: &App, cfg: &Config, cal: &Calibration, bus: &mut Option<CalibrationBus>) {
    let handled = *app.safety.lock().unwrap();
    app.latch_stop();
    let (mut selected, mut verified, mut owner) = (1u8, true, "owner".to_string());
    assert_eq!(
        observe_stop(
            app,
            cfg,
            cal,
            bus,
            handled,
            &mut selected,
            &mut verified,
            &mut owner
        ),
        handled + 1
    );
    assert!(
        selected == 0 && !verified && owner.is_empty(),
        "STOP clears ownership"
    );
}
#[test]
fn stop_readback_loss_restamps_a_bound_session_and_disabled_absent_motors_are_notes() {
    // FPGA answers; no motor ever does. Physical execution, so the bus is
    // kept and every axis is attempted (about 1.2 s of reply timeouts).
    let (bus, peer, dir) = fake_bench(fpga_reply);
    let (app, _rx) = app_fixture(physical());
    {
        let mut s = app.state.lock().unwrap();
        s["coordinate_session"] = json!("S");
        s["connected"] = json!(true);
    }
    let cfg = config_fixture(dir.clone());
    let mut cal = Calibration::default();
    cal.axes.insert(
        1,
        AxisCalibration {
            role: "knee".into(),
            lower: Some(100),
            coordinate_session: Some("S".into()),
            ..Default::default()
        },
    );
    for id in [2, 3] {
        cal.axes.insert(
            id,
            AxisCalibration {
                role: cfg.roles[&id].clone(),
                disabled: true,
                ..Default::default()
            },
        );
    }
    let mut bus = Some(bus);
    run_observe_stop(&app, &cfg, &cal, &mut bus);
    let s = app.state.lock().unwrap().clone();
    assert!(bus.is_some(), "physical keeps its bus");
    assert_ne!(
        s["coordinate_session"],
        json!("S"),
        "knee's turns were reset, so its bound poses are invalidated"
    );
    assert_eq!(s["connected"], json!(false));
    let error = s["error"].as_str().unwrap();
    assert!(
        error.starts_with(
            "knee: Stop readback unverified after bounded retries: ID 1: serial reply timeout"
        ) && !error.contains("worm"),
        "{error}"
    );
    let message = s["message"].as_str().unwrap();
    assert!(
        message.contains("torque-off readback unverified")
            && message.contains("Note: disabled worm (ID 2)")
            && message.contains("disabled belt/hip (ID 3)"),
        "{message}"
    );
    drop(bus);
    peer.join().unwrap();
    fs::remove_dir_all(dir).ok();
}
#[test]
fn disabled_axis_that_answers_but_fails_to_stop_is_an_unverified_torque_off() {
    // The FPGA answers; disabled motor 2 answers its readback with a device error.
    let (bus, peer, dir) = fake_bench(|frame| {
        fpga_reply(frame).or_else(|| {
            (frame[2] == 2)
                .then(|| crate::acquisition::servo_bus::packet(2, 0x20, &[0; 15]).unwrap())
        })
    });
    let (app, _rx) = app_fixture(physical());
    {
        let mut s = app.state.lock().unwrap();
        s["coordinate_session"] = json!("S");
        s["connected"] = json!(true);
    }
    let mut cfg = config_fixture(dir.clone());
    cfg.roles.retain(|id, _| *id == 2);
    let mut cal = Calibration::default();
    cal.axes.insert(
        2,
        AxisCalibration {
            role: "worm".into(),
            disabled: true,
            coordinate_session: Some("S".into()),
            ..Default::default()
        },
    );
    let mut bus = Some(bus);
    run_observe_stop(&app, &cfg, &cal, &mut bus);
    let s = app.state.lock().unwrap().clone();
    assert_eq!(s["error"], json!("worm: ID 2: device error 32"));
    assert!(
        s["message"]
            .as_str()
            .unwrap()
            .contains("torque-off readback unverified")
            && !s["message"].as_str().unwrap().contains("Note:")
    );
    assert_eq!(
        s["coordinate_session"],
        json!("S"),
        "it answered, so its turns were kept"
    );
    assert_eq!(s["connected"], json!(true));
    drop(bus);
    peer.join().unwrap();
    fs::remove_dir_all(dir).ok();
}
#[test]
fn job_captured_before_a_stop_is_refused_and_never_runs() {
    // No bus is open (None), so the worker's torque-off touches no device.
    let output =
        std::env::temp_dir().join(format!("stale-stop-{}-{}", std::process::id(), stamp()));
    let (app, rx) = app_fixture(physical());
    let epoch = *app.safety.lock().unwrap();
    let worker_app = app.clone();
    let cfg = config_fixture(output.clone());
    // The worker thread parks on its queue afterwards (App holds the sender).
    std::thread::spawn(move || worker(worker_app, rx, cfg, None));
    let (reply, answer) = mpsc::channel();
    app.latch_stop();
    app.jobs
        .send(Job {
            queued: Instant::now(),
            execution: None,
            stop_epoch: epoch,
            request: serde_json::from_value(json!({"action":"inspect"})).unwrap(),
            client: crate::hardware::protocol::new_client_id(),
            reply,
        })
        .unwrap();
    let refused = answer.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(refused, Err(STOP_INTERRUPTED.to_string()));
    assert!(app.stop.load(Ordering::SeqCst) && app.state.lock().unwrap()["enabled_id"].is_null());
    assert!(
        app.arm(epoch, true).is_err(),
        "even an explicit rearm captured before STOP is refused"
    );
    let (reply, answer) = mpsc::channel();
    app.jobs
        .send(Job {
            queued: Instant::now()
                .checked_sub(HTTP_WAIT + Duration::from_secs(1))
                .unwrap(),
            execution: None,
            stop_epoch: *app.safety.lock().unwrap(),
            request: serde_json::from_value(json!({"action":"inspect"})).unwrap(),
            client: crate::hardware::protocol::new_client_id(),
            reply,
        })
        .unwrap();
    assert_eq!(
        answer.recv_timeout(Duration::from_secs(3)).unwrap(),
        Err("Command expired before execution; nothing ran".to_string())
    );
    fs::remove_dir_all(&output).ok();
}
#[test]
fn socket_failures_count_as_lost_links_and_virtual_never_opens_serial() {
    for lost in [
        "Broken pipe (os error 32)",
        "Connection reset by peer (os error 54)",
        "ID 1: serial reply timeout (0 reply bytes received)",
        "Stop readback unverified after bounded retries: ID 2: serial reply timeout (0 reply bytes received)",
    ] {
        assert!(transport_lost(lost), "{lost}");
    }
    let binding =
        format!("{BINDING_REFUSED}: Virtual bench disconnected; restart and reconnect explicitly");
    for kept in [
        "Unknown motor ID",
        "Stop readback unverified after bounded retries: Stop not verified: cut motor supply power",
        binding.as_str(),
        "No space left on device (os error 28)",
        "Read-only file system (os error 30)",
        "Permission denied (os error 13)",
    ] {
        assert!(!transport_lost(kept), "{kept}");
    }
    let (app, _rx) = app_fixture(identity());
    let cfg = config_fixture(std::env::temp_dir().join("never-opened"));
    assert!(
        app.open_bus(&cfg)
            .is_err_and(|e| e.starts_with(BINDING_REFUSED)),
        "virtual execution without a socket refuses; no serial fallback"
    );
}
#[test]
fn campaign_refused_by_stop_creates_nothing_and_resume_ignores_empty_directories() {
    let root = std::env::temp_dir()
        .join(format!("campaign-arm-{}-{}", std::process::id(), stamp()))
        .join("campaigns");
    let (app, _rx) = app_fixture(physical());
    let epoch = *app.safety.lock().unwrap();
    app.latch_stop();
    assert_eq!(
        campaign_directory(&app, &root, false, epoch).unwrap_err(),
        STOP_INTERRUPTED
    );
    assert!(!root.exists(), "a refused campaign writes no directory");
    // An interrupted campaign with a receipt, then a newer empty one.
    let receipt = json!({"stage":"breakaway","id":1,"completed":true,"metrics":null,"samples":0,"simulated_s":0.0});
    fs::create_dir_all(root.join("campaign-1/receipts")).unwrap();
    fs::write(
        root.join("campaign-1/receipts/001-breakaway-1.json"),
        receipt.to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("campaign-2/receipts")).unwrap();
    let epoch = *app.safety.lock().unwrap();
    app.arm(epoch, true).unwrap(); // the operator selects again after STOP
    let (dir, receipts) = campaign_directory(&app, &root, true, epoch).unwrap();
    assert_eq!(dir, root.join("campaign-1"));
    assert_eq!(receipts.len(), 1);
    assert!(
        root.join("campaign-2").exists(),
        "existing campaign directories are never deleted"
    );
    fs::remove_dir_all(root.parent().unwrap()).ok();
}

#[test]
fn authorized_select_cannot_cross_an_intervening_stop() {
    let identity = identity();
    let (app, queue) = app_fixture(identity.clone());
    let handle = Handle { app: app.clone() };
    let owner = crate::hardware::protocol::new_client_id();
    let accepted_epoch = handle.stop_epoch();
    handle.stop_immediate();
    let refusal = handle
        .command_at_epoch(
            &owner,
            json!({"action":"select","id":1,"sequence":1,"hold_others":true}),
            Some((identity, 1)),
            accepted_epoch,
        )
        .unwrap_err();
    assert_eq!(refusal, STOP_INTERRUPTED);
    assert!(
        queue.try_recv().is_err(),
        "stale selection never reaches acquisition"
    );
    assert!(app.stop.load(Ordering::SeqCst));
}

#[test]
fn closed_worker_refuses_commands_instead_of_timing_out() {
    let (app, queue) = app_fixture(identity());
    app.shutdown.store(true, Ordering::SeqCst);
    let handle = Handle { app };
    let owner = crate::hardware::protocol::new_client_id();
    let error = handle
        .command(&owner, json!({"action":"inspect"}), None)
        .unwrap_err();
    assert!(error.contains("closed; reconnect"));
    assert!(queue.try_recv().is_err());
    assert_eq!(
        handle
            .command(&owner, json!({"action":"stop"}), None)
            .unwrap()["stop_latched"],
        true
    );
}
