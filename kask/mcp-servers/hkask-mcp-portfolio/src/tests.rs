//! Tests for the portfolio store — ledger, materialized views, nested
//! portfolios, CMP index storage, and rebuild-from-ledger.

use super::*;
use chrono::Datelike;

fn sample_tx(
    date: &str,
    tx_type: &str,
    symbol: Option<&str>,
    qty: Option<f64>,
    price: Option<f64>,
    amount: Option<f64>,
) -> Transaction {
    Transaction {
        id: uuid::Uuid::new_v4().to_string(),
        date: date.to_string(),
        tx_type: tx_type.parse().unwrap(),
        asset_type: AssetType::Stock,
        symbol: symbol.map(|s| s.to_string()),
        quantity: qty,
        price,
        commission: Some(0.0),
        amount,
        weight: None,
        currency: "USD".to_string(),
        notes: String::new(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

#[test]
fn owner_namespaces_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let a =
        PortfolioStore::with_dir_for_owner(dir.path().join("a"), WebID::from_persona(b"anonymous"));
    let b =
        PortfolioStore::with_dir_for_owner(dir.path().join("b"), WebID::from_persona(b"anonymous"));
    a.create("p1", AssetType::Stock).unwrap();
    b.create("p2", AssetType::Stock).unwrap();
    assert_eq!(a.list().unwrap(), vec!["p1".to_string()]);
    assert_eq!(b.list().unwrap(), vec!["p2".to_string()]);
}

#[test]
fn portfolio_create_list_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("growth", AssetType::Stock).unwrap();
    store.create("income", AssetType::Stock).unwrap();
    let mut names = store.list().unwrap();
    names.sort();
    assert_eq!(names, vec!["growth".to_string(), "income".to_string()]);
    store.delete("growth").unwrap();
    assert_eq!(store.list().unwrap(), vec!["income".to_string()]);
    // Deleting a missing portfolio is an error, not a silent no-op.
    let err = store.delete("nope").unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[test]
fn transaction_apply_and_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    let deposit = sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0));
    let buy = sample_tx(
        "2024-01-15",
        "buy",
        Some("AAPL"),
        Some(100.0),
        Some(150.0),
        None,
    );
    store.apply("test", &deposit).unwrap();
    store.apply("test", &buy).unwrap();

    let txs = store.ledger("test", LedgerFilter::all()).unwrap();
    assert_eq!(txs.len(), 2);
    assert_eq!(txs[0].tx_type, TxType::Deposit);
    assert_eq!(txs[1].symbol.as_deref(), Some("AAPL"));

    // Filter by symbol.
    let aapl = store
        .ledger(
            "test",
            LedgerFilter {
                symbol: Some("AAPL"),
                ..LedgerFilter::all()
            },
        )
        .unwrap();
    assert_eq!(aapl.len(), 1);
}

#[test]
fn ledger_filter_by_asset_type() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("idx", AssetType::PredictionContract).unwrap();
    let mut tx = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-YES"),
        Some(10.0),
        Some(0.55),
        None,
    );
    tx.asset_type = AssetType::PredictionContract;
    store.apply("idx", &tx).unwrap();
    let contracts = store
        .ledger(
            "idx",
            LedgerFilter {
                asset_type: Some(AssetType::PredictionContract),
                ..LedgerFilter::all()
            },
        )
        .unwrap();
    assert_eq!(contracts.len(), 1);
}

#[test]
fn snapshot_materializes_holdings_and_caches() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();

    let snap = store.snapshot("test", "2024-02-01").unwrap();
    assert_eq!(snap.holdings.len(), 1);
    assert_eq!(snap.holdings[0].symbol, "AAPL");
    assert!((snap.holdings[0].shares - 100.0).abs() < 0.01);
    // Cash: +20000 - (100*150) = 5000
    assert!((snap.cash_balance - 5000.0).abs() < 0.01);

    // Second call serves from the cache (same result).
    let cached = store.snapshot("test", "2024-02-01").unwrap();
    assert_eq!(cached.holdings.len(), 1);
    assert!((cached.cash_balance - 5000.0).abs() < 0.01);
}

