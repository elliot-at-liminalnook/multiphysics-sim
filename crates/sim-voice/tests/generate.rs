//! Narration generation against a local stand-in for OpenRouter's speech,
//! transcription and credits endpoints (no network, no spend).
use sim_lesson::narration::{Explainer, TimingKind};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// Serves the three endpoints; counts requests; can fail transcription.
struct Stub {
    url: String,
    speech: Arc<Mutex<Vec<serde_json::Value>>>,
    transcribe_fails: Arc<Mutex<bool>>,
}

fn stub() -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let speech = Arc::new(Mutex::new(Vec::new()));
    let fails = Arc::new(Mutex::new(false));
    let (s, f) = (speech.clone(), fails.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            if reader.read_line(&mut first).is_err() {
                continue;
            }
            let mut length = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            let (status, ctype, payload): (&str, &str, Vec<u8>) = if path.ends_with("/audio/speech") {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                // 0.1 s of audio per word, as 24 kHz mono 16-bit PCM.
                let words = v["input"].as_str().unwrap().split_whitespace().filter(|w| !w.starts_with('<') && !w.ends_with('>')).count();
                s.lock().unwrap().push(v);
                ("200 OK", "audio/pcm;rate=24000;channels=1", vec![1u8; words * 4800])
            } else if path.ends_with("/audio/transcriptions") {
                if *f.lock().unwrap() {
                    ("400 Bad Request", "application/json", br#"{"error":{"message":"verbose_json not supported","code":400}}"#.to_vec())
                } else {
                    let words: Vec<serde_json::Value> = ["Every", "motor", "comes", "down", "to", "two", "equations"].iter().enumerate().map(|(i, w)| serde_json::json!({"word": w, "start": i as f64 * 0.1, "end": i as f64 * 0.1 + 0.09})).collect();
                    ("200 OK", "application/json", serde_json::to_vec(&serde_json::json!({"text": "Every motor comes down to two equations", "duration": 0.7, "words": words})).unwrap())
                }
            } else if path.ends_with("/credits") {
                ("200 OK", "application/json", br#"{"data":{"total_credits":10.0,"total_usage":1.25}}"#.to_vec())
            } else {
                ("404 Not Found", "application/json", br#"{"error":{"message":"not found","code":404}}"#.to_vec())
            };
            let mut out = stream;
            let _ = write!(out, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
            let _ = out.write_all(&payload);
        }
    });
    Stub { url, speech, transcribe_fails: fails }
}

fn lesson_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sim-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SCRIPT: &str = "---\nvoice: Kore\nstyle: warm teacher\n---\n## a: One\n[[scroll top]] Every motor <short pause> comes down to [[highlight \"two\"]] two equations.\n\n## b: Two\n@style: brighter\nCurrent makes torque.\n";

#[test]
fn generates_per_section_aligns_and_regenerates_only_what_changed() {
    let stub = stub();
    let dir = lesson_dir("gen");
    std::fs::write(dir.join("explainer.md"), SCRIPT).unwrap();
    let ex = Explainer::load(&dir.join("explainer.md")).unwrap();
    assert!(sim_voice::plan(&ex).iter().all(|p| p.status == sim_voice::Status::Missing));
    let voice = sim_voice::Voice::with_base_url("test-key", Some(&stub.url)).unwrap();
    let report = sim_voice::generate(&ex, &voice, &sim_voice::Request { sections: &[], force: false, budget_usd: 1.0 }, &|_| {}).unwrap();
    assert_eq!(report.generated, vec!["a".to_string(), "b".into()]);
    // The request carried the spoken text with its inline tag, the voice and both styles.
    let sent = stub.speech.lock().unwrap().clone();
    assert_eq!(sent[0]["input"], "Every motor <short pause> comes down to two equations.");
    assert_eq!(sent[0]["voice"], "Kore");
    assert_eq!(sent[0]["model"], "google/gemini-3.8-flash-tts");
    assert_eq!(sent[1]["provider"]["options"]["google-ai-studio"]["speech_metadata"]["style"], "warm teacher; brighter");
    // WAV on disk, aligned timing, cues on their words.
    let ex = Explainer::load(&dir.join("explainer.md")).unwrap();
    let manifest = ex.manifest();
    let entry = &manifest.sections["a"];
    let wav = std::fs::read(ex.narration_dir().join(&entry.file)).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    assert!((entry.duration_s - 0.7).abs() < 1e-9, "{}", entry.duration_s);
    let timing = ex.timing(&manifest, &ex.sections[0]);
    assert_eq!(timing.kind, TimingKind::Aligned);
    let cues = sim_lesson::narration::cue_times(&ex.sections[0], &timing);
    assert_eq!(cues, vec![0.0, 0.5], "highlight fires on \"two\"");
    assert!(sim_voice::plan(&ex).iter().all(|p| p.status == sim_voice::Status::Current));
    // Moving a cue needs no audio; rewording section b regenerates b only.
    std::fs::write(dir.join("explainer.md"), SCRIPT.replace("[[scroll top]] ", "").replace("Current makes torque.", "Current makes TORQUE.")).unwrap();
    let ex = Explainer::load(&dir.join("explainer.md")).unwrap();
    let plan = sim_voice::plan(&ex);
    assert_eq!(plan[0].status, sim_voice::Status::Current);
    assert_eq!(plan[1].status, sim_voice::Status::Stale);
    let before = stub.speech.lock().unwrap().len();
    let report = sim_voice::generate(&ex, &voice, &sim_voice::Request { sections: &[], force: false, budget_usd: 1.0 }, &|_| {}).unwrap();
    assert_eq!(report.generated, vec!["b".to_string()]);
    assert_eq!(stub.speech.lock().unwrap().len(), before + 1);
    // Naming a current section regenerates it.
    let report = sim_voice::generate(&ex, &voice, &sim_voice::Request { sections: &["a".into()], force: false, budget_usd: 1.0 }, &|_| {}).unwrap();
    assert_eq!(report.generated, vec!["a".to_string()]);
    assert_eq!(std::fs::read_dir(ex.narration_dir()).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "wav")).count(), 2, "old audio removed");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn budget_refuses_before_spending_and_failed_alignment_falls_back() {
    let stub = stub();
    let dir = lesson_dir("budget");
    std::fs::write(dir.join("explainer.md"), SCRIPT).unwrap();
    let ex = Explainer::load(&dir.join("explainer.md")).unwrap();
    let voice = sim_voice::Voice::with_base_url("test-key", Some(&stub.url)).unwrap();
    let e = sim_voice::generate(&ex, &voice, &sim_voice::Request { sections: &[], force: false, budget_usd: 0.0001 }, &|_| {}).unwrap_err();
    assert!(e.contains("exceeds"), "{e}");
    assert!(stub.speech.lock().unwrap().is_empty(), "nothing was spent");
    *stub.transcribe_fails.lock().unwrap() = true;
    let report = sim_voice::generate(&ex, &voice, &sim_voice::Request { sections: &["a".into()], force: false, budget_usd: 1.0 }, &|_| {}).unwrap();
    assert_eq!(report.warnings.len(), 1);
    let ex = Explainer::load(&dir.join("explainer.md")).unwrap();
    let manifest = ex.manifest();
    assert_eq!(ex.timing(&manifest, &ex.sections[0]).kind, TimingKind::Estimated);
    assert!(!manifest.sections["a"].note.is_empty());
    assert_eq!(ex.timing(&manifest, &ex.sections[1]).kind, TimingKind::Silent);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wav_wraps_and_unwraps_pcm() {
    let pcm = vec![7u8; 480];
    let wav = sim_voice::wav(&pcm, 24_000);
    assert_eq!(wav.len(), 524);
    assert_eq!(sim_voice::pcm_of(&wav), (pcm.clone(), 24_000));
    assert_eq!(sim_voice::pcm_of(&pcm), (pcm, 24_000));
}
