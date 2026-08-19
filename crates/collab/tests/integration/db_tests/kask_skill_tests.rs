use std::sync::Arc;

use collab::db::Database;
use collab::db::UserId;
use collab::db::queries::kask_skills::NewKaskSkillVersion;

use crate::test_both_dbs;

test_both_dbs!(
    test_kask_skills_insert_and_fetch,
    test_kask_skills_insert_and_fetch_postgres,
    test_kask_skills_insert_and_fetch_sqlite
);

test_both_dbs!(
    test_kask_skill_download_count,
    test_kask_skill_download_count_postgres,
    test_kask_skill_download_count_sqlite
);

test_both_dbs!(
    test_kask_skill_vote,
    test_kask_skill_vote_postgres,
    test_kask_skill_vote_sqlite
);

test_both_dbs!(
    test_kask_skill_uniqueness,
    test_kask_skill_uniqueness_postgres,
    test_kask_skill_uniqueness_sqlite
);

test_both_dbs!(
    test_kask_skill_expiry_filter,
    test_kask_skill_expiry_filter_postgres,
    test_kask_skill_expiry_filter_sqlite
);

test_both_dbs!(
    test_kask_skill_expiry_sweep,
    test_kask_skill_expiry_sweep_postgres,
    test_kask_skill_expiry_sweep_sqlite
);

// zed-kask: D30 — pin the local fallback blob store (no-S3 publish path).
// `put`/`get` round-trips the tarball bytes; re-`put` of the same triple
// upserts (replaces); `delete_kask_skill_tarballs` clears a namespace.
test_both_dbs!(
    test_kask_skill_tarball_round_trips_through_db,
    test_kask_skill_tarball_round_trips_through_db_postgres,
    test_kask_skill_tarball_round_trips_through_db_sqlite
);

test_both_dbs!(
    test_kask_skill_tarball_delete_by_namespace,
    test_kask_skill_tarball_delete_by_namespace_postgres,
    test_kask_skill_tarball_delete_by_namespace_sqlite
);

async fn make_version(
    source_user: &str,
    skill_name: &str,
    version: &str,
    description: &str,
) -> NewKaskSkillVersion {
    make_version_with_expiry(
        source_user,
        skill_name,
        version,
        description,
        "2027-01-01T00:00:00Z",
    )
    .await
}

async fn make_version_with_expiry(
    source_user: &str,
    skill_name: &str,
    version: &str,
    description: &str,
    expires_at: &str,
) -> NewKaskSkillVersion {
    let t0 = time::OffsetDateTime::from_unix_timestamp_nanos(0).unwrap();
    let t0 = time::PrimitiveDateTime::new(t0.date(), t0.time());
    NewKaskSkillVersion {
        source_user: source_user.into(),
        skill_name: skill_name.into(),
        version: version.into(),
        description: description.into(),
        dependencies: Vec::new(),
        tarball_sha256: "abc123".into(),
        public_key: "aa".repeat(32),
        signature: "bb".repeat(64),
        expires_at: expires_at.into(),
        published_at: t0,
    }
}

async fn test_kask_skills_insert_and_fetch(db: &Arc<Database>) {
    let known = db.get_known_kask_skill_versions().await.unwrap();
    assert!(known.is_empty());

    db.insert_kask_skill_versions(&[
        make_version("alice", "bug-hunt", "1.0.0", "Bug hunting skill").await,
        make_version("bob", "essentialist", "1.0.0", "Eliminative interrogation").await,
    ])
    .await
    .unwrap();

    let skills = db.get_kask_skills().await.unwrap();
    assert_eq!(skills.len(), 2);

    let alice_skill = db.get_kask_skill("alice/bug-hunt").await.unwrap().unwrap();
    assert_eq!(alice_skill.manifest.source_user, "alice");
    assert_eq!(alice_skill.manifest.skill_name, "bug-hunt");
    assert_eq!(alice_skill.manifest.version, "1.0.0");
    assert_eq!(alice_skill.manifest.description, "Bug hunting skill");
    assert_eq!(alice_skill.download_count, 0);
    assert_eq!(alice_skill.upvote_count, 0);
    assert_eq!(alice_skill.downvote_count, 0);

    let known = db.get_known_kask_skill_versions().await.unwrap();
    assert!(known.contains_key("alice/bug-hunt"));
    assert!(known.contains_key("bob/essentialist"));
}

