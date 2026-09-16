//! Cryptographic provenance boundary.
//! The trait is the interface — Lodestone Nexus or any host application
//! is one implementation behind it, never a dependency of this crate.
//! No networking: real providers are wired by the host, never here.

use crate::error::StorageError;
use crate::model::NodeId;

/// A Merkle-signed provenance record for one doc link: what was signed
/// (`doc_id`, `commit_hash`), the Merkle root over both, and the
/// provider's signature over that root. Round-trips through the
/// `doc_links.provenance_*` columns intact — `verify` recomputes both
/// the root and the signature from the record's own fields, so every
/// field must survive storage for tampering to stay detectable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub doc_id: NodeId,
    pub commit_hash: String,
    pub merkle_root: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyResult {
    /// Root and signature both recompute exactly.
    Verified,
    /// The record was altered after `attach`; `reason` names the check
    /// that failed.
    Tampered { reason: String },
}

pub trait ProvenanceProvider {
    /// Signs (`doc_id`, `commit_hash`) into a `Provenance` record.
    fn attach(&self, doc_id: NodeId, commit_hash: &str) -> Result<Provenance, StorageError>;

    /// Recomputes root and signature. A record tampered after `attach`
    /// is reported, never silently accepted.
    fn verify(&self, provenance: &Provenance) -> VerifyResult;
}

/// Reference implementation proving the boundary decouples:
/// a host app wires a real provider, tests
/// wire this one. FNV-1a stands in for the real hash/signature
/// primitives so the contract is exercised with zero new dependencies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MockProvenanceProvider {
    key: u64,
}

impl MockProvenanceProvider {
    pub fn new() -> Self {
        Self {
            key: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Distinct keys produce incompatible signatures — one provider
    /// cannot verify another's records.
    pub fn with_key(key: u64) -> Self {
        Self { key }
    }
}

impl Default for MockProvenanceProvider {
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

// `\0` separator: without it, ("1", "23") and ("12", "3") would hash
// identically and distinct records could collide.
fn merkle_root(doc_id: NodeId, commit_hash: &str) -> String {
    format!(
        "{:016x}",
        fnv1a(format!("{doc_id}\0{commit_hash}").as_bytes(), 0)
    )
}

fn sign(root: &str, key: u64) -> String {
    format!("{:016x}", fnv1a(root.as_bytes(), key))
}

impl ProvenanceProvider for MockProvenanceProvider {
    fn attach(&self, doc_id: NodeId, commit_hash: &str) -> Result<Provenance, StorageError> {
        let root = merkle_root(doc_id, commit_hash);
        Ok(Provenance {
            doc_id,
            commit_hash: commit_hash.to_string(),
            signature: sign(&root, self.key),
            merkle_root: root,
        })
    }

    fn verify(&self, provenance: &Provenance) -> VerifyResult {
        let expected_root = merkle_root(provenance.doc_id, &provenance.commit_hash);
        if provenance.merkle_root != expected_root {
            return VerifyResult::Tampered {
                reason: "merkle_root does not match (doc_id, commit_hash)".to_string(),
            };
        }
        if provenance.signature != sign(&provenance.merkle_root, self.key) {
            return VerifyResult::Tampered {
                reason: "signature does not verify under this provider".to_string(),
            };
        }
        VerifyResult::Verified
    }
}

#[cfg(test)]
mod tests;
