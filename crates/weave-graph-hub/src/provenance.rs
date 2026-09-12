//! Snapshot provenance boundary (`impl.md` M3.6, feature `hub-provenance`):
//! Merkle-signed trust for a whole snapshot push/pull — separate from
//! `weave_graph_core::provenance::ProvenanceProvider` (M2.3), which signs
//! one doc link, never a multi-megabyte blob. Real signers (Lodestone
//! Nexus or any host) are wired by the deployment, never a dependency
//! here — same boundary shape M2.3 already established.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error("snapshot signature does not match for {repo_id}@{commit_sha}")]
    Tampered { repo_id: String, commit_sha: String },
}

/// Corrects a gap in `hub_enhancement_external.md`'s original sketch: its
/// `verify_snapshot` took no payload, so it could never actually detect a
/// tampered snapshot body — only a spoofed identity. Both methods here
/// sign/check the payload bytes, not just `(repo_id, commit_sha)`.
pub trait SnapshotProvenanceVerifier: Send + Sync {
    fn sign_snapshot(&self, repo_id: &str, commit_sha: &str, payload: &[u8]) -> Vec<u8>;

    fn verify_snapshot(
        &self,
        repo_id: &str,
        commit_sha: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<(), ProvenanceError>;
}

/// Reference implementation proving the boundary decouples, same role
/// `MockProvenanceProvider` plays for M2.3: FNV-1a stands in for a real
/// Merkle/signature primitive so the contract is exercised with zero new
/// dependencies. A real deployment supplies its own `AuthProvider`-style
/// implementation; this one is never wired against a live hub by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MockSnapshotProvenanceVerifier {
    key: u64,
}

impl MockSnapshotProvenanceVerifier {
    pub fn new() -> Self {
        Self {
            key: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Distinct keys sign incompatibly — one verifier can't check another's.
    pub fn with_key(key: u64) -> Self {
        Self { key }
    }
}

impl Default for MockSnapshotProvenanceVerifier {
    fn default() -> Self {
        Self::new()
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed ^ FNV_OFFSET, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// `\0` separators: without them `("a", "1", [2,3])` and `("a1", "", [2,3])`
/// could hash identically and distinct snapshots could collide.
fn merkle_root(repo_id: &str, commit_sha: &str, payload: &[u8]) -> u64 {
    let mut bytes = Vec::with_capacity(repo_id.len() + commit_sha.len() + 2);
    bytes.extend_from_slice(repo_id.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(commit_sha.as_bytes());
    bytes.push(0);
    let root_prefix = fnv1a(&bytes, 0);
    fnv1a(payload, root_prefix)
}

impl SnapshotProvenanceVerifier for MockSnapshotProvenanceVerifier {
    fn sign_snapshot(&self, repo_id: &str, commit_sha: &str, payload: &[u8]) -> Vec<u8> {
        let root = merkle_root(repo_id, commit_sha, payload);
        fnv1a(&root.to_le_bytes(), self.key).to_le_bytes().to_vec()
    }

    fn verify_snapshot(
        &self,
        repo_id: &str,
        commit_sha: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<(), ProvenanceError> {
        let expected = self.sign_snapshot(repo_id, commit_sha, payload);
        if expected == signature {
            Ok(())
        } else {
            Err(ProvenanceError::Tampered {
                repo_id: repo_id.to_string(),
                commit_sha: commit_sha.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests;
