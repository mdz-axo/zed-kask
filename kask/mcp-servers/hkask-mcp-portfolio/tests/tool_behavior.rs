//! Tool-behavior contract tests for the portfolio MCP server.
//!
//! Drives the real `#[tool]` methods through their public `Parameters<T>`
//! seam over a temp-dir SQLite store — the repo testing standard
//! (docs/reference/mcp-servers/README.md §Testing standard). The store-level
//! suite in `src/tests.rs` pins the computation; this suite pins the TOOL
//! contracts: request validation, the seed→returns loop, the missing-price
//! gate (a data gap is an error naming the gap, never a zero valuation),
//! view invalidation, and error specificity.

#![forbid(unsafe_code)]

use hkask_mcp_portfolio::server::{
    LedgerApplyRequest, LedgerReadRequest, PortfolioAttributionRequest,
    PortfolioContributionRequest, PortfolioCreateRequest, PortfolioHistoricalWhatIfRequest,
    PortfolioNameRequest, PortfolioReturnsRequest, PortfolioServer, PortfolioSnapshotRequest,
    PortfolioWhatIfRequest, PriceSeedEntry, PriceSeedRequest, WhatIfPresentation,
};
use hkask_mcp_portfolio::{
    AssetType, ClassificationObservation, PortfolioStore, Transaction, TxType,
};
use hkask_types::WebID;
use hkask_types::spreadsheet::{CellEdit, EditTransaction, TableValue};
use rmcp::handler::server::wrapper::Parameters;

/// Extract the MCP tool-result envelope: `{"content": <value>}`.
fn unwrap_content(output: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(output)
        .unwrap_or_else(|e| panic!("tool output must be valid JSON, got: {output} ({e})"));
    parsed
        .get("content")
        .cloned()
        .unwrap_or_else(|| panic!("tool output must have 'content' key, got: {parsed}"))
}

fn make_server() -> (PortfolioServer, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "hkask-portfolio-tool-behavior-{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let owner = WebID::new();
    let store = PortfolioStore::with_dir_for_owner(dir.clone(), owner);
    let spreadsheet =
        hkask_spreadsheet::WorkbookService::start_with_root(dir.join("spreadsheet-workbooks"))
            .expect("spreadsheet engine actor");
    let server = PortfolioServer::new(WebID::new(), store, spreadsheet);
    (server, dir)
}