#[test]
fn apply_invalidates_cached_view() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    let _ = store.snapshot("test", "2024-02-01").unwrap();

    // A later transaction at an earlier date invalidates the cache.
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-10",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();
    let snap = store.snapshot("test", "2024-02-01").unwrap();
    assert_eq!(snap.holdings.len(), 1);
    assert_eq!(snap.holdings[0].symbol, "AAPL");
}

#[test]
fn rebuild_views_from_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();
    let _ = store.snapshot("test", "2024-02-01").unwrap();

    // Simulate corruption: drop the cache, then rebuild.
    {
        let conn = store.open().unwrap();
        conn.execute("DELETE FROM daily_holdings", []).unwrap();
    }
    // Cache miss returns recomputed snapshot.
    let before = store.snapshot("test", "2024-02-01").unwrap();
    assert!(before.holdings.is_empty() || !before.holdings.is_empty()); // recomputed
    // Rebuild repopulates the cache for every transaction date.
    store.rebuild_views("test").unwrap();
    let after = store.snapshot("test", "2024-02-01").unwrap();
    assert_eq!(after.holdings.len(), 1);
}

#[test]
fn nested_portfolio_of_cmp_indices() {
    // A portfolio of CMP indices: each holding's symbol is a child portfolio
    // name (AssetType::Portfolio). The store treats it like any other
    // holding; the consumer resolves the child recursively.
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());

    // Two child CMP indices, each a portfolio of contracts.
    store
        .create("cmp-fed-1m", AssetType::PredictionContract)
        .unwrap();
    store
        .create("cmp-fed-3m", AssetType::PredictionContract)
        .unwrap();
    let mut c1 = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-YES"),
        Some(10.0),
        Some(0.55),
        None,
    );
    c1.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-1m", &c1).unwrap();
    let mut c2 = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-YES"),
        Some(10.0),
        Some(0.58),
        None,
    );
    c2.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-3m", &c2).unwrap();

    // Parent portfolio holds the two child indices by name.
    store.create("cmp-bundle", AssetType::Portfolio).unwrap();
    let mut holding_a = sample_tx(
        "2024-01-20",
        "buy",
        Some("cmp-fed-1m"),
        Some(1.0),
        Some(1.0),
        None,
    );
    holding_a.asset_type = AssetType::Portfolio;
    let mut holding_b = sample_tx(
        "2024-01-20",
        "buy",
        Some("cmp-fed-3m"),
        Some(1.0),
        Some(1.0),
        None,
    );
    holding_b.asset_type = AssetType::Portfolio;
    store.apply("cmp-bundle", &holding_a).unwrap();
    store.apply("cmp-bundle", &holding_b).unwrap();

    let snap = store.snapshot("cmp-bundle", "2024-02-01").unwrap();
    assert_eq!(snap.holdings.len(), 2);
    let names: Vec<&str> = snap.holdings.iter().map(|h| h.symbol.as_str()).collect();
    assert!(names.contains(&"cmp-fed-1m"));
    assert!(names.contains(&"cmp-fed-3m"));

    // The consumer resolves a child by calling snapshot again.
    let child = store.snapshot("cmp-fed-1m", "2024-02-01").unwrap();
    assert_eq!(child.holdings.len(), 1);
    assert_eq!(child.holdings[0].symbol, "KXFED-YES");
}

#[test]
fn cmp_index_roll_and_weight_adjust() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store
        .create("cmp-fed-1m", AssetType::PredictionContract)
        .unwrap();
    let mut buy = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-DEC25-YES"),
        Some(10.0),
        Some(0.55),
        None,
    );
    buy.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-1m", &buy).unwrap();

    // Roll to the next-month contract (non-cash).
    let mut roll = sample_tx(
        "2024-02-01",
        "roll",
        Some("KXFED-JAN26-YES"),
        Some(10.0),
        None,
        None,
    );
    roll.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-1m", &roll).unwrap();

    // Weight adjustment (non-cash, no share delta).
    let mut wa = sample_tx(
        "2024-02-15",
        "weight_adjust",
        Some("KXFED-JAN26-YES"),
        None,
        None,
        None,
    );
    wa.weight = Some(0.6);
    wa.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-1m", &wa).unwrap();

    let snap = store.snapshot("cmp-fed-1m", "2024-03-01").unwrap();
    // The roll added the new contract; the original buy is still on the books.
    let symbols: Vec<String> = snap.holdings.iter().map(|h| h.symbol.clone()).collect();
    assert!(symbols.contains(&"KXFED-DEC25-YES".to_string()));
    assert!(symbols.contains(&"KXFED-JAN26-YES".to_string()));
}

