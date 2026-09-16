use super::*;

#[test]
fn sign_then_verify_succeeds() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let sig = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert_eq!(
        verifier.verify_snapshot("auth-service", "abc123", b"snapshot bytes", &sig),
        Ok(())
    );
}

#[test]
fn tampered_payload_is_detected() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let sig = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert_eq!(
        verifier.verify_snapshot("auth-service", "abc123", b"corrupted bytes!", &sig),
        Err(ProvenanceError::Tampered {
            repo_id: "auth-service".to_string(),
            commit_sha: "abc123".to_string(),
        })
    );
}

#[test]
fn tampered_repo_id_is_detected() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let sig = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert!(
        verifier
            .verify_snapshot("payment-service", "abc123", b"snapshot bytes", &sig)
            .is_err()
    );
}

#[test]
fn tampered_commit_sha_is_detected() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let sig = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert!(
        verifier
            .verify_snapshot("auth-service", "def456", b"snapshot bytes", &sig)
            .is_err()
    );
}

#[test]
fn a_verifier_cannot_check_another_verifiers_signature() {
    let signer = MockSnapshotProvenanceVerifier::with_key(1);
    let checker = MockSnapshotProvenanceVerifier::with_key(2);
    let sig = signer.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert!(
        checker
            .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &sig)
            .is_err()
    );
}

#[test]
fn sign_snapshot_is_deterministic() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let a = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    let b = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert_eq!(a, b);
}

#[test]
fn distinct_repo_ids_produce_distinct_signatures_for_the_same_bytes() {
    let verifier = MockSnapshotProvenanceVerifier::new();
    let a = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    let b = verifier.sign_snapshot("payment-service", "abc123", b"snapshot bytes");
    assert_ne!(a, b);
}

#[test]
fn default_matches_new() {
    assert_eq!(
        MockSnapshotProvenanceVerifier::default(),
        MockSnapshotProvenanceVerifier::new()
    );
}

#[test]
fn hmac_verifier_rejects_weak_secrets() {
    assert_eq!(
        HmacSnapshotProvenanceVerifier::new("too-short").unwrap_err(),
        ProvenanceError::WeakKey
    );
}

#[test]
fn hmac_verifier_authenticates_every_snapshot_field() {
    let verifier = HmacSnapshotProvenanceVerifier::new([7u8; 32]).unwrap();
    let signature = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");

    assert_eq!(signature.len(), 32);
    assert!(
        verifier
            .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &signature)
            .is_ok()
    );
    assert!(
        verifier
            .verify_snapshot("other-service", "abc123", b"snapshot bytes", &signature)
            .is_err()
    );
    assert!(
        verifier
            .verify_snapshot("auth-service", "def456", b"snapshot bytes", &signature)
            .is_err()
    );
    assert!(
        verifier
            .verify_snapshot("auth-service", "abc123", b"changed", &signature)
            .is_err()
    );
}
