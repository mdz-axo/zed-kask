use super::*;
use hkask_storage::database::driver::{DatabaseDriver, TransactionOps};
use hkask_storage::database::value::{DbRow, DbValue};
use hkask_types::DbError;
use tracing::instrument::WithSubscriber;

struct ScanCounter {
    inner: Arc<dyn DatabaseDriver>,
    scans: AtomicUsize,
    fail: AtomicBool,
}
impl DatabaseDriver for ScanCounter {
    fn execute(&self, sql: &str, params: &[DbValue]) -> Result<usize, DbError> {
        self.inner.execute(sql, params)
    }
    fn execute_batch(&self, sql: &str) -> Result<(), DbError> {
        self.inner.execute_batch(sql)
    }
    fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError> {
        if sql.contains("FROM hmems")
            && matches!(params, [DbValue::Text(entity)] if entity == "kanban:goal")
        {
            self.scans.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(DbError::Connection("injected goal scan failure".into()));
            }
        }
        self.inner.query(sql, params)
    }
    fn query_optional(&self, sql: &str, params: &[DbValue]) -> Result<Option<DbRow>, DbError> {
        self.inner.query_optional(sql, params)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }
    fn sqlite_pool(&self) -> Option<&r2d2::Pool<hkask_storage::SqliteConnectionManager>> {
        self.inner.sqlite_pool()
    }
    fn transaction(
        &self,
        operations: &mut dyn FnMut(&dyn TransactionOps) -> Result<(), DbError>,
    ) -> Result<(), DbError> {
        self.inner.transaction(operations)
    }
    fn is_durable(&self) -> bool {
        self.inner.is_durable()
    }
}
fn observed_host(directory: &std::path::Path) -> (Arc<GoalHost>, Arc<ScanCounter>, Arc<Provider>) {
    let provider = Arc::new(Provider::default());
    let mut host = open(directory, provider.clone()).expect("file-backed host");
    let counter = Arc::new(ScanCounter {
        inner: host.ledger.driver().clone(),
        scans: AtomicUsize::new(0),
        fail: AtomicBool::new(false),
    });
    let ledger = HMemStore::from_driver(counter.clone()).expect("observed ledger");
    let unshared = Arc::get_mut(&mut host).expect("host is not shared during fixture setup");
    unshared.service = KanbanService::new(ledger.clone());
    unshared.ledger = ledger;
    (host, counter, provider)
}
fn unsupported(host: &GoalHost, owner: WebID) -> HMem {
    let id = hkask_types::GoalID::new();
    let row = HMem::new(
        "kanban:goal",
        &id.to_string(),
        serde_json::json!({"schema_version":2,"id":id,"history":"retain bytes"}),
        owner,
    );
    host.ledger.insert(&row).expect("unsupported fixture");
    row
}
fn rejection_messages(warnings: &std::sync::Mutex<Vec<String>>) -> Vec<String> {
    warnings
        .lock()
        .expect("warnings")
        .iter()
        .filter(|message| message.contains("Goal row rejected"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn one_full_scan_drains_supported_owners_and_retains_rejected_bytes() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, counter, provider) = observed_host(dir.path());
    let first = WebID::new();
    let second = WebID::new();
    let rejected_only = WebID::new();
    let goals = [
        declare(&host, first).expect("first"),
        declare(&host, second).expect("second"),
    ];
    let rejected = [unsupported(&host, first), unsupported(&host, rejected_only)];
    let raw = format!(
        " {{ \"schema_version\": 2, \"id\": \"{}\" }}\n",
        rejected[0].attribute
    );
    host.ledger
        .driver()
        .execute(
            "UPDATE hmems SET value = ?1 WHERE id = ?2",
            &[
                DbValue::Text(raw.clone()),
                DbValue::Text(rejected[0].id.to_string()),
            ],
        )
        .expect("noncanonical whitespace fixture");
    // Prove the counter observes the real store operation before trusting its reading.
    counter.scans.store(0, Ordering::SeqCst);
    host.ledger
        .query_by_entity("kanban:goal")
        .expect("positive control");
    assert_eq!(counter.scans.load(Ordering::SeqCst), 1);
    counter.scans.store(0, Ordering::SeqCst);
    host.drain_once().await.expect("drain");
    assert_eq!(
        counter.scans.load(Ordering::SeqCst),
        1,
        "one scan, not one per owner plus enumeration"
    );
    for goal in goals {
        let current = host
            .service
            .goal_get(goal.id, goal.owner)
            .expect("read")
            .expect("goal");
        assert_eq!(current.indexed_revision, current.revision);
    }
    assert_eq!(provider.embedding_calls.load(Ordering::SeqCst), 2);
    let retained = host
        .ledger
        .driver()
        .query_optional(
            "SELECT value FROM hmems WHERE id = ?1",
            &[DbValue::Text(rejected[0].id.to_string())],
        )
        .expect("raw read")
        .expect("row");
    assert_eq!(retained.get_str(0).expect("stored bytes"), raw);
    let own = host.service.goal_list(&first).expect("explicit listing");
    assert_eq!(own.rejected.len(), 1);
    assert!(own.rejected[&rejected[0].attribute].contains("expected version 3"));
    assert!(!own.rejected.contains_key(&rejected[1].attribute));
    assert!(
        host.service
            .goal_list(&WebID::new())
            .expect("foreign listing")
            .rejected
            .is_empty()
    );
}

#[tokio::test]
async fn unchanged_rejections_are_quiet_but_changes_and_recurrences_warn() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, counter, _) = observed_host(dir.path());
    let owner = WebID::new();
    let row = unsupported(&host, owner);
    let warnings = Arc::new(std::sync::Mutex::new(Vec::new()));
    for _ in 0..2 {
        host.drain_once()
            .with_subscriber(RejectionWarnings(warnings.clone()))
            .await
            .expect("drain");
    }
    assert_eq!(
        rejection_messages(&warnings).len(),
        1,
        "unchanged failures must not repeat even with no supported work"
    );
    counter.fail.store(true, Ordering::SeqCst);
    assert!(
        host.drain_once()
            .await
            .expect_err("query failure must surface")
            .to_string()
            .contains("injected goal scan failure")
    );
    counter.fail.store(false, Ordering::SeqCst);
    host.drain_once()
        .with_subscriber(RejectionWarnings(warnings.clone()))
        .await
        .expect("recovery");
    assert_eq!(
        rejection_messages(&warnings).len(),
        1,
        "query failure must not erase the diagnostic snapshot"
    );
    host.ledger
        .driver()
        .execute(
            "UPDATE hmems SET value = ?1 WHERE id = ?2",
            &[
                DbValue::Text("{\"schema_version\":3}".into()),
                DbValue::Text(row.id.to_string()),
            ],
        )
        .expect("changed failure fixture");
    host.drain_once()
        .with_subscriber(RejectionWarnings(warnings.clone()))
        .await
        .expect("changed rejection");
    let messages = rejection_messages(&warnings);
    assert_eq!(messages.len(), 2);
    assert!(messages[1].contains(&row.attribute) && messages[1].contains("goal decode"));
    let new = unsupported(&host, owner);
    host.drain_once()
        .with_subscriber(RejectionWarnings(warnings.clone()))
        .await
        .expect("new rejection");
    assert_eq!(rejection_messages(&warnings).len(), 3);
    assert!(rejection_messages(&warnings)[2].contains(&new.attribute));
    // Simulate external resolution, then recurrence; the drainer must not keep an ever-seen set.
    host.ledger
        .delete_by_id(&row.id)
        .expect("fixture resolution");
    host.drain_once()
        .with_subscriber(RejectionWarnings(warnings.clone()))
        .await
        .expect("observe resolution");
    host.ledger.insert(&row).expect("fixture recurrence");
    host.drain_once()
        .with_subscriber(RejectionWarnings(warnings.clone()))
        .await
        .expect("recurrence");
    assert_eq!(rejection_messages(&warnings).len(), 4);
    assert!(rejection_messages(&warnings)[3].contains(&row.attribute));
}

