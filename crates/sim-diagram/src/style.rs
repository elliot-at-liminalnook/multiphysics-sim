//! Generic domain styling and trace semantics; domain names are not renderer cases.
use crate::Selection;
use egui::Color32;
use sim_inspect::{PortKind, SystemDescription};
use std::collections::BTreeSet;
pub struct DomainStyle {
    pub key: String,
    pub label: String,
    pub color: Color32,
}
pub fn port_domain(d: &SystemDescription, p: &PortKind) -> DomainStyle {
    let (key, label) = match p {
        PortKind::Physical { connector } => (
            format!("{}@{}", connector.name, connector.version),
            d.definitions
                .connectors
                .iter()
                .find(|c| &c.id == connector)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| connector.name.clone()),
        ),
        PortKind::SignalInput { .. } | PortKind::SignalOutput { .. } => {
            ("signals".into(), "Signals".into())
        }
        PortKind::Unresolved { declared_type } => {
            (format!("unresolved:{declared_type}"), declared_type.clone())
        }
    };
    let hash = blake3::hash(key.as_bytes());
    let h = u16::from_le_bytes([hash.as_bytes()[0], hash.as_bytes()[1]]) as f32 / 65536.;
    let color = if key == "signals" {
        Color32::from_rgb(45, 106, 193)
    } else {
        egui::ecolor::Hsva::new(h, 0.85, 0.36, 1.).into()
    };
    DomainStyle { key, label, color }
}
pub fn net_domain(d: &SystemDescription, id: &str) -> DomainStyle {
    port_domain(d, &d.ports[&d.nets[id].ports[0]].schema)
}
pub fn incident_nets(d: &SystemDescription, selected: Option<&Selection>) -> BTreeSet<String> {
    d.nets
        .values()
        .filter(|n| match selected {
            Some(Selection::Net(id)) => &n.id == id,
            Some(Selection::Port(id)) => n.ports.contains(id),
            Some(Selection::Component(id)) => n
                .ports
                .iter()
                .any(|p| d.ports.get(p).is_some_and(|p| &p.component == id)),
            None => false,
        })
        .map(|n| n.id.clone())
        .collect()
}
pub fn number(value: f64) -> String {
    if value == 0. {
        "0".into()
    } else if value.abs() >= 1e5 || value.abs() < 1e-3 {
        format!("{value:.3e}")
    } else {
        let s = format!("{value:.4}");
        s.trim_end_matches('0').trim_end_matches('.').into()
    }
}
