//! Property layer for the portfolio ledger math
//! (`kask/docs/reference/testing-protocol.md`). Batch 3 of the propagation
//! plan (`kask/docs/reference/testing-protocol.md`): Phase 0 measured 59
//! value-assert tests and zero property sites here. The loop-closure layer
//! (`create_apply_batch_seed_returns_materialize_loop`, attribution
//! reconciliation, the missing-price degradation pins) is this crate's
//! strength and stays untouched. Each property states a falsifiable
//! hypothesis with its declared domain; a shrunk counterexample is a finding
//! to report, never a signal to weaken the property.

use super::*;
use proptest::prelude::*;

fn tx(
    id: &str,
    date: &str,
    tx_type: TxType,
    symbol: Option<&str>,
    quantity: Option<f64>,
    price: Option<f64>,
    commission: Option<f64>,
    amount: Option<f64>,
) -> Transaction {
    Transaction {
        id: id.to_string(),
        date: date.to_string(),
        tx_type,
        asset_type: AssetType::Stock,
        symbol: symbol.map(str::to_string),
        quantity,
        price,
        commission,
        amount,
        weight: None,
        currency: "USD".to_string(),
        notes: String::new(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

proptest! {
    /// Hypothesis: a buy-then-sell round trip moves exactly the spread minus
    /// commissions — no money vanishes or appears beyond
    /// q·(p_sell − p_buy) − (c_buy + c_sell), and the position nets to zero.
    #[test]
    fn round_trip_cash_flow_is_spread_minus_commissions(
        qty in 0.1f64..=1000.0,
        buy_price in 1.0f64..=500.0,
        sell_price in 1.0f64..=500.0,
        buy_commission in 0.0f64..=10.0,
        sell_commission in 0.0f64..=10.0,
    ) {
        let buy = tx(
            "b", "2024-01-10", TxType::Buy, Some("AAPL"),
            Some(qty), Some(buy_price), Some(buy_commission), None,
        );
        let sell = tx(
            "s", "2024-01-20", TxType::Sell, Some("AAPL"),
            Some(qty), Some(sell_price), Some(sell_commission), None,
        );
        let expected = qty * (sell_price - buy_price) - (buy_commission + sell_commission);
        prop_assert!(
            (buy.cash_flow() + sell.cash_flow() - expected).abs() < 1e-6,
            "round-trip cash must be spread minus commissions"
        );
        prop_assert!(
            (buy.position_delta() + sell.position_delta()).abs() < 1e-12,
            "round-trip positions must net to zero"
        );
    }

    /// Hypothesis: rolls and weight adjustments never touch cash — the
    /// documented CMP-index contract ("non-cash for the index level"). A roll
    /// moves only quantity; a weight adjustment moves nothing at all.
    #[test]
    fn cmp_index_transactions_never_move_cash(
        qty in 0.1f64..=1000.0,
        weight in 0.0f64..=1.0,
    ) {
        let roll = tx(
            "r", "2024-01-10", TxType::Roll, Some("CMP-7D"),
            Some(qty), None, None, None,
        );
        prop_assert_eq!(roll.cash_flow(), 0.0);
        prop_assert!((roll.position_delta() - qty).abs() < 1e-12);

        let adjust = Transaction {
            weight: Some(weight),
            ..tx(
                "w", "2024-01-10", TxType::WeightAdjust, Some("CMP-7D"),
                None, None, None, None,
            )
        };
        prop_assert_eq!(adjust.cash_flow(), 0.0);
        prop_assert_eq!(adjust.position_delta(), 0.0);
    }

    /// Hypothesis: the ledger read-back is exactly what was applied — for any
    /// generated buy/sell sequence, the recalled ledger carries every
    /// transaction and its per-symbol delta fold matches a reference fold
    /// over the applied deltas. A write path whose recall fold diverges is a
    /// loop silently dropped.
    #[test]
    fn ledger_read_back_folds_to_applied_deltas(
        cases in proptest::collection::vec(
            (1usize..=9, any::<bool>(), 0.1f64..=50.0),
            1..12,
        ),
    ) {
        let dir = tempfile::tempdir().expect("temporary directory");
        let store = PortfolioStore::with_dir(dir.path().to_path_buf());
        store.create("p", AssetType::Stock).expect("create portfolio");
        store
            .apply("p", &tx(
                "d", "2024-01-01", TxType::Deposit, None, None, None, None,
                Some(1_000_000.0),
            ))
            .expect("apply deposit");

        let mut reference = 0.0f64;
        for (i, (day, is_buy, qty)) in cases.iter().enumerate() {
            let tx_type = if *is_buy { TxType::Buy } else { TxType::Sell };
            let t = tx(
                &format!("t{i}"), &format!("2024-01-{day:02}"), tx_type, Some("AAPL"),
                Some(*qty), Some(100.0), Some(0.0), None,
            );
            store.apply("p", &t).expect("apply transaction");
            reference += t.position_delta();
        }

        let recalled = store.ledger("p", LedgerFilter::all()).expect("read ledger back");
        prop_assert_eq!(
            recalled.len(),
            cases.len() + 1,
            "every applied transaction must recall"
        );
        let recalled_fold: f64 = recalled
            .iter()
            .filter(|t| t.symbol.as_deref() == Some("AAPL"))
            .map(Transaction::position_delta)
            .sum();
        prop_assert!(
            (recalled_fold - reference).abs() < 1e-9,
            "recalled fold {recalled_fold} diverged from applied fold {reference}"
        );
    }

    /// Hypothesis: the snapshot projection conserves money and shares — a
    /// generated deposit followed by buys projects to exactly
    /// deposit − Σ(q·p + c) in cash and the exact share count, with the
    /// degradation surface (issues) empty for valid input.
    #[test]
    fn snapshot_projects_cash_and_positions_exactly(
        deposit in 1000.0f64..=100000.0,
        buys in proptest::collection::vec(
            (0.1f64..=100.0, 1.0f64..=500.0, 0.0f64..=9.99),
            1..8,
        ),
        day in 2usize..=9,
    ) {
        let dir = tempfile::tempdir().expect("temporary directory");
        let store = PortfolioStore::with_dir(dir.path().to_path_buf());
        store.create("p", AssetType::Stock).expect("create portfolio");
        store
            .apply("p", &tx(
                "d", "2024-01-01", TxType::Deposit, None, None, None, None,
                Some(deposit),
            ))
            .expect("apply deposit");

        let mut spent = 0.0f64;
        let mut shares = 0.0f64;
        for (i, (qty, price, commission)) in buys.iter().enumerate() {
            let t = tx(
                &format!("b{i}"), "2024-01-01", TxType::Buy, Some("MSFT"),
                Some(*qty), Some(*price), Some(*commission), None,
            );
            store.apply("p", &t).expect("apply buy");
            spent += qty * price + commission;
            shares += qty;
        }

        let snap = store
            .snapshot("p", &format!("2024-01-{day:02}"))
            .expect("snapshot");
        prop_assert!(
            (snap.cash_balance - (deposit - spent)).abs() < 1e-6,
            "cash {} must equal deposit − spend {}",
            snap.cash_balance,
            deposit - spent
        );
        let holding = snap
            .holdings
            .iter()
            .find(|h| h.symbol == "MSFT")
            .expect("MSFT holding present");
        prop_assert!(
            (holding.shares - shares).abs() < 1e-9,
            "shares {} must equal the applied quantity {shares}",
            holding.shares
        );
        prop_assert!(
            snap.issues.is_empty(),
            "valid input must surface no issues, got {:?}",
            snap.issues
        );
    }

    /// Hypothesis: deposits are never counted as return — under constant
    /// prices, a generated mid-window deposit leaves total_return at exactly
    /// zero and reports the deposit only as net cash flow (the conservation
    /// the missing-price fix protects).
    #[test]
    fn deposits_are_never_counted_as_return(
        initial_deposit in 1000.0f64..=100000.0,
        mid_deposit in 1.0f64..=50000.0,
        qty in 0.1f64..=100.0,
    ) {
        let dir = tempfile::tempdir().expect("temporary directory");
        let store = PortfolioStore::with_dir(dir.path().to_path_buf());
        store.create("p", AssetType::Stock).expect("create portfolio");
        store
            .apply("p", &tx(
                "d0", "2024-01-01", TxType::Deposit, None, None, None, None,
                Some(initial_deposit),
            ))
            .expect("apply initial deposit");
        store
            .apply("p", &tx(
                "b0", "2024-01-01", TxType::Buy, Some("AAPL"),
                Some(qty), Some(50.0), Some(0.0), None,
            ))
            .expect("apply buy");
        store
            .apply("p", &tx(
                "d1", "2024-01-05", TxType::Deposit, None, None, None, None,
                Some(mid_deposit),
            ))
            .expect("apply mid-window deposit");

        let resolver = CachedPriceResolver::new(&store, "p");
        resolver.seed_cache("AAPL", "2024-01-01", 50.0, "fixture")
            .expect("seed start price");
        resolver.seed_cache("AAPL", "2024-01-10", 50.0, "fixture")
            .expect("seed end price");

        let report = returns(&store, "p", "2024-01-01", "2024-01-10", &resolver)
            .expect("returns report");
        prop_assert!(
            report.total_return.abs() < 1e-9,
            "a zero-movement window with a deposit must report zero return, got {}",
            report.total_return
        );
        prop_assert!(
            (report.net_cash_flows - mid_deposit).abs() < 1e-9,
            "the deposit must surface as net cash flow, got {}",
            report.net_cash_flows
        );
    }
}
