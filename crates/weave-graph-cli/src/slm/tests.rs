use super::*;

fn table(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

fn route(question: &str, names: &[&str]) -> Result<RoutedCall, RouterError> {
    FuzzyRouter.route(question, &table(names))
}

#[test]
fn deterministic_router_maps_keywords_to_tools() {
    let cases = [
        ("who calls `helper`?", "callers"),
        ("what depends on token_service?", "callers"),
        ("what does parse_file call?", "callees"),
        ("what is the impact of changing session_manager?", "impact"),
        ("what breaks if I edit jwt_auth?", "impact"),
    ];
    for (question, tool) in cases {
        let symbols = table(&["helper", "token_service", "parse_file", "session_manager", "jwt_auth"]);
        let call = FuzzyRouter.route(question, &symbols).unwrap();
        assert_eq!(call.tool, tool, "question {question:?} must route to {tool}");
    }
}

#[test]
fn backticked_symbol_wins_even_when_uncommon() {
    let call = route("who calls `helper`?", &["helper", "caller"]).unwrap();
    assert_eq!(call.symbol, "helper");
}

#[test]
fn identifier_looking_token_is_preferred_over_plain_words() {
    let call = route("what is the impact of changing session_manager?", &["session_manager"]).unwrap();
    assert_eq!(call.symbol, "session_manager");
}

#[test]
fn substring_near_miss_corrects_against_the_symbol_table() {
    // "jwt" is not a symbol; a unique substring hit corrects it (§5.4).
    let call = route("who calls jwt?", &["verifyJWTSession", "unrelated"]).unwrap();
    assert_eq!(call.symbol, "verifyJWTSession");
}

#[test]
fn ambiguous_substring_stays_unresolved() {
    let err = route("who calls event?", &["event_bus", "event_sink"]).unwrap_err();
    assert!(matches!(err, RouterError::Malformed(_)));
}

#[test]
fn path_question_extracts_both_endpoints() {
    let call = route("path between event_bus and event_sink", &["event_bus", "event_sink", "event_src"]).unwrap();
    assert_eq!(call.tool, "path");
    assert_eq!(call.symbol, "event_bus");
    assert_eq!(call.second.as_deref(), Some("event_sink"));
    assert_eq!(call.expression(), "path(event_bus,event_sink)");
}

#[test]
fn single_symbol_call_has_no_second_and_renders_exact_expression() {
    let call = route("who calls `helper`?", &["helper"]).unwrap();
    assert_eq!(call.expression(), "callers(helper)");
}

#[test]
fn no_symbol_like_token_is_a_reported_failure_never_a_guess() {
    let err = route("what time is it?", &["helper"]).unwrap_err();
    assert!(matches!(err, RouterError::Malformed(_)));
}

#[test]
fn model_router_without_weights_is_unavailable_and_select_router_degrades() {
    let name = "never-pulled-model";
    assert!(!model_available(name));
    let router = select_router(name);
    assert_eq!(router.name(), "deterministic");
    let err = LlamaCliRouter::new(name)
        .route("who calls helper?", &table(&["helper"]))
        .unwrap_err();
    assert!(matches!(err, RouterError::Unavailable(_)));
}

#[test]
fn parse_route_accepts_conforming_json_and_rejects_unknown_tool() {
    let call = parse_route(r#"noise {"tool": "impact", "symbol": "jwt_auth", "second": null} tail"#).unwrap();
    assert_eq!(call.tool, "impact");
    assert_eq!(call.symbol, "jwt_auth");
    assert!(call.second.is_none());

    assert!(parse_route(r#"{"tool": "impact"}"#).is_err());
    assert!(parse_route(r#"{"tool": "explode", "symbol": "x"}"#).is_err());
    assert!(parse_route("no json here").is_err());
}

#[test]
fn model_spec_lookup_matches_the_registry_and_rejects_unknown_names() {
    for spec in MODEL_REGISTRY.iter() {
        assert_eq!(model_spec(spec.name).map(|m| m.name), Some(spec.name));
    }
    assert!(model_spec("does-not-exist").is_none());
}

#[test]
fn pull_model_refuses_a_malformed_checksum_before_any_network_io() {
    let spec = &MODEL_REGISTRY[0];
    let dest = std::env::temp_dir().join("weave-test-should-not-download.gguf");
    let err = pull_model(spec, "tooshort", &dest).unwrap_err();
    assert!(err.contains("64-char hex"), "{err}");
    assert!(!dest.exists(), "no download may start for a bad checksum");
}

#[test]
fn held_out_set_is_exactly_aced_by_the_deterministic_router() {
    let outcome = run_doctor(&FuzzyRouter);
    assert_eq!(outcome.tool_ok, HELD_OUT.len(), "failures: {:?}", outcome.failures);
    assert_eq!(outcome.ground_ok, HELD_OUT.len());
    assert!(outcome.pass(), "{:?}", outcome.failures);
}

#[test]
fn doctor_report_renders_targets_and_pass_verdict() {
    let outcome = run_doctor(&FuzzyRouter);
    let report = render_doctor("deterministic", &outcome);
    assert!(report.contains("tool selection"));
    assert!(report.contains("param grounding"));
    assert!(report.contains("TTFT p50"));
    assert!(report.contains("→ PASS"));
}

#[test]
fn percentile_picks_the_right_order_statistic() {
    assert_eq!(percentile(vec![5.0], 50.0), 5.0);
    assert_eq!(percentile(vec![1.0, 2.0, 3.0, 4.0], 50.0), 3.0);
    assert_eq!(percentile(vec![4.0, 1.0, 2.0], 95.0), 4.0);
}
