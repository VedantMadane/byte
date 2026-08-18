use byte::claude::snapshot::AccountSnapshot;
use byte::store::secrets::{KeyringStore, MemoryStore, SecretStore};
use serde_json::json;

fn oauth(refresh: &str) -> serde_json::Value {
    json!({"refreshToken": refresh})
}

#[test]
fn get_returns_what_put_stored() {
    let store = MemoryStore::new();
    store.put("u1", &oauth("r1")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(oauth("r1")));
}

#[test]
fn get_returns_none_for_an_unknown_account() {
    let store = MemoryStore::new();
    assert_eq!(store.get("missing").unwrap(), None);
}

#[test]
fn put_overwrites_an_existing_secret() {
    let store = MemoryStore::new();
    store.put("u1", &oauth("old")).unwrap();
    store.put("u1", &oauth("new")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(oauth("new")));
}

#[test]
fn delete_removes_the_secret() {
    let store = MemoryStore::new();
    store.put("u1", &oauth("r1")).unwrap();
    store.delete("u1").unwrap();

    assert_eq!(store.get("u1").unwrap(), None);
}

#[test]
fn deleting_something_absent_is_not_an_error() {
    let store = MemoryStore::new();
    assert!(store.delete("missing").is_ok());
}

#[test]
fn accounts_are_stored_independently_by_uuid() {
    let store = MemoryStore::new();
    store.put("u1", &oauth("r1")).unwrap();
    store.put("u2", &oauth("r2")).unwrap();

    // Each uuid must return its own secret, not just *a* secret — a store
    // that kept a single shared slot and ignored the uuid key would still
    // pass an `is_some()`-only check here.
    assert_eq!(store.get("u1").unwrap(), Some(oauth("r1")));
    assert_eq!(store.get("u2").unwrap(), Some(oauth("r2")));

    store.delete("u1").unwrap();

    assert_eq!(store.get("u1").unwrap(), None);
    assert_eq!(store.get("u2").unwrap(), Some(oauth("r2")));
}

// The tests below guard the keychain-size fix: `byte capture` used to fail
// on Windows with "Value of 'password encoded as UTF-16' is longer than the
// platform limit of 2560 chars" because `KeyringStore` stored the *whole*
// `AccountSnapshot` (oauth + a full oauthAccount + userID) as one keychain
// entry. Windows Credential Manager's `CRED_MAX_CREDENTIAL_BLOB_SIZE` is
// 2560 *bytes*, but `windows-native-keyring-store` encodes the payload as
// UTF-16 (2 bytes/char for the ASCII/BMP range this payload uses) before
// comparing against it — so the real budget is 1280 *characters*, not 2560.
// A realistic full snapshot measured at 1281 characters against a real
// account: one character over. `MemoryStore`, used by every other test in
// this suite, enforces no size limit at all, which is exactly why 113 tests
// passed against a payload the real store rejected — so this guard
// deliberately does not use it, and instead pins the size of what
// `KeyringStore` actually sends over the wire.

#[test]
fn realistic_keychain_payload_stays_comfortably_under_the_windows_character_budget() {
    // Built to be as large as a real payload gets: a full 19-field
    // oauthAccount and ~110-character tokens, matching the shape of the
    // account that originally triggered the bug. The account/user_id
    // fields are included here (a realistic `AccountSnapshot` needs them to
    // be realistic) but must NOT affect the measured payload below — after
    // the fix, only `snapshot.oauth` ever reaches the keychain. If a future
    // change moved a field back into `oauth`, or started serializing the
    // whole snapshot again, this is the test that catches it.
    let access_token = format!("sk-ant-oat01-{}", "a".repeat(97)); // 110 chars
    let refresh_token = format!("sk-ant-ort01-{}", "b".repeat(97)); // 110 chars
    let oauth = json!({
        "accessToken": access_token,
        "refreshToken": refresh_token,
        "expiresAt": 1_786_000_000_000i64,
        "scopes": ["user:inference", "user:profile", "org:billing"],
        "subscriptionType": "max",
        "rateLimitTier": "tier_4",
    });

    let account = json!({
        "accountUuid": "9f2c9e2e-1b1a-4a3e-9c3d-2f6a7b8c9d0e",
        "emailAddress": "someone.realistic@example-organization.com",
        "organizationUuid": "8a1b2c3d-4e5f-6789-0abc-def123456789",
        "organizationName": "A Realistically Long Organization Name Inc.",
        "displayName": "Someone Realistic",
        "billingType": "organization",
        "organizationRole": "member",
        "rateLimitTier": "tier_4",
        "hasClaudeMax": true,
        "hasClaudePro": false,
        "isOrgFirstParty": false,
        "primaryOrganizationUuid": "8a1b2c3d-4e5f-6789-0abc-def123456789",
        "organizationSettings": {"restrictConsole": false},
        "accountType": "member",
        "workspaceRole": "developer",
        "ssoIdentityProvider": "google",
        "mfaEnabled": true,
        "createdAt": "2025-01-01T00:00:00Z",
        "region": "us-east-1",
    });
    assert_eq!(
        account.as_object().unwrap().len(),
        19,
        "test fixture drifted from the realistic 19-field oauthAccount it's meant to model"
    );

    let snapshot = AccountSnapshot::new(
        oauth,
        account,
        Some("user-01ABCDEFGHJKMNPQRSTVWXYZ".to_string()),
    );

    // The exact call `KeyringStore::put` makes -- not a hand-rolled
    // `serde_json::to_string`, so this pins the real code path, not a
    // reimplementation of it.
    let payload = KeyringStore::serialize_payload(&snapshot.oauth).unwrap();

    assert!(
        payload.len() < 1280,
        "keychain payload is {} chars, at or over the real Windows budget of 1280 \
         (CRED_MAX_CREDENTIAL_BLOB_SIZE, 2560 bytes, halved by UTF-16 encoding): {payload}",
        payload.len()
    );
    // "Comfortably" under, not just technically under: the original bug was
    // *one character* over budget, so merely being < 1280 is not reassuring
    // on its own. This leaves more than half the budget as headroom for
    // Anthropic adding a field to claudeAiOauth before this trips again.
    assert!(
        payload.len() < 700,
        "keychain payload is {} chars -- not comfortably under the 1280 budget, \
         leaves too little headroom for oauth to grow: {payload}",
        payload.len()
    );
}

#[test]
fn keychain_payload_excludes_the_non_secret_account_and_user_id() {
    // The other half of the same guard, stated as a direct property rather
    // than a size bound: the account block and userID must not be part of
    // the keychain payload AT ALL, not merely small enough to fit. A
    // regression that re-merged them but happened to stay under 1280 chars
    // (e.g. a small oauthAccount) would slip past the size test above but
    // not this one.
    let snapshot = AccountSnapshot::new(
        json!({"refreshToken": "r", "expiresAt": 1i64}),
        json!({
            "accountUuid": "u1",
            "emailAddress": "a@example.com",
            "displayName": "should-not-reach-the-keychain"
        }),
        Some("user-should-not-reach-the-keychain".to_string()),
    );

    let payload = KeyringStore::serialize_payload(&snapshot.oauth).unwrap();

    assert!(!payload.contains("should-not-reach-the-keychain"));
    assert!(!payload.contains("accountUuid"));
    assert!(!payload.contains("displayName"));
}
