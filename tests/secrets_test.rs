use byte::claude::snapshot::AccountSnapshot;
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn snap(refresh: &str) -> AccountSnapshot {
    AccountSnapshot::new(
        json!({"refreshToken": refresh}),
        json!({"accountUuid": "u1"}),
        None,
    )
}

#[test]
fn get_returns_what_put_stored() {
    let store = MemoryStore::new();
    store.put("u1", &snap("r1")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(snap("r1")));
}

#[test]
fn get_returns_none_for_an_unknown_account() {
    let store = MemoryStore::new();
    assert_eq!(store.get("missing").unwrap(), None);
}

#[test]
fn put_overwrites_an_existing_secret() {
    let store = MemoryStore::new();
    store.put("u1", &snap("old")).unwrap();
    store.put("u1", &snap("new")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(snap("new")));
}

#[test]
fn delete_removes_the_secret() {
    let store = MemoryStore::new();
    store.put("u1", &snap("r1")).unwrap();
    store.delete("u1").unwrap();

    assert_eq!(store.get("u1").unwrap(), None);
}

#[test]
fn deleting_something_absent_is_not_an_error() {
    let store = MemoryStore::new();
    assert!(store.delete("missing").is_ok());
}
