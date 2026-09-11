use super::*;

#[test]
fn module_label_picks_the_most_common_directory() {
    let files = vec![
        "src/foo.rs".to_string(),
        "src/bar.rs".to_string(),
        "other/baz.rs".to_string(),
    ];
    assert_eq!(module_label(&files), "src");
}

#[test]
fn module_label_falls_back_to_root_for_top_level_files() {
    let files = vec!["main.rs".to_string()];
    assert_eq!(module_label(&files), "(root)");
}
