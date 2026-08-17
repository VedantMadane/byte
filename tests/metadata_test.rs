use byte::Error;
use byte::claude::snapshot::AccountSnapshot;
use byte::paths::{HostPaths, TestPaths};
use byte::store::metadata::AccountsFile;
use serde_json::json;

fn snap(uuid: &str, email: &str) -> AccountSnapshot {
    AccountSnapshot::new(
        json!({"refreshToken": "r", "subscriptionType": "max"}),
        json!({"accountUuid": uuid, "emailAddress": email, "organizationName": "Org"}),
        Some("uid".to_string()),
    )
}

#[test]
fn upsert_adds_a_new_account_labelled_by_email() {
    let mut file = AccountsFile::default();
    let meta = file.upsert_from("u1", &snap("u1", "a@example.com"));

    assert_eq!(meta.label, "a@example.com");
    assert_eq!(meta.uuid, "u1");
    assert_eq!(meta.subscription_type.as_deref(), Some("max"));
    assert_eq!(file.accounts.len(), 1);
}

#[test]
fn upsert_updates_in_place_and_keeps_a_custom_label() {
    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "a@example.com"));
    file.rename("u1", "work").unwrap();

    file.upsert_from("u1", &snap("u1", "a@example.com"));

    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.accounts[0].label, "work");
}

#[test]
fn resolve_matches_label_then_email_then_uuid_prefix() {
    let mut file = AccountsFile::default();
    file.upsert_from("abcdef123456", &snap("abcdef123456", "a@example.com"));
    file.rename("abcdef123456", "personal").unwrap();
    file.upsert_from("999999999999", &snap("999999999999", "b@example.com"));

    assert_eq!(file.resolve("personal").unwrap().uuid, "abcdef123456");
    assert_eq!(file.resolve("b@example.com").unwrap().uuid, "999999999999");
    assert_eq!(file.resolve("abcdef").unwrap().uuid, "abcdef123456");
}

#[test]
fn resolve_is_case_insensitive() {
    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "Alice@Example.com"));

    assert_eq!(file.resolve("alice@example.com").unwrap().uuid, "u1");
}

#[test]
fn resolve_reports_an_unknown_name() {
    let file = AccountsFile::default();
    assert!(matches!(
        file.resolve("nobody"),
        Err(Error::NoSuchAccount(_))
    ));
}

#[test]
fn resolve_reports_ambiguity_rather_than_guessing() {
    let mut file = AccountsFile::default();
    file.upsert_from("aaa111", &snap("aaa111", "x@example.com"));
    file.upsert_from("aaa222", &snap("aaa222", "y@example.com"));

    assert!(matches!(
        file.resolve("aaa"),
        Err(Error::AmbiguousAccount { count: 2, .. })
    ));
}

#[test]
fn resolve_rejects_an_empty_query_even_with_exactly_one_account() {
    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "a@example.com"));

    // With a single stored account, "" and uuid.starts_with("") are both
    // true, so an unguarded prefix fallback would resolve a blank query to
    // that account. Zero or 2+ accounts already error correctly; this is
    // the one case that previously slipped through.
    assert!(file.resolve("").is_err());
    assert!(file.resolve("   ").is_err());
}

#[test]
fn remove_deletes_the_account_and_clears_active_when_it_matches() {
    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "a@example.com"));
    file.set_active("u1");

    let removed = file.remove("u1").unwrap();

    assert_eq!(removed.uuid, "u1");
    assert!(file.accounts.is_empty());
    assert!(file.active.is_none());
}

#[test]
fn accounts_survive_a_save_and_load_cycle() {
    let tp = TestPaths::new().unwrap();
    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "a@example.com"));
    file.set_active("u1");
    file.save(&tp.accounts_file(), &tp.backup_dir()).unwrap();

    let loaded = AccountsFile::load(&tp.accounts_file()).unwrap();

    assert_eq!(loaded.accounts.len(), 1);
    assert_eq!(loaded.active.as_deref(), Some("u1"));
    assert_eq!(loaded.accounts[0].uuid, "u1");
    assert_eq!(loaded.accounts[0].label, "a@example.com");
    assert_eq!(loaded.accounts[0].email.as_deref(), Some("a@example.com"));
}

#[test]
fn loading_a_missing_file_yields_an_empty_store() {
    let tp = TestPaths::new().unwrap();
    let loaded = AccountsFile::load(&tp.accounts_file()).unwrap();
    assert!(loaded.accounts.is_empty());
}

#[test]
fn save_prunes_accounts_json_backups_to_ten() {
    // Regression test for task-11 review Finding 4: unlike .claude.json and
    // .credentials.json (pruned by JsonDocument::save), accounts.json's own
    // save() never called atomic::prune at all, so its backups grew
    // unbounded -- one more per capture, switch, rename, and remove.
    let tp = TestPaths::new().unwrap();
    let path = tp.accounts_file();
    let backup_dir = tp.backup_dir();
    let stem = path.file_name().unwrap().to_string_lossy().to_string();

    // Seed the accounts file so this save()'s own backup() call has
    // something to back up, plus 12 synthetic pre-existing backups already
    // over the retention limit. Synthetic filenames (rather than driving 12
    // real save() calls in a loop) sidestep atomic::backup's
    // millisecond-granularity timestamps, which a tight loop could
    // otherwise collide on and silently produce fewer than 12 real files.
    std::fs::write(&path, "{}").unwrap();
    for i in 0..12u64 {
        std::fs::write(backup_dir.join(format!("{stem}.{i:013}.bak")), "old").unwrap();
    }

    let mut file = AccountsFile::default();
    file.upsert_from("u1", &snap("u1", "a@example.com"));
    file.save(&path, &backup_dir).unwrap();

    let remaining: Vec<String> = std::fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(&format!("{stem}.")))
        .collect();

    // 12 synthetic backups plus 1 real one from this save()'s own backup()
    // call = 13 candidates; without prune wired up, all 13 survive.
    assert_eq!(
        remaining.len(),
        10,
        "expected accounts.json backups capped at 10, found {}: {remaining:?}",
        remaining.len()
    );
}
