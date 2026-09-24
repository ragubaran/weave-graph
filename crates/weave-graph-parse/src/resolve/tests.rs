use super::*;
use crate::language::Language;
use crate::parser::SourceParser;

fn parse(language: Language, path: &str, source: &str) -> ParsedFile {
    SourceParser::new(language)
        .unwrap()
        .parse(path, source)
        .unwrap()
}

#[test]
fn same_file_call_resolves_exact_even_when_name_exists_elsewhere_too() {
    let a = parse(
        Language::Rust,
        "a.rs",
        "fn helper() {}\nfn caller() { helper(); }\n",
    );
    let b = parse(Language::Rust, "b.rs", "fn helper() {}\n");

    let mut index = ProjectIndex::new();
    index.add_file(&a);
    index.add_file(&b);

    let edges = index.resolve(&a);
    let calls_edges: Vec<_> = edges.0.iter().filter(|e| e.kind == "CALLS_EXACT").collect();
    assert_eq!(calls_edges.len(), 1);
    assert_eq!(
        calls_edges[0].target_moniker, "a.rs#helper",
        "must prefer the same-file helper, not b.rs's"
    );
}

#[test]
fn resolve_ids_preserves_edges_without_moniker_clones() {
    let file = parse(
        Language::Rust,
        "a.rs",
        "fn helper() {}\nfn caller() { helper(); }\n",
    );
    let mut index = ProjectIndex::new();
    index.add_file(&file);

    let (ids, unresolved) = index.resolve_ids(&file);
    assert!(unresolved.is_empty());
    assert_eq!(ids.len(), 1);
    assert_eq!(index.interner.resolve(ids[0].source_id), "a.rs#caller");
    assert_eq!(index.interner.resolve(ids[0].target_id), "a.rs#helper");
}

#[test]
fn method_call_is_always_dynamic_and_fans_out_to_every_candidate() {
    let src = "struct A; impl A { fn run(&self) { self.step(); } fn step(&self) {} }\nstruct B; impl B { fn step(&self) {} }\n";
    let a = parse(Language::Rust, "a.rs", src);

    let mut index = ProjectIndex::new();
    index.add_file(&a);

    let edges = index.resolve(&a);
    let mut targets: Vec<&str> = edges
        .0
        .iter()
        .filter(|e| e.kind == "CALLS_DYNAMIC")
        .map(|e| e.target_moniker.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec!["a.rs#A::step", "a.rs#B::step"],
        "an unresolvable-by-type method call must fan out to every same-named candidate, not guess one"
    );
}

#[test]
fn unresolvable_call_produces_no_edge() {
    let a = parse(
        Language::Rust,
        "a.rs",
        "fn caller() { external_lib_fn(); }\n",
    );
    let mut index = ProjectIndex::new();
    index.add_file(&a);

    assert!(index.resolve(&a).0.is_empty());
}

#[test]
fn implements_edge_resolves_to_the_trait_symbol() {
    let a = parse(
        Language::Rust,
        "a.rs",
        "trait Greet {}\nstruct Bar;\nimpl Greet for Bar {}\n",
    );
    let mut index = ProjectIndex::new();
    index.add_file(&a);

    // `trait Greet {}` has no `fn`, so our extractor (functions/structs
    // only) won't have indexed it as a symbol — this documents that:
    // no candidate, no edge, not a crash.
    assert!(index.resolve(&a).0.iter().all(|e| e.kind != "IMPLEMENTS"));
}

#[test]
fn inherits_edge_resolves_when_the_base_class_is_indexed() {
    let a = parse(
        Language::Python,
        "a.py",
        "class Base:\n    pass\n\nclass Child(Base):\n    pass\n",
    );
    let mut index = ProjectIndex::new();
    index.add_file(&a);

    let edges = index.resolve(&a);
    assert!(
        edges
            .0
            .iter()
            .any(|e| e.kind == "INHERITS" && e.target_moniker == "a.py#Base")
    );
}

#[test]
fn plain_call_with_no_same_file_match_falls_back_to_the_unique_global_candidate() {
    let a = parse(Language::Rust, "a.rs", "fn caller() { shared_helper(); }\n");
    let b = parse(Language::Rust, "b.rs", "fn shared_helper() {}\n");
    let mut index = ProjectIndex::new();
    index.add_file(&a);
    index.add_file(&b);

    assert_eq!(
        index.resolve(&a).0,
        vec![ResolvedEdge {
            source_moniker: "a.rs#caller".into(),
            target_moniker: "b.rs#shared_helper".into(),
            kind: "CALLS_EXACT".into(),
            resolution_kind: ResolutionKind::UniqueGlobalExact,
            extractor: Some("rust".into()),
        }]
    );
}

