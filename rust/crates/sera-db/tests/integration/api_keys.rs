//! Integration tests for ApiKeyRepository.

#![cfg(feature = "integration")]

use crate::TestDb;
use sera_db::api_keys::ApiKeyRepository;
use uuid::Uuid;

fn unique_hash() -> String {
    format!("hash-{}", Uuid::new_v4().simple())
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_list() {
    let Some(db) = TestDb::new().await else { return };

    let owner = format!("owner-{}", Uuid::new_v4().simple());
    ApiKeyRepository::create(
        &db.pool,
        "my-key",
        &unique_hash(),
        &owner,
        &["read".to_string()],
    )
    .await
    .expect("create failed");

    let keys = ApiKeyRepository::list(&db.pool, Some(&owner))
        .await
        .expect("list failed");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "my-key");
    assert_eq!(keys[0].owner_sub, owner);
}

#[tokio::test]
async fn test_list_all_owners() {
    let Some(db) = TestDb::new().await else { return };

    let owner_a = format!("owner-a-{}", Uuid::new_v4().simple());
    let owner_b = format!("owner-b-{}", Uuid::new_v4().simple());

    ApiKeyRepository::create(&db.pool, "key-a", &unique_hash(), &owner_a, &[])
        .await
        .expect("create a failed");
    ApiKeyRepository::create(&db.pool, "key-b", &unique_hash(), &owner_b, &[])
        .await
        .expect("create b failed");

    let all = ApiKeyRepository::list(&db.pool, None)
        .await
        .expect("list all failed");
    assert!(all.iter().any(|k| k.owner_sub == owner_a));
    assert!(all.iter().any(|k| k.owner_sub == owner_b));
}

#[tokio::test]
async fn test_revoke_hides_from_list() {
    let Some(db) = TestDb::new().await else { return };

    let owner = format!("owner-{}", Uuid::new_v4().simple());
    let row = ApiKeyRepository::create(
        &db.pool,
        "revocable-key",
        &unique_hash(),
        &owner,
        &[],
    )
    .await
    .expect("create failed");

    let revoked = ApiKeyRepository::revoke(&db.pool, &row.id.to_string())
        .await
        .expect("revoke failed");
    assert!(revoked);

    let keys = ApiKeyRepository::list(&db.pool, Some(&owner))
        .await
        .expect("list after revoke failed");
    assert!(
        keys.iter().all(|k| k.id != row.id),
        "revoked key should not appear in list"
    );
}

#[tokio::test]
async fn test_revoke_nonexistent_returns_false() {
    let Some(db) = TestDb::new().await else { return };

    let missing = Uuid::new_v4().to_string();
    let revoked = ApiKeyRepository::revoke(&db.pool, &missing)
        .await
        .expect("revoke call failed");
    assert!(!revoked);
}

#[tokio::test]
async fn test_duplicate_key_hash_constraint() {
    let Some(db) = TestDb::new().await else { return };

    let hash = unique_hash();
    ApiKeyRepository::create(&db.pool, "key-1", &hash, "owner-x", &[])
        .await
        .expect("first create failed");

    let err = ApiKeyRepository::create(&db.pool, "key-2", &hash, "owner-y", &[])
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("duplicate") || msg.contains("unique") || msg.contains("already exists"),
        "expected constraint violation, got: {msg}"
    );
}

#[tokio::test]
async fn test_create_with_multiple_roles() {
    let Some(db) = TestDb::new().await else { return };

    let owner = format!("owner-{}", Uuid::new_v4().simple());
    let row = ApiKeyRepository::create(
        &db.pool,
        "multi-role-key",
        &unique_hash(),
        &owner,
        &["read".to_string(), "write".to_string(), "admin".to_string()],
    )
    .await
    .expect("create failed");

    assert_eq!(row.roles.len(), 3);
    assert!(row.roles.contains(&"admin".to_string()));
}