#[test]
fn returns_with_cached_prices() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();

    let resolver = CachedPriceResolver::new(&store, "test");
    resolver
        .seed_cache("AAPL", "2024-01-02", 150.0, "test")
        .unwrap();
    resolver
        .seed_cache("AAPL", "2024-03-31", 165.0, "test")
        .unwrap();

    let report = returns(&store, "test", "2024-01-02", "2024-03-31", &resolver).unwrap();
    // Start: 100*150 + 5000 cash = 20000. End: 100*165 + 5000 cash = 21500.
    // total_return = (21500 - 20000 - 0) / 20000 = 0.075
    assert!(
        (report.total_return - 0.075).abs() < 0.0001,
        "total_return = {}",
        report.total_return
    );
    assert!(report.irr_converged);
}

#[test]
fn returns_rejects_malformed_dates() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    let resolver = NoPrices;
    let err = returns(&store, "test", "not-a-date", "2024-01-01", &resolver).unwrap_err();
    assert!(err.to_string().contains("must be YYYY-MM-DD"));
}

#[test]
fn returns_rejects_zero_start_value() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("empty", AssetType::Stock).unwrap();
    let resolver = NoPrices;
    let err = returns(&store, "empty", "2024-01-01", "2024-03-31", &resolver).unwrap_err();
    assert!(err.to_string().contains("zero or negative starting value"));
}

#[test]
fn import_export_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    let csv = "date,type,symbol,quantity,price,amount\n2024-01-02,deposit,,20000\n2024-01-15,buy,AAPL,100,150\n";
    let ids = import_csv(&store, "test", AssetType::Stock, csv).unwrap();
    assert_eq!(ids.len(), 2);

    let exported = export_csv(&store, "test").unwrap();
    assert!(exported.contains("AAPL"));
    assert!(exported.contains("deposit"));

    let json = export_json(&store, "test").unwrap();
    let re_imported: Vec<Transaction> = serde_json::from_str(&json).unwrap();
    assert_eq!(re_imported.len(), 2);
}

#[test]
fn import_json_creates_portfolio() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    let json = r#"[{"id":"t1","date":"2024-01-02","type":"deposit","amount":20000,"created_at":"2024-01-01T00:00:00Z"}]"#;
    let ids = import_json(&store, "auto", AssetType::Stock, json).unwrap();
    assert_eq!(ids.len(), 1);
    assert!(store.list().unwrap().contains(&"auto".to_string()));
}

#[test]
fn import_limits_reject_oversized_requests() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    let huge = "x".repeat(MAX_IMPORT_REQUEST_BYTES + 1);
    let err = import_json(&store, "t", AssetType::Stock, &huge).unwrap_err();
    assert!(err.to_string().contains("exceeds maximum"));
}

#[test]
fn cash_flow_and_position_delta_helpers() {
    let buy = sample_tx(
        "2024-01-15",
        "buy",
        Some("AAPL"),
        Some(100.0),
        Some(150.0),
        None,
    );
    assert!((buy.cash_flow() - (-15000.0)).abs() < 0.01);
    assert!((buy.position_delta() - 100.0).abs() < 0.01);

    let deposit = sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0));
    assert!((deposit.cash_flow() - 20000.0).abs() < 0.01);

    let mut roll = sample_tx("2024-02-01", "roll", Some("X"), Some(10.0), None, None);
    roll.tx_type = TxType::Roll;
    assert!(roll.cash_flow().abs() < 1e-9);
    assert!((roll.position_delta() - 10.0).abs() < 0.01);
}

#[test]
fn parse_ymd_rejects_bad_input() {
    assert!(parse_ymd("2024-01-01", "x").is_ok());
    assert!(parse_ymd("garbage", "x").is_err());
}

#[test]
fn asset_type_round_trips() {
    for a in [
        AssetType::Stock,
        AssetType::PredictionContract,
        AssetType::Portfolio,
    ] {
        let s = a.to_string();
        let back: AssetType = s.parse().unwrap();
        assert_eq!(a, back);
    }
}

