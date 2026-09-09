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
        json!({"accessToken": "access-2", "refreshToken": "refresh-2", "expiresAt": 99i64}),
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
        json!({"accessToken": "access-2", "refreshToken": "refresh-2", "expiresAt": 99i64}),
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
fn apply_succeeds_even_when_backup_pruning_cannot_remove_an_old_entry() {
    // Finding C1: `atomic::prune` failing must never invalidate a write
    // that has already been committed and verified -- previously
    // `JsonDocument::save` propagated a prune failure with `?` *after* the
    // new bytes were already on disk and verified, so `apply()` reported a
    // failed switch even though the credentials file had already been
    // replaced.
    //
    // This seeds exactly ten pre-existing `.credentials.json` backups (the
    // retention limit), so apply()'s own creds.save() call -- which backs
    // up the live file before overwriting it -- pushes the count to eleven
    // and prune() must remove one. The oldest (lowest-timestamp) entry is a
    // DIRECTORY rather than a file: `std::fs::remove_file` fails on a
    // directory on every platform (EISDIR on POSIX, access-denied on
    // Windows), which portably forces prune() to hit the exact failure this
    // finding is about without relying on permissions or ACLs.
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let backup_dir = tp.backup_dir();
    let stem = tp
        .claude_credentials()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    std::fs::create_dir(backup_dir.join(format!("{stem}.0000000000000.bak"))).unwrap();
    for i in 1..10u64 {
        std::fs::write(backup_dir.join(format!("{stem}.{i:013}.bak")), b"old").unwrap();
    }

    let files = ClaudeFiles::new(&tp);
    let target = AccountSnapshot::new(
        json!({"accessToken": "access-2", "refreshToken": "refresh-2", "expiresAt": 99i64}),
        json!({"accountUuid": "uuid-2", "emailAddress": "b@example.com"}),
        Some("user-2".to_string()),
    );

    let result = files.apply(&target);

    assert!(
        result.is_ok(),
        "apply() must not fail merely because pruning an old backup failed: {result:?}"
    );

    // Both live files must agree on the NEW account -- the switch must have
    // gone all the way through, not stalled or partially reverted because
    // of the unrelated backup-pruning failure.
    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();
    assert_eq!(creds["claudeAiOauth"]["refreshToken"], json!("refresh-2"));
    assert_eq!(cfg["oauthAccount"]["accountUuid"], json!("uuid-2"));

    // The undeletable directory backup must still be there -- proves prune
    // actually hit (and survived) the failure, rather than this test
    // accidentally not exercising it at all.
    assert!(
        backup_dir
            .join(format!("{stem}.0000000000000.bak"))
            .is_dir()
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
fn apply_refuses_to_create_missing_claude_files_from_scratch() {
    // Finding I3: apply() previously used load_or_empty() on Claude Code's
    // OWN files, so a missing `.claude.json`/`.credentials.json` (a stale
    // or mistyped CLAUDE_CONFIG_DIR, or a machine that has never logged
    // into Claude Code) made byte silently fabricate both from an empty
    // document and report a successful switch -- violating spec §9 row 1
    // ("Clear message naming the expected paths; exit non-zero. No files
    // created."). Neither file is written here at all -- TestPaths::new()
    // creates the parent directories but not the files themselves -- so
    // apply() must fail loudly instead of creating anything.
    let tp = TestPaths::new().unwrap();
    let files = ClaudeFiles::new(&tp);

    let target = AccountSnapshot::new(
        json!({"accessToken": "a", "refreshToken": "refresh-2", "expiresAt": 1i64}),
        json!({"accountUuid": "uuid-2"}),
        Some("user-2".to_string()),
    );

    let err = files.apply(&target).unwrap_err();

    assert!(
        matches!(err, byte::Error::ClaudeFileMissing(_)),
        "expected ClaudeFileMissing, got: {err}"
    );
    assert!(
        !tp.claude_credentials().exists(),
        "apply() must not create .credentials.json when it was absent"
    );
    assert!(
        !tp.claude_config().exists(),
        "apply() must not create .claude.json when it was absent"
    );
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
fn clear_refuses_when_claude_files_are_missing() {
    // The clear()-side counterpart of
    // apply_refuses_to_create_missing_claude_files_from_scratch: the add
    // flow's begin() calls clear() unconditionally, so this must refuse
    // just as loudly rather than fabricating a "logged out" state out of
    // files that were never there.
    let tp = TestPaths::new().unwrap();
    let files = ClaudeFiles::new(&tp);

    let err = files.clear().unwrap_err();

    assert!(
        matches!(err, byte::Error::ClaudeFileMissing(_)),
        "expected ClaudeFileMissing, got: {err}"
    );
    assert!(!tp.claude_credentials().exists());
    assert!(!tp.claude_config().exists());
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
fn validate_rejects_a_snapshot_with_no_expires_at() {
    // Finding M1: spec §7 step 4 requires "refreshToken non-empty, expiresAt
    // parses, schema version known" -- the expiresAt check was silently
    // dropped when the identity check was added. A snapshot missing it must
    // be refused, the same as a missing refresh token.
    let snap = AccountSnapshot::new(
        json!({"refreshToken": "r"}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(matches!(
        snap.validate(),
        Err(byte::Error::InvalidSnapshot { .. })
    ));
}

#[test]
fn validate_rejects_a_non_numeric_expires_at() {
    // "expiresAt parses" -- a present-but-wrong-typed value must be
    // rejected too, not just an absent key.
    let snap = AccountSnapshot::new(
        json!({"refreshToken": "r", "expiresAt": "not-a-number"}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(matches!(
        snap.validate(),
        Err(byte::Error::InvalidSnapshot { .. })
    ));
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
        json!({"refreshToken": "r", "expiresAt": 1i64}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(snap.validate().is_ok());
}

#[test]
fn account_snapshot_survives_a_json_round_trip() {
    // Since the keychain-size fix, nothing in production code serializes a
    // whole `AccountSnapshot` anymore -- `KeyringStore` stores only the
    // `oauth` block (see `tests/secrets_test.rs`), and `account`/`user_id`
    // are plain fields on `AccountMeta`, populated via `upsert_from`, not a
    // nested `AccountSnapshot` (see `tests/metadata_test.rs`). This test
    // still pins the type's own `Serialize`/`Deserialize` round trip, since
    // it remains part of the public API and is cheap insurance against a
    // silently dropped or renamed field. Nested, multi-key oauth/account
    // objects so such a field would be caught, not masked by an empty
    // value.
    let original = AccountSnapshot::new(
        json!({
            "accessToken": "access-1",
            "refreshToken": "refresh-1",
            "expiresAt": 1234567890i64,
            "scopes": ["a", "b"],
            "subscriptionType": "max"
        }),
        json!({
            "accountUuid": "uuid-1",
            "emailAddress": "a@example.com",
            "organizationName": "Test Org",
            "organizationUuid": "org-uuid-1"
        }),
        Some("user-1".to_string()),
    );

    let text = serde_json::to_string(&original).unwrap();
    let restored: AccountSnapshot = serde_json::from_str(&text).unwrap();

    assert_eq!(restored.schema, original.schema);
    assert_eq!(restored.oauth, original.oauth);
    assert_eq!(restored.account, original.account);
    assert_eq!(restored.user_id, original.user_id);
    assert_eq!(restored, original);
}

#[test]
fn clear_rolls_back_credentials_when_the_config_write_fails() {
    // `clear()` commits the credentials file FIRST, exactly as `apply()`
    // does -- so it needs the same rollback `apply()` has, and did not have
    // one. Without it: the credentials write commits (Claude Code is now
    // logged out), the config half then fails, and the error surfaced to
    // the user describes only that second failure -- reading like nothing
    // happened. Worse, the failure propagates out of `AddSession::begin`,
    // so no `AddSession` value ever exists and `abort()`/`resolve_add_failure`
    // -- the entire "a failed add must restore you" apparatus -- never run.
    // `byte current`, the tray tooltip, and the menu's active marker all
    // still name the old account, because `accounts.json` was never touched.
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    // A regular file sits where the config file's parent directory would
    // need to be, so the config half can never be loaded or written.
    std::fs::write(
        tp.root().join("not-a-directory"),
        b"blocks directory creation",
    )
    .unwrap();

    let before_creds = std::fs::read_to_string(tp.claude_credentials()).unwrap();
    let paths = UnwritableConfigPaths { inner: &tp };

    let err = ClaudeFiles::new(&paths).clear().unwrap_err();

    // The rollback itself should succeed -- the credentials file's own
    // directory is untouched -- so the caller sees the plain config failure.
    assert!(
        !matches!(err, byte::Error::ApplyRollbackFailed { .. }),
        "expected a clean rollback (plain config error), got: {err}"
    );

    let after_creds = std::fs::read_to_string(tp.claude_credentials()).unwrap();
    assert_eq!(
        after_creds, before_creds,
        "a failed clear() must leave the credentials file exactly as it was"
    );
    assert!(
        after_creds.contains("claudeAiOauth"),
        "a failed clear() must leave the user logged IN, not silently logged out: {after_creds}"
    );
}

/// Paths whose byte config directory -- and so the backup directory under
/// it -- can never be created, so the very first step of any `save` fails
/// while both Claude files stay untouched.
struct UnwritableBackupPaths<'a> {
    inner: &'a TestPaths,
}

impl HostPaths for UnwritableBackupPaths<'_> {
    fn claude_config(&self) -> std::path::PathBuf {
        self.inner.claude_config()
    }
    fn claude_credentials(&self) -> std::path::PathBuf {
        self.inner.claude_credentials()
    }
    fn byte_config_dir(&self) -> std::path::PathBuf {
        self.inner.root().join("not-a-directory").join("byte")
    }
}

#[test]
fn a_precommit_failure_reports_its_own_cause_not_a_rollback_failure() {
    // Issue #10 part 2, made more reachable by giving `clear()` a rollback:
    // every pre-commit failure of `creds.save` (permission denied, disk
    // full, read-only volume) recurs identically on the rollback attempt,
    // so routing it through `rollback_credentials` would report
    // `ApplyRollbackFailed` -- "the credentials and config files may now
    // disagree about which account is active and must be checked by hand"
    // -- when nothing was written and the two files are perfectly
    // consistent. That sends the user into backup recovery after a no-op,
    // where restoring a stale `.claude.json` would discard unrelated Claude
    // Code state. Nothing was committed, so the real cause must survive.
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    std::fs::write(
        tp.root().join("not-a-directory"),
        b"blocks directory creation",
    )
    .unwrap();

    let before_creds = std::fs::read_to_string(tp.claude_credentials()).unwrap();
    let paths = UnwritableBackupPaths { inner: &tp };

    let err = ClaudeFiles::new(&paths).clear().unwrap_err();

    assert!(
        !matches!(err, byte::Error::ApplyRollbackFailed { .. }),
        "nothing was committed, so this must report its own cause: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(tp.claude_credentials()).unwrap(),
        before_creds,
        "the credentials file must be untouched"
    );
}
