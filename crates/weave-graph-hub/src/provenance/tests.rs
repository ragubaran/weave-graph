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
