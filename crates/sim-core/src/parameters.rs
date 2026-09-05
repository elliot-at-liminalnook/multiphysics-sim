//! Native component parameter declarations shared by every authoring adapter.
use crate::{BehaviorDescriptor, ConnectorKind, EquationError, PortSchema};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct ParameterDeclaration {
    pub name: String,
    pub unit: String,
    pub required: bool,
    pub default: Option<f64>,
    pub default_label: Option<String>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: bool,
    pub integer: bool,
}

impl ParameterDeclaration {
    pub fn alternative(name: impl Into<String>, unit: &str) -> Self {
        Self {
            required: false,
            ..Self::required(name, unit)
        }
    }
    pub fn required(name: impl Into<String>, unit: &str) -> Self {
        Self {
            name: name.into(),
            unit: unit.into(),
            required: true,
            default: None,
            default_label: None,
            minimum: None,
            maximum: None,
            exclusive_minimum: false,
            integer: false,
        }
    }
    pub fn optional(name: impl Into<String>, unit: &str, default: f64) -> Self {
        Self {
            required: false,
            default: default.is_finite().then_some(default),
            default_label: (!default.is_finite()).then(|| default.to_string()),
            ..Self::required(name, unit)
        }
    }
    pub fn positive(mut self) -> Self {
        self.minimum = Some(0.);
        self.exclusive_minimum = true;
        self
    }
    pub fn nonnegative(mut self) -> Self {
        self.minimum = Some(0.);
        self
    }
    pub fn at_most(mut self, maximum: f64) -> Self {
        self.maximum = Some(maximum);
        self
    }
    pub fn integer(mut self, minimum: f64, maximum: f64) -> Self {
        self.integer = true;
        self.minimum = Some(minimum);
        self.maximum = Some(maximum);
        self
    }
    fn matches(&self, name: &str) -> bool {
        if let Some((prefix, suffix)) = self.name.split_once('*') {
            name.starts_with(prefix)
                && name.ends_with(suffix)
                && name.len() > prefix.len() + suffix.len()
        } else {
            self.name == name
        }
    }
}

impl BehaviorDescriptor {
    pub fn with_parameters(mut self, mut parameters: Vec<ParameterDeclaration>) -> Self {
        let acausal: Vec<_> = self
            .ports
            .iter()
            .filter_map(|port| match port.schema {
                PortSchema::Acausal(kind) => Some((port.name.to_string(), kind)),
                _ => None,
            })
            .flat_map(|(name, kind)| match kind {
                ConnectorKind::Composite(members) => members
                    .iter()
                    .map(|member| (format!("{name}.{}", member.name()), *member))
                    .collect::<Vec<_>>(),
                _ => vec![(name, kind)],
            })
            .collect();
        // Initial node values follow the compiler's native lane names and units.
        // Preserve explicitly declared initial states such as inertia.speed.
        for (name, kind) in &acausal {
            for lane in kind.lanes() {
                let mut names = vec![format!("initial.{name}.{}", lane.across)];
                if acausal.len() == 1 && !name.contains('*') {
                    names.push(format!("initial.{}", lane.across));
                }
                for name in names {
                    if !parameters.iter().any(|p| p.name == name) {
                        let mut initial = ParameterDeclaration::alternative(name, lane.across_kind.unit());
                        initial.default_label = Some("native state or connected constraint; otherwise 0".into());
                        parameters.push(initial);
                    }
                }
            }
        }
        self.parameters = Some(parameters);
        self
    }

    pub fn validate_parameters(&self, values: &BTreeMap<String, f64>) -> Result<(), EquationError> {
        let Some(parameters) = &self.parameters else {
            return Ok(());
        };
        for parameter in parameters {
            if parameter.required && !values.keys().any(|name| parameter.matches(name)) {
                return Err(EquationError::InvalidParameter(
                    parameter.name.clone(),
                    "required parameter is missing".into(),
                ));
            }
        }
        for (name, value) in values {
            let parameter = parameters.iter().find(|p| p.matches(name)).ok_or_else(|| {
                EquationError::InvalidParameter(
                    name.clone(),
                    format!("unknown parameter for {}", self.type_id.0),
                )
            })?;
            // A wildcard initial value must identify an instantiated family
            // member. Otherwise it would validate but never reach a node.
            if parameter.name.starts_with("initial.") && parameter.name.contains('*') {
                let port_name = name
                    .strip_prefix("initial.")
                    .unwrap()
                    .rsplit_once('.')
                    .map(|(port, _)| port)
                    .unwrap_or("");
                if !values.contains_key(port_name) {
                    return Err(EquationError::InvalidParameter(
                        name.clone(),
                        format!("initial value refers to undeclared port `{port_name}`"),
                    ));
                }
            }
            if parameter.default_label.as_deref() == Some(value.to_string().as_str()) {
                continue;
            }
            if !value.is_finite()
                || (parameter.integer && value.fract() != 0.)
                || parameter.minimum.is_some_and(|min| {
                    *value < min || (parameter.exclusive_minimum && *value == min)
                })
                || parameter.maximum.is_some_and(|max| *value > max)
            {
                return Err(EquationError::InvalidParameter(
                    name.clone(),
                    format!(
                        "value {value} is outside the declared range ({})",
                        parameter.unit
                    ),
                ));
            }
        }
        Ok(())
    }
}
