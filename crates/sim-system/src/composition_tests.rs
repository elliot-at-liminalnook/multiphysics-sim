use crate::composition::*;
use serde_json::json;
use std::collections::BTreeMap;
fn graph() -> CadGraph {
    serde_json::from_value(json!({"version":1,"components":{
    "a":{"id":"a","name":"Imported case","type":"thermal.capacitance","body_id":"body-17","binding":"cad/body-17/case","parameters":{},"derivation":{"kind":"body_thermal_capacity","specific_heat":900}},
    "b":{"id":"b","name":"Link","type":"thermal.capacitance","parameters":{"heat_capacity":2}},
    "c":{"id":"c","name":"Second link","type":"thermal.capacitance","parameters":{"heat_capacity":3}}
},"connections":{"persistent-net":{"id":"persistent-net","ports":[{"component_id":"a","port":"port"},{"component_id":"b","port":"port"},{"component_id":"c","port":"port"}]}}})).unwrap()
}
fn catalogue() -> Vec<SystemType> {
    serde_json::from_value(json!([{"type":"thermal.capacitance","name":"Thermal capacitance","parameters_complete":true,"parameters":[{"name":"heat_capacity","unit":"J/K","required":true,"default":null,"default_label":null,"minimum":0,"maximum":null,"exclusive_minimum":true,"integer":false}],"ports":[{"name":"port","schema":{"Acausal":"Thermal"},"direction":"acausal","lanes":[]}]}])).unwrap()
}
#[test]
fn real_storage_round_trip_keeps_import_binding_body_recipe_and_multiterminal_id() {
    let g = graph();
    g.validate_storage().unwrap();
    let encoded = serde_json::to_value(&g).unwrap();
    assert_eq!(serde_json::from_value::<CadGraph>(encoded).unwrap(), g);
    let imports:Vec<ImportedComponent>=serde_json::from_value(json!([{"binding":"cad/body-17/case","name":"drive.case","type":"thermal.capacitance","body_id":"body-17","parameters":{"heat_capacity":2.7},"ports":[{"name":"port","schema":{"Acausal":"Thermal"},"direction":"acausal","lanes":[]}]}])).unwrap();
    let recipes:BTreeMap<String,Recipe>=serde_json::from_value(json!({"body_thermal_capacity":{"type":"thermal.capacitance","outputs":{"heat_capacity":"J/K"},"inputs":{"specific_heat":{"label":"Specific heat","unit":"J/(kg·K)","required":false,"default":null,"minimum":0,"exclusive_minimum":true}}}})).unwrap();
    let c = adapter::adapt(&g, 23, &catalogue(), &imports, &recipes).unwrap();
    assert_eq!(c.description.nets["persistent-net"].ports.len(), 3);
    assert_eq!(g.components["a"].body_id.as_deref(), Some("body-17"));
    assert!(
        c.description.components["a"].cad.is_none(),
        "graph route provides no CAD artifact digest"
    );
    assert!(
        c.description.components["a"].parameters.is_empty(),
        "geometry-derived values cannot be invented by the adapter"
    );
    assert_eq!(
        g.components["a"].derivation.as_ref().unwrap()["specific_heat"],
        900
    );
    assert_eq!(imports[0].evidence["parameters"]["heat_capacity"], 2.7);
}
#[test]
fn unknown_types_duplicate_ports_and_derived_overrides_name_paths() {
    let mut g = graph();
    g.components.get_mut("a").unwrap().component_type = "not.registered".into();
    let error = adapter::adapt(&g, 1, &catalogue(), &[], &BTreeMap::new())
        .err()
        .unwrap();
    assert!(error.contains("graph.components.a.type"));
    let mut g = graph();
    let port = g.connections["persistent-net"].ports[0].clone();
    g.connections
        .get_mut("persistent-net")
        .unwrap()
        .ports
        .push(port);
    assert!(
        g.validate_storage()
            .unwrap_err()
            .contains("graph.connections.persistent-net.ports[3]")
    );
    let mut g = graph();
    g.components
        .get_mut("a")
        .unwrap()
        .parameters
        .insert("heat_capacity".into(), 123.);
    let recipes:BTreeMap<String,Recipe>=serde_json::from_value(json!({"body_thermal_capacity":{"type":"thermal.capacitance","outputs":{"heat_capacity":"J/K"},"inputs":{"specific_heat":{"label":"Specific heat","unit":"J/(kg·K)","required":false,"default":null,"minimum":0,"exclusive_minimum":true}}}})).unwrap();
    let error =
        adapter::validate_component(&g.components["a"], &catalogue(), &recipes).unwrap_err();
    assert!(error.contains("graph.components.a.parameters.heat_capacity"));
    assert!(error.contains("source-owned"));
}

#[test]
fn physical_signal_and_quantity_mismatches_name_connection() {
    let mut g = graph();
    g.components.remove("a");
    g.connections
        .get_mut("persistent-net")
        .unwrap()
        .ports
        .remove(0);
    let mut d = adapter::adapt(&g, 23, &catalogue(), &[], &BTreeMap::new())
        .unwrap()
        .description;
    let ports = d.nets["persistent-net"].ports.clone();
    d.ports.get_mut(&ports[0]).unwrap().schema = sim_inspect::PortKind::SignalOutput {
        signal_type: sim_core::definitions::SignalType::Any,
    };
    assert!(
        validate_connections(&d)
            .unwrap_err()
            .contains("nets.persistent-net: physical and signal")
    );
    d.ports.get_mut(&ports[0]).unwrap().schema = sim_inspect::PortKind::SignalOutput {
        signal_type: sim_core::definitions::SignalType::Quantity(
            sim_core::definitions::DefinitionId::new("sim.quantity.boolean", 1),
        ),
    };
    d.ports.get_mut(&ports[1]).unwrap().schema = sim_inspect::PortKind::SignalInput {
        signal_type: sim_core::definitions::SignalType::Quantity(
            sim_core::definitions::DefinitionId::new("sim.quantity.integer", 1),
        ),
    };
    assert!(
        validate_connections(&d)
            .unwrap_err()
            .contains("nets.persistent-net: incompatible signal quantities")
    );
}
