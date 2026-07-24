//! Condenser benchmark — measures compression throughput
//!
//! Run with: cargo bench -p hkask-condenser

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::types::ContextCategory;

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
    tp.finish();
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
