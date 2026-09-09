use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Html)
        .unwrap()
        .parse("index.html", source)
        .unwrap()
}

#[test]
fn extracts_elements_with_ids_and_custom_elements() {
    let html = r#"
<!DOCTYPE html>
<html>
<body>
    <header id="main-header">
        <h1 id="title">Hello World</h1>
    </header>
    <main id="content">
        <user-card id="user-1"></user-card>
    </main>
</body>
</html>
"#;
    let file = parse(html);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("main-header", SymbolKind::Struct)));
    assert!(names.contains(&("title", SymbolKind::Struct)));
    assert!(names.contains(&("content", SymbolKind::Struct)));
    assert!(names.contains(&("user-1", SymbolKind::Struct)));
    assert!(names.contains(&("user-card", SymbolKind::Class)));
}

#[test]
fn extracts_stylesheet_and_script_imports() {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <link rel="stylesheet" href="styles.css">
    <script src="app.js"></script>
</head>
<body></body>
</html>
"#;
    let file = parse(html);

    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "index.html#<module>"
            && e.target_name == "styles.css"
            && e.kind == StructuralEdgeKind::Imports
    }));

    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "index.html#<module>"
            && e.target_name == "app.js"
            && e.kind == StructuralEdgeKind::Imports
    }));
}
