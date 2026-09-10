//! Preserve the document received at the episode boundary, before legacy model
//! parsing fills defaults. Receipts describe edits, not authenticated CAD origin.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_robot::PhysicalModel;
use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputOrigin {
    EpisodeDocument,
    /// A caller supplied an already parsed model; original field presence is unknown.
    ParsedModel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputOverride {
    pub pointer: String,
    pub original: OriginalValue,
    pub value: Value,
}

/// A tagged representation keeps absent fields distinct from JSON null after
/// crossing JSON/JavaScript boundaries (Option<Value> cannot do that).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "presence",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OriginalValue {
    Absent,
    Present(Value),
}
impl OriginalValue {
    fn capture(value: Option<&Value>) -> Self {
        value.map_or(Self::Absent, |v| Self::Present(v.clone()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReceipt {
    pub version: u32,
    pub origin: InputOrigin,
    pub overrides: Vec<InputOverride>,
}

#[derive(Clone, Debug)]
pub struct RobotInput {
    document: Arc<Value>,
    origin: InputOrigin,
    parsed_digest: String,
    inspection: Arc<OnceLock<Result<sim_domain_robot::contract::RobotContract, String>>>,
}

fn same(a: &Value, b: &Value) -> bool {
    a == b || crate::physics_context::fingerprint(a) == crate::physics_context::fingerprint(b)
}
fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn tokens(pointer: &str) -> Result<Vec<String>, String> {
    if !pointer.starts_with('/') {
        return Err("robot override requires a non-root JSON pointer".into());
    }
    pointer[1..]
        .split('/')
        .map(|part| {
            let mut token = String::new();
            let mut chars = part.chars();
            while let Some(c) = chars.next() {
                if c == '~' {
                    token.push(match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => return Err("invalid robot override pointer escape".into()),
                    });
                } else {
                    token.push(c);
                }
            }
            Ok(token)
        })
        .collect()
}

/// Arrays are atomic when their entity identities/order change. This avoids
/// assigning one CAD entity's unmodeled evidence to a different entity by index.
fn aligned(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| ["id", "name"].iter().all(|key| a.get(*key) == b.get(*key)))
}

fn identity(value: &Value) -> Option<(&str, &str)> {
    ["id", "name"].into_iter().find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(|value| (key, value))
    })
}

fn merge(
    input: &Value,
    baseline: &Value,
    current: &Value,
    pointer: &str,
    changes: &mut Vec<InputOverride>,
) -> Value {
    if same(baseline, current) {
        return input.clone();
    }
    if let (Some(original), Some(before), Some(after)) =
        (input.as_object(), baseline.as_object(), current.as_object())
    {
        // Removing schema/map keys is an atomic edit to their containing object.
        if before.keys().all(|key| after.contains_key(key)) {
            let mut output = original.clone();
            for (key, value) in after {
                let next = format!("{pointer}/{}", escape(key));
                match (before.get(key), original.get(key)) {
                    (Some(before), Some(input)) => {
                        output.insert(key.clone(), merge(input, before, value, &next, changes));
                    }
                    (Some(before), None) if same(before, value) => {}
                    _ => {
                        changes.push(InputOverride {
                            pointer: next,
                            original: OriginalValue::capture(original.get(key)),
                            value: value.clone(),
                        });
                        output.insert(key.clone(), value.clone());
                    }
                }
            }
            return Value::Object(output);
        }
        // A modeled key removal must not drop unrelated CAD metadata. Build the
        // containing-object replacement against the raw input and receipt it once.
        let mut output = original.clone();
        for key in before.keys().filter(|key| !after.contains_key(*key)) {
            output.remove(key);
        }
        for (key, value) in after {
            let value = match (before.get(key), original.get(key)) {
                (Some(before), Some(input)) => merge(input, before, value, "/field", &mut vec![]),
                (Some(before), None) if same(before, value) => continue,
                _ => value.clone(),
            };
            output.insert(key.clone(), value);
        }
        let output = Value::Object(output);
        changes.push(InputOverride {
            pointer: pointer.into(),
            original: OriginalValue::Present(input.clone()),
            value: output.clone(),
        });
        return output;
    }
    if let (Some(input), Some(before), Some(after)) =
        (input.as_array(), baseline.as_array(), current.as_array())
    {
        if input.len() == before.len() && aligned(before, after) {
            return Value::Array(
                input
                    .iter()
                    .zip(before)
                    .zip(after)
                    .enumerate()
                    .map(|(i, ((input, before), after))| {
                        merge(input, before, after, &format!("{pointer}/{i}"), changes)
                    })
                    .collect(),
            );
        }
        if input.len() == before.len() {
            let reordered = Value::Array(
                after
                    .iter()
                    .map(|value| {
                        let matches: Vec<_> = before
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| {
                                identity(value).is_some() && identity(item) == identity(value)
                            })
                            .collect();
                        if matches.len() == 1 {
                            let (index, baseline) = matches[0];
                            merge(&input[index], baseline, value, "/item", &mut vec![])
                        } else {
                            value.clone()
                        }
                    })
                    .collect(),
            );
            changes.push(InputOverride {
                pointer: pointer.into(),
                original: OriginalValue::Present(Value::Array(input.clone())),
                value: reordered.clone(),
            });
            return reordered;
        }
    }
    changes.push(InputOverride {
        pointer: pointer.into(),
        original: OriginalValue::Present(input.clone()),
        value: current.clone(),
    });
    current.clone()
}

