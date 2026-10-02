//! Source changes use the one authoritative CAD edit/undo owner.
use super::*;
pub(crate) fn source_edit(a: &ExperimentsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let prepared = (|| -> Result<(Stamp, Value, Option<String>), String> {
        if cx.review.active || cx.motion.active {
            return Err("Return to CAD before changing a reviewed or previewed source".into());
        }
        let st = &*cx.experiments;
        let d = form::guard(cx.doc, st)?;
        if st.busy() {
            return Err("experiments.source: previous outcome pending".into());
        }
        if a.draft_index.is_some_and(|i| Some(i) != st.current)
            || a.draft_sequence.is_some_and(|s| s != d.stamp.sequence)
        {
            return Err("experiments.draft: editor changed; original retained".into());
        }
        let payload = match a.op {
            ExperimentsOp::Batch => serde_json::to_value(form::candidate_request(cx.doc, st)?)
                .map_err(|e| e.to_string())?,
            ExperimentsOp::Script => {
                if d.get("script_path").is_empty() {
                    return Err("experiments.script_path: repository model script required".into());
                }
                let params: Value = serde_json::from_str(d.get("script_params"))
                    .map_err(|e| format!("experiments.script_params: {e}"))?;
                if !params.is_object() {
                    return Err("experiments.script_params: expected object".into());
                }
                json!({"path":d.get("script_path"),"params":params,"document_id":d.stamp.document_id,"expected_revision":d.stamp.revision,"label":d.get("label"),"changes":true})
            }
            _ => Value::Null,
        };
        let id = if a.op == ExperimentsOp::CandidateAccept {
            Some(st.candidate.clone().ok_or("Select a candidate first")?)
        } else if a.op == ExperimentsOp::RestoreGraph {
            Some(st.selected.clone().ok_or("Select a run first")?)
        } else {
            None
        };
        Ok((d.stamp.clone(), payload, id))
    })();
    let (stamp, payload, id) = match prepared {
        Ok(v) => v,
        Err(e) => {
            cx.experiments.error = Some(e.clone());
            cx.experiments.touch();
            return Outcome::Done(Err(e));
        }
    };
    let op = a.op;
    crate::cad::actions::edit_at(
        cx.doc,
        call,
        Some(stamp.revision),
        "Experiment source operation · authoritative undo".into(),
        move |client| {
            let result = match op {
                ExperimentsOp::CandidateAccept => serde_json::to_value(client.accept_candidate(
                    id.as_deref().unwrap_or(""),
                    &stamp.document_id,
                    stamp.revision,
                )?)
                .unwrap_or(Value::Null),
                ExperimentsOp::RestoreGraph => client.restore_experiment_inputs(
                    id.as_deref().unwrap_or(""),
                    &stamp.document_id,
                    stamp.revision,
                )?,
                ExperimentsOp::Script => client.model_script(&payload)?,
                ExperimentsOp::Batch => {
                    let request = serde_json::from_value(payload)
                        .expect("validated CandidateRequest serialized locally");
                    client.batch(&request)?
                }
                _ => unreachable!("only source operations routed here"),
            };
            Ok(crate::cad::document::EditDone{message:"Source updated by RoboCAD; Undo restores the previous source. Original draft retained.".into(),result})
        },
    )
}
