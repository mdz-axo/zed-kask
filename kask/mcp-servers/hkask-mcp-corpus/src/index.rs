//! One owner for passage identity, durable publication, hydration and invalidation.
//! Store operations are synchronous and serialized with cache mutations. Inference
//! runs outside the lock; scoped publication permits let clear/purge cancel old work.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use hkask_mcp_server::server::McpToolError;
use hkask_memory::MemoryStore;
use serde_json::{Value, json};

use crate::helpers::{map_corpus_io_error, map_memory_store_error, open_memory_store};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Database(PathBuf),
    Ephemeral(String),
}

#[derive(Clone, Debug)]
pub(crate) struct IndexedPassage {
    pub text: Option<String>,
    pub metadata: Value,
    pub embedding: Vec<f32>,
}

pub(crate) enum PublicationScope {
    Entities(Vec<String>),
    Prefixes(Vec<String>),
}

impl PublicationScope {
    fn overlaps(&self, prefix: &str) -> bool {
        match self {
            Self::Entities(references) => references
                .iter()
                .any(|reference| reference.starts_with(prefix)),
            Self::Prefixes(prefixes) => prefixes
                .iter()
                .any(|watched| watched.starts_with(prefix) || prefix.starts_with(watched)),
        }
    }
}

pub(crate) struct Publication {
    origin: Origin,
    scope: PublicationScope,
    cancelled: AtomicBool,
}

impl Publication {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub(crate) struct DurableWrite {
    store: MemoryStore,
    publication: Arc<Publication>,
}

impl DurableWrite {
    pub fn is_cancelled(&self) -> bool {
        self.publication.is_cancelled()
    }

    pub fn ensure_active(&self) -> Result<(), McpToolError> {
        PassageIndex::check_publication(&self.publication)
    }
}

#[derive(Default)]
struct IndexState {
    passages: BTreeMap<(Origin, String), IndexedPassage>,
    active: Vec<Weak<Publication>>,
}

#[derive(Default)]
pub(crate) struct PassageIndex {
    state: Mutex<IndexState>,
}

fn database_origin(path: &str) -> Result<Origin, McpToolError> {
    std::fs::canonicalize(path)
        .map(Origin::Database)
        .map_err(|error| map_corpus_io_error(error, "Cannot canonicalize corpus database"))
}

impl PassageIndex {
    fn lock(&self) -> Result<MutexGuard<'_, IndexState>, McpToolError> {
        self.state
            .lock()
            .map_err(|_| McpToolError::internal("Passage index mutex poisoned")) // rr0044-ok: poisoned internal state
    }

    fn begin(state: &mut IndexState, origin: Origin, scope: PublicationScope) -> Arc<Publication> {
        let publication = Arc::new(Publication {
            origin,
            scope,
            cancelled: AtomicBool::new(false),
        });
        state.active.retain(|entry| entry.strong_count() > 0);
        state.active.push(Arc::downgrade(&publication));
        publication
    }

    pub fn begin_durable(
        &self,
        path: &str,
        passphrase: &str,
        scope: PublicationScope,
    ) -> Result<DurableWrite, McpToolError> {
        let mut state = self.lock()?;
        let store = open_memory_store(path, passphrase)?;
        let publication = Self::begin(&mut state, database_origin(path)?, scope);
        Ok(DurableWrite { store, publication })
    }

    pub fn begin_ephemeral(
        &self,
        source: &str,
        references: Vec<String>,
    ) -> Result<Arc<Publication>, McpToolError> {
        let mut state = self.lock()?;
        Ok(Self::begin(
            &mut state,
            Origin::Ephemeral(source.into()),
            PublicationScope::Entities(references),
        ))
    }

