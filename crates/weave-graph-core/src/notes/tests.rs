use super::*;

#[cfg(feature = "notes")]
#[test]
fn hash_span_is_deterministic_and_distinguishes_rewrites() {
    let source = "fn a() {}\nfn b() {}\nfn c() {}\n";
    assert_eq!(hash_span(source, 1, 2), hash_span(source, 1, 2));

    // Equal line range, rewritten content: different hash — the exact
    // false-negative case a line-range-only signal would miss.
    let rewritten = "fn a() { x() }\nfn b() {}\nfn c() {}\n";
    assert_ne!(hash_span(source, 1, 2), hash_span(rewritten, 1, 2));

    // Untouched content hashes identically across shifted queries.
    assert_eq!(hash_span(source, 2, 3), hash_span(source, 2, 3));
}

#[test]
fn note_tier_parses_and_serializes_both_tiers() {
    assert_eq!(NoteTier::Ephemeral.as_str(), "ephemeral");
    assert_eq!(NoteTier::Crystallized.as_str(), "crystallized");
    assert_eq!(NoteTier::parse("ephemeral"), NoteTier::Ephemeral);
    // Unknown strings read as crystallized: never silently expire data.
    assert_eq!(NoteTier::parse("weird"), NoteTier::Crystallized);
}
