//! The client against an in-process fake server (std only; no hardware
//! server is started): exact bodies against the pages, the headers on the
//! wire, tolerant status parsing, loopback refusal, server errors and token
//! discovery.
use super::calibration::{self, GaitBinding, Input};
use super::{Body, CALIBRATION_MAX_BODY, Client, ClientError, Endpoint, Json, MOTOR_BENCH_MAX_BODY, ServerKind, bench, encode_uri_component, js_number, js_number_text, new_client_id, process_client_id, token};
use crate::acquisition::calibration_sweep::DriveMode;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// One request as the fake server received it.
struct Seen {
    head: String,
    body: String,
    /// Bytes received beyond the head and its Content-Length.
    trailing: usize,
}
impl Seen {
    fn request_line(&self) -> &str {
        self.head.lines().next().unwrap_or("")
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
    }
}

/// Accepts one connection per answer, records the request and replies with
/// `(status, body)` as the servers do (Content-Length, Connection: close).
fn serve(answers: Vec<(u16, String)>) -> (Endpoint, JoinHandle<Vec<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (status, answer) in answers {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("fake server: no connection: {e}"),
                }
            };
            // Accepted sockets may inherit the listener's non-blocking mode.
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = Vec::new();
            let mut chunk = [0u8; 4096];
            let end = loop {
                if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break p + 4;
                }
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0, "request ended before its headers");
                data.extend_from_slice(&chunk[..n]);
            };
            let head = String::from_utf8(data[..end].to_vec()).unwrap();
            let mut probe = Seen { head, body: String::new(), trailing: 0 };
            let length: usize = probe.header("content-length").map_or(0, |v| v.parse().unwrap());
            while data.len() < end + length {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0, "request ended before its body");
                data.extend_from_slice(&chunk[..n]);
            }
            probe.body = String::from_utf8(data[end..end + length].to_vec()).unwrap();
            probe.trailing = data.len() - end - length;
            let reason = if status == 200 { "OK" } else { "Response" };
            // Ignored: a `send_only` client has closed without reading, so
            // the answer may meet a reset socket.
            let _ = write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
            seen.push(probe);
        }
        seen
    });
    (Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), port).unwrap(), handle)
}

fn client(endpoint: Endpoint) -> Client {
    Client::new(endpoint, TOKEN.to_string(), new_client_id()).with_timeout(Duration::from_secs(5))
}

fn input(speed: f64, pwm: u16, motion: &str, target: f64, hold_others: bool, mode: &str) -> Input {
    Input { speed_counts_s: speed, drive_pwm: pwm, motion: motion.into(), target_raw: target, hold_others, drive_mode: mode.into() }
}

/// serve_actuator_calibration.rs `Request` / `GaitBinding`, field for field
/// (names, types, defaults, `deny_unknown_fields`): a body this accepts, the
/// server accepts. [`request_mirror_matches_the_server`] keeps it in step.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Request {
    action: String,
    #[serde(default)]
    id: u8,
    #[serde(default)]
    delta: i16,
    #[serde(default = "default_drive")]
    drive_pwm: u16,
    #[serde(default)]
    boundary: String,
    #[serde(default)]
    reverse: bool,
    #[serde(default)]
    supported: bool,
    #[serde(default)]
    sequence: u64,
    #[serde(default)]
    speed_counts_s: f64,
    #[serde(default)]
    clearance_counts: u16,
    #[serde(default)]
    run_id: u64,
    #[serde(default)]
    motion: String,
    #[serde(default)]
    target_raw: f64,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    hold_others: bool,
    #[serde(default)]
    drive_mode: DriveMode,
    #[serde(default)]
    resume: bool,
    #[serde(default)]
    gait: String,
    #[serde(default)]
    bindings: Vec<ServerGaitBinding>,
    #[serde(default)]
    speed_scale: f64,
    #[serde(default)]
    playing: bool,
    #[serde(default)]
    effort: f64,
    #[serde(default)]
    reference_joint_rad: Option<f64>,
    #[serde(default)]
    role: String,
    #[serde(default)]
    duty: f64,
    #[serde(default)]
    seconds: f64,
}
fn default_drive() -> u16 {
    25
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ServerGaitBinding {
    id: u8,
    joint: String,
    polarity: f64,
    home_rad: f64,
}

/// `name: Type` lines of a struct in the server's source.
fn fields(source: &str, header: &str) -> Vec<String> {
    let start = source.find(header).unwrap_or_else(|| panic!("{header} not in the server source")) + header.len();
    let body = &source[start..start + source[start..].find("\n}").unwrap()];
    body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("#[") && !l.starts_with("//")).map(|l| l.trim_end_matches(',').to_string()).collect()
}

#[test]
fn request_mirror_matches_the_server() {
    let source = include_str!("../../examples/serve_actuator_calibration.rs");
    let request = [
        "action: String", "id: u8", "delta: i16", "drive_pwm: u16", "boundary: String", "reverse: bool", "supported: bool", "sequence: u64",
        "speed_counts_s: f64", "clearance_counts: u16", "run_id: u64", "motion: String", "target_raw: f64", "disabled: bool", "hold_others: bool",
        "drive_mode: DriveMode", "resume: bool", "gait: String", "bindings: Vec<GaitBinding>", "speed_scale: f64", "playing: bool", "effort: f64",
        "reference_joint_rad: Option<f64>", "role: String", "duty: f64", "seconds: f64",
    ];
    assert_eq!(fields(source, "struct Request {\n"), request.map(String::from).to_vec(), "update the mirror `Request` and the builders");
    let binding = ["id: u8", "joint: String", "polarity: f64", "home_rad: f64"];
    assert_eq!(fields(source, "struct GaitBinding {\n"), binding.map(String::from).to_vec());
}