    /// expect: Completed writes are searchable once, with their original passage text.
    /// [P8] Motivating: durable and warm retrieval agree.
    /// pre: permit acquired before inference; post: store and cache are published together,
    /// or an explicit error is returned (replacement can be partially applied on DB error).
    pub fn publish_durable(
        &self,
        write: &DurableWrite,
        entity_ref: &str,
        text: &str,
        embedding: &[f32],
        model: &str,
    ) -> Result<(), McpToolError> {
        let mut state = self.lock()?;
        Self::check_publication(&write.publication)?;
        let key = (write.publication.origin.clone(), entity_ref.to_string());
        // MemoryStore inserts, rather than upserts. Evict before replacement so a
        // partial DB failure cannot leave a known-stale cache entry serving answers.
        state.passages.remove(&key);
        write
            .store
            .delete_embeddings_by_entity(entity_ref)
            .map_err(|error| map_memory_store_error(error, "Cannot replace passage embedding"))?;
        write
            .store
            .store_embedding(entity_ref, embedding, model, Some(text))
            .map_err(|error| {
                map_memory_store_error(
                    error,
                    "Cannot store passage embedding (prior embedding may have been removed)",
                )
            })?;
        state.passages.insert(
            key,
            IndexedPassage {
                text: Some(text.into()),
                metadata: json!({"entity_ref":entity_ref}),
                embedding: embedding.to_vec(),
            },
        );
        Ok(())
    }

    pub fn publish_ephemeral(
        &self,
        publication: &Publication,
        passages: &[(String, String)],
        vectors: Vec<Vec<f32>>,
    ) -> Result<usize, McpToolError> {
        let mut state = self.lock()?;
        Self::check_publication(publication)?;
        let Origin::Ephemeral(source) = &publication.origin else {
            return Err(McpToolError::invalid_argument(
                "Ephemeral publication requires an ephemeral origin",
            ));
        };
        for (position, ((entity_ref, text), embedding)) in passages.iter().zip(vectors).enumerate()
        {
            state.passages.insert(
                (publication.origin.clone(), entity_ref.clone()),
                IndexedPassage {
                    text: Some(text.clone()),
                    metadata: json!({"entity_ref":entity_ref,"source":source,"position":position}),
                    embedding,
                },
            );
        }
        Ok(passages.len())
    }

    fn check_publication(publication: &Publication) -> Result<(), McpToolError> {
        if publication.is_cancelled() {
            return Err(McpToolError::failed_precondition(
                "Passage publication cancelled by corpus_clear_index or an overlapping corpus_purge_qa; rerun the operation to publish new data",
            ));
        }
        Ok(())
    }

