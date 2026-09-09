use super::*;

#[test]
fn symbol_kind_as_str_matches_the_schema_kind_column_contract() {
    assert_eq!(SymbolKind::Function.as_str(), "function");
    assert_eq!(SymbolKind::Method.as_str(), "method");
    assert_eq!(SymbolKind::Struct.as_str(), "struct");
    assert_eq!(SymbolKind::Class.as_str(), "class");
    assert_eq!(SymbolKind::Interface.as_str(), "interface");
    assert_eq!(SymbolKind::Impl.as_str(), "impl");
}

#[test]
fn structural_edge_kind_as_str_matches_the_two_tier_edge_taxonomy() {
    assert_eq!(StructuralEdgeKind::Imports.as_str(), "IMPORTS");
    assert_eq!(StructuralEdgeKind::Inherits.as_str(), "INHERITS");
    assert_eq!(StructuralEdgeKind::Implements.as_str(), "IMPLEMENTS");
}
