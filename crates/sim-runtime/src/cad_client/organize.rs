//! The outliner's organization edits as typed `Ops` calls and the
//! `PATCH /nodes/{id}` move (commands.py:339-400, api.py `patch`):
//!
//! - `group(ids, name)`: a new group (a unique name from `name`, under the
//!   first root's parent) holding the selection's roots; one undo step
//!   "Group"; `result` is the group's id. Refused (422) for component
//!   members.
//! - `move_nodes(ids, new_parent, index)`: the selection's roots under
//!   `new_parent` (a group; `None` is the top level), at `index` onwards;
//!   one undo step "Move in outliner". Refused for a non-group target and
//!   for a group into itself or its descendants.
//! - `set_active_group(group_id)`: RoboCAD's active group (`None` clears
//!   it); not an undo step (RoboCAD only notifies).
//! - `set_locked(ids, locked)`: one undo step "Lock" / "Unlock".
//! - `PATCH /nodes/{id} {"parent", "index"}`: one node moved
//!   (`Ops.move_node`, one undo step "Move in outliner"); answers the node.
//!
//! Each is a RoboCAD edit: use a client with [`super::EDIT_TIMEOUT`].
use super::{CadClient, CadError, NodeDetail, OpResult};
use serde_json::{Map, Value, json};

impl CadClient {
    /// `group(ids, name)`: `result` is the new group's id.
    pub fn group(&self, ids: &[String], name: &str) -> Result<OpResult, CadError> {
        self.op("group", &[json!(ids), json!(name)], &Map::new())
    }
    /// `move_nodes(ids, new_parent, index)`.
    pub fn move_nodes(&self, ids: &[String], new_parent: Option<&str>, index: Option<i64>) -> Result<OpResult, CadError> {
        self.op("move_nodes", &[json!(ids), json!(new_parent), json!(index)], &Map::new())
    }
    /// `set_active_group(group_id)` (`None` clears it).
    pub fn set_active_group(&self, group_id: Option<&str>) -> Result<OpResult, CadError> {
        self.op("set_active_group", &[json!(group_id)], &Map::new())
    }
    /// `set_locked(ids, locked)`.
    pub fn set_locked(&self, ids: &[String], locked: bool) -> Result<OpResult, CadError> {
        self.op("set_locked", &[json!(ids), json!(locked)], &Map::new())
    }
    /// `PATCH /nodes/{id} {"parent": parent, "index": index}` (`parent`
    /// `None`: the top level; `index` `None`: the end).
    pub fn move_node(&self, id: &str, parent: Option<&str>, index: Option<i64>) -> Result<NodeDetail, CadError> {
        let mut attrs = Map::new();
        attrs.insert("parent".into(), parent.map_or(Value::Null, |p| Value::String(p.to_string())));
        attrs.insert("index".into(), index.map_or(Value::Null, Value::from));
        self.patch(id, &attrs)
    }
}
