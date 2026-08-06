//! Stage-2 contract fetch — fetch contracts per matched event only.

#![allow(unused_crate_dependencies)]

use hkask_mcp_prediction_markets::economic_object::BaseEconomicObject;
use hkask_mcp_prediction_markets::provider_kalshi::KalshiMarket;
use hkask_mcp_prediction_markets::semantic_mapping::{
    map_gamma_event, map_kalshi_event, MappedEvent,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct KalshiEvent {
    event_ticker: String,
    series_ticker: String,
    title: String,
    #[allow(dead_code)]
    sub_title: String,
    category: String,
}

#[derive(Debug, Deserialize, Default)]
struct GammaTag {
    label: String,
    #[allow(dead_code)]
    slug: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GammaEvent {
    id: String,
    title: String,
    slug: String,
    tags: Vec<GammaTag>,
    end_date: String,
    markets: Vec<GammaMarketEmbedded>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GammaMarketEmbedded {
    id: String,
    question: String,
    condition_id: String,
    #[allow(dead_code)]
    slug: String,
    #[allow(dead_code)]
    description: String,
    end_date: String,
    #[allow(dead_code)]
    outcomes: String,
    #[allow(dead_code)]
    outcome_prices: String,
    active: bool,
    closed: bool,
    #[allow(dead_code)]
    volume: String,
    volume_num: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    last_trade_price: Option<f64>,
    spread: Option<f64>,
    uma_resolution_status: String,
    #[allow(dead_code)]
    updated_at: String,
}

#[tokio::main]
async fn main() {
    let kalshi_events_path = PathBuf::from("tasks/bayesian-apt/catalogs/kalshi_events.jsonl");
    let gamma_events_path = PathBuf::from("tasks/bayesian-apt/catalogs/gamma_events.jsonl");
    let contracts_dir = PathBuf::from("tasks/bayesian-apt/catalogs/contracts");

    eprintln!("Stage 2: semantic mapping over event catalogs...");
    let mut all_mapped: Vec<MappedEvent> = Vec::new();
    let mut kalshi_event_tickers: Vec<String> = Vec::new();
    let mut gamma_event_records: Vec<GammaEvent> = Vec::new();

    if kalshi_events_path.exists() {
        let file = std::fs::File::open(&kalshi_events_path).expect("open kalshi events");
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line { Ok(l) => l, Err(_) => continue };
            if line.trim().is_empty() { continue; }
            let event: KalshiEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(mapped) = map_kalshi_event(
                &event.event_ticker, &event.series_ticker, &event.title, &event.category, None,
            ) {
                kalshi_event_tickers.push(event.event_ticker.clone());
                all_mapped.push(mapped);
            }
        }
    }

    if gamma_events_path.exists() {
        let file = std::fs::File::open(&gamma_events_path).expect("open gamma events");
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line { Ok(l) => l, Err(_) => continue };
            if line.trim().is_empty() { continue; }
            let event: GammaEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let tags: Vec<String> = event.tags.iter().map(|t| t.label.clone()).collect();
            if let Some(mapped) = map_gamma_event(
                &event.id, &event.title, &event.slug, &tags,
                if event.end_date.is_empty() { None } else { Some(&event.end_date) },
            ) {
                all_mapped.push(mapped);
                gamma_event_records.push(event);
            }
        }
    }

    eprintln!("  Matched {} Kalshi events, {} Gamma events",
        kalshi_event_tickers.len(), gamma_event_records.len());

    eprintln!("Stage 2: fetching Kalshi contracts per matched event...");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build reqwest client");

    let mut kalshi_contracts: HashMap<BaseEconomicObject, Vec<serde_json::Value>> = HashMap::new();
    let mut kalshi_fetched = 0u32;
    let mut kalshi_failed = 0u32;

    let event_to_object: HashMap<String, BaseEconomicObject> = all_mapped
        .iter()
        .filter(|e| e.event_id.starts_with("KX"))
        .filter_map(|e| e.base_object.map(|bo| (e.event_id.clone(), bo)))
        .collect();

    for event_ticker in &kalshi_event_tickers {
        let base_object = match event_to_object.get(event_ticker) {
            Some(bo) => *bo,
            None => continue,
        };
        let mut markets: Vec<KalshiMarket> = Vec::new();
        let mut success = false;
        for attempt in 0..3u32 {
            match fetch_kalshi_event_async(&client, event_ticker).await {
                Ok(m) => {
                    markets = m;
                    success = true;
                    break;
                }
                Err(e) => {
                    if attempt < 2 {
                        eprintln!("  retry {}/3 for {}: {}", attempt + 1, event_ticker, e);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1))).await;
                    } else {
                        eprintln!("  FAILED {}: {}", event_ticker, e);
                    }
                }
            }
        }
        if !success {
            kalshi_failed += 1;
            continue;
        }
        kalshi_fetched += 1;
        for market in &markets {
            let record = json!({
                "source": "kalshi",
                "event_ticker": event_ticker,
                "base_object": base_object.label(),
                "market_ticker": market.ticker,
                "title": market.title,
                "status": market.status,
                "close_time": market.close_time,
                "expiration_time": market.expiration_time,
                "yes_bid": market.yes_bid_dollars,
                "yes_ask": market.yes_ask_dollars,
                "volume_fp": market.volume_fp,
                "liquidity_dollars": market.liquidity_dollars,
                "result": market.result,
                "rules_primary": market.rules_primary,
            });
            kalshi_contracts.entry(base_object).or_default().push(record);
        }
    }
    eprintln!("  Kalshi: {} events fetched, {} failed, {} total contracts",
        kalshi_fetched, kalshi_failed,
        kalshi_contracts.values().map(|v| v.len()).sum::<usize>());

    eprintln!("Stage 2: extracting Gamma contracts from embedded markets...");
    let mut gamma_contracts: HashMap<BaseEconomicObject, Vec<serde_json::Value>> = HashMap::new();
    for event in &gamma_event_records {
        let base_object = all_mapped
            .iter()
            .find(|m| m.event_id == event.id)
            .and_then(|m| m.base_object);
        let base_object = match base_object {
            Some(bo) => bo,
            None => continue,
        };
        for market in &event.markets {
            let record = json!({
                "source": "gamma",
                "event_id": event.id,
                "base_object": base_object.label(),
                "market_id": market.id,
                "question": market.question,
                "condition_id": market.condition_id,
                "end_date": market.end_date,
                "closed": market.closed,
                "volume_num": market.volume_num,
                "best_bid": market.best_bid,
                "best_ask": market.best_ask,
                "last_trade_price": market.last_trade_price,
                "spread": market.spread,
                "uma_resolution_status": market.uma_resolution_status,
            });
            gamma_contracts.entry(base_object).or_default().push(record);
        }
    }
    eprintln!("  Gamma: {} total contracts from embedded markets",
        gamma_contracts.values().map(|v| v.len()).sum::<usize>());

    eprintln!("Stage 2: writing contracts to disk...");
    for object in BaseEconomicObject::ALL {
        let object_dir = contracts_dir.join(object.label());
        std::fs::create_dir_all(&object_dir).expect("create object dir");
        let kalshi_path = object_dir.join("kalshi.jsonl");
        let gamma_path = object_dir.join("gamma.jsonl");
        let kalshi_records = kalshi_contracts.get(&object).cloned().unwrap_or_default();
        let gamma_records = gamma_contracts.get(&object).cloned().unwrap_or_default();
        write_jsonl(&kalshi_path, &kalshi_records);
        write_jsonl(&gamma_path, &gamma_records);
        eprintln!("  {}: {} Kalshi + {} Gamma contracts",
            object.label(), kalshi_records.len(), gamma_records.len());
    }

    eprintln!();
    eprintln!("Stage 2: expiration inventory per base object:");
    eprintln!("─────────────────────────────────────────────────────────────────────");
    for object in BaseEconomicObject::ALL {
        let kalshi_records = kalshi_contracts.get(&object).cloned().unwrap_or_default();
        let gamma_records = gamma_contracts.get(&object).cloned().unwrap_or_default();
        inventory_expirations(object, &kalshi_records, &gamma_records);
    }

    eprintln!();
    eprintln!("Stage 2 complete. Contracts written to tasks/bayesian-apt/catalogs/contracts/");
}

