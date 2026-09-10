//! Optional experiment output. No clock or ranking value enters the solver.
use super::Recipe;
use serde::Serialize;
use sim_runtime::contact_planning::{JointContactMotion, JointContactReport, JointIpoptConfig};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Selected {
    candidate: JointContactMotion,
    report: JointContactReport,
    valid_report_index: usize,
}
pub struct Checkpoints {
    directory: PathBuf,
    recipe: Recipe,
    config: JointIpoptConfig,
    least: Option<Selected>,
    feasible: Option<Selected>,
    dirty: bool,
    last_write: Option<Instant>,
}
fn atomic_json(directory: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let temporary = directory.join(format!(".{name}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, directory.join(name)).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
impl Checkpoints {
    pub fn new(
        directory: &Path,
        recipe: &Recipe,
        config: &JointIpoptConfig,
        inputs: &[String],
    ) -> Result<Self, String> {
        // Exclusive ownership: never adopt or overwrite another run's directory.
        fs::create_dir(directory).map_err(|e| format!("new checkpoint directory required: {e}"))?;
        atomic_json(
            directory,
            "run.json",
            &serde_json::json!({"version":1,"input_paths":inputs,"recipe":recipe,"search":config,"scope":"Inputs and declared model for live candidate snapshots; executable/input hashes belong to the experiment launch manifest. Wall-clock flush timing is output only."}),
        )?;
        Ok(Self {
            directory: directory.to_owned(),
            recipe: recipe.clone(),
            config: config.clone(),
            least: None,
            feasible: None,
            dirty: false,
            last_write: None,
        })
    }
    pub fn observe(
        &mut self,
        candidate: &JointContactMotion,
        report: &JointContactReport,
        index: usize,
    ) -> Result<(), String> {
        let score = |r: &JointContactReport| {
            r.constraints
                .inequalities
                .iter()
                .copied()
                .fold(0.0_f64, f64::max)
        };
        let replace_least = self.least.as_ref().is_none_or(|old| {
            score(report) < score(&old.report)
                || (score(report) == score(&old.report)
                    && report.motion_report.speed_m_s > old.report.motion_report.speed_m_s)
        });
        let replace_feasible = report.sampled_feasible
            && self.feasible.as_ref().is_none_or(|old| {
                report.motion_report.speed_m_s > old.report.motion_report.speed_m_s
            });
        let selected = || Selected {
            candidate: candidate.clone(),
            report: report.clone(),
            valid_report_index: index,
        };
        if replace_least {
            self.least = Some(selected());
            self.dirty = true;
        }
        if replace_feasible {
            self.feasible = Some(selected());
            self.dirty = true;
        }
        if self.dirty
            && self
                .last_write
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            self.flush()?;
        }
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        for (name, kind, selected) in [
            (
                "least_violation.json",
                "least_sampled_constraint_violation",
                &self.least,
            ),
            (
                "best_sampled_feasible.json",
                "fastest_sampled_feasible",
                &self.feasible,
            ),
        ] {
            if let Some(selected) = selected {
                let mut recipe = self.recipe.clone();
                recipe.candidate = selected.candidate.clone();
                atomic_json(
                    &self.directory,
                    name,
                    &serde_json::json!({"version":1,"kind":kind,"valid_report_index":selected.valid_report_index,"recipe":recipe,"search":self.config,"report":selected.report,"scope":"Evaluated candidate under the included model and gates. May be a derivative probe or rejected NLP trial. Sampled feasibility is not dense, runtime, collision-continuity, browser, hardware or maximum-speed qualification."}),
                )?;
            }
        }
        self.dirty = false;
        self.last_write = Some(Instant::now());
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_replacement_preserves_existing_reader_and_exclusive_file_ownership() {
        let directory = std::env::temp_dir().join(format!(
            "joint-checkpoint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        atomic_json(&directory, "point.json", &vec![1, 2, 3]).unwrap();
        let mut old = File::open(directory.join("point.json")).unwrap();
        atomic_json(&directory, "point.json", &vec![4, 5]).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut old, &mut text).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<i32>>(&text).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            serde_json::from_slice::<Vec<i32>>(&fs::read(directory.join("point.json")).unwrap())
                .unwrap(),
            vec![4, 5]
        );
        assert!(fs::create_dir(&directory).is_err());
        fs::write(directory.join(".point.json.tmp"), b"owned elsewhere").unwrap();
        assert!(atomic_json(&directory, "point.json", &vec![6]).is_err());
        assert_eq!(
            fs::read(directory.join(".point.json.tmp")).unwrap(),
            b"owned elsewhere"
        );
        assert_eq!(fs::read(directory.join("point.json")).unwrap(), b"[4,5]\n");
        fs::remove_dir_all(directory).unwrap();
    }
}
