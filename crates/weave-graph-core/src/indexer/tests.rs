use super::*;

#[test]
fn bailout_uses_floor_on_tiny_repos() {
    let cfg = ReindexConfig::default();
    // 20-file repo: floor=100 dominates. 50 changed < 100 → no bailout.
    assert!(!should_bail_out(50, 20, &cfg));
    // 101 changed > 100 floor → bail
    assert!(should_bail_out(101, 20, &cfg));
}

#[test]
fn bailout_uses_ratio_on_large_repos() {
    let cfg = ReindexConfig::default();
    // 2000-file repo: 10% = 200. 150 changed < 200 → no bailout.
    assert!(!should_bail_out(150, 2000, &cfg));
    // 201 changed > 200 → bail
    assert!(should_bail_out(201, 2000, &cfg));
}

#[test]
fn zero_changed_never_bails_out() {
    assert!(!should_bail_out(0, 10_000, &ReindexConfig::default()));
}

#[test]
fn custom_config_is_respected() {
    let cfg = ReindexConfig {
        bailout_floor: 5,
        bailout_ratio: 0.50,
    };
    // 10-file repo: max(5, 5) = 5. 5 changed is NOT > 5 → no bail.
    assert!(!should_bail_out(5, 10, &cfg));
    // 6 changed > 5 → bail.
    assert!(should_bail_out(6, 10, &cfg));
}
