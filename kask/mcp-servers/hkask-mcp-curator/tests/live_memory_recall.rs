//! Opt-in verification through the real semantic tool, a read-only live store,
//! and the configured inference bridge. Default test runs never access live data.

use std::sync::Arc;

use hkask_mcp_curator::{CuratorDb, CuratorServer, CuratorStores};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct RepairedIdentity {
    previous_h_mem_id: String,
    h_mem_id: String,
}

#[tokio::test]
#[ignore = "Requires operator-approved live DB credentials, inference socket, repair identity file, and report path"]
async fn live_repaired_passages_resolve_to_their_own_rows() -> Result<(), Box<dyn std::error::Error>>
{
    let repairs: Vec<RepairedIdentity> =
        serde_json::from_slice(&std::fs::read(std::env::var("HKASK_LIVE_MEMORY_REPAIRS")?)?)?;
    assert!(!repairs.is_empty(), "provide explicit repair identities");
    let report_path = std::env::var("HKASK_LIVE_MEMORY_REPORT")?;
    let passphrase = hkask_mcp_server::server::resolve_db_passphrase(&Default::default())?;
    let memory = Arc::new(hkask_memory::MemoryStore::open_read_only(
        &std::env::var("HKASK_CURATOR_DB")?,
        &passphrase,
        hkask_storage::embedding_dim(),
    )?);
    let server = CuratorServer::new(
        std::env::var("HKASK_WEBID")?.parse()?,
        Arc::new(CuratorDb::from_stores(CuratorStores {
            regulation_store: None,
            memory: Some(Arc::clone(&memory)),
        })),
        hkask_inference::resolve_inference_port().await,
    );
    let mut results = Vec::with_capacity(repairs.len());
    for repair in repairs {
        assert!(
            memory
                .get_by_id(&repair.previous_h_mem_id.parse()?)?
                .is_none(),
            "old handle must be consumed"
        );
        let row = memory
            .get_by_id(&repair.h_mem_id.parse()?)?
            .expect("replacement exists");
        let passage =
            hkask_memory::semantic_passage_for_h_mem(&row).expect("canonical repaired passage");
        let response = server
            .curator_semantic_search(Parameters(
                hkask_mcp_curator::types::SemanticSearchRequest {
                    query: passage,
                    limit: Some(5),
                },
            ))
            .await?;
        let envelope: serde_json::Value = serde_json::from_str(&response)?;
        let content = &envelope["content"];
        assert_eq!(
            content["mode"], "semantic",
            "exact-entity degradation is not a semantic pass"
        );
        let hit = content["results"]
            .as_array()
            .expect("results array")
            .iter()
            .find(|hit| hit["id"].as_str() == Some(repair.h_mem_id.as_str()))
            .expect("named canonical passage must return its own replacement row");
        assert_eq!(
            envelope["usage"]["calls"], 1,
            "query must use real embedding inference"
        );
        println!(
            "semantic roundtrip {}: {} / {}",
            repair.h_mem_id, row.entity, row.attribute
        );
        results.push(json!({
            "previous_h_mem_id": repair.previous_h_mem_id,
            "h_mem_id": repair.h_mem_id,
            "entity": row.entity,
            "attribute": row.attribute,
            "mode": content["mode"],
            "distance": hit["distance"],
            "usage": envelope["usage"],
        }));
    }
    std::fs::write(
        report_path,
        serde_json::to_vec_pretty(&json!({
            "verified_rows": results.len(), "read_only": true, "results": results,
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires an operator-approved encrypted backup path and its DB credential"]
fn live_backup_preserves_vector_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let passphrase = hkask_mcp_server::server::resolve_db_passphrase(&Default::default())?;
    let database = hkask_storage::Database::open_read_only(
        &std::env::var("HKASK_LIVE_MEMORY_BACKUP")?,
        &passphrase,
    )?;
    let pool = database.sqlite_pool()?;
    let connection = pool.get()?;
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;
    let mismatches: i64 = connection.query_row(
        "SELECT COUNT(*) FROM embeddings e LEFT JOIN vec_embeddings v ON v.rowid=e.rowid \
         WHERE v.rowid IS NULL OR e.vector IS NOT v.embedding",
        [],
        |row| row.get(0),
    )?;
    println!("backup embedding rows: {count}; vector binding mismatches: {mismatches}");
    assert!(
        count > 0,
        "a nonempty backup is required for this verification"
    );
    assert_eq!(
        mismatches, 0,
        "backup metadata and vector-index rowids must agree"
    );
    Ok(())
}