fn transaction(
    date: &str,
    tx_type: TxType,
    symbol: Option<&str>,
    quantity: Option<f64>,
    price: Option<f64>,
    amount: Option<f64>,
) -> Transaction {
    Transaction {
        id: uuid::Uuid::new_v4().to_string(),
        date: date.to_string(),
        tx_type,
        asset_type: AssetType::Stock,
        symbol: symbol.map(str::to_string),
        quantity,
        price,
        commission: Some(0.0),
        amount,
        weight: None,
        currency: "USD".to_string(),
        notes: String::new(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

/// The full tool-seam loop: create → apply → batch-seed → returns →
/// materialize → daily_returns. Pins the interaction pattern the
/// portfolio-review skill composes.
#[tokio::test]
async fn create_apply_batch_seed_returns_materialize_loop() {
    let (server, dir) = make_server();

    // Create + list.
    let output = server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "growth".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    assert_eq!(unwrap_content(&output)["status"], "created");
    let list = server.portfolio_list().await.expect("list ok");
    let list_content = unwrap_content(&list);
    let names: Vec<String> = list_content["portfolios"]
        .as_array()
        .expect("portfolios array")
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"growth".to_string()),
        "created portfolio must list, got: {names:?}"
    );

    // Apply a deposit and a buy through the ledger tool.
    for tx in [
        transaction(
            "2026-01-05",
            TxType::Deposit,
            None,
            None,
            None,
            Some(20_000.0),
        ),
        transaction(
            "2026-01-10",
            TxType::Buy,
            Some("AAPL"),
            Some(100.0),
            Some(150.0),
            None,
        ),
    ] {
        let output = server
            .ledger_apply(Parameters(LedgerApplyRequest {
                portfolio: "growth".into(),
                transaction: tx,
            }))
            .await
            .expect("apply ok");
        assert_eq!(unwrap_content(&output)["status"], "applied");
    }

    // Snapshot shows the position and cash.
    let snapshot = server
        .portfolio_snapshot(Parameters(PortfolioSnapshotRequest {
            portfolio: "growth".into(),
            date: "2026-01-10".into(),
        }))
        .await
        .expect("snapshot ok");
    let snapshot_content = unwrap_content(&snapshot);
    assert_eq!(
        snapshot_content["holdings"].as_array().map(Vec::len),
        Some(1),
        "one holding at the snapshot date, got: {snapshot_content}"
    );

    // Batch-seed prices — one call instead of N (the 2026-09-03 addition).
    let seed = server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "growth".into(),
            symbol: None,
            date: None,
            close: None,
            source: None,
            prices: Some(vec![
                PriceSeedEntry {
                    symbol: "AAPL".into(),
                    date: "2026-01-10".into(),
                    close: 150.0,
                    source: Some("test".into()),
                },
                PriceSeedEntry {
                    symbol: "AAPL".into(),
                    date: "2026-02-10".into(),
                    close: 165.0,
                    source: Some("test".into()),
                },
            ]),
        }))
        .await
        .expect("seed ok");
    let seed_content = unwrap_content(&seed);
    assert_eq!(seed_content["seeded_count"], 2, "both prices seeded");

    // Returns now compute (TWR over the seeded window). The 2026-01-05
    // deposit lands before the window start, so start_value = 5k cash +
    // 100 AAPL @ 150 = 20k; the gain is 100 × (165 − 150) = +1,500 → +7.5%.
    let returns = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-02-10".into(),
        }))
        .await
        .expect("returns ok");
    let returns_content = unwrap_content(&returns);
    let total_return = returns_content["total_return"]
        .as_f64()
        .expect("total_return is a number");
    assert!(
        (total_return - 0.075).abs() < 1e-9,
        "+1500 gain on a 20k start is +7.5%, got {total_return}: {returns_content}"
    );

    // Materialize + read the daily series.
    server
        .portfolio_materialize_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-01-12".into(),
        }))
        .await
        .expect("materialize ok");
    let daily = server
        .portfolio_daily_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-01-12".into(),
        }))
        .await
        .expect("daily ok");
    let daily_content = unwrap_content(&daily);
    let rows = daily_content["rows"]
        .as_array()
        .or_else(|| daily_content["daily_returns"].as_array())
        .unwrap_or_else(|| panic!("daily returns rows, got: {daily_content}"));
    assert_eq!(rows.len(), 3, "three calendar days materialized");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The missing-price gate through the TOOL seam: an unseeded holding errors
