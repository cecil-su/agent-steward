use std::path::Path;
use steward_application::Service;

#[test]
fn http_checkout_path_validation_precedes_even_database_io() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("must-not-create").join("isolated.db");
    let service = Service::new(database.clone());
    let mut rejected = vec![
        r"\\untrusted.invalid\share\checkout".to_owned(),
        r"\\?\UNC\untrusted.invalid\share\checkout".into(),
        r"\\.\PIPE\untrusted".into(),
        r"\\?\GLOBALROOT\Device\untrusted".into(),
        "//untrusted.invalid/share/checkout".into(),
        r"C:\checkout:stream".into(),
    ];
    rejected.push(
        temp.path()
            .join("missing")
            .join("..")
            .join("checkout")
            .to_string_lossy()
            .into_owned(),
    );
    for input in rejected {
        let error = service
            .project_source_http_worktree("##1", 1, Path::new(&input))
            .unwrap_err();
        assert_eq!(error.body.code, "INVALID_INPUT");
        assert!(
            !database.parent().unwrap().exists(),
            "lexical rejection must not open even the database"
        );
    }
}
