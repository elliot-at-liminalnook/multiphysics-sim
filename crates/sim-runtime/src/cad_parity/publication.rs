//! All records are prewritten and fsynced. Atomic hard links refuse overwrite;
//! completion marker is the publication boundary for the whole report bundle.
use super::{contract::Report, isolation::relative};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
pub struct Publication {
    root: PathBuf,
    directory: super::owned_path::Directory,
}
impl Publication {
    pub fn create(path: &Path) -> Result<Self, String> {
        let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
        let parent = absolute.parent().ok_or("output has no parent")?;

        relative(
            absolute
                .file_name()
                .ok_or("output has no name")?
                .to_str()
                .ok_or("output name non UTF8")?,
        )?;
        let directory = super::owned_path::Directory::open(parent)?.create(
            absolute
                .file_name()
                .ok_or("missing output name")?
                .to_str()
                .ok_or("output nonUTF8")?,
        )?;
        Ok(Self {
            root: absolute,
            directory,
        })
    }
    pub fn publish(&self, report: &Report) -> Result<PathBuf, String> {
        relative(&report.scenario_id)?;

        let json = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
        let mut readable = format!(
            "CAD parity {} / {}\nCode: {}\nReport schema: {}\nModel: {:?}\nModel schema: {}\nDocument: {}\nUnits/frame: {} / {}\nCoverage: {:?}\nUnavailable: {:?}\nReference: {:?}\nNative: {:?}\n",
            report.manifest.id,
            report.scenario_id,
            report.code_identity,
            report.schema_version,
            report.manifest.source,
            report.manifest.model_schema,
            report.manifest.document_id,
            report.manifest.units,
            report.manifest.frame,
            report.manifest.coverage,
            report.manifest.unavailable,
            report.reference.identity,
            report.native.identity,
        );
        for (side, run) in [("reference", &report.reference), ("native", &report.native)] {
            for r in &run.receipts {
                readable.push_str(&format!(
                    "receipt {side}/{} {:?} expected={:?} executed_at={:?} document={:?} revision={:?}: {}\n",
                    r.step_id, r.status, r.expected, r.executed_at,
                    r.process_document_id, r.revision, r.message,
                ));
            }
        }
        for d in &report.diagnostics {
            readable.push_str(&format!(
                "{:?} {} {}: {} unit={:?} abs={:?} rel={:?} tolerance={:?}\n",
                d.status,
                d.step_id,
                d.path,
                d.message,
                d.unit,
                d.absolute_error,
                d.relative_error,
                d.tolerance
            ));
        }
        for g in &report.gates {
            readable.push_str(&format!("{:?}: {} {:?}\n", g.kind, g.passed, g.reasons));
        }
        let mut links = Vec::new();
        for (extension, bytes) in [("json", json.as_slice()), ("txt", readable.as_bytes())] {
            let final_path = PathBuf::from(format!("{}.{extension}", report.scenario_id));
            let temp = PathBuf::from(format!(".{}.{extension}.pending", report.scenario_id));
            let mut f = self.directory.new_file(&temp)?;
            f.write_all(bytes).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            links.push((temp, final_path));
        }
        for (temp, final_path) in &links {
            self.directory
                .link(temp, final_path)
                .map_err(|e| format!("atomic non-overwrite publication: {e}"))?;
        }
        let marker = PathBuf::from(format!(".{}.complete.pending", report.scenario_id));
        let mut f = self.directory.new_file(&marker)?;
        f.write_all(b"schema=1; json and txt fsynced before completion\n")
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        self.directory.link(
            &marker,
            Path::new(&format!("{}.complete", report.scenario_id)),
        )?;
        self.directory.sync()?;
        Ok(self.root.join(&links[0].1))
    }
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn output_names_refuse_escape() {
        assert!(relative("../overwrite").is_err());
        assert!(relative("/absolute").is_err());
        assert!(relative("safe-report").is_ok());
    }
}

#[cfg(test)]
mod report_fixtures {
    use super::*;
    use crate::cad_parity::{contract::*, runner};
    #[test]
    fn planning_bundle_atomic_and_refuses_overwrite() {
        let m: Manifest =
            serde_json::from_str(include_str!("../../../../examples/cad-parity/wheeled.json"))
                .unwrap();
        let s = &m.scenarios[0];
        let reference = runner::not_run(runner::reference_identity(), &m, s, "fixture plan");
        let native = runner::not_run(crate::cad_parity::native::identity(), &m, s, "fixture plan");
        let diagnostics = crate::cad_parity::compare::compare(s, &reference, &native);
        let gates = crate::cad_parity::gates::aggregate(s, &reference, &native, &diagnostics);
        assert!(gates.iter().all(|g| !g.passed));
        assert!(
            reference
                .receipts
                .iter()
                .chain(&native.receipts)
                .all(|r| r.executed_at.is_none())
        );
        let report = Report {
            schema_version: SCHEMA_VERSION,
            code_identity: "unexecuted fixture".into(),
            manifest: m.clone(),
            scenario_id: s.id.clone(),
            reference,
            native,
            diagnostics,
            gates,
        };
        let path = std::env::temp_dir().join(format!(
            "cad-parity-publication-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let output = Publication::create(&path).unwrap();
        let published = output.publish(&report).unwrap();
        let before = fs::read(&published).unwrap();
        assert!(output.publish(&report).is_err());
        assert_eq!(before, fs::read(published).unwrap());
        assert!(
            path.join(format!("{}.complete", report.scenario_id))
                .is_file()
        );
        assert!(Publication::create(&path).is_err());
    }
}