async fn fetch_kalshi_event_async(
    client: &reqwest::Client,
    event_ticker: &str,
) -> Result<Vec<KalshiMarket>, String> {
    let url = format!(
        "https://external-api.kalshi.com/trade-api/v2/markets?limit=1000&event_ticker={}",
        event_ticker
    );
    let response = client.get(&url).send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| format!("body read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, &body[..body.len().min(200)]));
    }
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        markets: Vec<KalshiMarket>,
    }
    let resp: Resp = serde_json::from_str(&body).map_err(|e| format!("parse failed: {e}"))?;
    Ok(resp.markets)
}

fn write_jsonl(path: &PathBuf, records: &[serde_json::Value]) {
    if records.is_empty() {
        let _ = std::fs::write(path, "");
        return;
    }
    let mut file = std::fs::File::create(path).expect("create jsonl");
    for record in records {
        writeln!(file, "{}", record).expect("write jsonl line");
    }
}

fn inventory_expirations(
    object: BaseEconomicObject,
    kalshi: &[serde_json::Value],
    gamma: &[serde_json::Value],
) {
    let mut expirations: Vec<(String, String)> = Vec::new();
    for record in kalshi {
        if let Some(close_time) = record.get("close_time").and_then(|v| v.as_str()) {
            if !close_time.is_empty() {
                expirations.push(("kalshi".into(), close_time.to_string()));
            }
        }
    }
    for record in gamma {
        if let Some(end_date) = record.get("end_date").and_then(|v| v.as_str()) {
            if !end_date.is_empty() {
                expirations.push(("gamma".into(), end_date.to_string()));
            }
        }
    }
    let now = chrono::Utc::now();
    let mut days: Vec<(String, f64)> = Vec::new();
    for (source, ts) in &expirations {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
            let dt = dt.with_timezone(&chrono::Utc);
            let diff = (dt - now).num_days();
            if diff >= 0 {
                days.push((source.clone(), diff as f64));
            }
        }
    }
    days.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut bucket_1m = 0u32;
    let mut bucket_3m = 0u32;
    let mut bucket_6m = 0u32;
    let mut bucket_long = 0u32;
    for (_, d) in &days {
        if *d < 45.0 { bucket_1m += 1; }
        else if *d < 120.0 { bucket_3m += 1; }
        else if *d < 210.0 { bucket_6m += 1; }
        else { bucket_long += 1; }
    }
    eprintln!(
        "  {:<28} {:>4} contracts  1m(<45d):{:>3}  3m(45-120d):{:>3}  6m(120-210d):{:>3}  long(>210d):{:>3}",
        object.label(), days.len(), bucket_1m, bucket_3m, bucket_6m, bucket_long
    );
    if !days.is_empty() {
        let min = days.first().unwrap().1;
        let max = days.last().unwrap().1;
        let median = days[days.len() / 2].1;
        eprintln!("    min: {:.0}d  median: {:.0}d  max: {:.0}d", min, median, max);
    }
}
