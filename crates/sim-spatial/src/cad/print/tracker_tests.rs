//! The print job poller without a window, against a fake RoboCAD on
//! loopback (its accept loop runs on a `crate::jobs::Job`): a watched
//! job's progress on the status line, its done text and the refresh, the
//! blockers while it runs, and cancel sending exactly one `DELETE
//! /print/jobs/{id}` for the one running job, only after the confirmation.
use super::jobs_panel;
use super::jobs_tracker::{self, capitalize, done_text, line, publishes};
use super::studies::Started;
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::jobs::{Job, Pool};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, DocState, Health, PrintJob};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// (method, path) of every request, in arrival order.
type Log = Arc<Mutex<Vec<(String, String)>>>;

/// A fake RoboCAD answering `GET /print/jobs` with `jobs` and `DELETE
/// /print/jobs/{id}` by marking that job cancelled. Dropping it cancels its
/// job, which ends the accept loop.
struct Fake {
    url: String,
    log: Log,
    jobs: Arc<Mutex<Value>>,
    _job: Job<()>,
}
impl Fake {
    fn start(jobs: Value) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("address").port();
        let (log, jobs): (Log, _) = (Arc::default(), Arc::new(Mutex::new(jobs)));
        let (served, listed) = (log.clone(), jobs.clone());
        let job = Job::spawn(Pool::Dedicated, 0, "fake RoboCAD print jobs", move |ctx| {
            while !ctx.cancelled() {
                match listener.accept() {
                    // One request at a time: the poller sends at most one.
                    Ok((stream, _)) => answer(stream, &served, &listed),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        Fake { url: format!("http://127.0.0.1:{port}"), log, jobs, _job: job }
    }
    fn deletes(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().filter(|(m, _)| m == "DELETE").map(|(_, p)| p.clone()).collect()
    }
    fn set(&self, jobs: Value) {
        *self.jobs.lock().unwrap() = jobs;
    }
}

fn answer(mut stream: TcpStream, log: &Log, jobs: &Arc<Mutex<Value>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
        if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).to_string();
    let length = head.lines().filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok()).unwrap_or(0);
    while data.len() < end + length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    }
    let mut words = head.split_whitespace();
    let (method, path) = (words.next().unwrap_or("").to_string(), words.next().unwrap_or("").to_string());
    log.lock().unwrap().push((method.clone(), path.clone()));
    let (status, reply) = match (method.as_str(), path.strip_prefix("/print/jobs")) {
        ("GET", Some("")) => (200, jobs.lock().unwrap().clone()),
        ("DELETE", Some(rest)) if rest.starts_with('/') => {
            let id = &rest[1..];
            let mut list = jobs.lock().unwrap();
            match list.as_array_mut().and_then(|a| a.iter_mut().find(|j| j["id"] == id)) {
                Some(j) => {
                    j["state"] = json!("cancelled");
                    (200, j.clone())
                }
                None => (404, json!({"error": format!("no print job {id}")})),
            }
        }
        _ => (404, json!({"error": format!("no route {method} {path}")})),
    };
    let text = reply.to_string();
    let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
    let _ = stream.flush();
}

