//! Typed revision-guarded source composition operations; blocking job calls.
use super::{CadClient, CadError, encode_uri_component};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use sim_system::composition::{CadComponent, CadConnection, CadEndpoint, CadGraph};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub revision: u64,
    #[serde(default)]
    pub document_id: Option<String>,
    pub graph: CadGraph,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphEdit {
    pub revision: u64,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub graph: CadGraph,
}
pub use sim_system::composition::{ImportedComponent, Parameter, Port, Recipe, SystemType};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedSnapshot {
    pub run_id: String,
    pub revision: Option<u64>,
    pub state: String,
    pub error: Option<String>,
    pub stale: bool,
    #[serde(default)]
    pub metadata_stale: bool,
    pub imported: Vec<ImportedComponent>,
    pub resolved: Option<Value>,
    #[serde(default)]
    pub guard_document_id: Option<String>,
    #[serde(default)]
    pub guard_revision: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GraphCommand {
    Add { component: CadComponent },
    Update { id: String, component: CadComponent },
    Remove { id: String },
    Connect { ports: Vec<CadEndpoint> },
    Open { id: String },
    Replace { graph: CadGraph },
}
impl CadClient {
    pub fn composition(&self) -> Result<GraphSnapshot, CadError> {
        self.get("/system")
    }
    pub fn system_types(&self) -> Result<Vec<SystemType>, CadError> {
        self.get("/experiments/catalogue")
    }
    pub fn geometry_recipes(&self) -> Result<BTreeMap<String, Recipe>, CadError> {
        #[derive(Deserialize)]
        struct Envelope {
            recipes: BTreeMap<String, Recipe>,
        }
        self.get::<Envelope>("/component-recipes")
            .map(|e| e.recipes)
    }
    pub fn system_imports(&self, check: &str) -> Result<ImportedSnapshot, CadError> {
        self.get(&format!(
            "/experiments/{}/components",
            encode_uri_component(check)
        ))
    }
    pub fn composition_edit(
        &self,
        expected_revision: u64,
        command: &GraphCommand,
    ) -> Result<GraphEdit, CadError> {
        self.composition_edit_checked(expected_revision, command, None)
    }
    pub fn composition_edit_checked(
        &self,
        expected_revision: u64,
        command: &GraphCommand,
        check_id: Option<&str>,
    ) -> Result<GraphEdit, CadError> {
        self.composition_edit_guarded(expected_revision, command, check_id, None)
    }
    pub fn composition_edit_guarded(
        &self,
        expected_revision: u64,
        command: &GraphCommand,
        check_id: Option<&str>,
        document_id: Option<&str>,
    ) -> Result<GraphEdit, CadError> {
        use serde_json::json;
        match command {
            GraphCommand::Add { component } => {
                let mut fields = serde_json::to_value(component).map_err(|e| CadError { method:"POST",route:"/system/components".into(),status:None,message:e.to_string() })?;
                if let Some(map)=fields.as_object_mut() { map.remove("id"); }
                self.send("POST", "/system/components", Some(&json!({"expected_revision":expected_revision,"document_id":document_id,"check_id":check_id,"component":fields})))
            }
            GraphCommand::Update { id, component } => self.send("PATCH", &format!("/system/components/{}", encode_uri_component(id)), Some(&json!({"expected_revision":expected_revision,"document_id":document_id,"check_id":check_id,"component":{"id":component.id,"name":component.name,"type":component.component_type,"parameters":component.parameters,"body_id":component.body_id,"binding":component.binding,"derivation":component.derivation}}))),
            GraphCommand::Connect { ports } => self.send("POST", "/system/connections", Some(&json!({"expected_revision":expected_revision,"document_id":document_id,"check_id":check_id,"ports":ports}))),
            GraphCommand::Replace { graph } => self.send("PUT", "/system", Some(&json!({"expected_revision":expected_revision,"document_id":document_id,"check_id":check_id,"graph":graph}))),
            GraphCommand::Remove { id } | GraphCommand::Open { id } => {
                let section = if matches!(command, GraphCommand::Remove { .. }) { "components" } else { "connections" };
                let check=check_id.map(|id|format!("&check_id={}",encode_uri_component(id))).unwrap_or_default();
                let document=document_id.map(|id|format!("&document_id={}",encode_uri_component(id))).unwrap_or_default();
                self.send::<Value, GraphEdit>("DELETE", &format!("/system/{section}/{}?expected_revision={expected_revision}{check}{document}", encode_uri_component(id)), None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cad_client::tests::{Answer, assert_request, ok, serve};
    #[test]
    fn add_lets_service_assign_identity_and_keeps_revision_import_guard() {
        let response = r#"{"revision":8,"id":"persisted-17","graph":{"version":1,"components":{},"connections":{}}}"#;
        let (client, server) = serve(vec![ok(response)]);
        let component = CadComponent {
            id: "draft".into(),
            name: "Case".into(),
            component_type: "thermal.capacitance".into(),
            body_id: Some("body17".into()),
            binding: Some("cad/body17/case".into()),
            parameters: BTreeMap::new(),
            derivation: None,
        };
        assert_eq!(
            client
                .composition_edit_checked(7, &GraphCommand::Add { component }, Some("check17"))
                .unwrap()
                .id
                .as_deref(),
            Some("persisted-17")
        );
        let requests = server.join().unwrap();
        assert_request(
            &requests[0],
            "POST /system/components HTTP/1.1",
            client.endpoint.port,
            Some(
                r#"{"check_id":"check17","component":{"binding":"cad/body17/case","body_id":"body17","name":"Case","parameters":{},"type":"thermal.capacitance"},"document_id":null,"expected_revision":7}"#,
            ),
        );
    }
    #[test]
    fn stale_source_refusal_is_returned_with_route_and_status() {
        let (client, server) = serve(vec![Answer::Json(
            409,
            r#"{"error":"revision conflict: expected 7, current 9"}"#.into(),
        )]);
        let error = client
            .composition_edit(7, &GraphCommand::Open { id: "net17".into() })
            .unwrap_err();
        assert_eq!(error.status, Some(409));
        assert!(error.message.contains("current 9"));
        let requests = server.join().unwrap();
        assert_request(
            &requests[0],
            "DELETE /system/connections/net17?expected_revision=7 HTTP/1.1",
            client.endpoint.port,
            None,
        );
    }
    #[test]
    fn same_revision_foreign_document_guard_is_sent_and_refusal_retained() {
        let (client, server) = serve(vec![Answer::Json(
            409,
            r#"{"error":"System graph document identity changed"}"#.into(),
        )]);
        let error = client
            .composition_edit_guarded(
                7,
                &GraphCommand::Open { id: "net17".into() },
                None,
                Some("old-document"),
            )
            .unwrap_err();
        assert_eq!(error.status, Some(409));
        assert!(error.message.contains("document identity"));
        let requests = server.join().unwrap();
        assert_request(
            &requests[0],
            "DELETE /system/connections/net17?expected_revision=7&document_id=old-document HTTP/1.1",
            client.endpoint.port,
            None,
        );
    }
}