async fn test_kask_skill_download_count(db: &Arc<Database>) {
    db.insert_kask_skill_versions(&[
        make_version("alice", "bug-hunt", "1.0.0", "Bug hunting").await
    ])
    .await
    .unwrap();

    let exists = db
        .record_kask_skill_download("alice", "bug-hunt", "1.0.0")
        .await
        .unwrap();
    assert!(exists);

    let exists = db
        .record_kask_skill_download("alice", "bug-hunt", "1.0.0")
        .await
        .unwrap();
    assert!(exists);

    let skill = db.get_kask_skill("alice/bug-hunt").await.unwrap().unwrap();
    assert_eq!(skill.download_count, 2);

    let exists = db
        .record_kask_skill_download("alice", "bug-hunt", "2.0.0")
        .await
        .unwrap();
    assert!(!exists, "nonexistent version should return false");

    let exists = db
        .record_kask_skill_download("charlie", "nonexistent", "1.0.0")
        .await
        .unwrap();
    assert!(!exists, "nonexistent skill should return false");
}

async fn test_kask_skill_vote(db: &Arc<Database>) {
    db.insert_kask_skill_versions(&[
        make_version("alice", "bug-hunt", "1.0.0", "Bug hunting").await
    ])
    .await
    .unwrap();

    let user_id = UserId(1);

    let (up, down) = db
        .vote_kask_skill("alice", "bug-hunt", user_id, 1)
        .await
        .unwrap();
    assert_eq!(up, 1);
    assert_eq!(down, 0);

    let (up, down) = db
        .vote_kask_skill("alice", "bug-hunt", UserId(2), 1)
        .await
        .unwrap();
    assert_eq!(up, 2);
    assert_eq!(down, 0);

    let (up, down) = db
        .vote_kask_skill("alice", "bug-hunt", UserId(3), -1)
        .await
        .unwrap();
    assert_eq!(up, 2);
    assert_eq!(down, 1);

    let (up, down) = db
        .vote_kask_skill("alice", "bug-hunt", user_id, -1)
        .await
        .unwrap();
    assert_eq!(up, 1);
    assert_eq!(down, 2);
}

async fn test_kask_skill_uniqueness(db: &Arc<Database>) {
    db.insert_kask_skill_versions(&[make_version("alice", "bug-hunt", "1.0.0", "First").await])
        .await
        .unwrap();

    db.insert_kask_skill_versions(&[make_version("alice", "bug-hunt", "2.0.0", "Second").await])
        .await
        .unwrap();

    let skills = db.get_kask_skills().await.unwrap();
    let alice_skills: Vec<_> = skills
        .iter()
        .filter(|s| s.manifest.source_user == "alice" && s.manifest.skill_name == "bug-hunt")
        .collect();
    assert_eq!(
        alice_skills.len(),
        1,
        "duplicate insert should not create a second row"
    );
    assert_eq!(alice_skills[0].manifest.version, "2.0.0");
    assert_eq!(alice_skills[0].manifest.description, "Second");

    db.insert_kask_skill_versions(&[
        make_version("bob", "bug-hunt", "1.0.0", "Bob's bug hunt").await
    ])
    .await
    .unwrap();

    let skills = db.get_kask_skills().await.unwrap();
    assert_eq!(
        skills.len(),
        2,
        "different source_user should be a separate skill"
    );
}

/// zed-kask: pin the expiry filter (plan Phase 3 / D2) — a skill whose
/// signed `expires_at` has passed is not listed in the catalog, even before
/// the sweep runs.
async fn test_kask_skill_expiry_filter(db: &Arc<Database>) {
    db.insert_kask_skill_versions(&[
        make_version_with_expiry(
            "alice",
            "fresh",
            "1.0.0",
            "Fresh skill",
            "2999-01-01T00:00:00Z",
        )
        .await,
        make_version_with_expiry(
            "alice",
            "stale",
            "1.0.0",
            "Stale skill",
            "2020-01-01T00:00:00Z",
        )
        .await,
    ])
    .await
    .unwrap();

    let skills = db.get_kask_skills().await.unwrap();
    let ids: Vec<String> = skills.iter().map(|s| s.id.to_string()).collect();
    assert!(
        ids.contains(&"alice/fresh".to_string()),
        "unexpired skill must be listed: {ids:?}"
    );
    assert!(
        !ids.contains(&"alice/stale".to_string()),
        "expired skill must not be listed: {ids:?}"
    );
}

/// zed-kask: pin the expiry sweep (plan Phase 3 / D2) — expired versions are
/// purged and their now-orphaned skill rows are removed. Unparseable
/// `expires_at` counts as expired (fail closed, plan D5).
async fn test_kask_skill_expiry_sweep(db: &Arc<Database>) {
    db.insert_kask_skill_versions(&[
        make_version_with_expiry(
            "alice",
            "stale",
            "1.0.0",
            "Stale skill",
            "2020-01-01T00:00:00Z",
        )
        .await,
        make_version_with_expiry("bob", "kept", "1.0.0", "Kept skill", "2999-01-01T00:00:00Z")
            .await,
    ])
    .await
    .unwrap();

    let purged = db.purge_expired_kask_skill_versions().await.unwrap();
    assert_eq!(purged, 1, "exactly the expired version is purged");

    let skills = db.get_kask_skills().await.unwrap();
    let ids: Vec<String> = skills.iter().map(|s| s.id.to_string()).collect();
    assert!(
        ids.contains(&"bob/kept".to_string()),
        "unexpired skill must survive the sweep: {ids:?}"
    );
    assert!(
        !ids.contains(&"alice/stale".to_string()),
        "expired skill must be purged with its version: {ids:?}"
    );
}

