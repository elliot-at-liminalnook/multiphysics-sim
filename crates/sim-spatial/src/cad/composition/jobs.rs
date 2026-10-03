//! Guarded reads/layout on jobs; source drafts remain owned by the parent resource.
use super::*;
pub(super) fn imports_current(doc: &CadDocument, imports: &ImportedSnapshot) -> bool {
    let id = doc.doc.as_ref().and_then(|d| d.document_id.as_deref());
    id.is_some()
        && imports.guard_document_id.as_deref() == id
        && imports.guard_revision == Some(doc.shown_revision())
}
pub(super) fn tick(doc: Option<ResMut<CadDocument>>, mut st: ResMut<CadCompositionState>) {
    let Some(mut doc) = doc else { return };
    let key = (doc.generation, doc.shown_revision());
    if st.snapshot_key.is_some_and(|k| k.0 != key.0)
        || st.snapshot.as_ref().is_some_and(|s| {
            s.graph.document_id.as_deref()
                != doc.doc.as_ref().and_then(|d| d.document_id.as_deref())
        })
    {
        st.snapshot = None;
        st.snapshot_key = None;
        st.imports = None;
        // Retain stamped intent; its next consumer refuses the changed document.
        st.failed = None;
    }
    if let Some((generation, job)) = &st.imported_job {
        if let Some(result) = job.poll() {
            let generation = *generation;
            st.imported_job = None;
            if generation == doc.generation {
                match result {
                    Ok(imported) if imports_current(&doc,&imported) && imported.run_id==st.check_id => {
                        st.imports = Some(imported);
                        st.snapshot = None;
                        st.read=None;
                        st.snapshot_key=None;
                        st.failed = None
                    }
                    Ok(_) => st.error=Some("composition.imports: document or revision changed during read; read the completed check again".into()),
                    Err(e) => st.error = Some(e),
                };
                st.revision += 1;
                doc.touch();
            }
        }
    }
    if let Some((k, job)) = &st.read {
        if *k != key {
            st.read = None;
        } else if let Some(result) = job.poll() {
            st.read = None;
            match result {
                Ok(mut snapshot)
                    if snapshot.graph.revision == key.1
                        && snapshot.check_id == st.check_id
                        && snapshot.graph.document_id.is_some()
                        && snapshot.graph.document_id.as_deref()
                            == doc.doc.as_ref().and_then(|d| d.document_id.as_deref()) =>
                {
                    if snapshot
                        .imports
                        .as_ref()
                        .is_some_and(|i| !imports_current(&doc, i))
                    {
                        if let Some(imported) = snapshot.imports.as_mut() {
                            imported.metadata_stale = true;
                        }
                        snapshot.presentation = None;
                        snapshot.presentation_error=Some("composition.imports: document or revision changed during read; repair source or refresh check metadata".into());
                    }
                    st.imports = snapshot.imports.clone();
                    st.snapshot = Some(snapshot);
                    st.snapshot_key = Some(key);
                    st.error = st
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.presentation_error.clone())
                }
                // The service answered for this revision and check but named
                // no document: never shown as this document's graph, and not
                // read again every frame until the source key changes.
                Ok(snapshot)
                    if snapshot.graph.revision == key.1
                        && snapshot.check_id == st.check_id
                        && snapshot.graph.document_id.is_none() =>
                {
                    st.failed = Some(key);
                    st.error = Some("composition.document_id: RoboCAD's /system answer names no document, so it is not shown as this document's graph; it is read again when the document or revision changes".into());
                }
                Ok(_) => st.failed = None,
                Err(e) => {
                    st.failed = Some(key);
                    st.error = Some(e)
                }
            };
            st.revision += 1;
            doc.touch();
        }
    }
    if st.read.is_some()
        || st.failed == Some(key)
        || st.snapshot_key == Some(key) && st.snapshot.is_some()
    {
        return;
    }
    let Some(client) = doc.client.clone() else {
        return;
    };
    let check = (!st.check_id.is_empty()).then(|| st.check_id.clone());
    let focus = st.focus.clone();
    st.read = Some((
        key,
        Job::spawn(
            Pool::Dedicated,
            key.0,
            "CAD composition catalogue and layout",
            move |ctx| {
                let graph = client.composition().map_err(|e| e.to_string())?;
                let mut errors = Vec::new();
                let types = client.system_types().unwrap_or_else(|e| {
                    errors.push(e.to_string());
                    Vec::new()
                });
                let recipes = client.geometry_recipes().unwrap_or_else(|e| {
                    errors.push(e.to_string());
                    BTreeMap::new()
                });
                let imports = check
                    .as_deref()
                    .map(|id| client.system_imports(id))
                    .transpose()
                    .unwrap_or_else(|e| {
                        errors.push(e.to_string());
                        None
                    });
                let adapted = adapter::adapt(
                    &graph.graph,
                    graph.revision,
                    &types,
                    imports
                        .as_ref()
                        .filter(|i| !i.metadata_stale)
                        .map(|i| i.imported.as_slice())
                        .unwrap_or(&[]),
                    &recipes,
                );
                let focus = focus.map(sim_diagram::projection::NodeSource::Component);
                let (mut presentation, mut presentation_error) = match adapted {
                    Ok(composition) => (
                        Some(
                            sim_diagram::composition::present(
                                &composition.description,
                                &BTreeSet::new(),
                                focus.as_ref(),
                                ctx.cancel_flag(),
                            )
                            .ok_or("composition layout cancelled")?,
                        ),
                        None,
                    ),
                    Err(error) => (None, Some(error)),
                };
                if !errors.is_empty() {
                    presentation = None;
                    presentation_error = Some(errors.join("; "));
                }
                Ok(Snapshot {
                    graph,
                    types,
                    recipes,
                    presentation,
                    presentation_error,
                    check_id: check.unwrap_or_default(),
                    imports,
                })
            },
        ),
    ));
}
