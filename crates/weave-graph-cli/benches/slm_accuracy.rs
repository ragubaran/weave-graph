//! `benches/slm_accuracy.rs` (`slm-spec.md` §4.3): router accuracy verification.
//! Measures fallback tool-selection and grounding rates against the held-out set;
//! establishes the deterministic baseline that any candidate model must match
//! to ensure zero invented symbols and predictable routing behavior.
//!
//! `weave-graph-cli` is a bin-only package, so the module is included
//! by path. `required-features = ["slm"]` in Cargo.toml skips building
//! this target entirely without the feature — an empty `#![cfg(...)]`
//! file has no `main`, which criterion's macro can't produce from nothing.
#![cfg(feature = "slm")]

#[path = "../src/slm.rs"]
// Standalone bench compilation sees only a slice of the module's
// internal API — dead-code analysis is meaningless in this context.
#[allow(dead_code)]
mod slm;

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use slm::{FuzzyRouter, HELD_OUT, HeldOutPrompt, IntentRouter};

fn symbol_table(prompt: &HeldOutPrompt) -> Vec<String> {
    prompt
        .symbol_table
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn bench_held_out_accuracy_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("slm_accuracy");
    group.throughput(criterion::Throughput::Elements(HELD_OUT.len() as u64));
    group.bench_function("deterministic_held_out_set", |b| {
        b.iter(|| {
            let mut tool_ok = 0usize;
            let mut ground_ok = 0usize;
            for prompt in HELD_OUT.iter() {
                let table = symbol_table(prompt);
                if let Ok(call) = FuzzyRouter.route(prompt.question, black_box(&table)) {
                    tool_ok += (call.tool == prompt.expected_tool) as usize;
                    ground_ok += (call.symbol.to_lowercase()
                        == prompt.expected_symbol.to_lowercase())
                        as usize;
                }
            }
            black_box((tool_ok, ground_ok))
        })
    });
    group.finish();
}

criterion_group!(benches, bench_held_out_accuracy_set);
criterion_main!(benches);
