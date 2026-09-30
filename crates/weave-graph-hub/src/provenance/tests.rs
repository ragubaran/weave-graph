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

#[test]
fn ed25519_verifier_rejects_a_key_of_the_wrong_length() {
    assert_eq!(
        Ed25519SnapshotProvenanceVerifier::new([1u8; 31]).unwrap_err(),
        ProvenanceError::InvalidEd25519KeyLength(31)
    );
    assert_eq!(
        Ed25519SnapshotProvenanceVerifier::new([1u8; 33]).unwrap_err(),
        ProvenanceError::InvalidEd25519KeyLength(33)
    );
}

#[test]
fn ed25519_verifier_authenticates_every_snapshot_field() {
    let verifier = Ed25519SnapshotProvenanceVerifier::new([9u8; 32]).unwrap();
    let signature = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");

    assert_eq!(signature.len(), 64);
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

#[test]
fn ed25519_verifier_rejects_a_signature_of_the_wrong_length() {
    let verifier = Ed25519SnapshotProvenanceVerifier::new([9u8; 32]).unwrap();
    assert_eq!(
        verifier
            .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &[0u8; 63])
            .unwrap_err(),
        ProvenanceError::InvalidEd25519SignatureLength(63)
    );
}

#[test]
fn ed25519_verifier_cannot_check_another_verifiers_signature() {
    let signer = Ed25519SnapshotProvenanceVerifier::new([1u8; 32]).unwrap();
    let checker = Ed25519SnapshotProvenanceVerifier::new([2u8; 32]).unwrap();
    let sig = signer.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert!(
        checker
            .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &sig)
            .is_err()
    );
}

#[test]
fn provider_kind_parses_known_names_and_rejects_unknown_ones() {
    assert_eq!(
        "hmac".parse::<ProvenanceProviderKind>().unwrap(),
        ProvenanceProviderKind::Hmac
    );
    assert_eq!(
        "ed25519".parse::<ProvenanceProviderKind>().unwrap(),
        ProvenanceProviderKind::Ed25519
    );
    assert_eq!(
        "rsa".parse::<ProvenanceProviderKind>().unwrap_err(),
        ProvenanceError::UnknownProvider("rsa".to_string())
    );
}

#[test]
fn build_verifier_round_trips_for_each_provider_kind() {
    for kind in [
        ProvenanceProviderKind::Hmac,
        ProvenanceProviderKind::Ed25519,
    ] {
        let verifier = build_verifier(kind, [4u8; 32]).unwrap();
        let sig = verifier.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
        assert!(
            verifier
                .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &sig)
                .is_ok()
        );
    }
}

#[test]
fn build_verifier_cross_provider_signature_never_verifies() {
    let hmac = build_verifier(ProvenanceProviderKind::Hmac, [4u8; 32]).unwrap();
    let ed25519 = build_verifier(ProvenanceProviderKind::Ed25519, [4u8; 32]).unwrap();
    let sig = hmac.sign_snapshot("auth-service", "abc123", b"snapshot bytes");
    assert!(
        ed25519
            .verify_snapshot("auth-service", "abc123", b"snapshot bytes", &sig)
            .is_err()
    );
}