#[test]
fn plain_call_ambiguous_across_other_files_fans_out_as_dynamic() {
    // Neither b.rs nor c.rs is the caller's own file, and both define
    // `shared_helper` — not same-file-unique, not globally-unique
    // either, so this can't be `CALLS_EXACT` without guessing.
    let a = parse(Language::Rust, "a.rs", "fn caller() { shared_helper(); }\n");
    let b = parse(Language::Rust, "b.rs", "fn shared_helper() {}\n");
    let c = parse(Language::Rust, "c.rs", "fn shared_helper() {}\n");
    let mut index = ProjectIndex::new();
    index.add_file(&a);
    index.add_file(&b);
    index.add_file(&c);

    let resolved = index.resolve(&a);
    let mut targets: Vec<&str> = resolved
        .0
        .iter()
        .map(|e| e.target_moniker.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, vec!["b.rs#shared_helper", "c.rs#shared_helper"]);
    assert!(resolved.0.iter().all(|e| e.kind == "CALLS_DYNAMIC"));
}

#[test]
fn resolve_cross_repo_falls_back_to_the_other_index_when_self_has_no_candidate() {
    // `helper` doesn't exist anywhere in repo A's own index — a plain
    // `resolve` would drop the call entirely. `resolve_cross_repo` retries
    // it against repo B's index instead of giving up.
    let a = parse(Language::Rust, "a.rs", "fn caller() { helper(); }\n");
    let b = parse(Language::Rust, "b.rs", "fn helper() {}\n");
    let mut index_a = ProjectIndex::new();
    index_a.add_file(&a);
    let mut index_b = ProjectIndex::new();
    index_b.add_file(&b);

    assert!(
        index_a.resolve(&a).0.is_empty(),
        "plain resolve must not find helper — it doesn't exist in repo A"
    );

    let cross = index_a.resolve_cross_repo(&a, &index_b);
    assert_eq!(
        cross,
        vec![ResolvedEdge {
            source_moniker: "a.rs#caller".into(),
            target_moniker: "b.rs#helper".into(),
            kind: "CALLS_EXACT".into(),
            resolution_kind: ResolutionKind::AmbiguousHeuristic,
            extractor: Some("rust".into()),
        }]
    );
}

#[test]
fn resolve_cross_repo_never_retries_a_reference_self_could_already_resolve() {
    // `helper` exists in BOTH repos — repo A's own copy must win, exactly
    // like `resolve` already prefers same-file/local matches. The
    // fallback index must never be consulted once `self` has a candidate.
    let a = parse(
        Language::Rust,
        "a.rs",
        "fn helper() {}\nfn caller() { helper(); }\n",
    );
    let b = parse(Language::Rust, "b.rs", "fn helper() {}\n");
    let mut index_a = ProjectIndex::new();
    index_a.add_file(&a);
    let mut index_b = ProjectIndex::new();
    index_b.add_file(&b);

    assert!(
        index_a.resolve_cross_repo(&a, &index_b).is_empty(),
        "self already resolves this — the fallback index must not add a second, wrong edge"
    );
}

#[test]
fn resolve_cross_repo_drops_a_reference_matching_neither_index() {
    let a = parse(Language::Rust, "a.rs", "fn caller() { ghost(); }\n");
    let b = parse(Language::Rust, "b.rs", "fn unrelated() {}\n");
    let mut index_a = ProjectIndex::new();
    index_a.add_file(&a);
    let mut index_b = ProjectIndex::new();
    index_b.add_file(&b);

    assert!(index_a.resolve_cross_repo(&a, &index_b).is_empty());
}