/// naming the (symbol, date) gap — never a zero valuation.
#[tokio::test]
async fn returns_tool_errors_naming_missing_prices() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "gap".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    server
        .ledger_apply(Parameters(LedgerApplyRequest {
            portfolio: "gap".into(),
            transaction: transaction(
                "2026-01-05",
                TxType::Buy,
                Some("MSFT"),
                Some(10.0),
                Some(380.0),
                None,
            ),
        }))
        .await
        .expect("apply ok");

    let error = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "gap".into(),
            from: "2026-01-05".into(),
            to: "2026-02-05".into(),
        }))
        .await
        .expect_err("returns must fail on the missing price");
    let error = error.message;
    assert!(
        error.contains("missing cached prices"),
        "the error must name the gap, got: {error}"
    );
    assert!(
        error.contains("MSFT"),
        "the error must name the symbol, got: {error}"
    );
    assert!(
        error.contains("portfolio_seed_price"),
        "the error must name the remedy, got: {error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Seeding invalidates materialized views from the seeded date forward —
/// materialize-then-seed never serves stale rows (pinned at the tool seam).
#[tokio::test]
async fn seed_tool_invalidates_materialized_views() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "invalidate".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    server
        .ledger_apply(Parameters(LedgerApplyRequest {
            portfolio: "invalidate".into(),
            transaction: transaction(
                "2026-01-05",
                TxType::Buy,
                Some("AAPL"),
                Some(10.0),
                Some(150.0),
                None,
            ),
        }))
        .await
        .expect("apply ok");
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "invalidate".into(),
            symbol: Some("AAPL".into()),
            date: Some("2026-01-05".into()),
            close: Some(150.0),
            source: Some("test".into()),
            prices: None,
        }))
        .await
        .expect("seed ok");
    server
        .portfolio_materialize_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "invalidate".into(),
            from: "2026-01-05".into(),
            to: "2026-01-07".into(),
        }))
        .await
        .expect("materialize ok");

    // Re-seed from 01-06: rows from that date forward are invalidated.
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "invalidate".into(),
            symbol: Some("AAPL".into()),
            date: Some("2026-01-06".into()),
            close: Some(160.0),
            source: Some("test".into()),
            prices: None,
        }))
        .await
        .expect("re-seed ok");
    let daily = server
        .portfolio_daily_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "invalidate".into(),
            from: "2026-01-05".into(),
            to: "2026-01-07".into(),
        }))
        .await
        .expect("daily ok");
    let content = unwrap_content(&daily);
    let rows = content["rows"]
        .as_array()
        .or_else(|| content["daily_returns"].as_array())
        .expect("rows");
    assert!(
        rows.iter().all(|row| {
            row["date"]
                .as_str()
                .map(|d| d < "2026-01-06")
                .unwrap_or(true)
        }),
        "rows from the seeded date forward must be invalidated, got: {rows:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Invalid input through the tool seam: malformed dates are rejected with
/// error-specific messages (never silently substituted).
#[tokio::test]
async fn returns_tool_rejects_malformed_dates() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "dates".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    let error = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "dates".into(),
            from: "not-a-date".into(),
            to: "2026-02-05".into(),
        }))
        .await
        .expect_err("malformed date must fail");
    let error = error.message;
    assert!(
        error.contains("from") || error.contains("date"),
        "the error must name the malformed field, got: {error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Delete removes the portfolio and its ledger.
#[tokio::test]
async fn delete_tool_removes_portfolio_and_ledger() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "doomed".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    let output = server
        .portfolio_delete(Parameters(PortfolioNameRequest {
            name: "doomed".into(),
        }))
        .await
        .expect("delete ok");
    assert_eq!(unwrap_content(&output)["status"], "deleted");
    let list = server.portfolio_list().await.expect("list ok");
    let list_content = unwrap_content(&list);
    let names: Vec<String> = list_content["portfolios"]
        .as_array()
        .expect("portfolios")
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        !names.contains(&"doomed".to_string()),
        "deleted portfolio is gone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn investor_reports_emit_viewer_hints_and_what_if_keeps_the_ledger_immutable() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "investor".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create portfolio");
    for tx in [
        transaction(
            "2025-01-01",
            TxType::Deposit,
            None,
            None,
            None,
            Some(1_000.0),
        ),
        transaction(
            "2025-01-01",
            TxType::Buy,
            Some("A"),
            Some(50.0),
            Some(10.0),
            None,
        ),
    ] {
        server
            .ledger_apply(Parameters(LedgerApplyRequest {
                portfolio: "investor".into(),
                transaction: tx,
            }))
            .await
            .expect("apply transaction");
    }
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "investor".into(),
            symbol: None,
            date: None,
            close: None,
            source: None,
            prices: Some(vec![
                PriceSeedEntry {
                    symbol: "A".into(),
                    date: "2025-01-01".into(),
                    close: 10.0,
                    source: Some("fixture".into()),
                },
                PriceSeedEntry {
                    symbol: "A".into(),
                    date: "2025-12-31".into(),
                    close: 11.0,
                    source: Some("fixture".into()),
                },
                PriceSeedEntry {
                    symbol: "B".into(),
                    date: "2025-12-31".into(),
                    close: 20.0,
                    source: Some("fixture".into()),
                },
            ]),
        }))
        .await
        .expect("seed prices");

    let contribution_output = server
        .portfolio_contribution(Parameters(PortfolioContributionRequest {
            portfolio: "investor".into(),
            from: "2025-01-01".into(),
            to: "2025-12-31".into(),
        }))
        .await
        .expect("contribution report");
    let contribution = unwrap_content(&contribution_output);
    let hint = contribution
        .get("display_hint")
        .and_then(serde_json::Value::as_str)
        .expect("server-authored portfolio display hint");
    assert!(hint.starts_with("```portfolio\n"));
    assert!(hint.contains("\"report_kind\":\"contribution\""));
    assert_eq!(contribution["report"]["reconciliation_residual"], 0.0);

    let before = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "investor".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await
        .expect("read ledger before");
    let before_count = unwrap_content(&before)["count"].clone();
    let what_if_output = server
        .portfolio_historical_what_if(Parameters(PortfolioHistoricalWhatIfRequest {
            portfolio: "investor".into(),
            from: "2025-01-01".into(),
            to: "2025-12-31".into(),
            hypothetical_transactions: vec![transaction(
                "2025-06-30",
                TxType::Buy,
                Some("B"),
                Some(10.0),
                Some(10.0),
                None,
            )],
        }))
        .await
        .expect("historical what-if report");
    let what_if = unwrap_content(&what_if_output);
    assert_eq!(what_if["report"]["authoritative_state_changed"], false);
    assert_eq!(what_if["report"]["value_difference"], 100.0);

    let after = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "investor".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await
        .expect("read ledger after");
    assert_eq!(unwrap_content(&after)["count"], before_count);

    std::fs::remove_dir_all(&dir).expect("remove test directory");
}

