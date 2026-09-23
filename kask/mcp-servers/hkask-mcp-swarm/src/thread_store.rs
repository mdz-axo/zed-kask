//! Durable, swarm-scoped dialogue turns. This encrypted database is separate
//! from agent semantic memory and does not participate in embedding retrieval.

use crate::error::LocalSwarmError;
use hkask_storage::DatabaseDriver;
use hkask_storage::database::value::DbValue;

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ThreadTurn {
    pub sequence: i64,
    pub agent_id: String,
    pub task: String,
    pub response: String,
    pub created_at: String,
}

pub struct SwarmThreadStore {
    path: String,
    passphrase: String,
    database: tokio::sync::OnceCell<hkask_storage::Database>,
    /// Hold across history read, inference, and append to prevent overlapping
    /// delegations from committing stale or duplicate histories.
    gate: tokio::sync::Mutex<()>,
}

impl SwarmThreadStore {
    pub fn new(path: String, passphrase: String) -> Self {
        Self {
            path,
            passphrase,
            database: tokio::sync::OnceCell::new(),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.gate.lock().await
    }

    async fn database(&self) -> Result<&hkask_storage::Database, LocalSwarmError> {
        self.database
            .get_or_try_init(|| async {
                if self.passphrase.len() < 8 {
                    return Err(LocalSwarmError::InvalidInput(
                        "HKASK_DB_PASSPHRASE must be at least eight characters for swarm threads"
                            .into(),
                    ));
                }
                if let Some(parent) = std::path::Path::new(&self.path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        LocalSwarmError::Io(format!("cannot create thread DB directory: {e}"))
                    })?;
                }
                let db =
                    hkask_storage::Database::open(&self.path, &self.passphrase).map_err(|e| {
                        LocalSwarmError::Database(format!("cannot open thread DB: {e}"))
                    })?;
                let pool = db.sqlite_pool().map_err(|e| {
                    LocalSwarmError::Database(format!("cannot connect to thread DB: {e}"))
                })?;
                hkask_storage::SqliteDriver::new(pool)
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS swarm_thread_turns (\
                         swarm_id TEXT NOT NULL, sequence INTEGER NOT NULL, \
                         agent_id TEXT NOT NULL, task TEXT NOT NULL, response TEXT NOT NULL, \
                         created_at TEXT NOT NULL, PRIMARY KEY (swarm_id, sequence));",
                    )
                    .map_err(|e| LocalSwarmError::Database(format!("thread schema: {e}")))?;
                Ok(db)
            })
            .await
    }

    pub async fn turns(&self, swarm_id: &str) -> Result<Vec<ThreadTurn>, LocalSwarmError> {
        let pool = self
            .database()
            .await?
            .sqlite_pool()
            .map_err(|e| LocalSwarmError::Database(format!("thread DB pool: {e}")))?;
        let driver = hkask_storage::SqliteDriver::new(pool);
        let rows = driver
            .query(
                "SELECT sequence, agent_id, task, response, created_at FROM swarm_thread_turns \
             WHERE swarm_id = ?1 ORDER BY sequence ASC",
                &[DbValue::Text(swarm_id.into())],
            )
            .map_err(|e| LocalSwarmError::Database(format!("thread query: {e}")))?;
        rows.iter()
            .map(|row| {
                Ok(ThreadTurn {
                    sequence: row.get_int(0)?,
                    agent_id: row.get_str(1)?.into(),
                    task: row.get_str(2)?.into(),
                    response: row.get_str(3)?.into(),
                    created_at: row.get_str(4)?.into(),
                })
            })
            .collect::<Result<Vec<_>, hkask_storage::database::types::DbError>>()
            .map_err(|e| LocalSwarmError::Database(format!("thread row: {e}")))
    }

    pub async fn append(
        &self,
        swarm_id: &str,
        agent_id: &str,
        task: &str,
        response: &str,
    ) -> Result<ThreadTurn, LocalSwarmError> {
        let pool = self
            .database()
            .await?
            .sqlite_pool()
            .map_err(|e| LocalSwarmError::Database(format!("thread DB pool: {e}")))?;
        let driver = hkask_storage::SqliteDriver::new(pool);
        let sequence = driver
            .query_optional(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM swarm_thread_turns WHERE swarm_id = ?1",
                &[DbValue::Text(swarm_id.into())],
            )
            .map_err(|e| LocalSwarmError::Database(format!("thread sequence: {e}")))?
            .ok_or_else(|| {
                LocalSwarmError::Database("thread sequence query returned no row".into())
            })?
            .get_int(0)
            .map_err(|e| LocalSwarmError::Database(format!("thread sequence: {e}")))?;
        let turn = ThreadTurn {
            sequence,
            agent_id: agent_id.into(),
            task: task.into(),
            response: response.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        driver.execute(
            "INSERT INTO swarm_thread_turns (swarm_id, sequence, agent_id, task, response, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                DbValue::Text(swarm_id.into()), DbValue::Integer(turn.sequence),
                DbValue::Text(turn.agent_id.clone()), DbValue::Text(turn.task.clone()),
                DbValue::Text(turn.response.clone()), DbValue::Text(turn.created_at.clone()),
            ],
        ).map_err(|e| LocalSwarmError::Database(format!("thread append: {e}")))?;
        Ok(turn)
    }
}
