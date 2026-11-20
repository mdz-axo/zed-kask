use super::*;
use hkask_storage::database::driver::{DatabaseDriver, TransactionOps};
use hkask_storage::database::value::{DbRow, DbValue};
use hkask_types::DbError;

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

#[tokio::test]
async fn one_full_scan_drains_supported_owners() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, counter, provider) = observed_host(dir.path());
    let first = WebID::new();
    let second = WebID::new();
    let goals = [
        declare(&host, first).expect("first"),
        declare(&host, second).expect("second"),
    ];
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
    counter.fail.store(true, Ordering::SeqCst);
    assert!(
        host.drain_once()
            .await
            .expect_err("query failure must surface")
            .to_string()
            .contains("injected goal scan failure")
    );
    counter.fail.store(false, Ordering::SeqCst);
    host.drain_once().await.expect("scan recovery");
}

#[tokio::test]
async fn native_scan_preserves_canonical_validation_and_caller_separation() {
    let dir = tempfile::tempdir().expect("directory");
    let (host, _, _) = observed_host(dir.path());
    let owners = [WebID::new(), WebID::new()];
    let first = declare(&host, owners[0]).expect("first");
    let second = declare(&host, owners[1]).expect("second");
    let mut scanned = host.service.goal_scan_for_host().expect("native scan");
    for goal in [&first, &second] {
        let goals = scanned.remove(&goal.owner).expect("owner");
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].id, goal.id);
        assert_eq!(
            serde_json::to_value(goals).expect("native goals"),
            serde_json::to_value(host.service.goal_list(&goal.owner).expect("caller goals"))
                .expect("caller JSON")
        );
    }
    assert!(scanned.is_empty());
    let malformed = HMem::new(
        "kanban:goal",
        &hkask_types::GoalID::new().to_string(),
        serde_json::json!({"schema_version":3}),
        owners[0],
    );
    host.ledger.insert(&malformed).expect("malformed fixture");
    let error = host
        .drain_once()
        .await
        .expect_err("invalid scan must surface");
    assert!(error.to_string().contains("goal decode"), "{error}");
    let other = host.service.goal_list(&owners[1]).expect("other owner");
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].id, second.id);
    assert!(
        host.service
            .goal_list(&WebID::new())
            .expect("stranger")
            .is_empty()
    );
    host.ledger
        .delete_by_id(&malformed.id)
        .expect("remove malformed fixture");
    host.drain_once().await.expect("scan recovery");
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
