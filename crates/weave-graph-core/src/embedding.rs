//! Embedding boundary (`impl.md` M3.7 Tier 2, feature `vector`): the
//! trait a real ONNX/`fastembed` model plugs into, plus a zero-ML
//! reference implementation exercising the same interface — same
//! "boundary here, real provider supplied by a deployment" shape already
//! used for `AuthProvider`/`ProvenanceProvider`/`SnapshotProvenanceVerifier`.

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("embedding provider failed: {0}")]
    Provider(String),
    #[error("embedding provider returned {actual} dimensions; expected {expected}")]
    Dimensions { actual: usize, expected: usize },
    #[error("embedding provider returned no vector")]
    Empty,
}

pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed(text)
    }

    fn dimensions(&self) -> usize;

    fn model_id(&self) -> &str;
}

/// Deterministic bag-of-hashed-words vector, L2-normalized. Not a real
/// embedding model — texts sharing vocabulary land closer together than
/// disjoint ones, which is enough to exercise real retrieval end to end
/// without an ML dependency this crate's Core Invariants forbid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MockEmbeddingProvider {
    dims: usize,
}

impl MockEmbeddingProvider {
    pub fn new() -> Self {
        Self { dims: 384 }
    }

    pub fn with_dimensions(dims: usize) -> Self {
        Self { dims }
    }
}

impl Default for MockEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

impl EmbeddingProvider for MockEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let mut vector = vec![0.0f32; self.dims];
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            if word.is_empty() {
                continue;
            }
            let bucket = (fnv1a(word.to_ascii_lowercase().as_bytes()) as usize) % self.dims;
            vector[bucket] += 1.0;
        }
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut vector {
                *x /= norm;
            }
        }
        Ok(vector)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn model_id(&self) -> &str {
        "mock-fnv-v1"
    }
}

#[cfg(test)]
mod tests;
