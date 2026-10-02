//! Native dispatch uses the existing typed client; reference dispatch is Python Ops.
use super::{contract::*, process::OwnedProcess};
use crate::cad_client::motion::{PoseContinuation, PoseRequest};
use crate::cad_client::{
    self, CadClient, CadError, ComponentJobState, ComponentOperation, ComponentStamp,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub struct NativeAdapter {
    pub cad_dir: PathBuf,
    pub workspace: super::isolation::Workspace,
}
pub fn identity() -> AdapterIdentity {
    AdapterIdentity {
        name: "native-typed-client".into(),
        version: "1".into(),
        implementation: "Rust CadClient -> RoboCAD validated service".into(),
        kernel: "Python/OCCT".into(),
        derivation: "robocad.physical/robotics".into(),
        independent: false,
    }
}
pub fn stamp_revision(revision: u64, offset: i64) -> Result<u64, String> {
    revision
        .checked_add_signed(offset)
        .ok_or_else(|| "revision offset out of range".into())
}
pub fn timestamp() -> String {
    format!(
        "unix:{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}
fn error(e: CadError) -> (ExecutionStatus, String) {
    let uncertain = e.message.contains("may still apply") || e.message.contains("applied it");
    (
        if uncertain {
            ExecutionStatus::Uncertain
        } else if matches!(e.status, Some(400 | 404 | 409 | 422)) {
            ExecutionStatus::Failed
        } else {
            ExecutionStatus::Incomplete
        },
        e.to_string(),
    )
}
const COMPONENT_GUARD: &str = "The document changed during preparation. Your edits are preserved; retry the component operation.";
fn component_guard_refusal(
    error: &Option<String>,
    document_id: &str,
    revision: u64,
    current: &cad_client::Health,
) -> bool {
    error.as_deref() == Some(COMPONENT_GUARD)
        && (current.document_id.as_deref() != Some(document_id) || current.revision != revision)
}
impl Adapter for NativeAdapter {
    fn identity(&self) -> AdapterIdentity {
        identity()
    }
    fn run(
        &mut self,
        m: &Manifest,
        s: &Scenario,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<AdapterRun, String> {
        super::runner::validate_model(&self.cad_dir, &self.workspace, m, cancelled)?;
        #[cfg(not(unix))]
        {
            return Err(
                "process-group ownership requires Unix; non-Unix execution unsupported".into(),
            );
        }
        let port = cad_client::service::free_port()?;
        let python = cad_client::service::interpreter(&self.cad_dir)?;
        // Build the same authoritative service invocation but all logs stay in ownership.
        let mut command = Command::new(python);
        command
            .args(["-m", "robocad.api"])
            .arg(&self.workspace.model)
            .args(["--port", &port.to_string(), "--host", "127.0.0.1"])
            .env("ROBOCAD_LOG_DIR", self.workspace.root.join("logs"))
            .env("TMPDIR", self.workspace.root.join("tmp"))
            .env("TMP", self.workspace.root.join("tmp"))
            .env("TEMP", self.workspace.root.join("tmp"))
            .current_dir(&self.cad_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(
                self.workspace
                    .directory
                    .new_file(std::path::Path::new("native.log"))?,
            );
        let mut child = OwnedProcess::spawn(&mut command)?;
        let client = CadClient::new(&format!("http://127.0.0.1:{port}"))
            .map_err(|e| e.to_string())?
            .with_timeout(Duration::from_secs(5));
        cad_client::service::wait_until_live(
            &client,
            Instant::now() + cad_client::service::START_TIMEOUT,
            || child.alive(),
            cancelled,
        )?;
        let mut receipts = Vec::new();
        let mut prior: Option<PoseContinuation> = None;
        let mut halted = false;
        for step in &s.operations {
            if halted || cancelled() {
                receipts.push(Receipt {
                    step_id: step.id.clone(),
                    status: ExecutionStatus::NotRun,
                    expected: step.expected.clone(),
                    message: "no dispatch after cancellation/uncertain outcome".into(),
                    process_document_id: None,
                    revision: None,
                    observations: BTreeMap::new(),
                    executed_at: None,
                });
                continue;
            }
            let health = match client.health() {
                Ok(health) => health,
                Err(error) => {
                    receipts.push(Receipt {
                        step_id: step.id.clone(),
                        status: ExecutionStatus::Incomplete,
                        expected: step.expected.clone(),
                        message: format!("pre-dispatch health unavailable: {error}"),
                        process_document_id: None,
                        revision: None,
                        observations: BTreeMap::new(),
                        executed_at: None,
                    });
                    halted = true;
                    continue;
                }
            };
            let document_id = health.document_id.clone();
            if document_id.as_deref() != Some(m.document_id.as_str()) {
                receipts.push(Receipt {
                    step_id: step.id.clone(),
                    status: ExecutionStatus::Incomplete,
                    expected: step.expected.clone(),
                    message: "service durable document identity mismatch before dispatch".into(),
                    process_document_id: document_id,
                    revision: Some(health.revision),
                    observations: BTreeMap::new(),
                    executed_at: None,
                });
                halted = true;
                continue;
            }
            let document_id = document_id.expect("validated document identity above");
            let outcome: Result<Value, (ExecutionStatus, String)> = (|| {
                let value = match &step.operation {
                    Operation::Observe => Value::Null,
                    Operation::Rename { node, name } => {
                        client.rename(node, name).map_err(error)?.result
                    }
                    Operation::Undo => {
                        json!(client.undo().map_err(error)?.undone.ok_or_else(|| (
                            ExecutionStatus::Failed,
                            "No command available to undo".into()
                        ))?)
                    }
                    Operation::Redo => {
                        json!(client.redo().map_err(error)?.redone.ok_or_else(|| (
                            ExecutionStatus::Failed,
                            "No command available to redo".into()
                        ))?)
                    }
                    Operation::Op { name, args, kwargs } => {
                        if !matches!(
                            name.as_str(),
                            "set_joint"
                                | "set_robot_setting"
                                | "set_ground"
                                | "set_joint_physics"
                                | "set_material_props"
                        ) {
                            return Err((
                                ExecutionStatus::Unsupported,
                                format!("op {name} is outside bounded non-filesystem allowlist"),
                            ));
                        }
                        client
                            .op(name, args, &kwargs.clone().into_iter().collect())
                            .map_err(error)?
                            .result
                    }
                    Operation::ConfigureRobot {
                        revision_offset,
                        updates,
                    } => {
                        client
                            .configure_robot(
                                stamp_revision(health.revision, *revision_offset)
                                    .map_err(|e| (ExecutionStatus::Incomplete, e))?,
                                Some(updates),
                                None,
                                None,
                                None,
                            )
                            .map_err(error)?
                            .result
                    }
                    Operation::Physical { flex, planar } => {
                        client.physical_model(*flex, *planar).map_err(error)?
                    }
                    Operation::Pose {
                        positions,
                        time,
                        continuation,
                        revision_offset,
                    } => {
                        let request = PoseRequest {
                            document_id: document_id.clone(),
                            expected_revision: stamp_revision(health.revision, *revision_offset)
                                .map_err(|e| (ExecutionStatus::Incomplete, e))?,
                            positions: positions.clone(),
                            time: *time,
                            program: None,
                            prior: if *continuation { prior.clone() } else { None },
                        };
                        let sample = client.sample_pose(&request).map_err(error)?;
                        prior = Some(sample.continuation());
                        json!(sample)
                    }
                    Operation::Captured {
                        capture,
                        action,
                        args,
                    } => match action.as_str() {
                        "snapshot" | "identity" | "geometry" => client
                            .parity_captured(capture, action, &m.source.sha256)
                            .map_err(error)?,
                        "sources" => client.experiment_sources(capture).map_err(error)?,
                        "sample" => client
                            .experiment_sample(
                                capture,
                                args.get("time").and_then(Value::as_f64).unwrap_or(0.),
                                args.get("flex_scale").and_then(Value::as_f64).unwrap_or(1.),
                            )
                            .map_err(error)?,
                        _ => {
                            return Err((
                                ExecutionStatus::Unsupported,
                                "unknown captured action".into(),
                            ));
                        }
                    },
                    Operation::Component {
                        operation,
                        revision_offset,
                        cancel,
                        interfere,
                    } => {
                        let op: ComponentOperation = serde_json::from_value(operation.clone())
                            .map_err(|e| (ExecutionStatus::Incomplete, e.to_string()))?;
                        if matches!(
                            op,
                            ComponentOperation::Import { .. } | ComponentOperation::Export { .. }
                        ) {
                            return Err((ExecutionStatus::Unsupported,"filesystem component operation requires explicit dependency/output contract".into()));
                        }
                        let started = client
                            .start_component(
                                &op,
                                &ComponentStamp {
                                    document_id: document_id.clone(),
                                    expected_revision: stamp_revision(
                                        health.revision,
                                        *revision_offset,
                                    )
                                    .map_err(|e| (ExecutionStatus::Incomplete, e))?,
                                },
                            )
                            .map_err(error)?;
                        if let Some(node) = interfere {
                            client
                                .rename(
                                    node,
                                    &format!(
                                        "{} parity-interference",
                                        client.node(node).map_err(error)?.summary.name
                                    ),
                                )
                                .map_err(error)?;
                        }
                        let mut deadline = Instant::now() + Duration::from_secs(120);
                        let id = started.job.id;
                        let mut cancel_requested = false;
                        loop {
                            if (*cancel || cancelled()) && !cancel_requested {
                                client.cancel_component_job(&id).map_err(error)?;
                                cancel_requested = true;
                                deadline = Instant::now() + Duration::from_secs(5);
                            }
                            let state = client.component_job(&id).map_err(error)?;
                            if state.state == ComponentJobState::Applied {
                                if cancel_requested {
                                    return Err((
                                        ExecutionStatus::Incomplete,
                                        "component applied before cancellation acknowledgement"
                                            .into(),
                                    ));
                                }
                                break json!(state);
                            }
                            if state.state == ComponentJobState::Failed {
                                return Err((
                                    if component_guard_refusal(
                                        &state.error,
                                        &state.document_id,
                                        state.revision,
                                        &client.health().map_err(error)?,
                                    ) {
                                        ExecutionStatus::Failed
                                    } else {
                                        ExecutionStatus::Incomplete
                                    },
                                    state
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "component preparation failed".into()),
                                ));
                            }
                            if state.state == ComponentJobState::Cancelled {
                                return Err((
                                    ExecutionStatus::Cancelled,
                                    "component cancelled".into(),
                                ));
                            }
                            if Instant::now() >= deadline {
                                if !cancel_requested {
                                    let _ = client.cancel_component_job(&id);
                                }
                                return Err((
                                    ExecutionStatus::Incomplete,
                                    "component deadline expired; no mutation retry".into(),
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    }
                };
                Ok(value)
            })();
            let (mut status, message, result) = match outcome {
                Ok(v) => (
                    ExecutionStatus::Passed,
                    "operation completed".into(),
                    Some(v),
                ),
                Err((st, msg)) => (st, msg, None),
            };
            let observed = client.parity_observations(&m.source.sha256);
            let mut observations = match observed {
                Ok(v) => v,
                Err(e) => {
                    if !matches!(
                        status,
                        ExecutionStatus::Uncertain | ExecutionStatus::Cancelled
                    ) {
                        status = ExecutionStatus::Incomplete;
                    }
                    let mut v = BTreeMap::new();
                    v.insert(
                        "observations.error".into(),
                        Observation {
                            owner: "robocad.parity_observations".into(),
                            unit: None,
                            frame: None,
                            provenance: None,
                            uncertainty: None,
                            value: ObservedValue::Invalid(e.to_string()),
                        },
                    );
                    v
                }
            };
            let actual = match status {
                ExecutionStatus::Passed => "success",
                ExecutionStatus::Failed => "rejected",
                ExecutionStatus::Cancelled => "cancelled",
                ExecutionStatus::Unsupported => "unsupported",
                ExecutionStatus::Uncertain => "uncertain",
                _ => "incomplete",
            };
            observations.insert(
                "operation.outcome".into(),
                Observation {
                    owner: "robocad.command".into(),
                    unit: None,
                    frame: None,
                    provenance: Some("derived: actual authoritative dispatch outcome".into()),
                    uncertainty: None,
                    value: ObservedValue::Present(json!(actual)),
                },
            );
            if let Some(value) = result {
                super::native_observations::expand(
                    &client,
                    &step.operation,
                    value,
                    &mut observations,
                    &mut status,
                );
            }
            if status != ExecutionStatus::Incomplete
                && ((actual == "success" && step.expected != ExpectedOutcome::Success)
                    || (actual == "rejected" && step.expected != ExpectedOutcome::Rejected))
            {
                status = ExecutionStatus::Failed;
            } else if status != ExecutionStatus::Incomplete
                && actual == "rejected"
                && step.expected == ExpectedOutcome::Rejected
            {
                status = ExecutionStatus::Passed;
            }
            halted = matches!(
                status,
                ExecutionStatus::Uncertain
                    | ExecutionStatus::Incomplete
                    | ExecutionStatus::Cancelled
            );
            receipts.push(Receipt {
                step_id: step.id.clone(),
                status,
                expected: step.expected.clone(),
                message,
                process_document_id: Some(document_id),
                revision: client.health().ok().map(|h| h.revision),
                observations,
                executed_at: Some(timestamp()),
            });
        }
        if let Err(error) = child.stop() {
            if let Some(receipt) = receipts.iter_mut().rev().find(|r| r.executed_at.is_some()) {
                if !matches!(
                    receipt.status,
                    ExecutionStatus::Uncertain | ExecutionStatus::Cancelled
                ) {
                    receipt.status = ExecutionStatus::Incomplete;
                }
                receipt
                    .message
                    .push_str(&format!("; owned service cleanup incomplete: {error}"));
            }
        }
        Ok(AdapterRun {
            identity: identity(),
            source: m.source.clone(),
            receipts,
        })
    }
}

#[cfg(test)]
mod lifecycle_fixtures {
    use super::*;
    #[test]
    fn uncertain_edit_and_revision_refusal_remain_distinct() {
        let uncertain = CadError {
            method: "POST",
            route: "/ops/rename".into(),
            status: None,
            message: "RoboCAD may still apply it; refresh before retrying".into(),
        };
        assert_eq!(error(uncertain).0, ExecutionStatus::Uncertain);
        let refused = CadError {
            method: "POST",
            route: "/ops/configure_robot".into(),
            status: Some(409),
            message: "revision changed".into(),
        };
        assert_eq!(error(refused).0, ExecutionStatus::Failed);
        let current = cad_client::Health {
            document_id: Some("doc".into()),
            revision: 2,
            ..Default::default()
        };
        assert!(component_guard_refusal(
            &Some(COMPONENT_GUARD.into()),
            "doc",
            1,
            &current
        ));
        assert!(!component_guard_refusal(
            &Some("Component worker failed".into()),
            "doc",
            1,
            &current
        ));
        assert!(!component_guard_refusal(
            &Some(COMPONENT_GUARD.into()),
            "doc",
            2,
            &current
        ));
        assert!(stamp_revision(0, -1).is_err());
        assert_eq!(stamp_revision(12, -1).unwrap(), 11);
    }
}
