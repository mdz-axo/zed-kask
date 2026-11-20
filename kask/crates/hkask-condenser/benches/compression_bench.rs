//! Condenser benchmark — measures compression throughput
//!
//! Run with: cargo bench -p hkask-condenser

use criterion::{Criterion, criterion_group, criterion_main};
use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::types::ContextCategory;
use std::hint::black_box;

fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compress");

    let inputs: Vec<(&str, String)> = vec![
        ("tiny_100B", "x".repeat(100)),
        ("small_1KB", "x".repeat(1_024)),
        ("medium_10KB", "x".repeat(10_240)),
        ("chat_50KB", "x".repeat(51_200)),
    ];

    for (label, input) in &inputs {
        group.bench_function(format!("heavy_{}", label), |b| {
            let mut engine = CondenserEngine::new();
            b.iter(|| {
                engine.compress(
                    "bench",
                    black_box(input),
                    Some(ContextCategory::ConversationHistory),
                );
            });
        });
    }

    group.finish();

    let mut tp = c.benchmark_group("throughput");
    let large = "The quick brown fox jumps over the lazy dog. ".repeat(200);
    tp.throughput(criterion::Throughput::Bytes(large.len() as u64));
    tp.bench_function("normal_9KB", |b| {
        let mut engine = CondenserEngine::new();
        b.iter(|| {
            engine.compress(
                "bench",
                black_box(&large),
                Some(ContextCategory::ConversationHistory),
            );
        });
    });
    let json_64k = format!(
        "{{\"content\":{{\"series_id\":\"BENCH_64K\",\"observations\":[{}]}},\"duration_ms\":12,\"usage\":null}}",
        (0..920)
            .map(|i| format!(
                "{{\"date\":\"2020-{:02}-{:02}\",\"country\":\"United States\",\"value\":{:.4}}}",
                (i % 12) + 1,
                (i % 28) + 1,
                if i == 250 {
                    -9.42
                } else if i == 710 {
                    14.85
                } else {
                    (i % 15) as f64 * 0.25
                }
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    tp.throughput(criterion::Throughput::Bytes(json_64k.len() as u64));
    tp.bench_function("structured_json_64KB", |b| {
        let mut engine = CondenserEngine::new();
        b.iter(|| {
            engine.compress("fred_get_observations", black_box(&json_64k), None);
        });
    });
    tp.bench_function("spillover_put_and_slice_64KB", |b| {
        b.iter(|| {
            let mut store = hkask_condenser::spillover::SpilloverStore::new();
            let receipt = store.put(
                "fred_get_observations",
                black_box(&json_64k),
                hkask_condenser::spillover::SpilloverPrivacy::Public,
            );
            let _ = store
                .read_slice(
                    &receipt.uri,
                    Some(1200),
                    Some(1300),
                    hkask_condenser::spillover::SpilloverPrivacy::Public,
                )
                .unwrap();
        });
    });
    tp.finish();
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
