//! Benchmarks throughput on the hand-checked fixtures used to verify
//! correctness, tiled to a representative size — avoids vendoring huge
//! real-world repos into a crate with no other external data deps. Set
//! `WEAVE_BENCH_CORPUS_DIR` to bench a local checkout instead.

use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use weave_graph_parse::parse_file;

fn fixture(name: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    (path, source)
}

fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("WEAVE_BENCH_CORPUS_DIR").map(PathBuf::from)
}

/// Repeats the fixture's lines until the source is at least `target_bytes`
/// long, so throughput is measured against a file-sized input rather than
/// a handful of lines dominated by parser setup cost.
fn tiled_to(source: &str, target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + source.len());
    while out.len() < target_bytes {
        out.push_str(source);
    }
    out
}

fn bench_language(c: &mut Criterion, group_name: &str, fixture_name: &str) {
    let mut group = c.benchmark_group(group_name);

    if let Some(dir) = corpus_dir() {
        let ext = Path::new(fixture_name)
            .extension()
            .unwrap()
            .to_str()
            .unwrap();
        for entry in walk_files(&dir, ext) {
            let source = std::fs::read_to_string(&entry).unwrap();
            group.throughput(Throughput::Bytes(source.len() as u64));
            group.bench_with_input(
                BenchmarkId::new("corpus", entry.display().to_string()),
                &(entry.clone(), source),
                |b, (path, source)| {
                    b.iter(|| parse_file(path, source));
                },
            );
        }
    } else {
        let (path, base_source) = fixture(fixture_name);
        for size in [1_000usize, 50_000, 200_000] {
            let source = tiled_to(&base_source, size);
            group.throughput(Throughput::Bytes(source.len() as u64));
            group.bench_with_input(BenchmarkId::new("fixture", size), &source, |b, source| {
                b.iter(|| parse_file(&path, source));
            });
        }
    }

    group.finish();
}

fn walk_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path, ext));
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    out
}

fn parser_throughput(c: &mut Criterion) {
    bench_language(c, "parse/rust", "sample.rs");
    bench_language(c, "parse/python", "sample.py");
    bench_language(c, "parse/javascript", "sample.js");
    bench_language(c, "parse/typescript", "sample.ts");
    bench_language(c, "parse/go", "sample.go");
    bench_language(c, "parse/java", "sample.java");
    bench_language(c, "parse/c", "sample.c");
    bench_language(c, "parse/cpp", "sample.cpp");
    bench_language(c, "parse/csharp", "sample.cs");
    bench_language(c, "parse/kotlin", "sample.kt");
    bench_language(c, "parse/swift", "sample.swift");
    bench_language(c, "parse/scala", "sample.scala");
    bench_language(c, "parse/zig", "sample.zig");
    bench_language(c, "parse/ruby", "sample.rb");
    bench_language(c, "parse/php", "sample.php");
    bench_language(c, "parse/bash", "sample.sh");
    bench_language(c, "parse/powershell", "sample.ps1");
    bench_language(c, "parse/lua", "sample.lua");
    bench_language(c, "parse/json", "sample.json");
    bench_language(c, "parse/yaml", "sample.yaml");
    bench_language(c, "parse/toml", "sample.toml");
    bench_language(c, "parse/properties", "sample.properties");
    bench_language(c, "parse/sql", "sample.sql");
    bench_language(c, "parse/dart", "sample.dart");
    bench_language(c, "parse/elixir", "sample.ex");
    bench_language(c, "parse/html", "sample.html");
    bench_language(c, "parse/css", "sample.css");
    bench_language(c, "parse/r", "sample.R");
}

criterion_group!(benches, parser_throughput);
criterion_main!(benches);
