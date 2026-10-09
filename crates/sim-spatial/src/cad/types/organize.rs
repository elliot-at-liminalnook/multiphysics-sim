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
//!   Unlike `move_nodes` it checks nothing (a body as parent, a group into
//!   its own descendant: `Document.move` silently does nothing but the step
//!   is pushed; an unknown parent is a 500), so the outliner moves through
//!   `move_nodes`; this call is the client's for REST parity only.
//!
//! Each is a RoboCAD edit: use a client with [`super::EDIT_TIMEOUT`].

