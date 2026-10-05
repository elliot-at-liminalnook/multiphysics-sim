//! Focused headless host: run a question, print activity, then acknowledge its reply.
//! cargo run -p sim-agent --example answer -- /repo /tmp/agent-state.json 'question'
use sim_agent::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(args.len() == 4, "cwd state-path question");
    let service = Supervisor::open(Config::from_env(
        PathBuf::from(&args[1]),
        PathBuf::from(&args[2]),
    ));
    let deadline = Instant::now() + Duration::from_secs(600);
    while !service.snapshot().ready {
        assert!(Instant::now() < deadline);
        if let Some(e) = service.snapshot().error {
            panic!("{e}")
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let id=service.ask(Input{instructions:None,developer:None,discussion:"example".into(),key:format!("example-{}",now()),revision:1,context:serde_json::json!({"purpose":"headless integration smoke test","available_part_paths":[]}),question:args[3].clone()}).unwrap();
    let mut cursor = 0;
    loop {
        let s = service.snapshot();
        for e in s.events.iter().filter(|e| e.sequence > cursor) {
            println!("{} {}", e.kind, e.message);
        }
        cursor = s.next_event;
        let r = s.runs.iter().find(|r| r.id == id).unwrap();
        match r.status {
            Status::Ready if r.delivery_ready => {
                println!("{}", serde_json::to_string_pretty(&r.reply).unwrap());
                service.delivered(&id, Ok(()));
                break;
            }
            Status::Failed | Status::Cancelled => panic!("{:?}", r.error),
            _ => {}
        }
        assert!(Instant::now() < deadline, "example timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
}