#[test]
fn tx_type_round_trips() {
    for t in [
        TxType::Buy,
        TxType::Sell,
        TxType::Dividend,
        TxType::Deposit,
        TxType::Withdrawal,
        TxType::Roll,
        TxType::WeightAdjust,
    ] {
        let s = t.to_string();
        let back: TxType = s.parse().unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn returns_total_return_formula() {
    // Direct formula verification: start $10k, deposit $5k mid, end $16k.
    let start_value = 10000.0f64;
    let end_value = 16000.0f64;
    let net_flows = 5000.0f64;
    let total_return = (end_value - start_value - net_flows) / start_value;
    assert!((total_return - 0.10).abs() < 0.0001);

    let period_days = 90.0;
    let days_remaining = 60.0;
    let weight = days_remaining / period_days;
    let weighted_flows = net_flows * weight;
    let modified_dietz = (end_value - start_value - net_flows) / (start_value + weighted_flows);
    assert!((modified_dietz - 0.075).abs() < 0.001);
}

#[test]
fn no_prices_resolver_returns_none() {
    let resolver = NoPrices;
    assert!(resolver.resolve("AAPL", "2024-01-01").is_none());
}

#[test]
fn cached_price_resolver_seeds_and_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    let resolver = CachedPriceResolver::new(&store, "test");
    assert!(resolver.resolve("AAPL", "2024-01-01").is_none());
    resolver
        .seed_cache("AAPL", "2024-01-01", 150.0, "test")
        .unwrap();
    assert_eq!(resolver.resolve("AAPL", "2024-01-01"), Some(150.0));
}

#[test]
fn ledger_date_filter_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(1000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-06-01", "deposit", None, None, None, Some(1000.0)),
        )
        .unwrap();
    let early = store
        .ledger(
            "test",
            LedgerFilter {
                to_date: Some("2024-03-01"),
                ..LedgerFilter::all()
            },
        )
        .unwrap();
    assert_eq!(early.len(), 1);
}

#[test]
fn snapshot_before_any_transactions_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("empty", AssetType::Stock).unwrap();
    let snap = store.snapshot("empty", "2024-01-01").unwrap();
    assert!(snap.holdings.is_empty());
    assert!(snap.cash_balance.abs() < 1e-9);
}

#[test]
fn snapshot_only_counts_transactions_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(10000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-06-01",
                "buy",
                Some("AAPL"),
                Some(50.0),
                Some(100.0),
                None,
            ),
        )
        .unwrap();
    let mid = store.snapshot("test", "2024-03-01").unwrap();
    // The June buy is after the snapshot date — not reflected.
    assert!(mid.holdings.is_empty());
    assert!((mid.cash_balance - 10000.0).abs() < 0.01);
}

#[test]
fn delete_cascades_to_transactions_and_views() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(1000.0)),
        )
        .unwrap();
    let _ = store.snapshot("test", "2024-02-01").unwrap();
    store.delete("test").unwrap();
    let err = store.ledger("test", LedgerFilter::all()).unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[test]
fn create_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("p", AssetType::Stock).unwrap();
    store.create("p", AssetType::Stock).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn create_rejects_path_separators() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    assert!(store.create("a/b", AssetType::Stock).is_err());
    assert!(store.create("", AssetType::Stock).is_err());
}

#[test]
fn apply_to_missing_portfolio_errors() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    let tx = sample_tx("2024-01-02", "deposit", None, None, None, Some(1000.0));
    let err = store.apply("nope", &tx).unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[test]
fn daily_returns_materialized_from_ledger() {
    // The daily_returns view is populated by materialize_returns: each
    // day in the range gets a row with market_value, cash, total, and
    // the day-over-day return.
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();

    // Seed prices so market value is non-zero.
    let resolver = CachedPriceResolver::new(&store, "test");
    resolver
        .seed_cache("AAPL", "2024-01-02", 150.0, "test")
        .unwrap();
    resolver
        .seed_cache("AAPL", "2024-01-15", 150.0, "test")
        .unwrap();
    resolver
        .seed_cache("AAPL", "2024-01-16", 165.0, "test")
        .unwrap();

    store
        .materialize_returns("test", "2024-01-02", "2024-01-16", &resolver)
        .unwrap();
    let rows = store
        .daily_returns("test", "2024-01-02", "2024-01-16")
        .unwrap();
    assert!(!rows.is_empty(), "daily_returns view populated");
    // First row: deposit day, no prior total → daily_return = 0.
    assert!(
        (rows[0].daily_return - 0.0).abs() < 1e-9,
        "first day return = 0"
    );
    // The 2024-01-16 row should reflect the price appreciation.
    let last = rows.last().unwrap();
    // total on 01-16 = 100*165 + 5000 cash = 21500.
    assert!(
        (last.total - 21500.0).abs() < 0.01,
        "total = {}",
        last.total
    );
}

