use super::*;
use serde_json::json;

#[test]
fn record_preserves_wire_format_but_never_deserializes_a_live_handle() {
    let temp = tempfile::tempdir().unwrap();
    let live = identify_existing(temp.path()).unwrap();
    let wire = serde_json::to_value(&live).unwrap();
    assert_eq!(
        wire,
        json!({"canonical_path":live.canonical_path,"object":{"first":live.object.first,"second":live.object.second}})
    );
    let record: ExistingPathIdentityRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(record, live.record());
    assert_eq!(wire, serde_json::to_value(&record).unwrap());
    assert_eq!(
        serde_json::to_vec(&live).unwrap(),
        serde_json::to_vec(&record).unwrap()
    );
    assert_eq!(wire, serde_json::to_value(live.clone()).unwrap());
    assert!(record.same_object(&live));
    assert_eq!(observe_recorded_identity(&record).unwrap(), live);
    let mut forged = wire.clone();
    forged["object"]["_handle"] = json!(42);
    assert!(serde_json::from_value::<ExistingPathIdentityRecord>(forged).is_err());
    let mut forged = wire;
    forged["pinned"] = json!(true);
    assert!(serde_json::from_value::<ExistingPathIdentityRecord>(forged).is_err());
}

#[test]
fn recorded_identity_requires_a_fresh_matching_observation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source");
    fs::create_dir(&path).unwrap();
    let record = identify_existing(&path).unwrap().record();
    fs::rename(&path, temp.path().join("old-source")).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(!record.same_object(&identify_existing(&path).unwrap()));
    assert!(matches!(
        observe_recorded_identity(&record),
        Err(GitError::PathIdentity(_))
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn serialized_records_do_not_retain_pins_and_clones_release_the_last_pin() {
    let temp = tempfile::tempdir().unwrap();
    let live = identify_existing(temp.path()).unwrap();
    let weak = std::sync::Arc::downgrade(&live.object._handle);
    let clone = live.clone();
    let text = serde_json::to_string(&live).unwrap();
    let record: ExistingPathIdentityRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(weak.strong_count(), 2);
    drop(live);
    assert_eq!(weak.strong_count(), 1);
    drop(clone);
    assert!(weak.upgrade().is_none());
    // Re-observation opens a different handle, not a resurrection from JSON.
    let fresh = observe_recorded_identity(&record).unwrap();
    assert!(record.same_object(&fresh));
    assert!(weak.upgrade().is_none());
}
