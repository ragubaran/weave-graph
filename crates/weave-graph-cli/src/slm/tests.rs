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
        let symbols = table(&[
            "helper",
            "token_service",
            "parse_file",
            "session_manager",
            "jwt_auth",
        ]);
        let call = FuzzyRouter.route(question, &symbols).unwrap();
        assert_eq!(
            call.tool, tool,
            "question {question:?} must route to {tool}"
        );
    }
}

#[test]
fn backticked_symbol_wins_even_when_uncommon() {
    let call = route("who calls `helper`?", &["helper", "caller"]).unwrap();
    assert_eq!(call.symbol, "helper");
}

#[test]
fn identifier_looking_token_is_preferred_over_plain_words() {
    let call = route(
        "what is the impact of changing session_manager?",
        &["session_manager"],
    )
    .unwrap();
    assert_eq!(call.symbol, "session_manager");
}

#[test]
fn substring_near_miss_corrects_against_the_symbol_table() {
    // "jwt" is not a symbol; a unique substring hit corrects it.
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
    let call = route(
        "path between event_bus and event_sink",
        &["event_bus", "event_sink", "event_src"],
    )
    .unwrap();
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
    let call =
        parse_route(r#"noise {"tool": "impact", "symbol": "jwt_auth", "second": null} tail"#)
            .unwrap();
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
    assert_eq!(
        outcome.tool_ok,
        HELD_OUT.len(),
        "failures: {:?}",
        outcome.failures
    );
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

#[test]
fn pull_model_creates_parent_directory_before_curl() {
    // A fresh CI runner has no ~/.cache/weave/models/ — pull_model must
    // create it before spawning curl or curl exits 23 (write failure).
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("a").join("b").join("c");
    assert!(!nested.exists());
    // Offline stub: localhost URL fails immediately without any network
    // round-trip, so this test never downloads anything.
    let spec = ModelSpec {
        name: "test-model",
        url: "http://127.0.0.1:1/does-not-exist",
        ram_mb: 0,
        role: "test",
    };
    // Valid-format sha256 passes the hex-length guard and
    // reaches create_dir_all before curl ever runs.
    let sha = "0".repeat(64);
    let dest = nested.join("model.gguf");
    let _ = pull_model(&spec, &sha, &dest);
    assert!(
        nested.exists(),
        "parent dir must be created before curl spawn"
    );
}

struct StubRouter(Result<RoutedCall, ()>);

impl IntentRouter for StubRouter {
    fn route(&self, _: &str, _: &[String]) -> Result<RoutedCall, RouterError> {
        self.0
            .clone()
            .map_err(|()| RouterError::Unavailable("stub".to_string()))
    }

    fn name(&self) -> &'static str {
        "stub"
    }
}

fn call(tool: &str, symbol: &str) -> RoutedCall {
    RoutedCall {
        tool: tool.to_string(),
        symbol: symbol.to_string(),
        second: None,
    }
}

#[test]
fn doctor_reports_wrong_tool_wrong_symbol_and_total_failure() {
    let wrong_tool = run_doctor(&StubRouter(Ok(call("nonexistent", "x"))));
    assert!(!wrong_tool.pass());
    assert!(wrong_tool.failures[0].contains("tool nonexistent"));

    let wrong_symbol = run_doctor(&StubRouter(Ok(call("callers", "zzz"))));
    assert!(
        wrong_symbol
            .failures
            .iter()
            .any(|f| f.contains("symbol zzz"))
    );

    let failed = run_doctor(&StubRouter(Err(())));
    assert!(failed.fallback_used);
    let report = render_doctor("m", &failed);
    assert!(report.contains("routing failed entirely"));
    assert!(report.contains("no graceful per-entry fallback"));
    assert!(report.ends_with("→ FAIL"));
}

#[test]
fn sha256_file_hashes_content_and_reports_missing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("f");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(
        sha256_file(&file).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(sha256_file(&tmp.path().join("missing")).is_err());
}

#[test]
fn pull_model_installs_on_match_and_removes_partial_on_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src.gguf");
    std::fs::write(&src, b"abc").unwrap();
    let url: &'static str = Box::leak(format!("file://{}", src.display()).into_boxed_str());
    let spec = ModelSpec {
        name: "local",
        url,
        ram_mb: 0,
        role: "test",
    };
    let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    let bad_dest = tmp.path().join("bad.gguf");
    let err = pull_model(&spec, &"0".repeat(64), &bad_dest).unwrap_err();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert!(!bad_dest.exists());
    assert!(!bad_dest.with_extension("gguf.part").exists());

    let dest = tmp.path().join("ok.gguf");
    pull_model(&spec, &good.to_uppercase(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
}

#[test]
fn pull_model_reports_a_failed_download() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = ModelSpec {
        name: "gone",
        url: "file:///nonexistent/weave-model.gguf",
        ram_mb: 0,
        role: "test",
    };
    let err = pull_model(&spec, &"0".repeat(64), &tmp.path().join("m.gguf")).unwrap_err();
    assert!(err.contains("download failed"), "{err}");
}

#[cfg(unix)]
fn fake_llama(dir: &std::path::Path, body: &str) -> LlamaCliRouter {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("llama");
    std::fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let model = dir.join("m.gguf");
    std::fs::write(&model, b"x").unwrap();
    LlamaCliRouter {
        llama_bin: bin.display().to_string(),
        model_path: model,
    }
}

#[cfg(unix)]
#[test]
fn llama_router_parses_subprocess_output_and_maps_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let symbols = table(&["helper"]);

    let ok = fake_llama(
        tmp.path(),
        r#"echo '{"tool":"callers","symbol":"helper","second":null}'"#,
    );
    let routed = ok.route("who calls helper?", &symbols).unwrap();
    assert_eq!(routed.expression(), "callers(helper)");

    let failing = fake_llama(tmp.path(), "exit 3");
    let err = failing.route("q", &symbols).unwrap_err();
    assert!(matches!(err, RouterError::Unavailable(m) if m.contains("exit status")));

    let missing_bin = LlamaCliRouter {
        llama_bin: tmp.path().join("nope").display().to_string(),
        model_path: tmp.path().join("m.gguf"),
    };
    let err = missing_bin.route("q", &symbols).unwrap_err();
    assert!(matches!(err, RouterError::Unavailable(_)));
}

#[test]
fn build_prompt_lists_at_most_200_symbols() {
    let names: Vec<String> = (0..300).map(|i| format!("sym{i}")).collect();
    let prompt = build_prompt("q?", &names);
    assert!(prompt.contains("sym199"));
    assert!(!prompt.contains("sym200"));
    assert!(prompt.ends_with("Question: q?"));
}
