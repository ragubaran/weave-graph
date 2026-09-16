use super::{expand_query, split_identifier};

#[test]
fn expand_query_ors_known_synonyms_and_ands_across_words() {
    // Worked example: "token" and
    // "lifetime" both sit inside a synonym group as members, not just as
    // group keys, so each expands to its whole group.
    let expanded = expand_query("token lifetime");
    assert_eq!(
        expanded,
        "(\"auth\" OR \"authentication\" OR \"login\" OR \"jwt\" OR \"token\" OR \"credential\") \
         AND (\"ttl\" OR \"expiry\" OR \"expiration\" OR \"timeout\" OR \"lifetime\" OR \"deadline\")"
    );
}

#[test]
fn expand_query_matches_by_abbreviation_too() {
    assert_eq!(expand_query("auth"), expand_query("token"));
}

#[test]
fn expand_query_word_with_no_synonym_entry_matches_only_itself() {
    assert_eq!(expand_query("verify"), "\"verify\"");
}

#[test]
fn expand_query_lowercases_before_lookup() {
    assert_eq!(expand_query("AUTH"), expand_query("auth"));
}

#[test]
fn expand_query_empty_string_is_empty() {
    assert_eq!(expand_query(""), "");
}

#[test]
fn split_identifier_camel_case_boundaries() {
    assert_eq!(
        split_identifier("JwtTokenExpirationHandler"),
        "Jwt Token Expiration Handler"
    );
}

#[test]
fn split_identifier_snake_case_boundaries() {
    assert_eq!(split_identifier("check_auth_ttl"), "check auth ttl");
}

#[test]
fn split_identifier_kebab_case_boundaries() {
    assert_eq!(split_identifier("check-auth-ttl"), "check auth ttl");
}

#[test]
fn split_identifier_keeps_acronym_runs_together() {
    assert_eq!(split_identifier("HTTPServer"), "HTTP Server");
}

#[test]
fn split_identifier_single_word_is_unchanged() {
    assert_eq!(split_identifier("login"), "login");
}

#[test]
fn split_identifier_empty_string_is_empty() {
    assert_eq!(split_identifier(""), "");
}