#[tokio::test]
async fn attribution_tool_reconciles_active_return_against_an_explicit_benchmark() {
    let (server, dir) = make_server();
    for name in ["portfolio", "benchmark"] {
        server
            .portfolio_create(Parameters(PortfolioCreateRequest {
                name: name.into(),
                asset_type: AssetType::Stock,
            }))
            .await
            .expect("create analysis portfolio");
    }
    for (portfolio, a_quantity, b_quantity) in
        [("portfolio", 60.0, 40.0), ("benchmark", 50.0, 50.0)]
    {
        for tx in [
            transaction(
                "2025-01-01",
                TxType::Deposit,
                None,
                None,
                None,
                Some(1_000.0),
            ),
            transaction(
                "2025-01-01",
                TxType::Buy,
                Some("A"),
                Some(a_quantity),
                Some(10.0),
                None,
            ),
            transaction(
                "2025-01-01",
                TxType::Buy,
                Some("B"),
                Some(b_quantity),
                Some(10.0),
                None,
            ),
        ] {
            server
                .ledger_apply(Parameters(LedgerApplyRequest {
                    portfolio: portfolio.into(),
                    transaction: tx,
                }))
                .await
                .expect("apply analysis transaction");
        }
        server
            .portfolio_seed_price(Parameters(PriceSeedRequest {
                portfolio: portfolio.into(),
                symbol: None,
                date: None,
                close: None,
                source: None,
                prices: Some(vec![
                    PriceSeedEntry {
                        symbol: "A".into(),
                        date: "2025-01-01".into(),
                        close: 10.0,
                        source: Some("fixture".into()),
                    },
                    PriceSeedEntry {
                        symbol: "B".into(),
                        date: "2025-01-01".into(),
                        close: 10.0,
                        source: Some("fixture".into()),
                    },
                    PriceSeedEntry {
                        symbol: "A".into(),
                        date: "2025-12-31".into(),
                        close: 12.0,
                        source: Some("fixture".into()),
                    },
                    PriceSeedEntry {
                        symbol: "B".into(),
                        date: "2025-12-31".into(),
                        close: 11.0,
                        source: Some("fixture".into()),
                    },
                ]),
            }))
            .await
            .expect("seed analysis prices");
    }

    let output = server
        .portfolio_attribution(Parameters(PortfolioAttributionRequest {
            portfolio: "portfolio".into(),
            benchmark: "benchmark".into(),
            from: "2025-01-01".into(),
            to: "2025-12-31".into(),
            classifications: vec![
                ClassificationObservation {
                    symbol: "A".into(),
                    group: "Growth".into(),
                },
                ClassificationObservation {
                    symbol: "B".into(),
                    group: "Value".into(),
                },
            ],
        }))
        .await
        .expect("attribution report");
    let report = unwrap_content(&output)["report"].clone();
    assert!(
        report["reconciliation_residual"]
            .as_f64()
            .is_some_and(|residual| residual.abs() < 1e-9),
        "attribution effects must reconcile to active return: {report}"
    );
    assert!(
        report["model"]
            .as_str()
            .is_some_and(|model| model.contains("Brinson-Fachler"))
    );

    std::fs::remove_dir_all(&dir).expect("remove test directory");
}

