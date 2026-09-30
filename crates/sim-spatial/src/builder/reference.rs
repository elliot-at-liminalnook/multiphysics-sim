//! Read-only source previews. File reads run away from the render thread.
use super::*;
use serde_json::{Value, json};
#[derive(Default)]
pub(super) struct Reference {
    pub target: Option<String>,
    pub result: Option<Result<sim_markdown::Source, String>>,
    pending: Option<Mutex<mpsc::Receiver<Result<sim_markdown::Source, String>>>>,
}
impl Reference {
    pub fn json(&self) -> Value {
        json!({"target":self.target,"loading":self.pending.is_some(),"result":self.result})
    }
    pub fn open(&mut self, target: String) -> Result<(), String> {
        if target.starts_with("https://") || target.starts_with("http://") {
            #[cfg(target_os = "macos")]
            let program = "open";
            #[cfg(not(target_os = "macos"))]
            let program = "xdg-open";
            std::process::Command::new(program)
                .arg(&target)
                .spawn()
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        sim_markdown::source_location(&target)?;
        // Source links resolve against the workspace root.
        let root = crate::workspace::root()?.to_path_buf();
        let (tx, rx) = mpsc::channel();
        let source = target.clone();
        std::thread::spawn(move || {
            let _ = tx.send(sim_markdown::read_source(&root, &source));
        });
        self.target = Some(target);
        self.result = None;
        self.pending = Some(Mutex::new(rx));
        Ok(())
    }
}
pub(super) fn tick(mut b: ResMut<Builder>) {
    let result = b
        .reference
        .pending
        .as_ref()
        .and_then(|rx| rx.lock().unwrap().try_recv().ok());
    if let Some(result) = result {
        b.reference.result = Some(result);
        b.reference.pending = None;
        b.panel_dirty = true;
    }
}
