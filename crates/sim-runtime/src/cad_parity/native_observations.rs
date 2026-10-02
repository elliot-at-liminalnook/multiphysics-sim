//! Operation-derived observation expansion; the command result remains intact.
use super::contract::*;
use crate::cad_client::CadClient;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub(super) fn expand(
    _client: &CadClient,
    operation: &Operation,
    value: Value,
    observations: &mut BTreeMap<String, Observation>,
    _status: &mut ExecutionStatus,
) {
    if matches!(operation, Operation::Captured { .. }) {
        observations.insert(
            "captured.identity".into(),
            Observation {
                owner: "robocad.captured_review".into(),
                unit: None,
                frame: None,
                provenance: Some("derived: immutable CAD snapshot".into()),
                uncertainty: None,
                value: captured_identity(&value),
            },
        );
    }
    if matches!(operation, Operation::Physical { .. }) {
        for key in ["links", "joints", "materials", "uncertainty", "source"] {
            observations.insert(
                format!("physical.{key}"),
                Observation {
                    owner: "robocad.physical".into(),
                    unit: None,
                    frame: None,
                    provenance: Some("derived: authoritative physical export".into()),
                    uncertainty: None,
                    value: required_field(&value, key),
                },
            );
        }
        observations.insert(
            "physical.schema".into(),
            Observation {
                owner: "robocad.physical".into(),
                unit: None,
                frame: None,
                provenance: Some("derived: authoritative physical export".into()),
                uncertainty: None,
                value: physical_schema(&value),
            },
        );
    }
    if matches!(operation, Operation::Pose { .. }) {
        for key in ["positions", "closure_error_mm", "identity", "prior_applied"] {
            observations.insert(
                format!("motion.{key}"),
                Observation {
                    owner: "robocad.motion_service".into(),
                    unit: match key {
                        "positions" => Some("mixed:mm,rad".into()),
                        "closure_error_mm" => Some("mm".into()),
                        _ => None,
                    },
                    frame: if matches!(key, "positions" | "closure_error_mm") {
                        Some("cad-world".into())
                    } else {
                        None
                    },
                    provenance: Some("derived: PoseModel reference kinematic solver".into()),
                    uncertainty: None,
                    value: required_field(&value, key),
                },
            );
        }
    }
    observations.insert(
        "operation.result".into(),
        Observation {
            owner: "robocad.command".into(),
            unit: None,
            frame: None,
            provenance: None,
            uncertainty: None,
            value: ObservedValue::Present(value),
        },
    );
}

fn captured_identity(value: &Value) -> ObservedValue {
    let identity = value.get("identity").unwrap_or(value);
    let keys = [
        "document_id",
        "revision",
        "source_kind",
        "source_id",
        "physical_hash",
        "archive_hash",
    ];
    if !identity.is_object() || keys.iter().any(|key| identity.get(key).is_none()) {
        ObservedValue::Missing("Captured reply omitted identity fields".into())
    } else if keys
        .iter()
        .any(|key| identity.get(key).is_some_and(Value::is_null))
    {
        ObservedValue::Invalid("Captured identity contains null required fields".into())
    } else {
        ObservedValue::Present(identity.clone())
    }
}
fn physical_schema(value: &Value) -> ObservedValue {
    if value.get("format").is_none() || value.get("version").is_none() {
        ObservedValue::Missing("Physical reply omitted format/version".into())
    } else if !value["format"].is_string() || !value["version"].is_u64() {
        ObservedValue::Invalid("Physical format/version have invalid types".into())
    } else {
        ObservedValue::Present(json!({"format":value["format"],"version":value["version"]}))
    }
}
fn required_field(value: &Value, key: &str) -> ObservedValue {
    if !value.is_object() {
        return ObservedValue::Invalid("Reply must be an object".into());
    }
    match value.get(key) {
        None => ObservedValue::Missing(format!("Authoritative reply omitted {key}")),
        Some(Value::Null) => ObservedValue::Invalid(format!("Required reply field is null: {key}")),
        Some(v) => ObservedValue::Present(v.clone()),
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn malformed_required_observations_keep_distinct_states() {
        assert!(matches!(
            required_field(&json!({}), "links"),
            ObservedValue::Missing(_)
        ));
        assert!(matches!(
            required_field(&json!({"links":null}), "links"),
            ObservedValue::Invalid(_)
        ));
        assert!(matches!(
            required_field(&json!([]), "links"),
            ObservedValue::Invalid(_)
        ));
        assert_eq!(
            required_field(&json!({"links":[]}), "links"),
            ObservedValue::Present(json!([]))
        );
    }
}