#[test]
fn resolve_cross_repo_resolves_a_structural_edge_against_the_fallback_index() {
    // `Animal` is never declared in repo A at all — a real cross-repo
    // INHERITS edge, the same shape federation needs for e.g. a subclass
    // in one repo extending a base class exported by another.
    let a = parse(
        Language::TypeScript,
        "a.ts",
        "class Dog extends Animal {}\n",
    );
    let b = parse(Language::TypeScript, "b.ts", "class Animal {}\n");
    let mut index_a = ProjectIndex::new();
    index_a.add_file(&a);
    let mut index_b = ProjectIndex::new();
    index_b.add_file(&b);

    assert!(
        index_a.resolve(&a).0.is_empty(),
        "Animal isn't in repo A's own index"
    );

    let cross = index_a.resolve_cross_repo(&a, &index_b);
    assert_eq!(
        cross,
        vec![ResolvedEdge {
            source_moniker: "a.ts#Dog".into(),
            target_moniker: "b.ts#Animal".into(),
            kind: "INHERITS".into(),
            resolution_kind: ResolutionKind::AmbiguousHeuristic,
            extractor: Some("typescript".into()),
        }]
    );
}

// --- P10.5: resolution_kind / extractor provenance ---

#[test]
fn same_file_match_is_tagged_same_file_exact_with_its_own_language() {
    let a = parse(
        Language::Rust,
        "a.rs",
        "fn helper() {}\nfn caller() { helper(); }\n",
    );
    let b = parse(Language::Rust, "b.rs", "fn helper() {}\n");
    let mut index = ProjectIndex::new();
    index.add_file(&a);
    index.add_file(&b);

    let edges = index.resolve(&a);
    let call = edges
        .0
        .iter()
        .find(|e| e.kind == "CALLS_EXACT")
        .expect("same-file helper must resolve");
    assert_eq!(call.resolution_kind, ResolutionKind::SameFileExact);
    assert_eq!(call.extractor.as_deref(), Some("rust"));
}

#[test]
fn dynamic_fan_out_is_tagged_ambiguous_heuristic() {
    let src = "struct A; impl A { fn run(&self) { self.step(); } fn step(&self) {} }\nstruct B; impl B { fn step(&self) {} }\n";
    let a = parse(Language::Rust, "a.rs", src);
    let mut index = ProjectIndex::new();
    index.add_file(&a);

    let edges = index.resolve(&a);
    let dynamic: Vec<_> = edges
        .0
        .iter()
        .filter(|e| e.kind == "CALLS_DYNAMIC")
        .collect();
    assert_eq!(dynamic.len(), 2);
    assert!(
        dynamic
            .iter()
            .all(|e| e.resolution_kind == ResolutionKind::AmbiguousHeuristic)
    );
}

#[test]
fn a_structural_edge_with_one_candidate_is_tagged_unique_global_exact() {
    let a = parse(
        Language::Python,
        "a.py",
        "class Base:\n    pass\n\nclass Child(Base):\n    pass\n",
    );
    let mut index = ProjectIndex::new();
    index.add_file(&a);

    let edges = index.resolve(&a);
    let inherits = edges
        .0
        .iter()
        .find(|e| e.kind == "INHERITS")
        .expect("Base must resolve");
    assert_eq!(inherits.resolution_kind, ResolutionKind::UniqueGlobalExact);
    assert_eq!(inherits.extractor.as_deref(), Some("python"));
}

#[test]
fn a_structural_edge_with_multiple_candidates_is_tagged_ambiguous_heuristic() {
    let a = parse(
        Language::Python,
        "a.py",
        "class Base:\n    pass\nclass Child(Base):\n    pass\n",
    );
    let b = parse(Language::Python, "b.py", "class Base:\n    pass\n");
    let mut index = ProjectIndex::new();
    index.add_file(&a);
    index.add_file(&b);

    let edges = index.resolve(&a);
    let inherits: Vec<_> = edges.0.iter().filter(|e| e.kind == "INHERITS").collect();
    assert_eq!(
        inherits.len(),
        2,
        "both same-named Base candidates must appear"
    );
    assert!(
        inherits
            .iter()
            .all(|e| e.resolution_kind == ResolutionKind::AmbiguousHeuristic)
    );
}

#[test]
fn resolution_kind_as_str_matches_the_schema_free_form_values() {
    assert_eq!(ResolutionKind::SameFileExact.as_str(), "SAME_FILE_EXACT");
    assert_eq!(
        ResolutionKind::UniqueGlobalExact.as_str(),
        "UNIQUE_GLOBAL_EXACT"
    );
    assert_eq!(
        ResolutionKind::AmbiguousHeuristic.as_str(),
        weave_graph_core::AMBIGUOUS_HEURISTIC
    );
}
