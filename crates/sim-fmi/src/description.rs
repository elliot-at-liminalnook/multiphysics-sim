//! `modelDescription.xml`, read leniently (unknown vendor attributes and
//! elements are ignored, as the standard allows) into what the importer
//! validates: the interface types offered, Co-Simulation capabilities, unit
//! and type definitions, and the scalar model variables.
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ValueType {
    Float64,
    Float32,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Boolean,
    String,
    Binary,
    Clock,
}

impl ValueType {
    fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "Float64" => Self::Float64,
            "Float32" => Self::Float32,
            "Int8" => Self::Int8,
            "UInt8" => Self::UInt8,
            "Int16" => Self::Int16,
            "UInt16" => Self::UInt16,
            "Int32" => Self::Int32,
            "UInt32" => Self::UInt32,
            "Int64" => Self::Int64,
            "UInt64" => Self::UInt64,
            "Boolean" => Self::Boolean,
            "String" => Self::String,
            "Binary" => Self::Binary,
            "Clock" => Self::Clock,
            _ => return None,
        })
    }
    /// A number the block can carry as a signal (`f64`).
    pub fn is_numeric(self) -> bool {
        !matches!(self, Self::String | Self::Binary | Self::Clock)
    }
    pub fn is_float(self) -> bool {
        matches!(self, Self::Float64 | Self::Float32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Causality {
    Parameter,
    CalculatedParameter,
    StructuralParameter,
    Input,
    Output,
    Local,
    Independent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Variability {
    Constant,
    Fixed,
    Tunable,
    Discrete,
    Continuous,
}

/// One model variable.
#[derive(Clone, Debug, Serialize)]
pub struct Variable {
    pub name: String,
    pub value_reference: u32,
    pub value_type: ValueType,
    pub causality: Causality,
    pub variability: Variability,
    /// The unit name, from the variable or its declared type.
    pub unit: Option<String>,
    /// The quantity name, from the variable or its declared type.
    pub quantity: Option<String>,
    pub start: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// `<Dimension>` children: an array variable.
    pub dimensions: usize,
    /// Clocked (a `clocks` attribute).
    pub clocked: bool,
    pub intermediate_update: bool,
    pub description: Option<String>,
}

/// A unit definition's SI base-unit exponents and conversion.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BaseUnit {
    /// kg, m, s, A, K, mol, cd, rad.
    pub exponents: [i32; 8],
    pub factor: f64,
    pub offset: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CoSimulation {
    pub model_identifier: String,
    pub needs_execution_tool: bool,
    pub can_be_instantiated_only_once_per_process: bool,
    pub can_get_and_set_fmu_state: bool,
    pub can_serialize_fmu_state: bool,
    pub can_handle_variable_communication_step_size: bool,
    pub fixed_internal_step_size: Option<f64>,
    pub has_event_mode: bool,
    pub provides_intermediate_update: bool,
    pub might_return_early_from_do_step: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelDescription {
    pub fmi_version: String,
    pub model_name: String,
    pub instantiation_token: String,
    pub description: Option<String>,
    pub generation_tool: Option<String>,
    pub co_simulation: Option<CoSimulation>,
    pub model_exchange: bool,
    pub scheduled_execution: bool,
    pub units: BTreeMap<String, BaseUnit>,
    pub variables: Vec<Variable>,
    pub default_step: Option<f64>,
}

impl ModelDescription {
    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.name == name)
    }
}

fn boolean(node: roxmltree::Node, name: &str) -> Result<bool, String> {
    match node.attribute(name) {
        None => Ok(false),
        Some("true") | Some("1") => Ok(true),
        Some("false") | Some("0") => Ok(false),
        Some(other) => Err(format!("<{}> attribute {name}=\"{other}\" is not a boolean", node.tag_name().name())),
    }
}

fn number(node: roxmltree::Node, name: &str) -> Result<Option<f64>, String> {
    match node.attribute(name) {
        None => Ok(None),
        Some("true") => Ok(Some(1.0)),
        Some("false") => Ok(Some(0.0)),
        Some(text) => {
            // `start` of a scalar is one value; arrays (several) are refused later.
            let first = text.split_whitespace().next().unwrap_or("");
            first.parse::<f64>().map(Some).map_err(|_| format!("variable attribute {name}=\"{text}\" is not a number"))
        }
    }
}

/// Parse `modelDescription.xml`.
pub fn parse(xml: &str) -> Result<ModelDescription, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("modelDescription.xml is not well-formed XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "fmiModelDescription" {
        return Err(format!("modelDescription.xml's root is <{}>, not <fmiModelDescription>", root.tag_name().name()));
    }
    let attr = |name: &str| root.attribute(name).map(str::to_owned);
    let mut md = ModelDescription {
        fmi_version: attr("fmiVersion").ok_or("modelDescription.xml has no fmiVersion")?,
        model_name: attr("modelName").unwrap_or_default(),
        // FMI 2 calls it `guid`.
        instantiation_token: attr("instantiationToken").or_else(|| attr("guid")).unwrap_or_default(),
        description: attr("description"),
        generation_tool: attr("generationTool"),
        ..Default::default()
    };
    // Simple types: name → (unit, quantity).
    let mut types: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for child in root.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "CoSimulation" => {
                md.co_simulation = Some(CoSimulation {
                    model_identifier: child.attribute("modelIdentifier").ok_or("<CoSimulation> has no modelIdentifier")?.to_owned(),
                    needs_execution_tool: boolean(child, "needsExecutionTool")?,
                    can_be_instantiated_only_once_per_process: boolean(child, "canBeInstantiatedOnlyOncePerProcess")?,
                    can_get_and_set_fmu_state: boolean(child, "canGetAndSetFMUState")?,
                    can_serialize_fmu_state: boolean(child, "canSerializeFMUState")?,
                    can_handle_variable_communication_step_size: boolean(child, "canHandleVariableCommunicationStepSize")?,
                    fixed_internal_step_size: number(child, "fixedInternalStepSize")?,
                    has_event_mode: boolean(child, "hasEventMode")?,
                    provides_intermediate_update: boolean(child, "providesIntermediateUpdate")?,
                    might_return_early_from_do_step: boolean(child, "mightReturnEarlyFromDoStep")?,
                })
            }
            "ModelExchange" => md.model_exchange = true,
            "ScheduledExecution" => md.scheduled_execution = true,
            "UnitDefinitions" => {
                for unit in child.children().filter(|n| n.has_tag_name("Unit")) {
                    let name = unit.attribute("name").ok_or("a <Unit> has no name")?.to_owned();
                    let mut base = BaseUnit { factor: 1.0, ..Default::default() };
                    if let Some(b) = unit.children().find(|n| n.has_tag_name("BaseUnit")) {
                        for (k, key) in ["kg", "m", "s", "A", "K", "mol", "cd", "rad"].iter().enumerate() {
                            if let Some(v) = b.attribute(*key) {
                                base.exponents[k] = v.parse().map_err(|_| format!("unit `{name}`: BaseUnit {key}=\"{v}\" is not an integer"))?;
                            }
                        }
                        base.factor = number(b, "factor")?.unwrap_or(1.0);
                        base.offset = number(b, "offset")?.unwrap_or(0.0);
                    }
                    md.units.insert(name, base);
                }
            }
            "TypeDefinitions" => {
                for t in child.children().filter(roxmltree::Node::is_element) {
                    if let Some(name) = t.attribute("name") {
                        types.insert(name.to_owned(), (t.attribute("unit").map(str::to_owned), t.attribute("quantity").map(str::to_owned)));
                    }
                }
            }
            "DefaultExperiment" => md.default_step = number(child, "stepSize")?,
            "ModelVariables" => {
                for v in child.children().filter(roxmltree::Node::is_element) {
                    let tag = v.tag_name().name();
                    // FMI 2 wraps the type: <ScalarVariable><Real/></ScalarVariable>.
                    let Some(value_type) = ValueType::from_tag(tag) else {
                        if tag == "ScalarVariable" {
                            continue;
                        }
                        return Err(format!("unknown model variable element <{tag}>"));
                    };
                    let name = v.attribute("name").ok_or_else(|| format!("a <{tag}> variable has no name"))?.to_owned();
                    let value_reference = v.attribute("valueReference").ok_or_else(|| format!("variable `{name}` has no valueReference"))?
                        .parse().map_err(|_| format!("variable `{name}`: valueReference is not an unsigned integer"))?;
                    let causality = match v.attribute("causality").unwrap_or("local") {
                        "parameter" => Causality::Parameter,
                        "calculatedParameter" => Causality::CalculatedParameter,
                        "structuralParameter" => Causality::StructuralParameter,
                        "input" => Causality::Input,
                        "output" => Causality::Output,
                        "local" => Causality::Local,
                        "independent" => Causality::Independent,
                        other => return Err(format!("variable `{name}`: unknown causality `{other}`")),
                    };
                    let default_variability = if value_type.is_float() { "continuous" } else { "discrete" };
                    let variability = match v.attribute("variability").unwrap_or(default_variability) {
                        "constant" => Variability::Constant,
                        "fixed" => Variability::Fixed,
                        "tunable" => Variability::Tunable,
                        "discrete" => Variability::Discrete,
                        "continuous" => Variability::Continuous,
                        other => return Err(format!("variable `{name}`: unknown variability `{other}`")),
                    };
                    let declared = v.attribute("declaredType").and_then(|t| types.get(t));
                    let unit = v.attribute("unit").map(str::to_owned).or_else(|| declared.and_then(|d| d.0.clone()));
                    let quantity = v.attribute("quantity").map(str::to_owned).or_else(|| declared.and_then(|d| d.1.clone()));
                    let numeric = value_type.is_numeric();
                    md.variables.push(Variable {
                        value_reference,
                        value_type,
                        causality,
                        variability,
                        unit,
                        quantity,
                        start: if numeric { number(v, "start")? } else { None },
                        min: if numeric { number(v, "min")? } else { None },
                        max: if numeric { number(v, "max")? } else { None },
                        dimensions: v.children().filter(|n| n.has_tag_name("Dimension")).count(),
                        clocked: v.attribute("clocks").is_some(),
                        intermediate_update: boolean(v, "intermediateUpdate")?,
                        description: v.attribute("description").map(str::to_owned),
                        name,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(md)
}
