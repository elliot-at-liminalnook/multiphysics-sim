//! Rendered CAD controls retain their source; activation cannot borrow a new document.
use bevy::prelude::*;
use crate::app::{ModeScope, ViewerMode};
use crate::ui_kit::activation::{Activated, ActivationSet, Ordinary};
use super::document::CadDocument;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct SourceStamp {
    generation: u64,
    document: Option<(Option<String>, u64)>,
    form: Option<Option<(String, u64, u64)>>,
    material: Option<(u64, bool)>,
    results: Option<(u64, bool)>,
    tree: Option<(u64, bool)>,
    files: Option<(u64, bool)>,
    numeric: Option<String>,
    rename: Option<Option<(String, u64)>>,
}
impl SourceStamp {
    fn of(doc: &CadDocument) -> Self {
        Self { generation: doc.generation, document: doc.doc_key.clone(), form: None, material: None, results: None, tree: None, files: None, numeric: None, rename: None }
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(PostUpdate, stamp.after(bevy::ui::UiSystems::Layout).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, tree_keyboard.in_set(crate::app::InputSet::Window).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, refuse.in_set(ActivationSet::Validate).run_if(in_state(ViewerMode::Cad)));
}

// Only newly rendered CAD-scoped controls are stamped. Updating an old entity's
// stamp would silently authorize a button drawn from a replaced source.
pub(in crate::cad) fn stamp(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    controls: Query<(Entity, Option<&crate::ui_kit::activation::InputIdentity>, Has<super::numeric::FieldButton>, Option<&super::tree::rows::TreeField>), (With<Ordinary>, Without<SourceStamp>)>,
    files: Option<Res<super::files::CadFiles>>,
    ancestry: Query<(Option<&ChildOf>, Option<&DespawnOnExit<ModeScope>>, Has<super::surfaces::form::FormRoot>, Has<crate::app::Persistent>, Has<Node>, Has<super::materials::panel::FormRoot>, Has<super::results::forms::ResultsFormRoot>, Has<super::files::form::FileFormRoot>, Has<super::tree::popup::TreeDialogRoot>)>,
) {
    let Some(doc) = doc else { return };
    for (entity, identity, numeric, tree_field) in &controls {
        let mut source = SourceStamp::of(&doc);
        if numeric { source.numeric = Some(doc.tool_state.numeric.key.clone()); }
        if tree_field.is_some_and(|f| *f == super::tree::rows::TreeField::Rename) {
            source.rename = Some(doc.tree.rename.as_ref().map(|r| (r.id.clone(), r.began)));
        }
        let mut next = Some(entity);
        while let Some(at) = next {
            let Ok((parent, scope, form, persistent, node, material, results, file, tree)) = ancestry.get(at) else { break };
            if persistent { break; }
            if form { source.form = Some(doc.ops.form.as_ref().map(|f| (f.op.to_string(), f.began, doc.ops.form_sequence))); }
            if material { source.material = Some((doc.materials.form_sequence, doc.materials.form.is_some())); }
            if results { source.results = Some((doc.results.form_sequence, doc.results.form.is_some())); }
            if tree { source.tree = Some((doc.tree.form_sequence, doc.tree.dialog.is_some())); }
            if file { source.files = files.as_deref().map(|f| (f.form_sequence, f.form.is_some())); }
            if scope.is_some_and(|s| s.0 == ModeScope::Cad) || (parent.is_none() && node) {
                if let Some(identity) = identity {
                    // Editor rebind includes the physical/form lifetime, not just
                    // FormHit's positional index in a new form.
                    commands.entity(entity).insert(crate::ui_kit::activation::InputIdentity(format!("{}|{source:?}", identity.0)));
                }
                commands.entity(entity).insert(source.clone());
                break;
            }
            next = parent.map(|p| p.parent());
        }
    }
}

// Deferred removal is flushed by the public Validate -> Window edge.
pub(in crate::cad) fn refuse(mut commands: Commands, doc: Option<Res<CadDocument>>, files: Option<Res<super::files::CadFiles>>, controls: Query<(Entity, Option<&SourceStamp>), (With<Activated>, With<Ordinary>)>, ancestry: Query<(Option<&ChildOf>, Has<crate::app::Persistent>)>) {
    for (entity, source) in &controls {
        let Some(source) = source else {
            let mut next = Some(entity);
            let mut persistent = false;
            while let Some(at) = next {
                let Ok((parent, retained)) = ancestry.get(at) else { break };
                if retained { persistent = true; break; }
                next = parent.map(|p| p.parent());
            }
            if !persistent { commands.entity(entity).remove::<Activated>(); }
            continue;
        };
        if !doc.as_deref().is_some_and(|doc| current(source, doc)) || !files_current(source, files.as_deref()) {
            commands.entity(entity).remove::<Activated>();
        }
    }
}

/// UI occurrence metadata. REST identities and validation remain unchanged.
pub(in crate::cad) fn guard(doc: &CadDocument, action: super::actions::CadAction) -> super::actions::CadAction {
    let mut source = SourceStamp::of(doc);
    if matches!(action, super::actions::CadAction::CadFormSet { .. } | super::actions::CadAction::CadFormSubmit | super::actions::CadAction::CadFormCancel) {
        source.form = Some(doc.ops.form.as_ref().map(|f| (f.op.to_string(), f.began, doc.ops.form_sequence)));
    }
    match &action {
        super::actions::CadAction::CadMaterials(a) if matches!(a.op, super::materials::MaterialsOp::FormSet | super::materials::MaterialsOp::FormSubmit | super::materials::MaterialsOp::FormCancel) => source.material = Some((doc.materials.form_sequence, doc.materials.form.is_some())),
        super::actions::CadAction::CadResults(a) if matches!(a.op, super::results::ResultsOp::FormSet | super::results::ResultsOp::FormSubmit | super::results::ResultsOp::FormCancel) => source.results = Some((doc.results.form_sequence, doc.results.form.is_some())),
        super::actions::CadAction::CadTree(a) if matches!(a.op, super::tree::TreeOp::Group | super::tree::TreeOp::GroupDialog) && doc.tree.dialog.is_some() => source.tree = Some((doc.tree.form_sequence, true)),
        _ => {}
    }
    super::actions::CadAction::Captured { source, action: Box::new(action) }
}
pub(in crate::cad) fn current(source: &SourceStamp, doc: &CadDocument) -> bool {
    let mut now = SourceStamp::of(doc);
    if source.form.is_some() { now.form = Some(doc.ops.form.as_ref().map(|f| (f.op.to_string(), f.began, doc.ops.form_sequence))); }
    if source.material.is_some() { now.material = Some((doc.materials.form_sequence, doc.materials.form.is_some())); }
    if source.results.is_some() { now.results = Some((doc.results.form_sequence, doc.results.form.is_some())); }
    if source.tree.is_some() { now.tree = Some((doc.tree.form_sequence, doc.tree.dialog.is_some())); }
    if source.numeric.is_some() { now.numeric = Some(doc.tool_state.numeric.key.clone()); }
    if source.rename.is_some() { now.rename = Some(doc.tree.rename.as_ref().map(|r| (r.id.clone(), r.began))); }
    now.files = source.files;
    &now == source
}

pub(in crate::cad) fn files_current(source: &SourceStamp, files: Option<&super::files::CadFiles>) -> bool {
    source.files.is_none() || source.files == files.map(|f| (f.form_sequence, f.form.is_some()))
}
pub(in crate::cad) fn guard_files(doc: &CadDocument, files: &super::files::CadFiles, action: super::actions::CadAction) -> super::actions::CadAction {
    let mut source = SourceStamp::of(doc);
    source.files = Some((files.form_sequence, files.form.is_some()));
    super::actions::CadAction::Captured { source, action: Box::new(action) }
}
pub(in crate::cad) fn guard_results(doc: &CadDocument, action: super::actions::CadAction) -> super::actions::CadAction {
    let mut source = SourceStamp::of(doc);
    source.results = Some((doc.results.form_sequence, doc.results.form.is_some()));
    super::actions::CadAction::Captured { source, action: Box::new(action) }
}
pub(in crate::cad) fn tree_keyboard(doc: Option<Res<CadDocument>>, rows: Query<&super::tree::rows::TreeRowId, With<Activated>>, mut out: MessageWriter<crate::app::actions::Act<super::actions::CadAction>>) {
    let Some(doc) = doc else { return };
    for row in &rows {
        if doc.has_node(&row.id) {
            let action = super::tree::select_action(&doc, &row.id, false, false);
            out.write(crate::app::actions::Act::ui(guard(&doc, action)));
        }
    }
}
