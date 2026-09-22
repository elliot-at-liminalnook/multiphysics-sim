//! Inspection at the composition boundary. No renderer or additional physics.
use sim_core::{BehaviorRegistry, Instance};
use sim_inspect::model::{ComponentIdentity, IdentityBindings};
use sim_inspect::{CadReference, GroupDescription, SourceReference, SystemDescription};

/// Source-owned identities attached as a native assembly is composed. Display
/// names are labels; renaming a CAD entity must not change these bindings.
#[derive(Debug, Clone, Default)]
pub struct CompositionMap {
    pub identities: IdentityBindings,
    source: Option<SourceReference>,
}

impl CompositionMap {
    pub fn from_cad_source(source: &serde_json::Value) -> Self {
        let source = source
            .get("cad_sha256")
            .and_then(|v| v.as_str())
            .zip(source.get("file").and_then(|v| v.as_str()))
            .map(|(hash, path)| SourceReference {
                artifact_hash: hash.into(),
                path: path.into(),
                line: None,
            });
        Self {
            identities: IdentityBindings::default(),
            source,
        }
    }

    pub fn group(&mut self, id: &str, label: &str, parent: Option<&str>) {
        self.identities
            .groups
            .entry(id.into())
            .or_insert_with(|| GroupDescription {
                id: id.into(),
                label: label.into(),
                parent: parent.map(str::to_owned),
            });
    }

    pub fn component(
        &mut self,
        instance: &Instance,
        id: impl Into<String>,
        group: &str,
        body_id: Option<&str>,
    ) {
        let id = id.into();
        let persistent = !id.starts_with("capture/");
        let cad = self
            .source
            .as_ref()
            .zip(body_id.filter(|id| !id.is_empty()))
            .map(|(source, body)| CadReference {
                artifact_hash: source.artifact_hash.clone(),
                body_id: body.into(),
            });
        self.identities.components.insert(
            instance.behavior,
            ComponentIdentity {
                id,
                persistent,
                source: self.source.clone(),
                cad,
                group: Some(group.into()),
            },
        );
    }
}

/// Export the exact compiled system behind the shared CAD runtime. Captured
/// scene hashes include experimental options; the source CAD hash stays separate.
pub fn describe_physical(
    robot: &crate::PhysicalRobot,
    registry: &BehaviorRegistry,
    capture_hash: &str,
    revision: u64,
) -> Result<SystemDescription, String> {
    let mut description = sim_inspect::model::describe(
        &robot.runtime.model,
        registry,
        capture_hash,
        revision,
        &robot.composition.identities,
    )
    .map_err(|e| e.to_string())?
    .description;
    let diagnostic = |code: &str, message: String| sim_inspect::Diagnostic {
        code: code.into(),
        message,
        subject: None,
    };
    description.diagnostics.extend(
        robot
            .warnings
            .iter()
            .map(|w| diagnostic("assembly_warning", w.clone())),
    );
    description.diagnostics.push(diagnostic("assembly_scope", format!(
        "CAD assembly: {} exported links, {} joint records, {} motor records. Mechanical internals are packaged in the articulated behavior; these are not separate diagram components.",
        robot.model.links.len(), robot.model.joints.len(), robot.model.motors.len())));
    if let Some(fidelity) = robot.model.source.get("fidelity").and_then(|v| v.as_str()) {
        description
            .diagnostics
            .push(diagnostic("captured_fidelity", fidelity.into()));
    }
    description.seal().map_err(|e| e.to_string())?;
    Ok(description)
}
