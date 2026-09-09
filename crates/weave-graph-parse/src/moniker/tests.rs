use super::*;

#[test]
fn same_symbol_in_different_files_gets_different_monikers() {
    assert_ne!(build("a.rs", "foo"), build("b.rs", "foo"));
}

#[test]
fn different_scopes_in_the_same_file_get_different_monikers() {
    assert_ne!(build("a.rs", "Bar::baz"), build("a.rs", "Other::baz"));
}
