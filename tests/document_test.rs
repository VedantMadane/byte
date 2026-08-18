use byte::claude::document::JsonDocument;
use byte::paths::{HostPaths, TestPaths};
use serde_json::json;

/// A stand-in for ~/.claude.json: many unrelated keys, deliberately NOT in
/// alphabetical order, with nested structures byte knows nothing about.
const PRETTY_FIXTURE: &str = r#"{
  "numStartups": 60,
  "installMethod": "native",
  "tipsHistory": {
    "new-user-warmup": 5,
    "zebra-tip": 1,
    "alpha-tip": 2
  },
  "oauthAccount": {
    "accountUuid": "old-uuid",
    "emailAddress": "old@example.com"
  },
  "userID": "old-user-id",
  "projects": {
    "/some/path": {
      "history": [
        1,
        2,
        3
      ]
    }
  },
  "autoUpdates": false
}"#;

/// `PRETTY_FIXTURE` with the middle key `oauthAccount` removed — the exact
/// text `shift_remove` must produce. `swap_remove` would instead move the
/// last key (`autoUpdates`) into `oauthAccount`'s slot, which is nowhere in
/// this text.
const FIXTURE_WITHOUT_OAUTH_ACCOUNT: &str = r#"{
  "numStartups": 60,
  "installMethod": "native",
  "tipsHistory": {
    "new-user-warmup": 5,
    "zebra-tip": 1,
    "alpha-tip": 2
  },
  "userID": "old-user-id",
  "projects": {
    "/some/path": {
      "history": [
        1,
        2,
        3
      ]
    }
  },
  "autoUpdates": false
}"#;

#[test]
fn patching_one_key_leaves_every_other_byte_identical() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("new-user-id"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    let expected = PRETTY_FIXTURE.replace("\"old-user-id\"", "\"new-user-id\"");
    assert_eq!(after, expected);
}

#[test]
fn key_order_is_never_alphabetised() {
    // Guards the serde_json `preserve_order` feature. Without it this fails.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("order.json");
    std::fs::write(&path, r#"{"zebra":1,"apple":2,"mango":3}"#).unwrap();

    let doc = JsonDocument::load(&path).unwrap();
    let out = String::from_utf8(doc.to_bytes().unwrap()).unwrap();

    assert!(out.find("zebra") < out.find("apple"));
    assert!(out.find("apple") < out.find("mango"));
}

#[test]
fn unknown_nested_keys_survive_a_round_trip() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("oauthAccount", json!({"accountUuid": "new-uuid"}));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["tipsHistory"]["zebra-tip"], json!(1));
    assert_eq!(after["projects"]["/some/path"]["history"], json!([1, 2, 3]));
    assert_eq!(after["numStartups"], json!(60));
}

#[test]
fn compact_files_stay_compact() {
    // .credentials.json is minified on disk; rewriting it pretty would
    // produce a gratuitous whole-file diff.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".credentials.json");
    std::fs::write(&path, r#"{"mcpOAuth":{"a":1},"claudeAiOauth":{"x":1}}"#).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("claudeAiOauth", json!({"x": 2}));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        !after.contains('\n'),
        "compact file gained newlines: {after}"
    );
    assert_eq!(after, r#"{"mcpOAuth":{"a":1},"claudeAiOauth":{"x":2}}"#);
}

#[test]
fn pretty_files_stay_pretty_with_two_space_indent() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("x"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("\n  \"numStartups\""));
}

#[test]
fn a_trailing_newline_is_preserved_when_present() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("nl.json");
    std::fs::write(&path, "{\"a\":1}\n").unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("a", json!(2));
    doc.save(&path, &tp.backup_dir()).unwrap();

    assert!(std::fs::read_to_string(&path).unwrap().ends_with("}\n"));
}

#[test]
fn a_missing_trailing_newline_is_not_added() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("nonl.json");
    std::fs::write(&path, "{\"a\":1}").unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("a", json!(2));
    doc.save(&path, &tp.backup_dir()).unwrap();

    assert!(!std::fs::read_to_string(&path).unwrap().ends_with('\n'));
}

#[test]
fn loading_malformed_json_fails_without_writing() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("bad.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    assert!(JsonDocument::load(&path).is_err());
    // The file must be untouched.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{ this is not json"
    );
}

#[test]
fn saving_creates_a_backup_first() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("changed"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let backups: Vec<_> = std::fs::read_dir(tp.backup_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(backups.len(), 1);
    let saved = std::fs::read_to_string(backups[0].path()).unwrap();
    assert_eq!(saved, PRETTY_FIXTURE);
}

#[test]
fn removing_a_middle_key_preserves_order_of_the_rest() {
    // Regression guard for `remove()`'s use of `shift_remove`. If that ever
    // regressed to `swap_remove`, the last top-level key (`autoUpdates`)
    // would jump into the removed key's slot instead of every later key
    // shifting up by one — this asserts the full byte-for-byte layout, not
    // just that the key is gone, so a reordering trips it.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.remove("oauthAccount");
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, FIXTURE_WITHOUT_OAUTH_ACCOUNT);
}

#[test]
fn float_roundtrip_feature_prevents_precision_drift() {
    // Regression guard for the `float_roundtrip` feature on `serde_json` in
    // Cargo.toml. This literal is copied verbatim from a real ~/.claude.json
    // (key `frame_duration_ms_min`): without the feature, serde_json's
    // default float parser is not correctly-rounded and this value
    // reserialises as "0.091100000005099" — one ULP off and one byte
    // shorter. If this test ever fails, the feature was dropped.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("floats.json");
    std::fs::write(&path, r#"{"frame_duration_ms_min":0.09110000000509899}"#).unwrap();

    let doc = JsonDocument::load(&path).unwrap();
    let out = String::from_utf8(doc.to_bytes().unwrap()).unwrap();

    assert_eq!(out, r#"{"frame_duration_ms_min":0.09110000000509899}"#);
}

#[test]
fn load_or_empty_gives_an_empty_object_for_a_missing_file() {
    let tp = TestPaths::new().unwrap();
    let doc = JsonDocument::load_or_empty(&tp.root().join("absent.json")).unwrap();
    assert!(doc.get("anything").is_none());
}
