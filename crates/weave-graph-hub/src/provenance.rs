//! Snapshot integrity verification for complete Hub payloads.
//! This is separate from document-link provenance because snapshots
//! require authentication of repository, commit, and payload together.

use std::sync::Arc;

use ed25519_dalek::{Signer, Verifier};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error("snapshot signature does not match for {repo_id}@{commit_sha}")]
    Tampered { repo_id: String, commit_sha: String },
    #[error("snapshot HMAC secret must contain at least 32 bytes")]
    WeakKey,
    #[error("ed25519 key must be exactly 32 bytes, got {0}")]
    InvalidEd25519KeyLength(usize),
    #[error("ed25519 signature must be exactly 64 bytes, got {0}")]
    InvalidEd25519SignatureLength(usize),
    #[error("unknown provenance provider {0:?} — expected \"hmac\" or \"ed25519\"")]
    UnknownProvider(String),
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

/// Deterministic non-cryptographic verifier retained for isolated tests.
/// Production registry construction must use the HMAC verifier below.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HmacSnapshotProvenanceVerifier {
    key: Vec<u8>,
}

impl HmacSnapshotProvenanceVerifier {
    pub fn new(key: impl AsRef<[u8]>) -> Result<Self, ProvenanceError> {
        let key = key.as_ref();
        if key.len() < 32 {
            return Err(ProvenanceError::WeakKey);
        }
        Ok(Self { key: key.to_vec() })
    }
}

fn snapshot_message(repo_id: &str, commit_sha: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(repo_id.len() + commit_sha.len() + payload.len() + 48);
    message.extend_from_slice(b"weave-graph:snapshot:v1");
    for field in [repo_id.as_bytes(), commit_sha.as_bytes(), payload] {
        message.extend_from_slice(&(field.len() as u64).to_be_bytes());
        message.extend_from_slice(field);
    }
    message
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;
    let mut normalized = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = Sha256::digest(key);
        normalized[..digest.len()].copy_from_slice(&digest);
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK_SIZE];
    let mut outer_pad = [0x5cu8; BLOCK_SIZE];
    for ((inner, outer), key_byte) in inner_pad
        .iter_mut()
        .zip(outer_pad.iter_mut())
        .zip(normalized)
    {
        *inner ^= key_byte;
        *outer ^= key_byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    outer.finalize().to_vec()
}

impl SnapshotProvenanceVerifier for HmacSnapshotProvenanceVerifier {
    fn sign_snapshot(&self, repo_id: &str, commit_sha: &str, payload: &[u8]) -> Vec<u8> {
        hmac_sha256(&self.key, &snapshot_message(repo_id, commit_sha, payload))
    }

    fn verify_snapshot(
        &self,
        repo_id: &str,
        commit_sha: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<(), ProvenanceError> {
        let expected = self.sign_snapshot(repo_id, commit_sha, payload);
        if weave_graph_core::auth::constant_time_eq(&expected, signature) {
            Ok(())
        } else {
            Err(ProvenanceError::Tampered {
                repo_id: repo_id.to_string(),
                commit_sha: commit_sha.to_string(),
            })
        }
    }
}

/// Asymmetric alternative to the HMAC verifier above, for deployments that
/// want to interoperate with existing open-source EdDSA tooling (e.g. a
/// signature produced by `ssh-keygen -Y sign` or any other Ed25519 signer)
/// instead of a bespoke HMAC scheme. Built from a 32-byte seed exactly like
/// `HmacSnapshotProvenanceVerifier` is built from a shared secret — both
/// sides of a `weave sync push`/`weave-registry` deployment hold the same
/// seed file, so this is still a shared-secret trust model (integrity and
/// shared-secret possession, not PKI or non-repudiation), just backed by a
/// different, widely-interoperable signature primitive.
#[derive(Clone, Debug)]
pub struct Ed25519SnapshotProvenanceVerifier {
    signing_key: ed25519_dalek::SigningKey,
}

impl Ed25519SnapshotProvenanceVerifier {
    pub fn new(key: impl AsRef<[u8]>) -> Result<Self, ProvenanceError> {
        let key = key.as_ref();
        let seed: [u8; 32] = key
            .try_into()
            .map_err(|_| ProvenanceError::InvalidEd25519KeyLength(key.len()))?;
        Ok(Self {
            signing_key: ed25519_dalek::SigningKey::from_bytes(&seed),
        })
    }
}

impl SnapshotProvenanceVerifier for Ed25519SnapshotProvenanceVerifier {
    fn sign_snapshot(&self, repo_id: &str, commit_sha: &str, payload: &[u8]) -> Vec<u8> {
        let message = snapshot_message(repo_id, commit_sha, payload);
        self.signing_key.sign(&message).to_bytes().to_vec()
    }

    fn verify_snapshot(
        &self,
        repo_id: &str,
        commit_sha: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<(), ProvenanceError> {
        let tampered = || ProvenanceError::Tampered {
            repo_id: repo_id.to_string(),
            commit_sha: commit_sha.to_string(),
        };
        let sig_bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| ProvenanceError::InvalidEd25519SignatureLength(signature.len()))?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        let message = snapshot_message(repo_id, commit_sha, payload);
        self.signing_key
            .verifying_key()
            .verify(&message, &signature)
            .map_err(|_| tampered())
    }
}

/// Which built-in `SnapshotProvenanceVerifier` a deployment selects at
/// runtime via `--provenance-provider`, on both `weave-registry` and
/// `weave sync push` — kept in one place so the two binaries can't drift
/// on what provider names mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceProviderKind {
    Hmac,
    Ed25519,
}

impl std::str::FromStr for ProvenanceProviderKind {
    type Err = ProvenanceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hmac" => Ok(Self::Hmac),
            "ed25519" => Ok(Self::Ed25519),
            other => Err(ProvenanceError::UnknownProvider(other.to_string())),
        }
    }
}

/// Builds the selected verifier from a raw key file's bytes. Shared by
/// `weave-registry` and `weave sync push` so provider selection can never
/// mean something different on the two sides of a deployment.
pub fn build_verifier(
    kind: ProvenanceProviderKind,
    key: impl AsRef<[u8]>,
) -> Result<Arc<dyn SnapshotProvenanceVerifier>, ProvenanceError> {
    match kind {
        ProvenanceProviderKind::Hmac => Ok(Arc::new(HmacSnapshotProvenanceVerifier::new(key)?)),
        ProvenanceProviderKind::Ed25519 => {
            Ok(Arc::new(Ed25519SnapshotProvenanceVerifier::new(key)?))
        }
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