impl RobotInput {
    pub fn document(&self) -> &Value {
        &self.document
    }
    pub fn origin(&self) -> InputOrigin {
        self.origin
    }
    pub(crate) fn input_contract(
        &self,
    ) -> Result<sim_domain_robot::contract::RobotContract, String> {
        self.inspection
            .get_or_init(|| {
                sim_domain_robot::contract::RobotDocument::new(self.document.as_ref().clone())?
                    .contract()
            })
            .clone()
    }

    pub(crate) fn receive(
        current: Value,
        receipt: Option<InputReceipt>,
        model: &PhysicalModel,
    ) -> Result<Self, String> {
        let origin = receipt
            .as_ref()
            .map_or(InputOrigin::EpisodeDocument, |r| r.origin);
        if receipt.as_ref().is_some_and(|r| r.version != 1) {
            return Err("unsupported robot input receipt".into());
        }
        if receipt.as_ref().is_none_or(|r| r.overrides.is_empty()) {
            return Ok(Self {
                document: Arc::new(current),
                origin,
                parsed_digest: crate::physics_context::fingerprint(&model.to_json_value_checked()?),
                inspection: Default::default(),
            });
        }
        let mut input = current.clone();
        if let Some(receipt) = receipt {
            if receipt.version != 1 {
                return Err("unsupported robot input receipt".into());
            }
            let mut paths: Vec<Vec<String>> = Vec::new();
            for edit in &receipt.overrides {
                let path = tokens(&edit.pointer)?;
                if paths
                    .iter()
                    .any(|p| p.starts_with(&path) || path.starts_with(p))
                {
                    return Err("overlapping robot input overrides".into());
                }
                paths.push(path);
                if !current
                    .pointer(&edit.pointer)
                    .is_some_and(|value| same(value, &edit.value))
                {
                    return Err(format!("robot override value mismatch: {}", edit.pointer));
                }
                if let OriginalValue::Present(original) = &edit.original {
                    *input
                        .pointer_mut(&edit.pointer)
                        .ok_or("missing robot override target")? = original.clone();
                } else {
                    let (parent, leaf) = edit.pointer.rsplit_once('/').unwrap();
                    let key = tokens(&format!("/{leaf}"))?.remove(0);
                    input
                        .pointer_mut(parent)
                        .and_then(Value::as_object_mut)
                        .ok_or("absent original robot field requires an object parent")?
                        .remove(&key);
                }
            }
        }
        // Parsing the reconstructed input catches receipts that cannot represent
        // a model. This does not establish its physical validity or calibration.
        let baseline =
            serde_json::from_value::<PhysicalModel>(input.clone()).map_err(|e| e.to_string())?;
        let result = Self {
            document: Arc::new(input),
            origin,
            parsed_digest: crate::physics_context::fingerprint(&baseline.to_json_value_checked()?),
            inspection: Default::default(),
        };
        let (rebuilt, _) = Self::serialize_model(Some(&result), model)?;
        if !same(&rebuilt, &current) {
            return Err("robot input receipt does not match parsed-model edits".into());
        }
        Ok(result)
    }

    pub(crate) fn serialize_model<'a>(
        input: Option<&'a Self>,
        model: &PhysicalModel,
    ) -> Result<(Cow<'a, Value>, Option<InputReceipt>), String> {
        let current = model.to_json_value_checked()?;
        let Some(input) = input else {
            serde_json::from_value::<PhysicalModel>(current.clone()).map_err(|e| {
                format!("cannot serialize a parsed model without its original input: {e}")
            })?;
            return Ok((
                Cow::Owned(current),
                Some(InputReceipt {
                    version: 1,
                    origin: InputOrigin::ParsedModel,
                    overrides: vec![],
                }),
            ));
        };
        if crate::physics_context::fingerprint(&current) == input.parsed_digest {
            return Ok((
                Cow::Borrowed(&input.document),
                (input.origin == InputOrigin::ParsedModel).then_some(InputReceipt {
                    version: 1,
                    origin: input.origin,
                    overrides: vec![],
                }),
            ));
        }
        let baseline: PhysicalModel =
            serde_json::from_value(input.document.as_ref().clone()).map_err(|e| e.to_string())?;
        let baseline = baseline.to_json_value_checked()?;
        let mut overrides = vec![];
        let document = merge(&input.document, &baseline, &current, "", &mut overrides);
        if overrides.iter().any(|e| e.pointer.is_empty()) {
            return Err("robot input root must remain an object".into());
        }
        let parsed: PhysicalModel = serde_json::from_value(document.clone())
            .map_err(|e| format!("cannot serialize robot edits: {e}"))?;
        if !same(&parsed.to_json_value_checked()?, &current) {
            return Err("robot input merge changed the parsed physical model".into());
        }
        let receipt = (!overrides.is_empty() || input.origin == InputOrigin::ParsedModel)
            .then_some(InputReceipt {
                version: 1,
                origin: input.origin,
                overrides,
            });
        Ok((Cow::Owned(document), receipt))
    }
}