/// Connected to `fake` at RoboCAD's revision 4.
fn document(fake: &Fake) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service(fake.url.clone()));
    doc.client = Some(CadClient::new(&fake.url).unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState { revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

/// Runs the poller's frames (`jobs_tracker::tick`, what its JobResults
/// system calls) until `done`, at most 5 s.
fn until(doc: &mut CadDocument, what: &str, done: impl Fn(&CadDocument) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        jobs_tracker::tick(doc);
        if done(doc) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; status {:?}, state {}", doc.status, jobs_tracker::state_json(doc));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn running(id: &str, kind: &str, fraction: f64, message: &str) -> Value {
    json!({"id": id, "kind": kind, "state": "running", "fraction": fraction, "message": message, "error": null, "result": null, "out_dir": null, "seconds": 1.5})
}

#[test]
fn a_watched_job_shows_its_progress_then_robocads_done_text_and_refreshes() {
    let fake = Fake::start(json!([running("a1", "analyze", 0.42, "meshing")]));
    let mut doc = document(&fake);
    // The start's edit answered with the job RoboCAD started (`studies::send` noted it).
    doc.edit_seq = 3;
    doc.print.studies.started = Some(Started { seq: 3, generation: doc.generation, kind: "analyze" });
    jobs_tracker::edit_answered(&mut doc, 3, Some(&json!({"id": "a1", "kind": "analyze", "state": "queued", "fraction": 0.0, "message": ""})));
    assert_eq!(doc.print.jobs.watched.len(), 1);
    assert!(doc.print.studies.started.is_none());
    until(&mut doc, "the progress line", |d| d.status == Some(Ok("analyze: meshing (42 %)".to_string())));
    // Leaving CAD mode would stop a self-started RoboCAD with it.
    assert_eq!(doc.print.jobs.blockers(), vec!["a print job is running in RoboCAD: analyze (42 %); wait for it, or cancel it in the Print jobs section".to_string()]);
    // Not connected, nothing can be confirmed or held.
    doc.connection = Connection::Lost { error: "gone".into(), since: Instant::now() };
    jobs_tracker::tick(&mut doc);
    assert!(doc.print.jobs.blockers().is_empty());
    doc.connection = Connection::Connected;
    // Done: RoboCAD's text, the document refetched and the robot reads taken again.
    doc.robot.data.key = Some((doc.generation, 4));
    let result = json!({"revision": 5, "parts": [
        {"node": "b2", "name": "Plate", "safety_factor": 3.0, "mode": "shear"},
        {"node": "b1", "name": "Bracket", "safety_factor": 1.5, "mode": "tension"},
        {"node": "b3", "name": "Leg", "safety_factor": 1.5, "mode": "bending"},
    ]});
    fake.set(json!([{"id": "a1", "kind": "analyze", "state": "done", "fraction": 1.0, "message": "publishing", "result": result}]));
    until(&mut doc, "the end", |d| d.print.jobs.watched.is_empty());
    assert_eq!(doc.status, Some(Ok("strength: least safety factor 1.50 on Bracket (tension); Print ▸ Strength overlay shows where".to_string())));
    assert!(doc.robot.data.key.is_none(), "the robot reads are taken again");
    assert!(doc.print.jobs.blockers().is_empty());
    assert_eq!(fake.deletes(), Vec::<String>::new());
}

#[test]
fn cancel_asks_first_then_sends_exactly_one_delete_per_running_job() {
    let done = json!({"id": "b2", "kind": "split", "state": "done", "fraction": 1.0, "message": "", "result": {"piece_nodes": ["p1", "p2"], "hardware": []}});
    let fake = Fake::start(json!([done, running("a1", "plan", 0.1, "")]));
    let mut doc = document(&fake);
    jobs_panel::show(&mut doc, Some(true)).unwrap();
    until(&mut doc, "the list", |d| d.print.jobs.listed);
    assert_eq!(doc.print.jobs.lines(), vec!["split b2: done 100 % ".to_string(), "plan a1: running 10 % ".to_string()]);
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    // Asking opens RoboCAD's question in the section; nothing is sent.
    assert!(matches!(jobs_tracker::cancel(&mut doc, &mut call, None, None), Outcome::Done(Ok(_))));
    assert!(doc.print.jobs.confirming);
    let ids: Vec<String> = jobs_panel::controls(&doc).into_iter().map(|c| c.0).collect();
    assert!(ids.contains(&"cad:print:cancel_yes".to_string()) && ids.contains(&"cad:print:cancel_no".to_string()), "{ids:?}");
    assert!(fake.deletes().is_empty());
    // No closes the question.
    assert!(matches!(jobs_tracker::cancel(&mut doc, &mut call, None, Some(false)), Outcome::Done(Ok(_))));
    assert!(!doc.print.jobs.confirming);
    assert!(fake.deletes().is_empty());
    // Yes: one DELETE for the one running job, then a poll shows it cancelled.
    assert!(matches!(jobs_tracker::cancel(&mut doc, &mut call, None, Some(true)), Outcome::Done(Ok(_))));
    until(&mut doc, "the cancel and the poll after it", |d| jobs_tracker::state_json(d)["cancelling"] == false && d.print.jobs.jobs.iter().all(PrintJob::ended));
    assert_eq!(fake.deletes(), vec!["/print/jobs/a1".to_string()]);
    // Nothing runs now: refused by name, nothing more sent.
    match jobs_tracker::cancel(&mut doc, &mut call, None, Some(true)) {
        Outcome::Done(Err(e)) => assert_eq!(e, "No print jobs are running"),
        _ => panic!("expected the refusal"),
    }
    assert_eq!(fake.deletes().len(), 1);
}

#[test]
fn robocads_texts_for_each_kind() {
    let job = |kind: &str, result: Value| PrintJob { id: "x".into(), kind: kind.into(), state: "done".into(), fraction: 1.0, result, ..PrintJob::default() };
    let split = job("split", json!({"revision": 6, "group": "g1", "piece_nodes": ["p1", "p2", "p3"], "hardware": [{"item": "screw", "size": "M3x12", "count": 4}]}));
    assert_eq!(done_text(&split), ("split into 3 pieces; hardware: 4× screw M3x12".to_string(), None));
    let plan = job("plan", json!({"plate_files": ["a.3mf", "b.3mf"], "total_hours": 5.26, "total_filament_g": 123.4, "plates": "/runs/plates"}));
    assert_eq!(done_text(&plan).0, "plan: 2 plate(s), about 5.3 h and 123 g (estimates); 3MF files in /runs/plates");
    assert_eq!(done_text(&job("strength_split", json!({"recommendation": "split", "why": "the joint is weaker"}))).0, "split: the joint is weaker");
    let assembly = job("assembly", json!({"guide": "/runs/a/guide.html", "steps": [1, 2, 3], "exploded": "n9"}));
    assert_eq!(done_text(&assembly), ("assembly: 3 steps; guide /runs/a/guide.html".to_string(), Some("/runs/a/guide.html".to_string())));
    assert!(publishes(&assembly) && publishes(&split) && !publishes(&job("assembly", json!({"guide": "/g.html", "steps": []}))));
    let coupons = job("coupons", json!({"coupons": [1, 2], "plates": ["p.3mf"], "protocol": "/runs/c/PROTOCOL.md"}));
    assert_eq!(done_text(&coupons), ("coupons: 2 on 1 plate(s); break them, fill results.json, then `sim-print promote results.json`".to_string(), Some("/runs/c".to_string())));
    assert_eq!(capitalize("strength_split"), "Strength_split");
    assert_eq!(capitalize("ANALYZE"), "Analyze");
    let mut quiet = job("analyze", Value::Null);
    quiet.state = "running".into();
    quiet.fraction = 0.126;
    assert_eq!(line(&quiet), "analyze x: running 13 % ");
}
