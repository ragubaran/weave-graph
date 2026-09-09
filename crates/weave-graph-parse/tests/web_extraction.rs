use std::path::Path;

use weave_graph_parse::{ProjectIndex, StructuralEdgeKind, SymbolKind, parse_file};

#[test]
fn html_extraction_extracts_ids_and_imports() {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <link rel="stylesheet" href="styles.css">
    <script src="app.js"></script>
</head>
<body>
    <header id="main-header">
        <h1 id="site-title">My Website</h1>
    </header>
    <nav id="navbar"></nav>
    <custom-modal id="modal-1"></custom-modal>
</body>
</html>
"#;

    let parsed = parse_file(Path::new("index.html"), html)
        .unwrap()
        .expect("must parse HTML");

    let ids: Vec<&str> = parsed
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Struct)
        .map(|s| s.symbol.as_str())
        .collect();

    assert!(ids.contains(&"main-header"));
    assert!(ids.contains(&"site-title"));
    assert!(ids.contains(&"navbar"));
    assert!(ids.contains(&"modal-1"));

    let custom_elements: Vec<&str> = parsed
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Class)
        .map(|s| s.symbol.as_str())
        .collect();
    assert!(custom_elements.contains(&"custom-modal"));

    // Imports stylesheet and script
    assert!(parsed.structural_edges.iter().any(|e| {
        e.source_moniker == "index.html#<module>"
            && e.target_name == "styles.css"
            && e.kind == StructuralEdgeKind::Imports
    }));
    assert!(parsed.structural_edges.iter().any(|e| {
        e.source_moniker == "index.html#<module>"
            && e.target_name == "app.js"
            && e.kind == StructuralEdgeKind::Imports
    }));
}

#[test]
fn css_extraction_extracts_classes_and_vars() {
    let css = r#"
@import "theme.css";

:root {
    --primary: #0070f3;
}

#main-header {
    background: var(--primary);
}

.button.primary {
    color: white;
}

@keyframes slideIn {
    from { transform: translateX(-100%); }
    to { transform: translateX(0); }
}
"#;

    let parsed = parse_file(Path::new("styles.css"), css)
        .unwrap()
        .expect("must parse CSS");

    let symbols: Vec<(&str, SymbolKind)> = parsed
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(symbols.contains(&("--primary", SymbolKind::Struct)));
    assert!(symbols.contains(&("main-header", SymbolKind::Struct)));
    assert!(symbols.contains(&("button", SymbolKind::Class)));
    assert!(symbols.contains(&("primary", SymbolKind::Class)));
    assert!(symbols.contains(&("slideIn", SymbolKind::Function)));

    // Imports theme.css
    assert!(parsed.structural_edges.iter().any(|e| {
        e.source_moniker == "styles.css#<module>"
            && e.target_name == "theme.css"
            && e.kind == StructuralEdgeKind::Imports
    }));

    // Var usage
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee_name == "--primary" && !c.is_member_call)
    );
}

#[test]
fn cross_file_html_css_linking() {
    let html = r#"
<html>
<head>
    <link rel="stylesheet" href="styles.css">
</head>
<body>
    <div id="main-header"></div>
</body>
</html>
"#;

    let css = r#"
#main-header {
    color: red;
}
"#;

    let html_file = parse_file(Path::new("index.html"), html)
        .unwrap()
        .expect("parse html");
    let css_file = parse_file(Path::new("styles.css"), css)
        .unwrap()
        .expect("parse css");

    let mut index = ProjectIndex::new();
    index.add_file(&html_file);
    index.add_file(&css_file);

    // Both files define or reference main-header
    assert!(html_file.symbols.iter().any(|s| s.symbol == "main-header"));
    assert!(css_file.symbols.iter().any(|s| s.symbol == "main-header"));
}
