use super::*;

fn attached() -> Provenance {
    MockProvenanceProvider::new()
        .attach(7, "0123456789abcdef0123456789abcdef01234567")
        .unwrap()
}

#[test]
fn attach_then_verify_reports_verified() {
    let provider = MockProvenanceProvider::new();
    let record = provider
        .attach(7, "0123456789abcdef0123456789abcdef01234567")
        .unwrap();
    assert_eq!(provider.verify(&record), VerifyResult::Verified);
}

#[test]
fn tampered_doc_id_is_detected() {
    let provider = MockProvenanceProvider::new();
    let mut record = attached();
    record.doc_id += 1;
    assert_eq!(
        provider.verify(&record),
        VerifyResult::Tampered {
            reason: "merkle_root does not match (doc_id, commit_hash)".to_string()
        }
    );
}

#[test]
fn tampered_commit_hash_is_detected() {
    let provider = MockProvenanceProvider::new();
    let mut record = attached();
    record.commit_hash = "ffffffffffffffffffffffffffffffffffffffff".to_string();
    assert_eq!(
        provider.verify(&record),
        VerifyResult::Tampered {
            reason: "merkle_root does not match (doc_id, commit_hash)".to_string()
        }
    );
}

#[test]
fn tampered_merkle_root_is_detected() {
    let provider = MockProvenanceProvider::new();
    let mut record = attached();
    record.merkle_root = "0000000000000000".to_string();
    assert_eq!(
        provider.verify(&record),
        VerifyResult::Tampered {
            reason: "merkle_root does not match (doc_id, commit_hash)".to_string()
        }
    );
}

#[test]
fn tampered_signature_is_detected() {
    let provider = MockProvenanceProvider::new();
    let mut record = attached();
    record.signature = "0000000000000000".to_string();
    assert_eq!(
        provider.verify(&record),
        VerifyResult::Tampered {
            reason: "signature does not verify under this provider".to_string()
        }
    );
}

#[test]
fn a_provider_cannot_verify_another_providers_records() {
    let signer = MockProvenanceProvider::with_key(1);
    let checker = MockProvenanceProvider::with_key(2);
    let record = signer.attach(7, "abc").unwrap();
    assert_eq!(
        checker.verify(&record),
        VerifyResult::Tampered {
            reason: "signature does not verify under this provider".to_string()
        }
    );
}

#[test]
fn attach_is_deterministic() {
    let provider = MockProvenanceProvider::new();
    let a = provider.attach(7, "abc").unwrap();
    let b = provider.attach(7, "abc").unwrap();
    assert_eq!(a, b);
}

#[test]
fn distinct_inputs_produce_distinct_roots() {
    let provider = MockProvenanceProvider::new();
    let a = provider.attach(7, "abc").unwrap();
    let b = provider.attach(8, "abc").unwrap();
    let c = provider.attach(7, "abd").unwrap();
    assert_ne!(a.merkle_root, b.merkle_root);
    assert_ne!(a.merkle_root, c.merkle_root);
    assert_ne!(a.signature, c.signature);
}

#[test]
fn default_matches_new() {
    assert_eq!(
        MockProvenanceProvider::default(),
        MockProvenanceProvider::new()
    );
}