    /// Hydration is an empty-index fallback, never a per-query database selector.
    /// It finishes synchronously before query inference; clear/purge cannot race a
    /// detached DB snapshot back into the cache.
    pub fn hydrate_if_empty(
        &self,
        path: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<(), McpToolError> {
        let mut state = self.lock()?;
        if !state.passages.is_empty() {
            return Ok(());
        }
        let Some(path) = path else {
            return Ok(());
        };
        let passphrase = passphrase
            .map(str::to_owned)
            .unwrap_or_else(crate::helpers::default_corpus_passphrase);
        if passphrase.is_empty() {
            return Err(McpToolError::permission_denied(
                "HKASK_DB_PASSPHRASE not configured — corpus_query requires the DB passphrase",
            ));
        }
        let store = open_memory_store(path, &passphrase)?;
        let origin = database_origin(path)?;
        let entries = store
            .all_embeddings_with_text()
            .map_err(|error| map_memory_store_error(error, "DB hydration failed"))?;
        for (entity_ref, embedding, text) in entries {
            state.passages.insert(
                (origin.clone(), entity_ref.clone()),
                IndexedPassage {
                    text: text.filter(|text| !text.trim().is_empty()),
                    metadata: json!({"entity_ref":entity_ref}),
                    embedding,
                },
            );
        }
        Ok(())
    }

    pub fn retrieve(
        &self,
        query: &[f32],
        k: usize,
        min_score: f32,
    ) -> Result<Retrieval, McpToolError> {
        let state = self.lock()?;
        Ok(Retrieval {
            total_indexed: state.passages.len(),
            missing_text: state
                .passages
                .values()
                .filter(|passage| passage.text.is_none())
                .count(),
            matches: search_passages(state.passages.values(), query, k, min_score),
        })
    }

    pub fn clear(&self) -> Result<usize, McpToolError> {
        let mut state = self.lock()?;
        for publication in state.active.iter().filter_map(Weak::upgrade) {
            publication.cancelled.store(true, Ordering::Relaxed);
        }
        let count = state.passages.len();
        state.passages.clear();
        Ok(count)
    }

    /// expect: Purging one DB/prefix stops its warm results without losing other corpora.
    /// [P8] Motivating: provenance-aware invalidation.
    /// pre: DB opens; post: matching cache and pending publications are invalidated
    /// even when subsequent durable deletion fails; failures never claim zero counts.
    pub fn purge(&self, path: &str, passphrase: &str, prefix: &str) -> Result<Value, McpToolError> {
        let mut state = self.lock()?;
        let store = open_memory_store(path, passphrase)?;
        let origin = database_origin(path)?;
        state.passages.retain(|(entry_origin, entity_ref), _| {
            entry_origin != &origin || !entity_ref.starts_with(prefix)
        });
        for publication in state.active.iter().filter_map(Weak::upgrade) {
            if publication.origin == origin && publication.scope.overlaps(prefix) {
                publication.cancelled.store(true, Ordering::Relaxed);
            }
        }
        let before = store.embedding_count().map_err(|error| {
            map_memory_store_error(
                error,
                "Embedding count before purge failed; cache invalidated",
            )
        })?;
        let references = store.embeddings_by_prefix(prefix).map_err(|error| {
            map_memory_store_error(error, "Embedding query for purge failed; cache invalidated")
        })?;
        let mut purged = 0;
        // Unlike MemoryStore::purge_by_prefix, the per-entity API propagates
        // deletion errors. Exact starts_with also excludes SQL LIKE wildcards.
        for (entity_ref, _) in references {
            if entity_ref.starts_with(prefix) {
                purged += store
                    .delete_embeddings_by_entity(&entity_ref)
                    .map_err(|error| {
                        map_memory_store_error(
                            error,
                            "Embedding purge partially applied; cache invalidated",
                        )
                    })?;
            }
        }
        // Bulk literal-prefix deletion has no recall-query cap or SQL wildcard
        // expansion, so unrelated newer rows cannot hide matching older rows.
        let purged_h_mems = store
            .delete_h_mems_by_entity_prefix(prefix)
            .map_err(|error| {
                map_memory_store_error(
                    error,
                    "h_mem purge failed; embeddings already purged and cache invalidated",
                )
            })?;
        let after = store.embedding_count().map_err(|error| {
            map_memory_store_error(
                error,
                "Embedding count after purge failed; cache invalidated",
            )
        })?;
        Ok(
            json!({"prefix":prefix,"embeddings_before":before,"embeddings_purged":purged,"embeddings_after":after,"h_mems_purged":purged_h_mems,"h_mem_errors":0}),
        )
    }
}

pub(crate) fn validate_vectors(vectors: &[Vec<f32>], expected: usize) -> Result<(), McpToolError> {
    if vectors.len() != expected
        || vectors.iter().any(|vector| {
            vector.len() != crate::embedding_dim() || vector.iter().any(|value| !value.is_finite())
        })
    {
        return Err(McpToolError::unavailable(format!(
            "Embedding response must contain {expected} finite vectors of dimension {}; got {} vectors",
            crate::embedding_dim(),
            vectors.len()
        )));
    }
    Ok(())
}

/// Only selected matches are cloned; large corpus vectors stay under the owner.
pub(crate) struct RetrievedPassage {
    pub score: f32,
    pub passage: IndexedPassage,
}

pub(crate) struct Retrieval {
    pub total_indexed: usize,
    pub missing_text: usize,
    pub matches: Vec<RetrievedPassage>,
}

pub(crate) fn search_passages<'a>(
    passages: impl Iterator<Item = &'a IndexedPassage>,
    query: &[f32],
    k: usize,
    min_score: f32,
) -> Vec<RetrievedPassage> {
    let mut scored: Vec<_> = passages
        .map(|passage| (crate::cosine_similarity(query, &passage.embedding), passage))
        .filter(|(score, _)| min_score <= 0.0 || *score >= min_score)
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    scored.truncate(k);
    scored
        .into_iter()
        .map(|(score, passage)| RetrievedPassage {
            score,
            passage: passage.clone(),
        })
        .collect()
}
