//! Loopback calibration adapter: serialized physical I/O, shared encoder policy,
//! persisted operator measurements, and the existing CAD/WASM viewer.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_runtime::acquisition::{
    calibration::{AxisCalibration, Calibration},
    calibration_serial::CalibrationBus,
    calibration_sweep::{MotionCommand, SweepInput, SweepTuning},
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type R<T> = Result<T, String>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    serial: String,
    viewer: PathBuf,
    output: PathBuf,
    fixture: String,
    roles: std::collections::BTreeMap<u8, String>,
    sweep_tuning: SweepTuning,
}
fn default_drive() -> u16 {
    25
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
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
}
struct Job {
    request: Request,
    client: String,
    reply: mpsc::Sender<R<Value>>,
}
struct BrowserSweep {
    owner: String,
    id: u8,
    run_id: u64,
    sequence: u64,
    input: SweepInput,
    last_seen: Instant,
    motion: MotionCommand,
    teaching: bool,
    capture: Option<String>,
}
impl BrowserSweep {
    fn update(&mut self, client: &str, r: &Request, tuning: &SweepTuning) -> R<()> {
        if client != self.owner
            || r.id != self.id
            || r.run_id != self.run_id
            || r.sequence <= self.sequence
        {
            return Err("Sweep update rejected: wrong owner, run, axis or stale sequence".into());
        }
        if self.last_seen.elapsed() > Duration::from_millis(750) {
            return Err("Sweep browser lease expired; start again explicitly".into());
        }
        let input = SweepInput {
            speed_counts_s: r.speed_counts_s,
            pwm_limit: r.drive_pwm,
        };
        input.validate(tuning)?;
        if self.teaching {
            self.motion = motion_request(r)?;
        }
        self.input = input;
        self.sequence = r.sequence;
        self.last_seen = Instant::now();
        Ok(())
    }
}
fn motion_request(r: &Request) -> R<MotionCommand> {
    match r.motion.as_str() {
        "upper" => Ok(MotionCommand::Jog(1)),
        "lower" => Ok(MotionCommand::Jog(-1)),
        "hold" => Ok(MotionCommand::Hold),
        "sweep" => Ok(MotionCommand::Sweep),
        "target" if r.target_raw.is_finite() => Ok(MotionCommand::Target(r.target_raw)),
        _ => Err("Choose upper, lower, hold or target".into()),
    }
}
struct App {
    state: Mutex<Value>,
    jobs: mpsc::SyncSender<Job>,
    stop: AtomicBool,
    cancel: AtomicBool,
    cancel_sequence: AtomicU64,
    origin: String,
    token: String,
    viewer: PathBuf,
    sweep: Mutex<Option<BrowserSweep>>,
    sweep_tuning: SweepTuning,
}
fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
fn save(path: &PathBuf, c: &Calibration) -> R<()> {
    c.validate()?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(c).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}