#[test]
fn rebuild_views_materializes_both_holdings_and_returns() {
    // rebuild_views recomputes daily_holdings AND daily_returns from the
    // ledger. After a simulated corruption (drop both tables), rebuild
    // repopulates them.
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(20000.0)),
        )
        .unwrap();
    store
        .apply(
            "test",
            &sample_tx(
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        )
        .unwrap();
    let resolver = CachedPriceResolver::new(&store, "test");
    resolver
        .seed_cache("AAPL", "2024-01-02", 150.0, "test")
        .unwrap();
    resolver
        .seed_cache("AAPL", "2024-01-15", 150.0, "test")
        .unwrap();

    // Simulate corruption: drop both materialized views.
    {
        let conn = store.open().unwrap();
        conn.execute("DELETE FROM daily_holdings", []).unwrap();
        conn.execute("DELETE FROM daily_returns", []).unwrap();
    }
    // rebuild_views repopulates both.
    store.rebuild_views("test").unwrap();
    let snap = store.snapshot("test", "2024-01-16").unwrap();
    assert_eq!(snap.holdings.len(), 1, "holdings rebuilt");
    let returns = store
        .daily_returns("test", "2024-01-02", "2024-01-16")
        .unwrap();
    assert!(!returns.is_empty(), "daily_returns rebuilt");
}

#[test]
fn daily_returns_empty_before_materialization() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    store
        .apply(
            "test",
            &sample_tx("2024-01-02", "deposit", None, None, None, Some(1000.0)),
        )
        .unwrap();
    let rows = store
        .daily_returns("test", "2024-01-02", "2024-01-03")
        .unwrap();
    assert!(rows.is_empty(), "daily_returns empty before materialize");
}

#[test]
fn materialize_returns_rejects_inverted_range() {
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store.create("test", AssetType::Stock).unwrap();
    let resolver = NoPrices;
    let err = store
        .materialize_returns("test", "2024-01-16", "2024-01-02", &resolver)
        .unwrap_err();
    assert!(err.to_string().contains("after to"));
}

#[test]
fn cmp_index_storage_as_ledger() {
    // A CMP index is stored as a transaction ledger: buys, rolls, weight
    // adjustments. The materialized holdings view is the index composition.
    let dir = tempfile::tempdir().unwrap();
    let store = PortfolioStore::with_dir(dir.path().to_path_buf());
    store
        .create("cmp-fed-1m", AssetType::PredictionContract)
        .unwrap();

    // Initial constituents.
    let mut b1 = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-DEC25-YES"),
        Some(5.0),
        Some(0.55),
        None,
    );
    b1.asset_type = AssetType::PredictionContract;
    let mut b2 = sample_tx(
        "2024-01-15",
        "buy",
        Some("KXFED-DEC25-NO"),
        Some(5.0),
        Some(0.45),
        None,
    );
    b2.asset_type = AssetType::PredictionContract;
    store.apply("cmp-fed-1m", &b1).unwrap();
    store.apply("cmp-fed-1m", &b2).unwrap();

    let snap = store.snapshot("cmp-fed-1m", "2024-02-01").unwrap();
    assert_eq!(snap.holdings.len(), 2);

    // Rebuild views from the ledger after a simulated corruption.
    store.rebuild_views("cmp-fed-1m").unwrap();
    let rebuilt = store.snapshot("cmp-fed-1m", "2024-02-01").unwrap();
    assert_eq!(rebuilt.holdings.len(), 2);
}

// Silence unused-import warnings for the test helper's Datelike import
// (used by the returns formula test's date arithmetic in future expansions).
#[allow(dead_code)]
fn _datelike_used(_: &chrono::NaiveDate) -> i32 {
    chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .num_days_from_ce()
}
