//! `benches/slm_routing.rs` (`suges-slm.md` §4.3): TTFT and end-to-end
//! `weave ask` routing latency across the held-out set. The
//! deterministic fallback is benched always — it is the no-model
//! baseline and the graceful-degradation floor. The `llama-cli` model
//! path is benched only when `WEAVE_SLM_BENCH_MODEL` names a downloaded
//! model (weights are never fetched by a bench).
//!
//! `weave-graph-cli` is a bin-only package, so the module is included
//! by path; this file compiles empty without `--features slm`.
#![cfg(feature = "slm")]

#[path = "../src/slm.rs"]
mod slm;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use slm::{FuzzyRouter, HELD_OUT, HeldOutPrompt, IntentRouter, LlamaCliRouter};

fn route_all(router: &dyn IntentRouter) {
    for prompt in HELD_OUT.iter() {
        let table = symbol_table(prompt);
        black_box(router.route(prompt.question, &table).ok());
    }
}

fn symbol_table(prompt: &HeldOutPrompt) -> Vec<String> {
    prompt
        .symbol_table
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn bench_deterministic(c: &mut Criterion) {
    let mut group = c.benchmark_group("slm_routing");
    group.throughput(criterion::Throughput::Elements(HELD_OUT.len() as u64));
    group.bench_function("deterministic_held_out_set", |b| {
        b.iter(|| route_all(black_box(&FuzzyRouter)))
    });
    group.finish();
}

fn bench_model_router_when_available(c: &mut Criterion) {
    let mut group = c.benchmark_group("slm_routing");
    match std::env::var("WEAVE_SLM_BENCH_MODEL") {
        Ok(model) if slm::model_available(&model) => {
            let router = LlamaCliRouter::new(&model);
            group.bench_function("model_router_held_out_set", |b| {
                b.iter(|| route_all(black_box(&router)))
            });
        }
        Ok(model) => eprintln!(
            "skipping model bench: {model} not downloaded (weave slm pull first)"
        ),
        Err(_) => eprintln!(
            "skipping model bench: WEAVE_SLM_BENCH_MODEL unset (weights are never fetched by a bench)"
        ),
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_deterministic,
    bench_model_router_when_available
);
criterion_main!(benches);
