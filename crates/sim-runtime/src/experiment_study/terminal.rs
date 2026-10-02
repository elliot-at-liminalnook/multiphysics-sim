//! Exact terminal diagnostics use the existing immutable Study content owner.
//! Serialization/hydration happens only in jobs; attachment is an Arc/metadata merge.
use super::{Study, input_content::ContentRef};
use super::refinement::{Capture, Outcome, ResultData};
use crate::controller_refinement::control;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub content_ref: ContentRef, pub kind: String, pub captured_unix_ns: String,
    pub cancelled: bool, pub unapplied: bool, pub unscored: bool, pub failure: Option<String>,
}
pub fn run_complete(r:&control::Run)->bool {
    !r.cancelled && r.failure.is_none() && !r.frames.is_empty() && !r.truth.is_empty()
        && (r.truth.last().unwrap()[0]-r.experiment.duration_s).abs()<=1e-9
        && r.electrical.as_ref().is_none_or(|t|t.samples.last().is_some_and(|v|(v.time_s-r.experiment.duration_s).abs()<=1e-9))
}
fn kind(r:&ResultData)->Option<&'static str>{match r{ResultData::Controller(v) if v.electrical.is_some()=>Some("controller"),ResultData::Prediction(v) if v.electrical.is_some()=>Some("prediction"),ResultData::Electrical(_)=>Some("electrical"),_=>None}}
/// Called before validation can suppress a returned value. Never embeds Capture or receipts.
pub fn capture_outcome(o:&mut Outcome)->Result<(),String>{
    let Ok(value)=&o.result else{return Ok(())};let Some(kind)=kind(value) else{return Ok(())};
    let bytes=serde_json::to_vec(value).map_err(|e|format!("refinement.terminals.content: {e}"))?;
    let content_ref=o.capture.study.input_contents.capture(bytes);
    let failure=match value{ResultData::Controller(r)=>r.failure.clone(),_=>None};
    let unscored=o.cancelled || failure.is_some() || matches!(value,ResultData::Controller(r) if !run_complete(r));
    let r=Reference{content_ref,kind:kind.into(),captured_unix_ns:o.capture.captured_unix_ns.clone(),cancelled:o.cancelled,unapplied:false,unscored,failure};
    o.capture.study.refinement_evidence.terminal_cache.insert(r.content_ref.blake3.clone(),Arc::new(value.clone()));
    o.capture.study.refinement_evidence.terminals.push(r);Ok(())
}
pub fn reference(o:&Outcome)->Option<Reference>{o.capture.study.refinement_evidence.terminals.iter().rev().find(|r|r.captured_unix_ns==o.capture.captured_unix_ns).cloned().map(|mut r|{r.cancelled|=o.cancelled;if o.cancelled || o.result.is_err(){r.unapplied=true;r.unscored=true;}r})}
pub fn retain(s:&mut Study,c:&Capture,cancelled:bool)->Result<(),String>{
    s.input_contents.merge(&c.study.input_contents)?;
    for original in c.study.refinement_evidence.terminals.iter().filter(|r|r.captured_unix_ns==c.captured_unix_ns){
        let mut r=original.clone();r.cancelled|=cancelled;r.unscored=true;r.unapplied=true;
        if let Some(old)=s.refinement_evidence.terminals.iter_mut().find(|old|old.captured_unix_ns==r.captured_unix_ns&&old.content_ref==r.content_ref){old.cancelled|=r.cancelled;old.unapplied=true;old.unscored=true;}else{s.refinement_evidence.terminals.push(r.clone());}
        if let Some(v)=c.study.refinement_evidence.terminal_cache.get(&r.content_ref.blake3){s.refinement_evidence.terminal_cache.insert(r.content_ref.blake3.clone(),v.clone());}
    }Ok(())
}
/// Attach refusal diagnostics without changing exact result bytes.
pub fn note_failure(s:&mut Study,c:&Capture,error:&str){
    for r in s.refinement_evidence.terminals.iter_mut().filter(|r|r.captured_unix_ns==c.captured_unix_ns){r.failure=Some(error.into());r.unapplied=true;r.unscored=true;}
}
pub fn cached<'a>(s:&'a Study,r:&Reference)->Option<&'a ResultData>{s.refinement_evidence.terminal_cache.get(&r.content_ref.blake3).map(Arc::as_ref)}
pub fn cache(s:&mut Study)->Result<(),String>{
    for r in &s.refinement_evidence.terminals{
        let bytes=s.input_contents.resolve(&r.content_ref.blake3)?;
        if bytes.len() as u64!=r.content_ref.byte_length||blake3::hash(bytes).to_hex().to_string()!=r.content_ref.blake3{return Err("refinement.terminals.content_ref: changed exact content".into());}
        super::portable::json_bounds(bytes,super::portable::MAX_OBJECT_BYTES)?;
        let value:ResultData=serde_json::from_slice(bytes).map_err(|e|format!("refinement.terminals.content: {e}"))?;
        if kind(&value)!=Some(r.kind.as_str()){return Err("refinement.terminals.kind: content mismatch".into());}
        s.refinement_evidence.terminal_cache.insert(r.content_ref.blake3.clone(),Arc::new(value));
    }validate(s)
}
pub fn validate(s:&Study)->Result<(),String>{
    for (i,r) in s.refinement_evidence.terminals.iter().enumerate(){
        if s.input_contents.references.get(&r.content_ref.blake3)!=Some(&r.content_ref)||r.captured_unix_ns.parse::<u128>().is_err()||!matches!(r.kind.as_str(),"controller"|"prediction"|"electrical")||((r.cancelled||r.unapplied||r.failure.is_some())&&!r.unscored){return Err(format!("refinement.terminals.{i}: invalid identity or scoring classification"));}
        if let Some(value)=cached(s,r){if kind(value)!=Some(r.kind.as_str()){return Err(format!("refinement.terminals.{i}.kind: mismatch"));}}
    }Ok(())
}
#[cfg(test)]
#[path="terminal_fixtures.rs"]
mod fixtures;
