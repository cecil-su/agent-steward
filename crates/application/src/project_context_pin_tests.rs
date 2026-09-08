use super::*;

#[test]
fn file_manifest_keeps_the_original_inode_pinned_until_second_observation() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let path = root.join("rule.md");
    std::fs::write(&path, "fixture").unwrap();
    let before = observe(&path, "rule", true, None, &mut 0, &mut BTreeSet::new()).unwrap();
    for _ in 0..128 {
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "fixture").unwrap();
        let after = observe(&path, "rule", true, None, &mut 0, &mut BTreeSet::new()).unwrap();
        assert_eq!(before.manifest["sha256"], after.manifest["sha256"]);
        assert_ne!(before.manifest["identity"], after.manifest["identity"]);
    }
}