#[tokio::test]
async fn native_scan_preserves_canonical_validation_and_caller_separation() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, _, _) = observed_host(dir.path());
    let owners = [WebID::new(), WebID::new()];
    let first = declare(&host, owners[0]).expect("first");
    let second = declare(&host, owners[1]).expect("second");
    let unsupported_first = unsupported(&host, owners[0]);
    unsupported(&host, owners[1]);
    let forged_id = hkask_types::GoalID::new();
    let mut forged = serde_json::to_value(&second).expect("goal JSON");
    forged["id"] = serde_json::json!(forged_id);
    let wrong_key = hkask_types::GoalID::new();
    let malformed = hkask_types::GoalID::new();
    for row in [
        HMem::new("kanban:goal", &forged_id.to_string(), forged, owners[0]),
        HMem::new(
            "kanban:goal",
            &wrong_key.to_string(),
            serde_json::to_value(&first).expect("goal JSON"),
            owners[0],
        ),
        HMem::new(
            "kanban:goal",
            &malformed.to_string(),
            serde_json::json!({"schema_version":3}),
            owners[0],
        ),
        HMem::new(
            "kanban:goal",
            &first.id.to_string(),
            serde_json::to_value(&first).expect("duplicate goal JSON"),
            owners[0],
        ),
    ] {
        host.ledger.insert(&row).expect("invalid canonical fixture");
    }
    let mut scanned = host.service.goal_scan_for_host().expect("native scan");
    let first_listing = scanned.remove(&owners[0]).expect("first owner");
    assert_eq!(first_listing.goals.len(), 1);
    assert_eq!(first_listing.goals[0].id, first.id);
    assert_eq!(first_listing.rejected.len(), 5);
    assert!(first_listing.rejected[&unsupported_first.attribute].contains("expected version 3"));
    assert!(
        first_listing.rejected[&forged_id.to_string()]
            .to_lowercase()
            .contains("permission denied")
    );
    assert!(first_listing.rejected[&wrong_key.to_string()].contains("key mismatch"));
    assert!(first_listing.rejected[&malformed.to_string()].contains("goal decode"));
    assert!(first_listing.rejected[&first.id.to_string()].contains("duplicate canonical identity"));
    assert_eq!(
        serde_json::to_value(first_listing).expect("native listing"),
        serde_json::to_value(host.service.goal_list(&owners[0]).expect("caller listing"))
            .expect("caller JSON")
    );
    let second_listing = scanned.remove(&owners[1]).expect("second owner");
    assert_eq!(second_listing.goals.len(), 1);
    assert_eq!(second_listing.goals[0].id, second.id);
    assert_eq!(second_listing.rejected.len(), 1);
    assert_eq!(
        serde_json::to_value(second_listing).expect("native listing"),
        serde_json::to_value(host.service.goal_list(&owners[1]).expect("caller listing"))
            .expect("caller JSON")
    );
    assert!(scanned.is_empty());
    let stranger = host
        .service
        .goal_list(&WebID::new())
        .expect("foreign listing");
    assert!(stranger.goals.is_empty() && stranger.rejected.is_empty());
}

#[tokio::test]
async fn bounded_work_retries_and_does_not_starve_other_owners() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, _, provider) = observed_host(dir.path());
    let owners = [WebID::new(), WebID::new()];
    let goals: Vec<_> = (0..GOALS_PER_TICK + 4)
        .map(|index| declare(&host, owners[index % owners.len()]).expect("goal"))
        .collect();
    provider.fail.store(true, Ordering::SeqCst);
    host.drain_once()
        .await
        .expect("failed work does not abort siblings");
    assert_eq!(
        provider.embedding_calls.load(Ordering::SeqCst),
        GOALS_PER_TICK
    );
    provider.fail.store(false, Ordering::SeqCst);
    for _ in 0..2 {
        let before = provider.embedding_calls.load(Ordering::SeqCst);
        host.drain_once().await.expect("retry work");
        assert!(provider.embedding_calls.load(Ordering::SeqCst) - before <= GOALS_PER_TICK);
    }
    for goal in goals {
        let current = host
            .service
            .goal_get(goal.id, goal.owner)
            .expect("read")
            .expect("goal");
        assert_eq!(
            current.indexed_revision, current.revision,
            "no owner is starved"
        );
    }
}
