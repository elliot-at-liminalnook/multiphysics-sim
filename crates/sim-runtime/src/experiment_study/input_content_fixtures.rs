//! Source-reading fixtures only: no fixture was executed for this repair.
use super::*;
pub(super) fn study() -> Study {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Study::new(crate::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"), &root).unwrap()).unwrap()
}
pub(super) fn directory() -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("study-input-fixture-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&path).unwrap(); path
}
pub(super) fn envelope(s: &mut Study, bytes: Vec<u8>) -> input_content::ContentRef {
    let reference = s.input_contents.capture(bytes);
    s.retained_fields.entry("native_offline_job_receipts".into()).or_insert_with(|| serde_json::json!([])).as_array_mut().unwrap().push(serde_json::json!({"launch":{"additional_input":{"content_ref":reference,"error":"Retained rejection; never scored"}}}));
    reference
}
#[test]
fn repeated_generation_receipts_reference_content_once_and_reopen() {
    let dir = directory(); let mut current = study();
    for generation in 0..3 {
        let path = dir.join(format!("generation-{generation}.json"));
        let source = serde_json::to_vec_pretty(&current).unwrap();
        let reference = envelope(&mut current, source.clone());
        let duplicate = current.input_contents.capture(source.clone());
        assert_eq!(reference, duplicate);
        current.save_new(&path).unwrap();
        let manifest = std::fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
        for row in value["native_offline_job_receipts"].as_array().unwrap() {
            assert!(row["launch"]["additional_input"].get("raw").is_none());
        }
        assert!(value["input_contents"].get("contents").is_none());
        current = Study::load_bytes(&path, &manifest).unwrap();
        assert_eq!(current.input_contents.resolve(&reference.blake3).unwrap(), source.as_slice());
        assert_eq!(current.input_contents.references.len(), generation + 1);
    }
}
#[test]
fn rejected_bytes_failed_manifest_and_cancelled_receipt_remain_recoverable() {
    let dir = directory(); let mut s = study();
    for bytes in [b"{ malformed".to_vec(), br#"{"version":999}"#.to_vec(), vec![0xff, 0, 1]] {
        let reference = envelope(&mut s, bytes.clone());
        assert_eq!(s.input_contents.resolve(&reference.blake3).unwrap(), bytes.as_slice());
    }
    s.retained_fields.insert("cancelled_capture".into(), serde_json::json!({"requested":true,"execution_observed":true}));
    let path = dir.join("occupied.json"); std::fs::write(&path, b"existing user evidence").unwrap();
    assert!(s.save_new(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing user evidence");
    assert_eq!(s.input_contents.contents.len(), 3);
    let retry = dir.join("retry.json"); s.save_new(&retry).unwrap();
    let loaded = Study::load(&retry).unwrap();
    for (hash, bytes) in &s.input_contents.contents { assert_eq!(loaded.input_contents.resolve(hash).unwrap(), bytes.as_slice()); }
}
#[test]
fn missing_corrupt_and_missing_manifest_reference_have_named_failures() {
    let dir = directory(); let mut s = study(); let reference = envelope(&mut s, b"invalid source".to_vec());
    let path = dir.join("source.json"); let bytes = serde_json::to_vec(&s).unwrap();
    assert!(Study::load_bytes(&path, &bytes).unwrap_err().contains("missing/unreadable artifact"));
    let artifact = input_content::Store::artifact_path(&path, &reference.blake3);
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap(); std::fs::write(&artifact, b"corrupt").unwrap();
    assert!(Study::load_bytes(&path, &bytes).unwrap_err().contains("corrupt artifact"));
    assert!(s.save_new(&path).unwrap_err().contains("corrupt artifact"));
    assert_eq!(std::fs::read(&artifact).unwrap(), b"corrupt");
    s.input_contents.references.clear();
    assert!(s.validate().unwrap_err().contains("additional_input.content_ref"));
}
#[test]
fn legacy_raw_and_opaque_payloads_remain_unmodified() {
    let dir = directory(); let mut s = study();
    s.retained_fields.insert("native_offline_job_receipts".into(), serde_json::json!([{"launch":{"additional_input":{"raw":"old full source","parsed_provenance":{"opaque":true}}}}]));
    s.retained_fields.insert("deferred_future".into(), serde_json::json!({"bytes":[1,2,3]}));
    let path = dir.join("legacy.json"); s.save_new(&path).unwrap();
    let loaded = Study::load(&path).unwrap();
    assert_eq!(s.retained_fields, loaded.retained_fields);
    assert!(loaded.input_contents.references.is_empty());
}
