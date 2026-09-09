use super::*;

#[test]
fn plain_note_with_no_frontmatter_has_empty_tags_and_aliases() {
    let doc = parse_markdown("Just a note with no frontmatter at all.");
    assert!(doc.tags.is_empty());
    assert!(doc.aliases.is_empty());
}

#[test]
fn inline_yaml_list_frontmatter_is_parsed() {
    let source = "---\ntags: [auth, security]\naliases: [\"Auth Service\"]\n---\nBody text.";
    let doc = parse_markdown(source);
    assert_eq!(doc.tags, vec!["auth", "security"]);
    assert_eq!(doc.aliases, vec!["Auth Service"]);
}

#[test]
fn block_yaml_list_frontmatter_is_parsed() {
    let source = "---\ntags:\n  - auth\n  - security\n---\nBody text.";
    let doc = parse_markdown(source);
    assert_eq!(doc.tags, vec!["auth", "security"]);
}

#[test]
fn single_scalar_frontmatter_value_becomes_a_one_item_list() {
    let source = "---\ntags: auth\n---\nBody.";
    let doc = parse_markdown(source);
    assert_eq!(doc.tags, vec!["auth"]);
}

#[test]
fn wikilink_without_section_is_extracted() {
    let doc = parse_markdown("See [[Auth Overview]] for details.");
    assert_eq!(doc.links.len(), 1);
    assert_eq!(doc.links[0].target, "Auth Overview");
    assert_eq!(doc.links[0].section, None);
}

#[test]
fn wikilink_with_section_splits_target_and_section() {
    let doc = parse_markdown("See [[Auth Overview#Login Flow]] for details.");
    assert_eq!(doc.links.len(), 1);
    assert_eq!(doc.links[0].target, "Auth Overview");
    assert_eq!(doc.links[0].section.as_deref(), Some("Login Flow"));
}

#[test]
fn multiple_wikilinks_in_one_document_are_all_found() {
    let doc = parse_markdown("[[A]] and [[B]] and [[C#section]].");
    let targets: Vec<&str> = doc.links.iter().map(|l| l.target.as_str()).collect();
    assert_eq!(targets, vec!["A", "B", "C"]);
}

#[test]
fn backtick_code_span_is_captured_as_a_code_ref() {
    let doc = parse_markdown("Call `AuthService.verify()` before granting access.");
    assert_eq!(doc.code_refs, vec!["AuthService.verify()"]);
}

#[test]
fn frontmatter_and_body_wikilinks_and_code_refs_all_coexist() {
    let source = "---\ntags: [auth]\n---\nSee [[Other Note]] and call `verify()`.";
    let doc = parse_markdown(source);
    assert_eq!(doc.tags, vec!["auth"]);
    assert_eq!(doc.links.len(), 1);
    assert_eq!(doc.code_refs, vec!["verify()"]);
}

#[test]
fn empty_double_brackets_produce_no_link() {
    let doc = parse_markdown("An empty [[]] wikilink should not crash or resolve.");
    assert!(doc.links.is_empty());
}