// zed-kask: D30 — local fallback blob store for the no-S3 publish path.
// These pin the DB methods that back the local upload/download/delete
// branches in `api/kask_skills.rs`. The signed-manifest gate is unchanged
// (the catalog row still carries `public_key`/`signature`/`expires_at`); these
// tests cover only the raw tarball-byte store.
async fn test_kask_skill_tarball_round_trips_through_db(db: &Arc<Database>) {
    // Nothing stored initially.
    assert!(
        db.get_kask_skill_tarball("alice", "bug-hunt", "1.0.0")
            .await
            .unwrap()
            .is_none()
    );

    // Put + get round-trips the bytes verbatim.
    let tarball = b"tarball-bytes-v1".to_vec();
    db.put_kask_skill_tarball("alice", "bug-hunt", "1.0.0", tarball.clone())
        .await
        .unwrap();
    let fetched = db
        .get_kask_skill_tarball("alice", "bug-hunt", "1.0.0")
        .await
        .unwrap()
        .expect("tarball must be present after put");
    assert_eq!(fetched, b"tarball-bytes-v1");

    // Re-put of the same triple upserts (replaces the bytes).
    let tarball_v2 = b"tarball-bytes-v2".to_vec();
    db.put_kask_skill_tarball("alice", "bug-hunt", "1.0.0", tarball_v2.clone())
        .await
        .unwrap();
    let fetched = db
        .get_kask_skill_tarball("alice", "bug-hunt", "1.0.0")
        .await
        .unwrap()
        .expect("tarball must be present after re-put");
    assert_eq!(
        fetched, b"tarball-bytes-v2",
        "re-put of the same triple must replace, not duplicate"
    );

    // A different version is a distinct row (not overwritten by the upsert).
    db.put_kask_skill_tarball("alice", "bug-hunt", "2.0.0", b"v2-bytes".to_vec())
        .await
        .unwrap();
    let v1 = db
        .get_kask_skill_tarball("alice", "bug-hunt", "1.0.0")
        .await
        .unwrap()
        .expect("v1 must still be present after v2 put");
    assert_eq!(v1, b"tarball-bytes-v2");
    let v2 = db
        .get_kask_skill_tarball("alice", "bug-hunt", "2.0.0")
        .await
        .unwrap()
        .expect("v2 must be present");
    assert_eq!(v2, b"v2-bytes");
}

async fn test_kask_skill_tarball_delete_by_namespace(db: &Arc<Database>) {
    db.put_kask_skill_tarball("alice", "bug-hunt", "1.0.0", b"a".to_vec())
        .await
        .unwrap();
    db.put_kask_skill_tarball("alice", "bug-hunt", "2.0.0", b"b".to_vec())
        .await
        .unwrap();
    db.put_kask_skill_tarball("alice", "essentialist", "1.0.0", b"c".to_vec())
        .await
        .unwrap();
    db.put_kask_skill_tarball("bob", "bug-hunt", "1.0.0", b"d".to_vec())
        .await
        .unwrap();

    // Delete by (source_user, skill_name) clears all versions of that skill
    // for that publisher, leaving other namespaces untouched.
    let deleted = db
        .delete_kask_skill_tarballs("alice", "bug-hunt")
        .await
        .unwrap();
    assert_eq!(deleted, 2, "both versions of alice/bug-hunt are deleted");

    assert!(
        db.get_kask_skill_tarball("alice", "bug-hunt", "1.0.0")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.get_kask_skill_tarball("alice", "bug-hunt", "2.0.0")
            .await
            .unwrap()
            .is_none()
    );

    // Other namespaces survive.
    assert_eq!(
        db.get_kask_skill_tarball("alice", "essentialist", "1.0.0")
            .await
            .unwrap()
            .unwrap(),
        b"c"
    );
    assert_eq!(
        db.get_kask_skill_tarball("bob", "bug-hunt", "1.0.0")
            .await
            .unwrap()
            .unwrap(),
        b"d"
    );

    // Deleting an empty namespace is a no-op (0 rows affected), not an error.
    let deleted = db
        .delete_kask_skill_tarballs("carol", "missing")
        .await
        .unwrap();
    assert_eq!(deleted, 0);
}
