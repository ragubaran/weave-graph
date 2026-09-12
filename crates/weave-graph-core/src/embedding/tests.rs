use super::*;

#[test]
fn embed_produces_the_configured_dimension_count() {
    let provider = MockEmbeddingProvider::with_dimensions(16);
    assert_eq!(provider.embed("hello world").len(), 16);
    assert_eq!(provider.dimensions(), 16);
}

#[test]
fn embed_is_deterministic() {
    let provider = MockEmbeddingProvider::new();
    assert_eq!(
        provider.embed("check jwt ttl"),
        provider.embed("check jwt ttl")
    );
}

#[test]
fn embed_is_l2_normalized() {
    let provider = MockEmbeddingProvider::new();
    let v = provider.embed("check jwt ttl verify token");
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
}

#[test]
fn empty_text_is_the_zero_vector_not_a_panic() {
    let provider = MockEmbeddingProvider::new();
    let v = provider.embed("");
    assert!(v.iter().all(|&x| x == 0.0));
}

#[test]
fn shared_vocabulary_is_closer_than_disjoint_vocabulary() {
    let provider = MockEmbeddingProvider::new();
    let a = provider.embed("check jwt token expiry");
    let b = provider.embed("verify jwt token lifetime");
    let c = provider.embed("render html template layout");

    let cosine = |x: &[f32], y: &[f32]| -> f32 { x.iter().zip(y).map(|(a, b)| a * b).sum() };
    assert!(cosine(&a, &b) > cosine(&a, &c));
}
