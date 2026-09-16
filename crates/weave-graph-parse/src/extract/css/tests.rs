use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Css)
        .unwrap()
        .parse("styles.css", source)
        .unwrap()
}

#[test]
fn extracts_classes_ids_and_custom_properties() {
    let css = r#"
@import "reset.css";

:root {
    --primary-color: #0070f3;
}

#main-header {
    background: var(--primary-color);
}

.nav.header {
    display: flex;
}

.title {
    font-size: 2rem;
}

@keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
}
"#;
    let file = parse(css);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("--primary-color", SymbolKind::Struct)));
    assert!(names.contains(&("main-header", SymbolKind::Struct)));
    assert!(names.contains(&("nav", SymbolKind::Class)));
    assert!(names.contains(&("header", SymbolKind::Class)));
    assert!(names.contains(&("title", SymbolKind::Class)));
    assert!(names.contains(&("fadeIn", SymbolKind::Function)));

    // Import reset.css
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "styles.css#<module>"
            && e.target_name == "reset.css"
            && e.kind == StructuralEdgeKind::Imports
    }));

    // Var reference
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "--primary-color" && !c.is_member_call)
    );
}

#[test]
fn extracts_import_url() {
    let css = r#"
@import url("fonts.css");
"#;
    let file = parse(css);
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "styles.css#<module>"
            && e.target_name == "fonts.css"
            && e.kind == StructuralEdgeKind::Imports
    }));
}

#[test]
fn extracts_nested_class_and_id_selectors() {
    let css = r#"
div#my-id .my-class {
    color: red;
}
"#;
    let file = parse(css);
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("my-id", SymbolKind::Struct)));
    assert!(names.contains(&("my-class", SymbolKind::Class)));
}
