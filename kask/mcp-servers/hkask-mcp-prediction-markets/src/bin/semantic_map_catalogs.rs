//! Semantic mapping binary — runs the ontological mapping over the event
//! catalogs on disk and produces a mapped inventory per base economic object.
//!
//! Usage:
//!   cargo run -p hkask-mcp-prediction-markets --bin semantic_map_catalogs
//!
//! Reads:
//!   tasks/bayesian-apt/catalogs/kalshi_events.jsonl
//!   tasks/bayesian-apt/catalogs/gamma_events.jsonl
//!
//! Writes to stdout: a mapped inventory per base object, with event counts
//! and sample event titles, so the user can review the mapping before
//! proceeding to contract fetching.

#![allow(unused_crate_dependencies)]

use hkask_mcp_prediction_markets::economic_object::BaseEconomicObject;
use hkask_mcp_prediction_markets::semantic_mapping::{
    EventConstellation, MappedEvent, build_constellations, map_gamma_event, map_kalshi_event,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
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
}

fn main() {
    let kalshi_path = PathBuf::from("tasks/bayesian-apt/catalogs/kalshi_events.jsonl");
    let gamma_path = PathBuf::from("tasks/bayesian-apt/catalogs/gamma_events.jsonl");

    let mut all_mapped: Vec<MappedEvent> = Vec::new();
    let mut kalshi_total = 0u32;
    let mut kalshi_mapped = 0u32;
    let mut gamma_total = 0u32;
    let mut gamma_mapped = 0u32;

    // Kalshi events.
    if kalshi_path.exists() {
        let file = std::fs::File::open(&kalshi_path).expect("open kalshi events");
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.trim().is_empty() {
                continue;
            }
            let event: KalshiEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            kalshi_total += 1;
            if let Some(mapped) = map_kalshi_event(
                &event.event_ticker,
                &event.series_ticker,
                &event.title,
                &event.category,
                None,
            ) {
                kalshi_mapped += 1;
                all_mapped.push(mapped);
            }
        }
    }

    // Gamma events.
    if gamma_path.exists() {
        let file = std::fs::File::open(&gamma_path).expect("open gamma events");
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.trim().is_empty() {
                continue;
            }
            let event: GammaEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            gamma_total += 1;
            let tags: Vec<String> = event.tags.iter().map(|t| t.label.clone()).collect();
            if let Some(mapped) = map_gamma_event(
                &event.id,
                &event.title,
                &event.slug,
                &tags,
                if event.end_date.is_empty() {
                    None
                } else {
                    Some(&event.end_date)
                },
            ) {
                gamma_mapped += 1;
                all_mapped.push(mapped);
            }
        }
    }

    // Build constellations.
    let constellations = build_constellations(&all_mapped);

    // Print the mapped inventory.
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║        SEMANTIC EVENT MAPPING — MAPPED INVENTORY                     ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Catalog scan:");
    println!(
        "  Kalshi: {kalshi_mapped}/{kalshi_total} events mapped ({:.1}%)",
        pct(kalshi_mapped, kalshi_total)
    );
    println!(
        "  Gamma:  {gamma_mapped}/{gamma_total} events mapped ({:.1}%)",
        pct(gamma_mapped, gamma_total)
    );
    println!(
        "  Total:  {} events mapped across both venues",
        all_mapped.len()
    );
    println!();

    // Per-constellation summary.
    println!("Constellations by base economic object:");
    println!("─────────────────────────────────────────────────────────────────────");
    for constellation in &constellations {
        print_constellation(constellation);
    }

    // Per-object event counts table.
    println!();
    println!("Event counts per base object:");
    println!("┌──────────────────────────────┬──────────┬──────────┬──────────┐");
    println!("│ Base Object                  │ Kalshi   │ Gamma    │ Total    │");
    println!("├──────────────────────────────┼──────────┼──────────┼──────────┤");
    let mut by_object_venue: HashMap<BaseEconomicObject, (u32, u32)> = HashMap::new();
    for event in &all_mapped {
        let entry = by_object_venue
            .entry(
                event
                    .base_object
                    .unwrap_or(BaseEconomicObject::RealGdpGrowth),
            )
            .or_default();
        if event.series.starts_with("KX") || event.event_id.starts_with("KX") {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    for object in BaseEconomicObject::ALL {
        let (k, g) = by_object_venue.get(&object).copied().unwrap_or((0, 0));
        println!(
            "│ {:<28} │ {:<8} │ {:<8} │ {:<8} │",
            object.label(),
            k,
            g,
            k + g
        );
    }
    println!("└──────────────────────────────┴──────────┴──────────┴──────────┘");

    // Sample events per constellation (first 5 per object).
    println!();
    println!("Sample events per constellation (up to 5 per object):");
    println!("─────────────────────────────────────────────────────────────────────");
    for constellation in &constellations {
        println!();
        println!(
            "  {} ({}) — FIBO: {}",
            constellation.base_object.label(),
            constellation.events.len(),
            constellation.fibo_concept
        );
        for event in constellation.events.iter().take(5) {
            println!("    • [{}] {}", event.series, event.title);
        }
        if constellation.events.len() > 5 {
            println!("    ... and {} more", constellation.events.len() - 5);
        }
    }
}

fn pct(mapped: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        (mapped as f64 / total as f64) * 100.0
    }
}

fn print_constellation(constellation: &EventConstellation) {
    println!(
        "  {:<28} {:>4} events  FIBO: {}",
        constellation.base_object.label(),
        constellation.events.len(),
        constellation.fibo_concept
    );
}
