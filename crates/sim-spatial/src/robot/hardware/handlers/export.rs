//! Download calibration: the export job, the file it writes (new files only,
//! with the mirror binding) and its simulated label.
use super::*;

/// Download calibration. A click starts the job (one at a time); REST
/// `hardware_export` starts it (or joins the running one) and answers
/// Pending until it has written its file, then `{"path": …, "simulated":
/// bool}` ([`Exported`]).
pub(in crate::robot::hardware) fn export(hw: &mut Hardware, call: &mut Call) -> Answer {
    if let Some(seq) = call.continuation.get("export").and_then(Value::as_u64) {
        return match &hw.export_done {
            Some((done, result)) if *done == seq => Answer::Done(result.clone().map(|e| Some(json!({"path": e.path.display().to_string(), "simulated": e.simulated})))),
            _ if hw.export.as_ref().is_some_and(|j| j.generation() == seq) => {
                if call.cancelled {
                    Answer::Done(Err("hardware_export: cancelled (the file is still written)".into()))
                } else {
                    Answer::Pending
                }
            }
            _ => Answer::Done(Err("hardware_export: the result was replaced by a newer export".into())),
        };
    }
    let running = hw.export.as_ref().map(|job| job.generation());
    let seq = match running {
        Some(seq) if call.rest() => seq,
        Some(_) => return Answer::Done(Err("a download is already being written".into())),
        None => match start_export(hw) {
            Ok(seq) => seq,
            Err(e) => return Answer::Done(Err(e)),
        },
    };
    if call.rest() {
        *call.continuation = json!({"export": seq});
        return Answer::Pending;
    }
    done()
}

/// The export line's label for a download from a virtual calibration bench.
pub(in crate::robot::hardware) const VIRTUAL_EXPORT: &str = "VIRTUAL (simulated)";

/// A written calibration download ([`write_export`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Exported {
    pub path: PathBuf,
    /// The document is labelled simulated (`"simulated": true`, a
    /// `-virtual` file name): from a pinned virtual link, or a server that
    /// labels its own export as virtual.
    pub simulated: bool,
}

pub(in crate::robot::hardware) fn start_export(hw: &mut Hardware) -> Result<u64, String> {
    let client = hw.link.as_ref().ok_or(NOT_CONNECTED)?.client.clone();
    // The mirror's binding as it is at the click (the page's `mirror?.record()`).
    let mirror = hw.mirror.record();
    let output = hw.snapshot.state.output.clone();
    // A virtual bench's document is labelled with its execution: simulated
    // results must never pass for physical measurements.
    let execution = match hw.snapshot.execution.as_ref().filter(|i| i.is_virtual_calibration()) {
        Some(identity) => Some(serde_json::to_value(identity).map_err(|e| format!("virtual execution identity: {e}"))?),
        None => None,
    };
    hw.export_seq += 1;
    let seq = hw.export_seq;
    hw.export_virtual = execution.is_some();
    hw.export_line = Some(if hw.export_virtual { format!("{VIRTUAL_EXPORT} Downloading calibration…") } else { "Downloading calibration…".into() });
    hw.export = Some(
        Job::spawn(Pool::Dedicated, seq, "calibration export", move |_| {
            let export = client.get("/calibration/export").map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            write_export(export, mirror, output.as_deref(), stamp, execution)
        })
        .complete_on_drop(),
    );
    Ok(seq)
}

/// Writes `{...export, display_mirror}` (the page's object; its keys in
/// serde_json's order) as 2-space JSON to
/// `<output>/viewer-exports/leg-calibration-<unix_ms>.json`, never
/// overwriting. A relative `output` (the server's working directory) is
/// taken from the workspace root, where the servers are started.
///
/// From a virtual calibration bench (`execution`: the link's pinned
/// identity as JSON) the document also carries `"execution"` and
/// `"simulated": true`, and the file is
/// `leg-calibration-<unix_ms>-virtual.json`. A document that names another
/// execution is refused rather than relabelled (the same one is fine).
/// Without a pin, a document the server labelled itself (`"simulated":
/// true`, or an `execution` of kind `virtual_calibration`) keeps its keys,
/// gains `"simulated": true` and is written as `-virtual` too: a simulated
/// result must never pass for a physical measurement.
pub(crate) fn write_export(export: Value, mirror: Value, output: Option<&str>, unix_ms: u128, execution: Option<Value>) -> Result<Exported, String> {
    let mut doc = match export {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    if !mirror.is_null() {
        doc.insert("display_mirror".into(), mirror);
    }
    let labelled_virtual = doc.get("simulated") == Some(&Value::Bool(true))
        || doc.get("execution").and_then(|e| e.get("kind")).and_then(Value::as_str) == Some("virtual_calibration");
    let simulated = execution.is_some() || labelled_virtual;
    if let Some(execution) = execution {
        if doc.get("execution").is_some_and(|own| !own.is_null() && *own != execution) {
            return Err("the exported calibration names another execution than this virtual link; reconnect and download again".into());
        }
        doc.insert("execution".into(), execution);
    }
    if simulated {
        doc.insert("simulated".into(), Value::Bool(true));
    }
    let output = output.filter(|o| !o.is_empty()).ok_or("the calibration server has not reported its output directory")?;
    let base = Path::new(output);
    let base = if base.is_relative() { crate::workspace::path(base)? } else { base.to_path_buf() };
    let dir = base.join("viewer-exports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("leg-calibration-{unix_ms}{}.json", if simulated { "-virtual" } else { "" }));
    let text = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Exported { path, simulated })
}
