use byte::claude::files::ClaudeFiles;
use byte::claude::snapshot::AccountSnapshot;
use byte::paths::{HostPaths, TestPaths};
use serde_json::json;

fn seed(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "mcpOAuth": {"server-a": {"accessToken": "keep-me"}},
            "claudeAiOauth": {
                "accessToken": "access-1",
                "refreshToken": refresh,
                "expiresAt": 1234567890i64,
                "scopes": ["a", "b"],
                "subscriptionType": "max"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "numStartups": 60,
            "oauthAccount": {
                "accountUuid": uuid,
                "emailAddress": email,
                "organizationName": "Test Org"
            },
            "userID": "user-1",
            "projects": {"/x": {"history": [1]}}
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn capture_reads_the_live_account() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");

    let snap = ClaudeFiles::new(&tp).capture().unwrap().unwrap();

    assert_eq!(snap.account_uuid(), Some("uuid-1"));
    assert_eq!(snap.email(), Some("a@example.com"));
    assert_eq!(snap.organization_name(), Some("Test Org"));
    assert_eq!(snap.subscription_type(), Some("max"));
    assert_eq!(snap.user_id.as_deref(), Some("user-1"));
}

#[test]
fn capture_returns_none_when_logged_out() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), r#"{"mcpOAuth":{}}"#).unwrap();
    std::fs::write(tp.claude_config(), r#"{"numStartups":1}"#).unwrap();

    assert!(ClaudeFiles::new(&tp).capture().unwrap().is_none());
}

#[test]
fn apply_swaps_the_account_and_preserves_everything_else() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    let target = AccountSnapshot::new(
        json!({"accessToken": "access-2", "refreshToken": "refresh-2", "expiresAt": 99i64}),
        json!({"accountUuid": "uuid-2", "emailAddress": "b@example.com"}),
        Some("user-2".to_string()),
    );
    files.apply(&target).unwrap();

    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();

    assert_eq!(creds["claudeAiOauth"]["refreshToken"], json!("refresh-2"));
    assert_eq!(cfg["oauthAccount"]["accountUuid"], json!("uuid-2"));
    assert_eq!(cfg["userID"], json!("user-2"));

    // Untouched neighbours.
    assert_eq!(
        creds["mcpOAuth"]["server-a"]["accessToken"],
        json!("keep-me")
    );
    assert_eq!(cfg["numStartups"], json!(60));
    assert_eq!(cfg["projects"]["/x"]["history"], json!([1]));
}

#[test]
fn apply_with_no_user_id_removes_the_key_instead_of_writing_null() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    let target = AccountSnapshot::new(
        json!({"accessToken": "access-2", "refreshToken": "refresh-2"}),
        json!({"accountUuid": "uuid-2", "emailAddress": "b@example.com"}),
        None,
    );
    files.apply(&target).unwrap();

    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();

    // `cfg["userID"]` would be `Value::Null` both when the key is absent and
    // when it is present with a JSON `null`, so that index alone cannot
    // distinguish the two — check key presence directly instead.
    assert!(
        !cfg.as_object().unwrap().contains_key("userID"),
        "userID key should be removed, not set to null: {cfg}"
    );
}

/// A `HostPaths` that reroutes the config file underneath a regular file
/// instead of a directory. `std::fs::create_dir_all` deterministically fails
/// when asked to create a directory at a path that already exists as a file
/// — on every platform, independent of permissions or ACLs — which makes
/// this a portable way to force `apply()`'s config write to fail without
/// touching production code or relying on OS-specific permission semantics
/// (Windows directory "read-only" attributes do not block writes into the
/// directory, so that approach was not usable here).
struct UnwritableConfigPaths<'a> {
    inner: &'a TestPaths,
}

impl HostPaths for UnwritableConfigPaths<'_> {
    fn claude_config(&self) -> std::path::PathBuf {
        self.inner
            .root()
            .join("not-a-directory")
            .join(".claude.json")
    }
    fn claude_credentials(&self) -> std::path::PathBuf {
        self.inner.claude_credentials()
    }
    fn byte_config_dir(&self) -> std::path::PathBuf {
        self.inner.byte_config_dir()
    }
}

#[test]
fn apply_rolls_back_credentials_when_the_config_write_fails() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    // A regular file sits where the config file's parent directory would
    // need to be created, so byte can never write it.
    std::fs::write(
        tp.root().join("not-a-directory"),
        b"blocks directory creation",
    )
    .unwrap();

    let before_creds = std::fs::read_to_string(tp.claude_credentials()).unwrap();
    let paths = UnwritableConfigPaths { inner: &tp };

    let target = AccountSnapshot::new(
        json!({"accessToken": "access-2", "refreshToken": "refresh-2"}),
        json!({"accountUuid": "uuid-2"}),
        Some("user-2".to_string()),
    );

    let err = ClaudeFiles::new(&paths).apply(&target).unwrap_err();

    // The rollback itself should succeed here — the credentials file's own
    // directory is untouched — so the caller should see the plain config
    // failure, not a compounded ApplyRollbackFailed.
    assert!(
        !matches!(err, byte::Error::ApplyRollbackFailed { .. }),
        "expected a clean rollback (plain config error), got: {err}"
    );

    let after_creds = std::fs::read_to_string(tp.claude_credentials()).unwrap();
    assert_eq!(
        after_creds, before_creds,
        "credentials must be rolled back to their pre-apply content when the config write fails"
    );
}

#[test]
fn capture_then_apply_round_trips_exactly() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    let before = files.capture().unwrap().unwrap();
    files.apply(&before).unwrap();
    let after = files.capture().unwrap().unwrap();

    assert_eq!(before, after);
}

#[test]
fn clear_removes_the_account_but_keeps_other_keys() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    files.clear().unwrap();

    assert!(files.capture().unwrap().is_none());
    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    assert_eq!(
        creds["mcpOAuth"]["server-a"]["accessToken"],
        json!("keep-me")
    );

    // capture()'s None verdict is driven only by the credentials file; check
    // the config file directly so a clear() that forgot to touch it (but
    // still cleared claudeAiOauth) would not pass this test by accident.
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();
    assert!(!cfg.as_object().unwrap().contains_key("oauthAccount"));
    assert!(!cfg.as_object().unwrap().contains_key("userID"));
    assert_eq!(cfg["numStartups"], json!(60));
    assert_eq!(cfg["projects"]["/x"]["history"], json!([1]));
}

#[test]
fn validate_rejects_a_snapshot_with_no_refresh_token() {
    let snap = AccountSnapshot::new(
        json!({"accessToken": "a", "refreshToken": ""}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(snap.validate().is_err());
}

#[test]
fn validate_rejects_an_unknown_schema_version() {
    let mut snap = AccountSnapshot::new(
        json!({"refreshToken": "r"}),
        json!({"accountUuid": "u"}),
        None,
    );
    snap.schema = 999;
    assert!(snap.validate().is_err());
}

#[test]
fn validate_accepts_a_well_formed_snapshot() {
    let snap = AccountSnapshot::new(
        json!({"refreshToken": "r"}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(snap.validate().is_ok());
}