fn worker(app: Arc<App>, rx: mpsc::Receiver<Job>, cfg: Config) {
    let path = cfg.output.join("calibration.json");
    let mut cal = if path.exists() {
        match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|v| serde_json::from_slice::<Calibration>(&v).map_err(|e| e.to_string()))
            .and_then(|c| c.validate().map(|_| c))
        {
            Ok(c) => c,
            Err(e) => {
                app.state.lock().unwrap()["error"] = json!(e);
                return;
            }
        }
    } else {
        let mut c = Calibration::default();
        c.fixture = cfg.fixture;
        c.axes = cfg
            .roles
            .iter()
            .map(|(id, role)| {
                (
                    *id,
                    AxisCalibration {
                        role: role.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        c
    };
    let mut bus = None;
    let mut owner = String::new();
    let mut selected = 0;
    let mut last_seq = 0;
    let mut verified = false;
    {
        let mut s = app.state.lock().unwrap();
        s["calibration"] = json!(cal);
        s["message"] = json!("Connect and inspect to read motors. No motion on page load.");
    }
    while let Ok(job) = rx.recv() {
        let result = (|| -> R<Value> {
            let r = &job.request;
            if r.action == "inspect" {
                if bus.is_none() {
                    bus = Some(CalibrationBus::open(
                        &cfg.serial,
                        &cfg.output.join("serial.jsonl"),
                    )?)
                }
                let b = bus.as_mut().unwrap();
                verified = false;
                owner.clear();
                selected = 0;
                b.reconnect_stopped(3)?;
                let mut samples = serde_json::Map::new();
                for id in cfg.roles.keys() {
                    samples.insert(id.to_string(), json!(b.feedback(*id)?));
                }
                let mut s = app.state.lock().unwrap();
                s["samples"] = json!(samples);
                s["connected"] = json!(true);
                s["enabled_id"] = Value::Null;
                s["message"] =
                    json!("Readback received. Select an axis and enable supervised teaching.");
                return Ok(s.clone());
            }
            if !cfg.roles.contains_key(&r.id) {
                return Err("Unknown motor ID".into());
            }
            if r.action == "select" {
                if bus.is_none() {
                    bus = Some(CalibrationBus::open(
                        &cfg.serial,
                        &cfg.output.join("serial.jsonl"),
                    )?);
                }
                let b = bus.as_mut().unwrap();
                verified = false;
                owner.clear();
                selected = r.id;
                b.reconnect_stopped(r.id)?;
                let t = b.prove_watchdogs(r.id)?;
                owner = job.client.clone();
                verified = true;
                last_seq = r.sequence;
                app.cancel_sequence.store(r.sequence, Ordering::SeqCst);
                app.stop.store(false, Ordering::SeqCst);
                let mut s = app.state.lock().unwrap();
                s["connected"] = json!(true);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(r.id);
                s["capture_message"] = Value::Null;
                s["sweep"] = Value::Null;
                s["message"] = json!(
                    "Ready. Hold Q toward upper or A toward lower; release to hold position."
                );
                return Ok(s.clone());
            }
            let b = bus.as_mut().ok_or("Select a motor first")?;
            if r.action == "enable" {
                if !r.supported {
                    return Err("Confirm the fixture is supported with torque disabled".into());
                }
                if !owner.is_empty() && owner != job.client {
                    return Err("Another tab owns the fixture; Stop before taking over".into());
                }
                owner = job.client.clone();
                selected = r.id;
                verified = false;
                last_seq = 0;
                app.cancel_sequence.store(0, Ordering::SeqCst);
                app.stop.store(false, Ordering::SeqCst);
                let t = b.prove_watchdogs(r.id)?;
                verified = true;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(selected);
                s["message"] = json!(
                    "Ready for deliberate jogs. Zero-drive watchdogs verified; mechanical stopping distance remains unqualified."
                );
                return Ok(s.clone());
            }
            if r.action == "stop" {
                let t = b.stop(r.id)?;
                verified = false;
                owner.clear();
                selected = 0;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = Value::Null;
                s["message"] = json!("Torque off and stationary encoder verified.");
                return Ok(s.clone());
            }
            if r.action == "clear" {
                // The worker serializes this after any sweep has stopped; immutable
                // snapshots preserve the exact bounds being replaced.
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().clear(&r.boundary)?;
                save(
                    &cfg.output
                        .join(format!("calibration-{}-before-clear.json", stamp())),
                    &cal,
                )?;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                verified = false;
                owner.clear();
                selected = 0;
                app.stop.store(true, Ordering::SeqCst);
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = Value::Null;
                s["message"] = json!(format!(
                    "{} cleared for this axis; previous values saved in history. Re-enable to teach new poses.",
                    r.boundary
                ));
                return Ok(s.clone());
            }
            if owner != job.client || selected != r.id || !verified {
                return Err("Enable this motor in this tab first".into());
            }
            if r.sequence <= last_seq {
                return Err("Stale or duplicate command rejected".into());
            }
            last_seq = r.sequence;
            if r.action == "sweep_start" || r.action == "motion_start" {
                let teaching = r.action == "motion_start";
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable this axis first".into());
                }
                let input = SweepInput {
                    speed_counts_s: if teaching { r.speed_counts_s } else { 5. },
                    pwm_limit: r.drive_pwm,
                };
                input.validate(&cfg.sweep_tuning)?;
                let axis = cal.axes[&r.id].clone();
                axis.validate()?;
                if !teaching && (axis.lower.is_none() || axis.upper.is_none()) {
                    return Err("Teach both poses before starting a sweep".into());
                }
                app.cancel.store(false, Ordering::SeqCst);
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Err("Sweep cancelled before starting".into());
                }
                let run_id = stamp() as u64;
                *app.sweep.lock().unwrap() = Some(BrowserSweep {
                    owner: job.client.clone(),
                    id: r.id,
                    run_id,
                    sequence: r.sequence,
                    input,
                    last_seen: Instant::now(),
                    motion: if teaching {
                        motion_request(r)?
                    } else {
                        MotionCommand::Sweep
                    },
                    teaching,
                    capture: None,
                });
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(true);
                    s["sweep"] = json!({"running":true,"run_id":run_id,"motor_id":r.id,"samples":[],"speed_counts_s":5.,"pwm_limit":r.drive_pwm,"clearance_counts":r.clearance_counts,"teaching":teaching});
                    s["message"] = json!(
                        "Starting continuous traversal at 5 motor counts/s. Keep this tab active; Stop ends the sweep."
                    );
                    let _ = job.reply.send(Ok(s.clone()));
                }
                let mut history = std::collections::VecDeque::new();
                let result=b.controlled_motion(r.id,&axis,r.clearance_counts,&cfg.sweep_tuning,&app.cancel,teaching,|| {
                    if app.cancel.load(Ordering::SeqCst) || app.stop.load(Ordering::SeqCst) {return Ok(None);}
                    let lock=app.sweep.lock().unwrap();let ctl=lock.as_ref().ok_or("Sweep controls closed")?;
                    if ctl.last_seen.elapsed()>Duration::from_millis(750) {return Err("Browser heartbeat lost; sweep stopped".into());}
                    let a:AxisCalibration=serde_json::from_value(app.state.lock().unwrap()["calibration"]["axes"][r.id.to_string()].clone()).map_err(|e|e.to_string())?;
                    if matches!(ctl.motion,MotionCommand::Target(_)|MotionCommand::Sweep) && (a.lower.is_none()||a.upper.is_none()){return Err("Teach both poses before using the angle dial".into());}
                    Ok(Some((ctl.input,ctl.motion,a)))
                },|t,sample| {
                    history.push_back(json!(sample));if history.len()>300 {history.pop_front();}
                    let capture={let mut lock=app.sweep.lock().unwrap();lock.as_mut().and_then(|ctl|ctl.capture.take())};
                    if let Some(boundary)=capture {
                        let stable=history.len()>=6 && history.iter().rev().take(6).all(|s|s["position_raw"].as_u64().is_some_and(|p|p.abs_diff(t.position_raw as u64)<=2));
                        if sample.holding && sample.velocity_counts_s.abs()<2. && (sample.target_raw-t.position_raw as f64).abs()<3. && stable {
                            let mut next=cal.clone();let a=next.axes.get_mut(&r.id).unwrap();
                            match boundary.as_str(){"lower"=>a.lower=Some(t.position_raw),"upper"=>a.upper=Some(t.position_raw),"reference"=>a.reference=Some(t.position_raw),_=>return Err("Unknown pose".into())};
                            next.validate()?;
                            if next.axes[&r.id].reversed()!=cal.axes[&r.id].reversed(){return Err("Pose would reverse upper/lower direction; swap direction explicitly first".into());}
                            save(&cfg.output.join(format!("calibration-{}.json",stamp())),&next)?;save(&path,&next)?;cal=next;
                            let mut s=app.state.lock().unwrap();s["calibration"]=json!(cal);s["capture_message"]=json!(format!("Saved {boundary} pose"));
                        }else{app.state.lock().unwrap()["capture_message"]=json!("Still settling. Release Q/A, wait for Holding, then save the pose.");}
                    }
                    let mut s=app.state.lock().unwrap();s["samples"][r.id.to_string()]=json!(t);
                    s["sweep"]["samples"]=json!(history);s["sweep"]["latest"]=json!(sample);
                    s["message"]=json!(if teaching {if sample.holding {"Holding position · Q/A to move · Z stops drive"}else{"Moving under feedback control · release to hold"}}else{"Sweeping between taught poses · Q/A takes over · Z stops drive"});
                    Ok(())
                });
                *app.sweep.lock().unwrap() = None;
                app.stop.store(true, Ordering::SeqCst);
                verified = false;
                owner.clear();
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(false);
                    s["sweep"]["running"] = json!(false);
                    s["enabled_id"] = Value::Null;
                }
                let outcome = result?;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(outcome.telemetry);
                s["sweep"]["motion_error"] = json!(outcome.motion_error);
                s["message"] = json!(format!(
                    "{}. Torque off and stationary encoder verified.",
                    outcome.motion_error.as_deref().unwrap_or("Sweep stopped")
                ));
                return Ok(s.clone());
            }
            if r.action == "halt" {
                let t = b.stop(r.id)?;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!("Released. Torque off and stationary encoder verified.");
                return Ok(s.clone());
            }
            if r.action == "flip" {
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                let reverse = !a.reversed();
                std::mem::swap(&mut a.lower, &mut a.upper);
                a.reverse = reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Upper/lower direction swapped. Physical encoder envelope preserved.");
                return Ok(s.clone());
            }
            if r.action == "direction" {
                if cal.axes[&r.id].lower.is_some() || cal.axes[&r.id].upper.is_some() {
                    return Err("Direction is fixed once a named limit is taught".into());
                }
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().reverse = r.reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Part direction saved. Upper/lower refer to the part's poses.");
                return Ok(s.clone());
            }
            if r.action == "jog" {
                app.state.lock().unwrap()["sweep"] = Value::Null;
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable teaching again".into());
                }
                app.cancel.store(false, Ordering::SeqCst);
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Ok(app.state.lock().unwrap().clone());
                }
                app.state.lock().unwrap()["busy"] = json!(true);
                let result = b.jog(r.id, &cal.axes[&r.id], r.delta, r.drive_pwm, &app.cancel);
                app.state.lock().unwrap()["busy"] = json!(false);
                let t = result?;
                if t.outside_jog_window || t.motion_error.is_some() {
                    verified = false;
                    owner.clear();
                    app.stop.store(true, Ordering::SeqCst);
                }
                let mut s = app.state.lock().unwrap();
                if t.outside_jog_window || t.motion_error.is_some() {
                    s["enabled_id"] = Value::Null;
                }
                s["samples"][r.id.to_string()] = json!(t.telemetry);
                s["last_jog"] = json!(t);
                s["message"] = json!(format!(
                    "{} Requested {:+} counts; actual {:+} counts at {}% PWM.{}{}",
                    t.reason,
                    t.requested_counts,
                    t.actual_counts,
                    t.drive_pwm as f64 / 10.,
                    if t.stop_reply_recoveries > 0 {
                        " Stop readback reply retried and verified; no motion command repeated."
                    } else {
                        ""
                    },
                    if t.outside_jog_window {
                        " Coast exceeded the jog window. Stopped; inspect clearance before enabling again."
                    } else {
                        ""
                    }
                ));
                return Ok(s.clone());
            }
            if r.action == "capture" {
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                match r.boundary.as_str() {
                    "lower" => a.lower = Some(t.position_raw),
                    "upper" => a.upper = Some(t.position_raw),
                    "reference" => a.reference = Some(t.position_raw),
                    _ => return Err("Unknown boundary".into()),
                };
                next.validate()?;
                // Keep previous measurements as immutable versions before replacing the current record.
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!(format!(
                    "{} captured from stationary physical encoder and saved to disk.",
                    r.boundary
                ));
                return Ok(s.clone());
            }
            Err("Unknown calibration action".into())
        })();
        if let Err(e) = &result {
            app.stop.store(true, Ordering::SeqCst);
            verified = false;
            owner.clear();
            let stopped = bus
                .as_mut()
                .map(|b| b.stop(if selected == 0 { 3 } else { selected }));
            let mut s = app.state.lock().unwrap();
            s["enabled_id"] = Value::Null;
            s["busy"] = json!(false);
            s["error"] = json!(e);
            s["message"] = json!(format!(
                "{e}. {}",
                if stopped.as_ref().is_some_and(|v| v.is_ok()) {
                    "Stopped and readback verified"
                } else {
                    "Physical stop unverified; keep motor power off until resolved"
                }
            ));
        } else {
            app.state.lock().unwrap()["error"] = Value::Null;
        }
        let _ = job.reply.send(result);
    }
}
fn reply(s: &mut TcpStream, status: u16, kind: &str, body: &[u8]) {
    let _ = write!(
        s,
        "HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
}
fn handle(mut stream: TcpStream, app: Arc<App>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> R<(String, Vec<u8>)> {
        let mut data = Vec::new();
        let end = loop {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete request".into());
            }
            data.extend(&b[..n]);
            if data.len() > 16384 {
                return Err("Request too large".into());
            }
            if let Some(p) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let headers = String::from_utf8(data[..end].to_vec()).map_err(|e| e.to_string())?;
        let first = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        if first.len() != 3 {
            return Err("Bad request".into());
        }
        let (method, path) = (first[0], first[1].split('?').next().unwrap());
        let h = |key: &str| {
            headers
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        };
        if h("host") != app.origin.strip_prefix("http://")
            || h("origin").is_some_and(|v| v != app.origin)
            || h("sec-fetch-site").is_some_and(|v| v != "same-origin" && v != "none")
        {
            return Err("Same-origin localhost access required".into());
        }
        if method == "GET" && !path.starts_with("/calibration/") {
            if path == "/actuator-motion-view.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/actuator-motion-view.mjs").to_vec(),
                ));
            }
            if path == "/calibration-ui.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/calibration-ui.mjs").to_vec(),
                ));
            }
            let root = app.viewer.canonicalize().map_err(|e| e.to_string())?;
            let file = root
                .join(if path == "/" {
                    "index.html"
                } else {
                    path.trim_start_matches('/')
                })
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !file.starts_with(&root) {
                return Err("Outside viewer".into());
            }
            let kind = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
                "html" => "text/html",
                "mjs" | "js" => "text/javascript",
                "wasm" => "application/wasm",
                "json" => "application/json",
                "css" => "text/css",
                _ => "application/octet-stream",
            };
            let mut bytes = fs::read(&file).map_err(|e| e.to_string())?;
            if path == "/" {
                bytes=String::from_utf8(bytes).map_err(|e|e.to_string())?.replace("</body>",&format!("<meta name=\"calibration-token\" content=\"{}\"><script type=\"module\" src=\"/calibration-ui.mjs\"></script></body>",app.token)).into_bytes()
            }
            return Ok((kind.into(), bytes));
        }
        if h("x-control-token") != Some(app.token.as_str()) {
            return Err("Session token required".into());
        }
        if method == "GET" && path == "/calibration/status" {
            return Ok((
                "application/json".into(),
                app.state.lock().unwrap().to_string().into_bytes(),
            ));
        }
        if method == "GET" && path == "/calibration/export" {
            return Ok((
                "application/json".into(),
                serde_json::to_vec_pretty(&app.state.lock().unwrap()["calibration"]).unwrap(),
            ));
        }
        if method != "POST" || path != "/calibration/command" {
            return Err("Unknown endpoint".into());
        }
        let client = h("x-client-id").ok_or("Tab identity required")?;
        if client.len() != 36 || !client.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("Invalid tab identity".into());
        }
        let length = h("content-length")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        if length > 4096 || h("transfer-encoding").is_some() {
            return Err("Invalid body length".into());
        }
        while data.len() < end + length {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete body".into());
            }
            data.extend(&b[..n]);
        }
        let request: Request =
            serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
        if request.action == "capture_hold" {
            let mut lock = app.sweep.lock().unwrap();
            let ctl = lock
                .as_mut()
                .ok_or("Move then release to hold before saving")?;
            if app.stop.load(Ordering::SeqCst)
                || app.cancel.load(Ordering::SeqCst)
                || motion_request(&request)? != MotionCommand::Hold
                || !ctl.teaching
                || ctl.motion != MotionCommand::Hold
                || !matches!(request.boundary.as_str(), "upper" | "lower" | "reference")
            {
                return Err("Release to hold before saving a pose".into());
            }
            ctl.update(client, &request, &app.sweep_tuning)?;
            if ctl.motion != MotionCommand::Hold {
                return Err("Release before saving".into());
            }
            ctl.capture = Some(request.boundary.clone());
            return Ok(("application/json".into(), b"{\"ok\":true}".to_vec()));
        }
        if request.action == "sweep_update" || request.action == "motion_update" {
            let mut lock = app.sweep.lock().unwrap();
            let control = lock.as_mut().ok_or("No continuous sweep is active")?;
            if app.stop.load(Ordering::SeqCst) || app.cancel.load(Ordering::SeqCst) {
                return Err("Sweep stop is latched".into());
            }
            control.update(client, &request, &app.sweep_tuning)?;
            return Ok(("application/json".into(), b"{\"ok\":true}".to_vec()));
        }
        if request.action == "clear" || request.action == "select" {
            app.cancel.store(true, Ordering::SeqCst);
            app.stop.store(true, Ordering::SeqCst);
        }
        if request.action == "stop" {
            app.stop.store(true, Ordering::SeqCst);
            app.cancel.store(true, Ordering::SeqCst);
        }
        if request.action == "halt" {
            // Cancel even if the earlier jog is queued but has not started.
            app.cancel_sequence
                .fetch_max(request.sequence, Ordering::SeqCst);
            app.cancel.store(true, Ordering::SeqCst);
        }
        let (tx, rx) = mpsc::channel();
        app.jobs
            .try_send(Job {
                request,
                client: client.into(),
                reply: tx,
            })
            .map_err(|_| "Hardware busy; no command queued")?;
        let value = rx
            .recv_timeout(Duration::from_secs(8))
            .map_err(|_| "Hardware response timed out")??;
        Ok(("application/json".into(), value.to_string().into_bytes()))
    })();
    match result {
        Ok((kind, bytes)) => reply(&mut stream, 200, &kind, &bytes),
        Err(e) => reply(
            &mut stream,
            400,
            "application/json",
            json!({"error":e}).to_string().as_bytes(),
        ),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("serve_actuator_calibration CONFIG HTTP_PORT".into());
    }
    let cfg: Config = serde_json::from_slice(&fs::read(&args[1])?)?;
    if cfg.roles.keys().copied().collect::<Vec<_>>() != vec![1, 2, 3] {
        return Err("This FPGA profile covers IDs 1,2,3".into());
    }
    cfg.sweep_tuning.validate()?;
    fs::create_dir_all(&cfg.output)?;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args[2]))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let mut bytes = [0; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let (tx, rx) = mpsc::sync_channel(1);
    let app = Arc::new(App {
        state: Mutex::new(
            json!({"connected":false,"enabled_id":null,"busy":false,"samples":{},"message":"Starting","error":null,"output":cfg.output}),
        ),
        jobs: tx,
        stop: AtomicBool::new(true),
        cancel: AtomicBool::new(true),
        cancel_sequence: AtomicU64::new(0),
        origin: origin.clone(),
        token,
        viewer: cfg.viewer.clone(),
        sweep: Mutex::new(None),
        sweep_tuning: cfg.sweep_tuning.clone(),
    });
    let a = app.clone();
    std::thread::spawn(move || worker(a, rx, cfg));
    println!("Calibration and robot viewer: {origin}");
    for stream in listener.incoming() {
        let s = stream?;
        let a = app.clone();
        std::thread::spawn(move || handle(s, a));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tuning() -> SweepTuning {
        serde_json::from_str::<Config>(include_str!(
            "../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json"
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
        lease.last_seen = Instant::now() - Duration::from_secs(1);
        assert!(lease.update("owner", &request, &tuning()).is_err());
    }
}
