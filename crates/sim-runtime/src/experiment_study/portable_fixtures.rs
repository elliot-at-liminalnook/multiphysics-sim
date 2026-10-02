//! T53 fixtures written for source review, deliberately UNEXECUTED.
use super::{Study, portable::*, input_content_fixtures::{study,directory,envelope}, refinement};
use crate::{publication::{Hooks,Stage}, controller_refinement::{control,recording, power}};
use std::{path::{Path,PathBuf},io,sync::atomic::AtomicBool};
fn reopened(bytes:&[u8])->Result<Study,String>{Study::load_bytes(Path::new("/nonexistent-relocated-source/study.simstudy"),bytes)}
fn framed(manifest:&[u8],count:u32,objects:&[u8])->Vec<u8>{
    let mut out=MAGIC.to_vec();out.extend_from_slice(&VERSION.to_le_bytes());out.extend_from_slice(&(manifest.len() as u64).to_le_bytes());out.extend_from_slice(&count.to_le_bytes());out.extend_from_slice(manifest);out.extend_from_slice(objects);out
}
fn objects_at(bytes:&[u8])->usize{24+u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize}
fn recording()->recording::Recording {
    let experiment=control::Experiment::default();let mut session=control::ControllerSession::new(experiment.clone()).unwrap();
    let mut frames=vec![];
    for time in [0.01,0.03] {
        let control=session.tick(time+0.005,control::Feedback{electrical:None,observed_s:time,request_s:time-0.001,completion_s:time+0.002,received_s:time+0.002,encoder_rad:0.1}).unwrap();
        let drive_counts=(control.applied_duty*1000.).round() as i16;
        frames.push(recording::MeasuredFrame{control,command_request_s:time+0.006,command_receipt_s:time+0.01,drive_counts,voltage_v:12.,temperature_c:25.,current_raw_uncalibrated:0});
    }
    recording::Recording{version:1,experiment,runtime:crate::physics_context::RuntimeIdentity::current(),frames,stop_request_s:0.05,stop_receipt_s:0.06,completed:true,failure:None,stop_verified:true,initial_registers:serde_json::json!({"future":"opaque acquisition"}),transactions_origin_host_s:0.,timing_evidence:"Synthetic source-review observation windows".into(),source_hashes:std::collections::BTreeMap::from([("fixture".into(),"a".repeat(64))])}
}
#[test]
fn relocated_artifact_recovers_recording_electrical_comparison_and_cancelled_terminal(){
    let mut s=study();let root=directory();let json=root.join("old.json");s.save_new(&json).unwrap();s=Study::load(&json).unwrap();
    s.notes="Edited after opening existing JSON; retained physical identities unchanged".into();
    refinement::electrical::command(&mut s,refinement::electrical::Command::SetSource(Some(power::Setup{source_component:"electrical.voltage_source".into(),source_parameters:std::collections::BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Synthetic ideal source fixture".into(),limits:Default::default()}))).unwrap();
    let recording=recording();let recording_bytes=serde_json::to_vec(&recording).unwrap();let recording_hash=recording.fingerprint();
    let raw_ref=envelope(&mut s,recording_bytes.clone());
    refinement::apply(&mut s,refinement::Command::ImportRecording{recording}).unwrap();
    s.limits=Some(crate::experiment_comparison::Limits{rmse:1.,final_abs_error:1.});
    let capture=refinement::prepare(&mut s,refinement::Operation::PredictRecording{recording_hash:recording_hash.clone(),purpose:recording::Purpose::RecordedCommandReplay}).unwrap();
    let outcome=refinement::execute(capture,&AtomicBool::new(false),|_,_|{}).unwrap();
    let exact=serde_json::to_vec(outcome.result.as_ref().unwrap()).unwrap();
    let mut cancelled=outcome.clone();cancelled.cancelled=true;refinement::apply_outcome(&mut s,cancelled);
    assert!(s.refinement.predictions.is_empty());
    refinement::apply_outcome(&mut s,outcome);
    assert_eq!(s.refinement.predictions.len(),1);
    let capture=refinement::prepare(&mut s,refinement::Operation::Electrical(refinement::electrical::Operation::CompareServoVoltage{recording_hash:recording_hash.clone(),prediction:0})).unwrap();
    let outcome=refinement::execute(capture,&AtomicBool::new(false),|_,_|{}).unwrap();refinement::apply_outcome(&mut s,outcome);
    assert_eq!(s.refinement.electrical_comparisons.len(),1);
    let additional=study().portable_bytes().unwrap();let additional_ref=envelope(&mut s,additional.clone());
    s.retained_fields.insert("native_unsubmitted_forms".into(),serde_json::json!({"voltage":"rejected raw text","future":{"opaque":[1,2,3]}}));
    let portable=root.join("published.simstudy");s.save_portable_new(&portable).unwrap();
    // Copy only the one published file. Destination has no sibling companion directory.
    let relocation=directory();let target=relocation.join("moved.simstudy");std::fs::copy(&portable,&target).unwrap();
    assert!(!relocation.join(".study-inputs").exists());let loaded=Study::load(&target).unwrap();
    assert_eq!(serde_json::to_value(&loaded).unwrap(),serde_json::to_value(&s).unwrap());
    assert_eq!(loaded.refinement.recordings[0].fingerprint(),recording_hash);
    assert_eq!(loaded.input_contents.resolve(&raw_ref.blake3).unwrap(),recording_bytes);
    assert_eq!(loaded.input_contents.resolve(&additional_ref.blake3).unwrap(),additional);
    let terminal=loaded.refinement_evidence.terminals.iter().find(|r|r.cancelled).unwrap();
    assert!(terminal.unapplied&&terminal.unscored);assert_eq!(loaded.input_contents.resolve(&terminal.content_ref.blake3).unwrap(),exact);
    assert!(super::terminal::cached(&loaded,terminal).is_some());
    for (hash,bytes) in &s.input_contents.contents{assert_eq!(loaded.input_contents.resolve(hash).unwrap(),bytes.as_slice());}
}
#[test]
fn opaque_legacy_manifest_and_exact_rejected_binary_bytes_survive_without_reinterpretation(){
    let mut s=study();s.retained_fields.insert("future_payload".into(),serde_json::json!({"version":999,"unknown":["keep",{"nested":true}]}));
    s.refinement.retained_fields.insert("future_workspace".into(),serde_json::json!({"raw":"opaque"}));
    let reference=envelope(&mut s,vec![0xff,0,1,b'{']);let duplicate=s.input_contents.capture(vec![0xff,0,1,b'{']);assert_eq!(reference,duplicate);
    let bytes=s.portable_bytes().unwrap();assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()),1);
    let loaded=reopened(&bytes).unwrap();assert_eq!(loaded.retained_fields,s.retained_fields);assert_eq!(loaded.refinement.retained_fields,s.refinement.retained_fields);
    assert_eq!(loaded.input_contents.resolve(&reference.blake3).unwrap(),[0xff,0,1,b'{']);
    assert_eq!(Study::manifest_value(&bytes).unwrap(),serde_json::to_value(&s).unwrap());
}
#[test]
fn malformed_unsupported_truncated_trailing_and_oversized_headers_fail_before_attachment(){
    let mut s=study();envelope(&mut s,b"content".to_vec());let bytes=s.portable_bytes().unwrap();
    let mut unsupported=bytes.clone();unsupported[8..12].copy_from_slice(&999u32.to_le_bytes());assert!(reopened(&unsupported).unwrap_err().contains("study.portable.version"));
    for end in [8,11,19,23,objects_at(&bytes)-1,bytes.len()-1]{assert!(reopened(&bytes[..end]).unwrap_err().contains("study.portable.truncated"));}
    let mut trailing=bytes.clone();trailing.push(0);assert!(reopened(&trailing).unwrap_err().contains("study.portable.trailing"));
    let mut oversized=bytes[..24].to_vec();oversized[12..20].copy_from_slice(&((MAX_MANIFEST_BYTES as u64)+1).to_le_bytes());assert!(reopened(&oversized).unwrap_err().contains("manifest_size"));
    let mut count=bytes[..24].to_vec();count[20..24].copy_from_slice(&((MAX_OBJECTS as u32)+1).to_le_bytes());assert!(reopened(&count).unwrap_err().contains("object_count"));
    assert!(reopened(&framed(b"{ invalid",0,&[])).unwrap_err().contains("study.portable.manifest"));
    let deep=format!("{}0{}","[".repeat(MAX_JSON_DEPTH+1),"]".repeat(MAX_JSON_DEPTH+1));assert!(reopened(&framed(deep.as_bytes(),0,&[])).unwrap_err().contains("json_depth"));
    // A sparse oversized file exercises the reader without allocating its advertised size.
    let root=directory();let path=root.join("oversized.simstudy");let f=std::fs::File::create(&path).unwrap();f.set_len((MAX_ARTIFACT_BYTES as u64)+1).unwrap();assert!(Study::read_source_bytes(&path).unwrap_err().contains("study.portable.size"));
}
#[test]
fn missing_duplicate_conflicting_hash_and_unreferenced_objects_are_named_errors(){
    let mut s=study();envelope(&mut s,b"first".to_vec());envelope(&mut s,b"second".to_vec());let bytes=s.portable_bytes().unwrap();let at=objects_at(&bytes);
    let first_len=u64::from_le_bytes(bytes[at+64..at+72].try_into().unwrap()) as usize;let first=&bytes[at..at+72+first_len];
    let mut duplicate=bytes[..at].to_vec();duplicate.extend_from_slice(first);duplicate.extend_from_slice(first);assert!(reopened(&duplicate).unwrap_err().contains("duplicate_object"));
    let mut missing=bytes[..at].to_vec();missing[20..24].copy_from_slice(&0u32.to_le_bytes());assert!(reopened(&missing).unwrap_err().contains("missing_object"));
    let mut conflict=bytes.clone();conflict[at+64..at+72].copy_from_slice(&999u64.to_le_bytes());assert!(reopened(&conflict).unwrap_err().contains("conflicting_object"));
    let mut corrupt=bytes.clone();corrupt[at+72]^=1;assert!(reopened(&corrupt).unwrap_err().contains("hash_identity"));
    let mut unknown=bytes.clone();unknown[at..at+64].copy_from_slice("0".repeat(64).as_bytes());assert!(reopened(&unknown).unwrap_err().contains("membership"));
    let mut invalid=bytes.clone();invalid[at]=b'Z';assert!(reopened(&invalid).unwrap_err().contains("identity"));
    let manifest=&bytes[24..at];let mut value:serde_json::Value=serde_json::from_slice(manifest).unwrap();let hash=s.input_contents.references.keys().next().unwrap();value["input_contents"]["references"][hash]["byte_length"]=serde_json::json!((MAX_OBJECT_BYTES as u64)+1);
    assert!(reopened(&framed(&serde_json::to_vec(&value).unwrap(),2,&bytes[at..])).unwrap_err().contains("object_size"));
    // Duplicate root and immutable-reference declarations may not be silently overwritten.
    let duplicate_manifest=format!("{{\"version\":1,{}",std::str::from_utf8(manifest).unwrap().trim_start_matches('{'));
    assert!(reopened(&framed(duplicate_manifest.as_bytes(),2,&bytes[at..])).unwrap_err().contains("duplicate manifest key"));
    let mut bad_store=s.clone();bad_store.input_contents.contents.clear();assert!(bad_store.portable_bytes().unwrap_err().contains("missing recoverable content"));
}
struct Fail{target:PathBuf,stage:Stage}
impl Hooks for Fail{fn before(&self,stage:Stage,destination:&Path,_:&Path)->io::Result<()>{if self.target==destination&&stage==self.stage{Err(io::Error::other("Portable fixture failure"))}else{Ok(())}}}
#[test]
fn portable_publication_conflict_failures_and_visible_recovery_preserve_exact_source(){
    for stage in [Stage::CreateTemp,Stage::Write,Stage::FileSync,Stage::Publish,Stage::DirectorySync]{
        let root=directory();let path=root.join("study.simstudy");let mut s=study();let reference=envelope(&mut s,b"exact retained evidence".to_vec());let exact=s.portable_bytes().unwrap();
        let error=s.save_portable_new_with(&path,&Fail{target:path.clone(),stage}).unwrap_err();assert!(!error.is_empty());assert!(!root.join(".study-inputs").exists());assert_eq!(s.input_contents.resolve(&reference.blake3).unwrap(),b"exact retained evidence");
        assert_eq!(path.exists(),stage==Stage::DirectorySync);
        if path.exists(){assert_eq!(std::fs::read(&path).unwrap(),exact);assert_eq!(Study::load(&path).unwrap().input_contents,s.input_contents);}
        let retry=root.join("retry.simstudy");s.save_portable_new(&retry).unwrap();assert_eq!(std::fs::read(&retry).unwrap(),exact);
        assert!(s.save_portable_new(&retry).is_err());assert_eq!(std::fs::read(&retry).unwrap(),exact);
    }
}
#[test]
fn portable_encoder_stops_deep_opaque_serialization_and_aggregate_reference_growth(){
    let mut s=study();let mut v=serde_json::json!("deep");for _ in 0..MAX_JSON_DEPTH+1{v=serde_json::json!([v]);}s.retained_fields.insert("opaque".into(),v);
    assert!(s.portable_bytes().unwrap_err().contains("json_depth"));
    let mut s=study();for index in 0..5{let hash=format!("{index:064x}");s.input_contents.references.insert(hash.clone(),super::input_content::ContentRef{version:1,blake3:hash,byte_length:MAX_OBJECT_BYTES as u64});}
    assert!(s.portable_bytes().unwrap_err().contains("aggregate_size"));
}