#[test]
fn calibration_bodies_are_the_pages_bytes() {
    let up = input(5.0, 1000, "upper", 0.0, true, "pwm");
    assert_eq!(
        calibration::motion_update(2, 7, 3, &up).text(),
        r#"{"action":"motion_update","id":2,"sequence":7,"run_id":3,"speed_counts_s":5,"drive_pwm":1000,"motion":"upper","target_raw":0,"hold_others":true,"drive_mode":"pwm"}"#
    );
    let held = input(12.5, 250, "upper", 1234.5, false, "servo_position");
    assert_eq!(
        calibration::capture_hold(1, 9, "lower", 4, &held, None).text(),
        r#"{"action":"capture_hold","id":1,"sequence":9,"boundary":"lower","run_id":4,"speed_counts_s":12.5,"drive_pwm":250,"motion":"hold","target_raw":1234.5,"hold_others":false,"drive_mode":"servo_position"}"#
    );
    assert_eq!(
        calibration::capture_hold(1, 10, "reference", 4, &held, Some(0.25)).text(),
        r#"{"action":"capture_hold","id":1,"sequence":10,"boundary":"reference","run_id":4,"speed_counts_s":12.5,"drive_pwm":250,"motion":"hold","target_raw":1234.5,"hold_others":false,"drive_mode":"servo_position","reference_joint_rad":0.25}"#
    );
    let bindings = [
        GaitBinding { id: 1, joint: "knee".into(), polarity: 1.0, home_rad: 0.5 },
        GaitBinding { id: 3, joint: "hip".into(), polarity: -1.0, home_rad: -0.125 },
    ];
    assert_eq!(
        calibration::gait_start(1, 11, "examples/g/compiled.json", &bindings, 1.0, 0.5, 1000, "pwm").text(),
        r#"{"action":"gait_start","id":1,"sequence":11,"supported":true,"gait":"examples/g/compiled.json","bindings":[{"id":1,"joint":"knee","polarity":1,"home_rad":0.5},{"id":3,"joint":"hip","polarity":-1,"home_rad":-0.125}],"speed_scale":1,"effort":0.5,"drive_pwm":1000,"drive_mode":"pwm"}"#
    );
    assert_eq!(calibration::gait_update(0.75, false).text(), r#"{"action":"gait_update","speed_scale":0.75,"playing":false}"#);
    assert_eq!(calibration::select(3, 1, true).text(), r#"{"action":"select","id":3,"sequence":1,"hold_others":true}"#);
    assert_eq!(calibration::stop(Some(3), 2).text(), r#"{"action":"stop","id":3,"sequence":2}"#);
    assert_eq!(calibration::stop(None, 2).text(), r#"{"action":"stop","sequence":2}"#);
    assert_eq!(calibration::jog(2, 5, -40, 250).text(), r#"{"action":"jog","id":2,"sequence":5,"delta":-40,"drive_pwm":250}"#);
    assert_eq!(calibration::capture(2, 6, "reference", None).text(), r#"{"action":"capture","id":2,"sequence":6,"boundary":"reference"}"#);
}

#[test]
fn the_server_accepts_every_calibration_body() {
    let i = input(80.25, 1000, "target", 2048.5, true, "servo_speed");
    let bindings = [GaitBinding { id: 1, joint: "knee".into(), polarity: -1.0, home_rad: 0.2 }];
    let bodies = [
        calibration::stop(Some(1), 1),
        calibration::stop(None, 2),
        calibration::select(1, 3, false),
        calibration::set_disabled(1, 4, true),
        calibration::motion_start(1, 5, &i),
        calibration::motion_update(1, 6, 9, &i),
        calibration::sweep_all(1, 7, &i),
        calibration::tune(1, 8, 500),
        calibration::campaign(1, 9, true),
        calibration::gait_start(1, 10, "examples/g/compiled.json", &bindings, 0.5, 0.25, 1000, "pwm"),
        calibration::gait_update(0.5, true),
        calibration::capture(1, 11, "upper", None),
        calibration::capture(1, 12, "reference", Some(-0.5)),
        calibration::capture_hold(1, 13, "lower", 9, &i, None),
        calibration::capture_hold(1, 14, "reference", 9, &i, Some(1.0)),
        calibration::clear(1, 15, "both"),
        calibration::flip(1, 16),
        calibration::jog(1, 17, -4095, 1000),
    ];
    for body in &bodies {
        let text = body.text();
        let r: Request = serde_json::from_str(&text).unwrap_or_else(|e| panic!("the server would reject {text}: {e}"));
        assert_eq!(r.action, body.get("action").unwrap().to_value().as_str().unwrap());
        // serde_json's order-free reading of the same bytes agrees.
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), body.to_value());
    }
    let r: Request = serde_json::from_str(&bodies[5].text()).unwrap();
    assert_eq!((r.id, r.sequence, r.run_id, r.drive_pwm, r.drive_mode, r.motion.as_str()), (1, 6, 9, 1000, DriveMode::ServoSpeed, "target"));
    let r: Request = serde_json::from_str(&bodies[14].text()).unwrap();
    assert_eq!((r.motion.as_str(), r.reference_joint_rad), ("hold", Some(1.0)));
}

fn bench_fixture() -> (Vec<bench::Binding>, bench::Sample) {
    let bindings = vec![
        bench::Binding { coordinate: "joint.front_left | knee".into(), motor_id: 4, polarity: 1 },
        bench::Binding { coordinate: "joint.front_left | hip".into(), motor_id: 5, polarity: -1 },
    ];
    // Not in sorted order: the page's coordinate order is kept.
    let sample = bench::Sample { sequence: 3, time_s: 1.5, targets: vec![("joint.front_left | knee".into(), 0.25), ("joint.front_left | hip".into(), -0.5)] };
    (bindings, sample)
}

#[test]
fn bench_bodies_are_the_pages_bytes_and_the_server_accepts_them() {
    use crate::controller_refinement::live_reference;
    let (bindings, sample) = bench_fixture();
    let open = bench::open(&bindings, 0.05, "wasd", &sample).text();
    assert_eq!(
        open,
        r#"{"bindings":[{"coordinate":"joint.front_left | knee","motor_id":4,"polarity":1},{"coordinate":"joint.front_left | hip","motor_id":5,"polarity":-1}],"amplitude":0.05,"source":"wasd","initial":{"sequence":3,"time_s":1.5,"targets_rad":{"joint.front_left | knee":0.25,"joint.front_left | hip":-0.5}}}"#
    );
    let text = bench::sample(&sample).text();
    assert_eq!(text, r#"{"sequence":3,"time_s":1.5,"targets_rad":{"joint.front_left | knee":0.25,"joint.front_left | hip":-0.5}}"#);
    assert_eq!(bench::stop().text(), "{}");
    let request: live_reference::Request = serde_json::from_str(&open).unwrap();
    assert_eq!((request.bindings.len(), request.bindings[1].polarity, request.initial.targets_rad.len()), (2, -1, 2));
    let parsed: live_reference::Sample = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed.sequence, 3);
}

#[test]
fn headers_on_the_wire() {
    let (endpoint, server) = serve(vec![(200, r#"{"ok":true}"#.into()), (200, r#"{"busy":false}"#.into()), (200, "<html></html>".into())]);
    let port = endpoint.port;
    let c = client(endpoint);
    let body = calibration::motion_update(2, 7, 3, &input(5.0, 1000, "upper", 0.0, true, "pwm"));
    assert_eq!(c.post(calibration::COMMAND, &body).unwrap(), json!({"ok": true}));
    let status: calibration::Status = c.get_as(calibration::STATUS).unwrap();
    assert!(!status.busy);
    assert_eq!(c.page("/").unwrap(), "<html></html>");
    let seen = server.join().unwrap();

    let post = &seen[0];
    assert_eq!(post.request_line(), "POST /calibration/command HTTP/1.1");
    assert_eq!(post.header("host"), Some(format!("127.0.0.1:{port}").as_str()));
    assert_eq!(post.header("x-control-token"), Some(TOKEN));
    let id = post.header("x-client-id").unwrap();
    assert_eq!(id.len(), 36);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    assert_eq!(post.header("content-type"), Some("application/json"));
    assert_eq!(post.header("content-length"), Some(body.text().len().to_string().as_str()));
    assert_eq!(post.header("connection"), Some("close"));
    assert_eq!(post.body, body.text());
    assert_eq!(post.trailing, 0);

    let get = &seen[1];
    assert_eq!(get.request_line(), "GET /calibration/status HTTP/1.1");
    assert_eq!(get.header("host"), Some(format!("127.0.0.1:{port}").as_str()));
    assert_eq!(get.header("x-control-token"), Some(TOKEN));
    assert_eq!(get.header("x-client-id"), Some(id));
    assert_eq!(get.header("content-type"), Some("application/json"));
    assert_eq!(get.header("content-length"), None);
    assert_eq!((get.body.as_str(), get.trailing), ("", 0));

    // A page load carries no credentials, as a browser's.
    let page = &seen[2];
    assert_eq!(page.request_line(), "GET / HTTP/1.1");
    assert_eq!(page.header("host"), Some(format!("127.0.0.1:{port}").as_str()));
    assert_eq!((page.header("x-control-token"), page.header("x-client-id")), (None, None));
}

#[test]
fn server_errors_surface_verbatim() {
    let (endpoint, server) = serve(vec![
        (400, r#"{"error":"Stale or duplicate command rejected"}"#.into()),
        (500, "oops".into()),
        (200, "not json".into()),
    ]);
    let c = client(endpoint);
    let e = c.post(calibration::COMMAND, &calibration::stop(Some(1), 1)).unwrap_err();
    assert_eq!(e, ClientError::Server { status: 400, error: "Stale or duplicate command rejected".into() });
    assert_eq!(e.to_string(), "Stale or duplicate command rejected");
    let e = c.get(calibration::STATUS).unwrap_err();
    assert_eq!(e.to_string(), "Request failed (HTTP 500)");
    assert!(matches!(c.get(calibration::STATUS), Err(ClientError::Decode(_))));
    server.join().unwrap();
}

const SWEEP_SAMPLE: &str = r#"{"elapsed_ms":1200,"position_raw":100,"position_continuous":4196,"target_raw":4200.5,"target_velocity_counts_s":0.0,"velocity_counts_s":-2.5,"requested_speed_counts_s":80.0,"pwm":-120,"toward_upper":true,"half_cycles":3,"holding":true,"adaptation":{"status":"Learning","permitted_speed_counts_s":60.0,"stopping_distance_counts":4.0,"decreasing_stops":2,"increasing_stops":1,"decreasing_speed_counts_s":0.0,"increasing_speed_counts_s":0.0,"decreasing_acceleration_counts_s2":0.0,"increasing_acceleration_counts_s2":0.0,"braking":false,"learning":true,"learning_complete":false,"evidence":null},"warnings":["Tracking resynchronised"]}"#;

/// A status as the server writes it (initial state, then select, a motion
/// session, tuning, a campaign stage and a leg gait), plus a field it does
/// not write.
fn status_text() -> String {
    let telemetry = |raw: u32, continuous: &str| {
        format!(
            r#"{{"position_raw":{raw},{continuous}"position_rad":1.5,"speed_raw":0,"speed_rad_s":0.0,"load_raw":0,"voltage_raw":121,"voltage_v":12.1,"temperature_c":31,"status":0,"moving":0,"current_raw":12,"current_a_uncalibrated":0.012}}"#
        )
    };
    // Raw text rather than `json!`: a literal this deep exceeds the macro's recursion limit.
    r#"{
        "coordinate_session": "1759000000000", "maximum_speed_counts_s": 500.0, "connected": true, "enabled_id": 2, "busy": true,
        "samples": {"1": @T1@, "2": @T2@, "3": @T3@},
        "message": "Holding position · Q/A to move · Z stops drive", "error": null, "output": "runs/calibration",
        "calibration": {"schema_version": 1, "fixture": "Leg fixture", "units": "HX motor encoder counts", "provenance": "Operator-taught", "axes": {
            "1": {"role": "knee", "lower": 100, "upper": 3000, "reference": 1500, "reverse": false, "coordinate_session": "1759000000000", "reference_joint_rad": 0.25,
                  "tuning": {"pid": {"kp": 1.5, "ki": 0.25, "kd": 0.0125, "integral_limit": 100.0, "duty_limit": 1000.0}, "friction_duty": 0.05,
                             "gain_counts_s_per_duty": 10.0, "time_constant_s": 0.05, "loop_delay_s": 0.01, "breakaway_duty": [0.04, 0.05], "record": "tuning-1.json", "method": "step"}},
            "2": {"role": "worm", "lower": null, "upper": null, "reference": null, "reverse": true, "coordinate_session": null, "disabled": true}}},
        "sweep": {"running": true, "run_id": 42, "motor_id": 2, "motor_ids": [2], "skipped": [], "all": false, "axes": {"2": @SAMPLE@}, "samples": [@SAMPLE@],
                  "speed_counts_s": 80.0, "pwm_limit": 1000, "drive_mode": "pwm", "clearance_counts": 0, "teaching": true, "latest": @SAMPLE@},
        "tuning": {"running": false, "motor_id": 1, "stage": "Done", "travel_counts": 200, "result": {"record": "tuning-1.json"}},
        "campaign": {"running": true, "axes": [{"id": 1}], "skipped": ["belt (untuned)"], "stage": "slow sweep knee", "completed": 3, "directory": "runs/c1", "log": ["Starting"],
                     "last": {"stage": "hold", "axis": 1, "completed": false, "abort": {"Sag": {"voltage_v": 10.5, "preflight_v": 12.1}}}},
        "gait": {"running": true, "phase": "playing", "gait": "examples/g/compiled.json", "t": 1.25, "speed_scale": 0.5, "effort": 0.5, "motor_ids": [1, 3],
                 "bindings": [{"id": 1, "joint": "knee", "polarity": 1.0, "home_rad": 0.5}],
                 "limits": {"1": {"role": "knee", "family": "hx", "governor_speed_counts_s": 812.5, "governor_acceleration_counts_s2": 4000.0},
                            "3": {"role": "hip", "family": "hx", "governor_speed_counts_s": null, "governor_acceleration_counts_s2": 2000.0}},
                 "targets": {"1": 1500.0, "3": null}, "errors": {"1": -3.5, "3": null}, "clamped": 2,
                 "statistics": {"1": {"role": "knee", "samples": 10, "tracking_rms_deg": 0.5, "tracking_peak_deg": 1.25, "simulated_tracking_rms_counts": null,
                                      "lag_s": 0.02, "mean_effort": 0.3, "saturated_fraction": 0.0, "governor_limited_fraction": 0.1, "clamped_fraction": 0.0,
                                      "peak_measured_acceleration_counts_s2": 900.0, "minimum_voltage_v": null, "maximum_temperature_c": 33.0}}},
        "gait_runs": [{"file": "run-1.json", "gait": "examples/g/compiled.json", "effort": 0.5, "speed_scale": 1.0, "gait_time_s": 4.5, "statistics": null, "outcome": "stopped"}, 7],
        "capture_message": null,
        "a_field_the_server_may_add": {"x": 1}
    }"#
    .replace("@SAMPLE@", SWEEP_SAMPLE)
    .replace("@T1@", &telemetry(2048, ""))
    .replace("@T2@", &telemetry(100, r#""position_continuous":4196,"#))
    .replace("@T3@", &telemetry(7, ""))
}

#[test]
fn status_parses_as_the_server_writes_it() {
    let s: calibration::Status = serde_json::from_str(&status_text()).unwrap();
    assert_eq!((s.connected, s.busy, s.enabled_id, s.maximum_speed_counts_s), (true, true, Some(2), Some(500.0)));
    assert_eq!(s.samples.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!((s.samples[&2].position(), s.samples[&1].position(), s.samples[&1].temperature_c), (4196.0, 2048.0, 31.0));
    let cal = s.calibration.as_ref().unwrap();
    let knee = &cal.axes[&1];
    assert_eq!((knee.lower, knee.upper, knee.reference_joint_rad), (Some(100), Some(3000), Some(0.25)));
    assert_eq!(knee.tuning.as_ref().unwrap().pid.kd, 0.0125);
    assert!(cal.axes[&2].disabled && cal.axes[&2].lower.is_none());
    let sweep = s.sweep.as_ref().unwrap();
    assert_eq!((sweep.run_id, sweep.motor_id, sweep.axes[&2].half_cycles), (Some(42), Some(2), Some(3)));
    let latest = sweep.latest.as_ref().unwrap();
    assert_eq!((latest.position(), latest.target_raw, latest.pwm, latest.holding), (4196.0, Some(4200.5), -120.0, true));
    let adaptation = latest.adaptation.as_ref().unwrap();
    assert_eq!((adaptation.permitted_speed_counts_s, adaptation.decreasing_stops, adaptation.learning_complete), (60.0, 2, false));
    assert_eq!(sweep.samples.len(), 1);
    assert_eq!(s.tuning.as_ref().unwrap().stage, "Done");
    let campaign = s.campaign.as_ref().unwrap();
    assert_eq!((campaign.completed, campaign.last.as_ref().unwrap().gate()), (3, Some("Sag")));
    let gait = s.gait.as_ref().unwrap();
    assert_eq!((gait.phase.as_deref(), gait.clamped, gait.limits["1"].governor_speed_counts_s), (Some("playing"), Some(2), Some(812.5)));
    // A non-finite value arrives as null: that entry is `None`, the rest stays.
    let hip = &gait.limits["3"];
    assert_eq!((hip.role.as_str(), hip.governor_speed_counts_s, hip.governor_acceleration_counts_s2), ("hip", None, Some(2000.0)));
    assert_eq!((gait.targets["1"], gait.targets["3"]), (Some(1500.0), None));
    let errors = gait.errors.as_ref().unwrap();
    assert_eq!((errors["1"], errors["3"]), (Some(-3.5), None));
    let row = &gait.statistics["1"];
    assert_eq!((row.tracking_rms_deg, row.minimum_voltage_v, row.simulated_tracking_rms_counts), (Some(0.5), None, None));
    // The malformed run (`7`) is dropped; a null `statistics` reads as empty.
    assert_eq!(s.gait_runs.len(), 1);
    assert_eq!((s.gait_runs[0].outcome.as_deref(), s.gait_runs[0].statistics.len()), (Some("stopped"), 0));
}

#[test]
fn one_malformed_section_does_not_fail_the_status() {
    let mut v: Value = serde_json::from_str(&status_text()).unwrap();
    v["campaign"] = json!(5);
    v["tuning"] = json!("not an object");
    v["samples"]["3"] = json!({"position_raw": "high"});
    v["sweep"]["latest"]["adaptation"] = json!("broken");
    let s: calibration::Status = serde_json::from_str(&v.to_string()).unwrap();
    assert_eq!((s.campaign, s.tuning), (None, None));
    assert_eq!(s.samples[&3].position_raw, 0);
    assert_eq!(s.samples[&2].position(), 4196.0);
    let latest = s.sweep.as_ref().unwrap().latest.as_ref().unwrap();
    assert_eq!((latest.adaptation.as_ref(), latest.half_cycles), (None, 3));
    assert_eq!(s.gait.unwrap().clamped, Some(2));
    // An empty object is a valid (default) status.
    assert_eq!(serde_json::from_str::<calibration::Status>("{}").unwrap(), calibration::Status::default());
}

#[test]
fn gait_list_and_bench_answers_parse() {
    let gaits: calibration::Gaits = serde_json::from_value(json!({"gaits": [
        {"path": "examples/a/compiled.json", "study": "gait-search-x", "trial": "t1", "objective": 1.0, "speed_m_s": 0.12, "tracking_rms_rad_max": 0.01, "measured_actuators": true, "values": {}},
        {"path": "examples/lab/results/p/compiled.json", "kind": "pose_sequence", "study": "lab", "trial": "stand", "speed_m_s": null, "measured_actuators": true, "summary": "Stand"}
    ]}))
    .unwrap();
    assert_eq!(gaits.gaits.len(), 2);
    assert_eq!((gaits.gaits[0].speed_m_s, gaits.gaits[0].kind.as_deref()), (Some(0.12), None));
    assert_eq!((gaits.gaits[1].kind.as_deref(), gaits.gaits[1].summary.as_deref()), (Some("pose_sequence"), Some("Stand")));

    let config: bench::Config = serde_json::from_value(json!({"ids": [4, 5, 6], "coordinates": ["joint.front_left | knee"], "source": "trace.json", "source_sha256": "abc", "streamed_only": true})).unwrap();
    assert_eq!((config.ids, config.streamed_only, config.source_sha256.as_deref()), (vec![4, 5, 6], true, Some("abc")));

    // Two live-telemetry.jsonl lines (hx_fpga.rs.inc, hx_device.rs.inc) and a finished session.
    let status: bench::Status = serde_json::from_str(
        r#"{
        "active": false, "run": "runs/bench/session-1", "request": null, "plan": {}, "inspection": {"completed": true},
        "result": {"completed": true, "result": {"stop_verified": true, "failure": null}, "process_exit_success": true},
        "samples": [
            {"frame": 3, "id": 4, "host_s": 0.1, "request_s": 0.01, "completion_s": 0.02, "telemetry": {"position_raw": 2100, "voltage_v": 12.0, "temperature_c": 30},
             "home_raw": 2048, "target_counts": 60, "previous_target_counts": 50, "pwm": -40, "pwm_limit": 100, "live_source": [7, 1.25, 0.04]},
            {"frame": 1, "id": 5, "period_s": 0.01, "total_frames": 1200, "previous_target_counts": 0, "pwm": 0, "pwm_limit": 100, "request_ticks": 1,
             "completion_ticks": 2, "host_s": 0.2, "telemetry": {"position_raw": 10}, "home_raw": 12},
            "garbage"
        ]
    }"#,
    )
    .unwrap();
    assert_eq!((status.active, status.run.as_deref(), status.samples.len()), (false, Some("runs/bench/session-1"), 2));
    let first = &status.samples[0];
    assert_eq!((first.id, first.input_age_s(), first.target_deg()), (4, Some(0.04), 50.0 * 360.0 / 4096.0));
    assert_eq!(first.measured_deg(), 52.0 * 360.0 / 4096.0);
    assert_eq!((status.samples[1].input_age_s(), status.samples[1].total_frames), (None, Some(1200.0)));
    assert_eq!(status.result.unwrap().summary(), "Motors stopped and verified. 12-second live session complete.");
    let failed = bench::SessionResult { completed: Some(false), error: Some("Acquisition exited without a result".into()), result: None };
    assert_eq!(failed.summary(), "Stop verification incomplete. Acquisition exited without a result");
}

#[test]
fn only_loopback_endpoints() {
    for url in ["http://192.168.1.2:4194", "http://example.com:80", "https://127.0.0.1:1", "http://127.0.0.1", "http://127.0.0.1:4194/status", "http://[::2]:4194", "http://user@127.0.0.1:4194", "http://127.0.0.1:0", "http://127.0.0.1:+80"] {
        match Endpoint::parse(url) {
            Err(ClientError::NotLoopback(why)) => assert!(why.starts_with(url), "{why}"),
            other => panic!("{url}: {other:?}"),
        }
    }
    // Loopback, but not where the servers listen: refused with the reason.
    for url in ["http://[::1]:4194", "http://127.0.0.2:4194", "http://127.1:4194", "http://0.0.0.0:4194"] {
        match Endpoint::parse(url) {
            Err(ClientError::NotLoopback(why)) => assert!(why.starts_with(url) && why.contains("listen on 127.0.0.1 only"), "{why}"),
            other => panic!("{url}: {other:?}"),
        }
    }
    let local = Endpoint::parse("http://localhost:4194/").unwrap();
    assert_eq!((local.host(), local.origin()), ("127.0.0.1:4194".to_string(), "http://127.0.0.1:4194".to_string()));
    assert_eq!(Endpoint::parse("http://127.0.0.1:4194").unwrap(), local);
    assert_eq!(Endpoint::parse("http://LOCALHOST:4194").unwrap(), local);
    assert_eq!(Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), 4194).unwrap(), local);
    for ip in ["10.0.0.1", "::1", "127.0.0.2"] {
        match Endpoint::loopback(ip.parse().unwrap(), 80) {
            Err(ClientError::NotLoopback(why)) => assert!(why.contains("listen on 127.0.0.1 only"), "{why}"),
            other => panic!("{ip}: {other:?}"),
        }
    }
    assert!(matches!(Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), 0), Err(ClientError::NotLoopback(_))));
    // A hand-built endpoint is checked again before any connection.
    let remote = Client::new(Endpoint { ip: "192.168.1.2".parse().unwrap(), port: 4194 }, TOKEN.into(), new_client_id());
    assert!(matches!(remote.get("/calibration/status"), Err(ClientError::NotLoopback(_))));
    assert!(matches!(remote.page("/"), Err(ClientError::NotLoopback(_))));
    let v6 = Client::new(Endpoint { ip: "::1".parse().unwrap(), port: 4194 }, TOKEN.into(), new_client_id());
    assert!(matches!(v6.send_only(calibration::COMMAND, &calibration::stop(None, 1)), Err(ClientError::NotLoopback(_))));
}

/// A body whose text is exactly `bytes` long: `{"x":"aaa…"}`.
fn body_of(bytes: usize) -> Body {
    let body = Body::new(vec![("x", Json::from("a".repeat(bytes - r#"{"x":""}"#.len())))]);
    assert_eq!(body.text().len(), bytes);
    body
}

#[test]
fn request_bodies_over_the_servers_limit_are_refused_before_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), listener.local_addr().unwrap().port()).unwrap();
    let calibration = client(endpoint.clone());
    assert_eq!((calibration.max_body, CALIBRATION_MAX_BODY, MOTOR_BENCH_MAX_BODY), (4096, 4096, 8192));
    let too_big = body_of(4097);
    let refused = ClientError::Transport("request body of 4097 bytes exceeds the server's 4096-byte limit".into());
    assert_eq!(calibration.post(calibration::COMMAND, &too_big).unwrap_err(), refused);
    assert_eq!(calibration.send_only(calibration::COMMAND, &too_big).unwrap_err(), refused);
    let motor_bench = client(endpoint).for_kind(ServerKind::MotorBench);
    assert_eq!(motor_bench.max_body, 8192);
    let e = motor_bench.post(bench::LIVE_SAMPLE, &body_of(8193)).unwrap_err();
    assert_eq!(e.to_string(), "request body of 8193 bytes exceeds the server's 8192-byte limit");
    // Nothing connected.
    assert_eq!(listener.accept().map(|_| ()).unwrap_err().kind(), ErrorKind::WouldBlock);
    assert_eq!(client(Endpoint::parse("http://127.0.0.1:1").unwrap()).for_kind(ServerKind::Calibration).max_body, 4096);

    // At the limit, the body is sent.
    let (endpoint, server) = serve(vec![(200, "{}".into()), (200, "{}".into())]);
    let c = client(endpoint);
    assert_eq!(c.post(calibration::COMMAND, &body_of(4096)).unwrap(), json!({}));
    assert_eq!(c.clone().for_kind(ServerKind::MotorBench).post(calibration::COMMAND, &body_of(8192)).unwrap(), json!({}));
    let seen = server.join().unwrap();
    assert_eq!((seen[0].body.len(), seen[1].body.len()), (4096, 8192));
}

#[test]
fn timeouts_are_at_least_a_millisecond() {
    let c = client(Endpoint::parse("http://127.0.0.1:1").unwrap());
    assert_eq!(c.clone().with_timeout(Duration::ZERO).timeout, Duration::from_millis(1));
    assert_eq!(c.with_timeout(Duration::from_secs(2)).timeout, Duration::from_secs(2));
}

#[test]
fn send_only_writes_the_whole_request_and_does_not_wait() {
    let (endpoint, server) = serve(vec![(200, r#"{"ok":true}"#.into())]);
    let port = endpoint.port;
    let c = client(endpoint);
    let body = calibration::stop(Some(3), 9);
    c.send_only(calibration::COMMAND, &body).unwrap();
    let seen = server.join().unwrap();
    let stop = &seen[0];
    assert_eq!(stop.request_line(), "POST /calibration/command HTTP/1.1");
    assert_eq!(stop.header("host"), Some(format!("127.0.0.1:{port}").as_str()));
    assert_eq!(stop.header("x-control-token"), Some(TOKEN));
    assert_eq!(stop.header("x-client-id"), Some(c.client_id.as_str()));
    assert_eq!(stop.header("content-type"), Some("application/json"));
    assert_eq!(stop.header("content-length"), Some(body.text().len().to_string().as_str()));
    assert_eq!(stop.header("connection"), Some("close"));
    assert_eq!((stop.body.as_str(), stop.trailing), (r#"{"action":"stop","id":3,"sequence":9}"#, 0));

    // It returns without an answer: a server that never replies does not hold it.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let silent = client(Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), listener.local_addr().unwrap().port()).unwrap());
    let started = Instant::now();
    silent.send_only(calibration::COMMAND, &body).unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    drop(listener);
}

#[test]
fn token_from_the_served_pages() {
    let calibration_page = format!("<html><body><meta name=\"motor-bridge-token\" content=\"__CONTROL_TOKEN__\"><main></main><meta name=\"calibration-token\" content=\"{TOKEN}\"><script type=\"module\" src=\"/calibration-ui.mjs\"></script></body></html>");
    assert_eq!(token::from_page(ServerKind::Calibration, &calibration_page).as_deref(), Some(TOKEN));
    assert_eq!(token::from_page(ServerKind::MotorBench, &calibration_page), None);
    let bench_page = format!("<script>const token='{TOKEN}',clientId=crypto.randomUUID();</script>");
    assert_eq!(token::from_page(ServerKind::MotorBench, &bench_page).as_deref(), Some(TOKEN));
    for unreplaced in [
        "<script>const token='__TOKEN__',clientId=1;</script>",
        "<meta name=\"motor-bridge-token\" content=\"__CONTROL_TOKEN__\">",
        "<meta name=\"motor-bridge-token\" content=\"\">",
        "<meta name=\"calibration-token\" content=\"\">",
    ] {
        assert_eq!(token::from_page(ServerKind::MotorBench, unreplaced), None, "{unreplaced}");
        assert_eq!(token::from_page(ServerKind::Calibration, unreplaced), None, "{unreplaced}");
    }
    assert!(!token::is_token(&TOKEN.to_uppercase()) && !token::is_token(&TOKEN[1..]) && token::is_token(TOKEN));

    let (endpoint, server) = serve(vec![(200, calibration_page)]);
    assert_eq!(token::discover(&endpoint, ServerKind::Calibration).unwrap(), TOKEN);
    assert_eq!(server.join().unwrap()[0].request_line(), "GET / HTTP/1.1");

    // The bench's `/` without a usable token: `/walking/` is read next.
    let walking = format!("<!doctype html><meta name=\"motor-bridge-token\" content=\"{TOKEN}\"><title>Robot lab</title>");
    let (endpoint, server) = serve(vec![(200, "<script>const token='__TOKEN__';</script>".into()), (200, walking)]);
    assert_eq!(token::discover(&endpoint, ServerKind::MotorBench).unwrap(), TOKEN);
    let seen = server.join().unwrap();
    assert_eq!((seen[0].request_line(), seen[1].request_line()), ("GET / HTTP/1.1", "GET /walking/ HTTP/1.1"));

    let (endpoint, server) = serve(vec![(200, "<html></html>".into())]);
    assert!(token::discover(&endpoint, ServerKind::Calibration).is_err());
    server.join().unwrap();
}

#[test]
fn token_files() {
    let path = std::env::temp_dir().join(format!("hardware-client-token-{}", std::process::id()));
    std::fs::write(&path, format!("{TOKEN}\n")).unwrap();
    assert_eq!(token::read_file(&path).unwrap(), TOKEN);
    std::fs::write(&path, "__TOKEN__").unwrap();
    assert!(token::read_file(&path).unwrap_err().starts_with(&path.display().to_string()));
    std::fs::remove_file(&path).unwrap();
    assert!(token::read_file(&path).unwrap_err().starts_with(&path.display().to_string()));
}

#[test]
fn numbers_paths_and_ids_as_javascript_writes_them() {
    let text = |x: f64| {
        let mut out = String::new();
        js_number(x).write(&mut out);
        out
    };
    assert_eq!(text(5.0), "5");
    assert_eq!(text(12.5), "12.5");
    assert_eq!(text(-0.0), "0");
    assert_eq!(text(-3.0), "-3");
    assert_eq!(text(0.1), "0.1");
    assert_eq!(text(f64::NAN), "null");
    assert_eq!(text(f64::INFINITY), "null");
    // ECMAScript Number::toString, which JSON.stringify uses: plain decimal
    // for 1e-7 < |x| < 1e21, shortest round-trip digits, `e+`/`e-` beyond.
    for (x, js) in [
        (3.2e-6, "0.0000032"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (-2.5e-7, "-2.5e-7"),
        (1e21, "1e+21"),
        (1.5e300, "1.5e+300"),
        (1e20, "100000000000000000000"),
        (2f64.powi(60), "1152921504606847000"),
        (-0.0, "0"),
        (0.1 + 0.2, "0.30000000000000004"),
        (123.0, "123"),
        (1.5, "1.5"),
        (-1234.5678, "-1234.5678"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (f64::NEG_INFINITY, "null"),
    ] {
        assert_eq!(js_number_text(x), js, "{x:e}");
        assert_eq!(text(x), js, "{x:e}");
    }
    // A body carries the same text, and its order-free reading is what a
    // server parsing those bytes gets.
    let body = Body::new(vec![("a", js_number(3.2e-6)), ("b", js_number(2f64.powi(60))), ("c", js_number(-0.0))]);
    assert_eq!(body.text(), r#"{"a":0.0000032,"b":1152921504606847000,"c":0}"#);
    assert_eq!(serde_json::from_str::<Value>(&body.text()).unwrap(), body.to_value());
    assert_eq!(encode_uri_component("examples/full-robot/a b/compiled.json"), "examples%2Ffull-robot%2Fa%20b%2Fcompiled.json");
    assert_eq!(encode_uri_component("a-_.!~*'()é"), "a-_.!~*'()%C3%A9");
    assert_eq!(calibration::gait_path("examples/x y/compiled.json"), "/calibration/gait?path=examples%2Fx%20y%2Fcompiled.json");
    let a = new_client_id();
    assert_ne!(a, new_client_id());
    assert_eq!(a.len(), 36);
    for (i, c) in a.chars().enumerate() {
        match i {
            8 | 13 | 18 | 23 => assert_eq!(c, '-'),
            14 => assert_eq!(c, '4'),
            _ => assert!(c.is_ascii_digit() || ('a'..='f').contains(&c), "{a}"),
        }
    }
}

#[test]
fn one_client_id_per_process() {
    let path = std::env::temp_dir().join(format!("hardware-client-id-token-{}", std::process::id()));
    std::fs::write(&path, TOKEN).unwrap();
    // A token file: nothing is contacted, so no server is needed.
    let first = token::connect("http://127.0.0.1:1", Some(&path), ServerKind::Calibration).unwrap();
    let again = token::connect("http://127.0.0.1:1", Some(&path), ServerKind::Calibration).unwrap();
    let bench = token::connect("http://localhost:2", Some(&path), ServerKind::MotorBench).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(first.client_id, again.client_id);
    assert_eq!(first.client_id, bench.client_id);
    assert_eq!(first.client_id, process_client_id());
    // The format both servers check: 36 characters of hex and dashes.
    assert_eq!(first.client_id.len(), 36);
    assert!(first.client_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
}

/// A refusal as both servers make it: read the head in 1024-byte pieces,
/// answer, close without reading the body. With body bytes unread the
/// kernel may reset the connection; the server's `error` still surfaces.
#[test]
fn a_refusal_before_the_body_surfaces_the_servers_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), listener.local_addr().unwrap().port()).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut data = Vec::new();
        while !data.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut b = [0u8; 1024];
            let n = stream.read(&mut b).unwrap();
            assert!(n > 0, "request ended before its headers");
            data.extend_from_slice(&b[..n]);
        }
        let answer = r#"{"error":"Session token required"}"#;
        let _ = write!(stream, "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
        // Dropped with the body unread (a reset where the platform sends one).
    });
    let c = client(endpoint);
    let e = c.post(calibration::COMMAND, &body_of(3000)).unwrap_err();
    server.join().unwrap();
    assert!(e.to_string().contains("Session token required"), "{e:?}");
}
