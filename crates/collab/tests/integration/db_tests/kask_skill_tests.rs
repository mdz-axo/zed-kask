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

async fn make_version(
    source_user: &str,
    skill_name: &str,
    version: &str,
    description: &str,
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
        expires_at: "2027-01-01T00:00:00Z".into(),
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