/// Phase 5 proving slice (plan §10 Phase 5, acceptance items 8-10; §11):
/// `portfolio_what_if` with `WorkbookWhatIf` presentation publishes the
/// hypothetical transaction set and report deltas as an immutable workbook
/// revision. The authoritative portfolio ledger never changes; staged
/// workbook edits never write through; the base revision stays
/// digest-intact after an applied edit mints a new revision.
#[tokio::test]
async fn what_if_workbook_is_immutable_and_never_touches_the_ledger() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "proving".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create portfolio");
    for tx in [
        transaction(
            "2025-01-01",
            TxType::Deposit,
            None,
            None,
            None,
            Some(1_000.0),
        ),
        transaction(
            "2025-01-01",
            TxType::Buy,
            Some("A"),
            Some(50.0),
            Some(10.0),
            None,
        ),
    ] {
        server
            .ledger_apply(Parameters(LedgerApplyRequest {
                portfolio: "proving".into(),
                transaction: tx,
            }))
            .await
            .expect("apply transaction");
    }
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "proving".into(),
            symbol: None,
            date: None,
            close: None,
            source: None,
            prices: Some(vec![PriceSeedEntry {
                symbol: "A".into(),
                date: "2025-12-31".into(),
                close: 12.0,
                source: Some("fixture".into()),
            }]),
        }))
        .await
        .expect("seed prices");

    let ledger_before = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "proving".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await
        .expect("read ledger before");
    let ledger_before = unwrap_content(&ledger_before);

    let output = server
        .portfolio_what_if(Parameters(PortfolioWhatIfRequest {
            portfolio: "proving".into(),
            date: "2025-12-31".into(),
            hypothetical_transactions: vec![transaction(
                "2025-12-31",
                TxType::Buy,
                Some("B"),
                Some(20.0),
                Some(10.0),
                None,
            )],
            observations: Vec::new(),
            presentation: WhatIfPresentation::WorkbookWhatIf,
        }))
        .await
        .expect("what-if with workbook presentation");
    let content = unwrap_content(&output);
    let hint = content["display_hint"].as_str().expect("display hint");
    assert!(hint.starts_with("```portfolio\n"));
    assert!(hint.contains("```spreadsheet\n"));

    // The ledger is byte-identical after publication.
    let ledger_after = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "proving".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await
        .expect("read ledger after");
    assert_eq!(unwrap_content(&ledger_after), ledger_before);

    // The workbook block parses as the strict wire contract.
    let body = hint
        .split("```spreadsheet\n")
        .nth(1)
        .and_then(|rest| rest.strip_suffix("\n```"))
        .expect("spreadsheet hint block");
    let block: hkask_types::spreadsheet::SpreadsheetBlock =
        serde_json::from_str(body).expect("block parses");
    block.validate().expect("block validates");
    assert_eq!(block.active_sheet, "What-if");
    assert!(block.mutation.is_dispatchable());
    assert!(block.artifact.validate().is_ok());

    // Staged edits never write through: stage on the live document, then a
    // FRESH ACTOR on the same root reads the revision from disk and still
    // shows the published values (the live handle intentionally keeps its
    // staged state; idempotent open).
    let document = server
        .spreadsheet
        .open(&block.artifact)
        .await
        .expect("open published workbook");
    document
        .stage(vec![CellEdit::SetCell {
            coordinate: hkask_types::spreadsheet::CellCoordinate::new("What-if".into(), 1, 1)
                .expect("coordinate"),
            value: TableValue::Number(9999.0),
        }])
        .await
        .expect("stage edit");
    let second_actor =
        hkask_spreadsheet::WorkbookService::start_with_root(dir.join("spreadsheet-workbooks"))
            .expect("second actor");
    let fresh = second_actor
        .open(&block.artifact)
        .await
        .expect("fresh open");
    let viewport = fresh
        .viewport(
            hkask_types::spreadsheet::SpreadsheetViewport::new("What-if".into(), 0, 0, 3, 4)
                .expect("viewport"),
        )
        .await
        .expect("viewport");
    assert_ne!(viewport.cells[1][1], TableValue::Number(9999.0));

    // An applied edit mints a new immutable revision; the base reopens
    // digest-intact (§11: a workbook edit cannot change the ledger
    // database — the base revision AND the ledger are both unchanged).
    let transaction = EditTransaction::new(
        block.artifact.clone(),
        "proving-slice-1".into(),
        hkask_types::spreadsheet::SpreadsheetAccess::WorkbookWhatIf,
        vec![CellEdit::SetCell {
            coordinate: hkask_types::spreadsheet::CellCoordinate::new("What-if".into(), 1, 1)
                .expect("coordinate"),
            value: TableValue::Number(4242.0),
        }],
    )
    .expect("transaction is valid");
    let publication = server
        .spreadsheet
        .apply(transaction)
        .await
        .expect("apply mints a new revision");
    match publication {
        hkask_spreadsheet::SpreadsheetPublication::Workbook { artifact, .. } => {
            assert_ne!(artifact.revision_id, block.artifact.revision_id);
        }
        other => panic!("expected a workbook publication, got {other:?}"),
    }
    server
        .spreadsheet
        .open(&block.artifact)
        .await
        .expect("base revision reopens digest-intact");

    let ledger_final = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "proving".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await
        .expect("read ledger final");
    assert_eq!(unwrap_content(&ledger_final), ledger_before);

    let _ = std::fs::remove_dir_all(&dir);
}
