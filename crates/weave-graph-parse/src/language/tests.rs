use super::*;

#[test]
fn detects_language_from_extension() {
    assert_eq!(
        Language::from_path(Path::new("a/b.rs")),
        Some(Language::Rust)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.py")),
        Some(Language::Python)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.js")),
        Some(Language::JavaScript)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.tsx")),
        Some(Language::TypeScript)
    );
    assert_eq!(Language::from_path(Path::new("a/b.go")), Some(Language::Go));
    assert_eq!(
        Language::from_path(Path::new("a/b.java")),
        Some(Language::Java)
    );
    assert_eq!(Language::from_path(Path::new("a/b.c")), Some(Language::C));
    assert_eq!(Language::from_path(Path::new("a/b.h")), Some(Language::C));
    assert_eq!(
        Language::from_path(Path::new("a/b.cpp")),
        Some(Language::Cpp)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.hpp")),
        Some(Language::Cpp)
    );
    assert_eq!(Language::from_path(Path::new("a/b.md")), None);
    assert_eq!(Language::from_path(Path::new("a/b")), None);
}

/// The `lang-extended` set is split out from the core test above so the
/// core assertions still run, unchanged, in a `--no-default-features`
/// ("mini") build.
#[test]
#[cfg(feature = "lang-extended")]
fn detects_extended_language_from_extension() {
    assert_eq!(
        Language::from_path(Path::new("a/b.cs")),
        Some(Language::CSharp)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.kt")),
        Some(Language::Kotlin)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.swift")),
        Some(Language::Swift)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.scala")),
        Some(Language::Scala)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.zig")),
        Some(Language::Zig)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.rb")),
        Some(Language::Ruby)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.php")),
        Some(Language::Php)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.sh")),
        Some(Language::Bash)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.ps1")),
        Some(Language::PowerShell)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.lua")),
        Some(Language::Lua)
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn detects_generic_query_vm_languages_from_extension() {
    assert_eq!(
        Language::from_path(Path::new("a/b.ex")),
        Some(Language::Elixir)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.exs")),
        Some(Language::Elixir)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.hs")),
        Some(Language::Haskell)
    );
    assert_eq!(
        Language::from_path(Path::new("a/b.dart")),
        Some(Language::Dart)
    );
    assert_eq!(
        Language::from_path(Path::new("a/schema.sql")),
        Some(Language::Sql)
    );
    assert_eq!(
        Language::from_path(Path::new("a/index.html")),
        Some(Language::Html)
    );
    assert_eq!(
        Language::from_path(Path::new("a/style.css")),
        Some(Language::Css)
    );
    assert_eq!(
        Language::from_path(Path::new("a/script.r")),
        Some(Language::R)
    );
    assert_eq!(
        Language::from_path(Path::new("a/analysis.R")),
        Some(Language::R)
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn dotenv_files_are_detected_by_name_not_extension() {
    assert_eq!(
        Language::from_path(Path::new(".env")),
        Some(Language::Properties)
    );
    assert_eq!(
        Language::from_path(Path::new(".env.local")),
        Some(Language::Properties)
    );
    assert_eq!(
        Language::from_path(Path::new("a/.env.production")),
        Some(Language::Properties)
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn detects_all_seventeen_new_languages_from_extension() {
    assert_eq!(
        Language::from_path(Path::new("src/main.ets")),
        Some(Language::ArkTs)
    );
    assert_eq!(
        Language::from_path(Path::new("src/legacy.m")),
        Some(Language::ObjC)
    );
    assert_eq!(
        Language::from_path(Path::new("src/legacy.mm")),
        Some(Language::ObjC)
    );
    assert_eq!(
        Language::from_path(Path::new("shaders/kernel.metal")),
        Some(Language::Metal)
    );
    assert_eq!(
        Language::from_path(Path::new("cuda/kernel.cu")),
        Some(Language::Cuda)
    );
    assert_eq!(
        Language::from_path(Path::new("cuda/kernel.cuh")),
        Some(Language::Cuda)
    );
    assert_eq!(
        Language::from_path(Path::new("ui/App.svelte")),
        Some(Language::Svelte)
    );
    assert_eq!(
        Language::from_path(Path::new("ui/App.vue")),
        Some(Language::Vue)
    );
    assert_eq!(
        Language::from_path(Path::new("pages/index.astro")),
        Some(Language::Astro)
    );
    assert_eq!(
        Language::from_path(Path::new("templates/index.liquid")),
        Some(Language::Liquid)
    );
    assert_eq!(
        Language::from_path(Path::new("math/unit.pas")),
        Some(Language::Pascal)
    );
    assert_eq!(
        Language::from_path(Path::new("math/unit.pp")),
        Some(Language::Pascal)
    );
    assert_eq!(
        Language::from_path(Path::new("game/script.luau")),
        Some(Language::Luau)
    );
    assert_eq!(
        Language::from_path(Path::new("web/page.cfm")),
        Some(Language::Cfml)
    );
    assert_eq!(
        Language::from_path(Path::new("web/service.cfc")),
        Some(Language::Cfml)
    );
    assert_eq!(
        Language::from_path(Path::new("bank/trans.cbl")),
        Some(Language::Cobol)
    );
    assert_eq!(
        Language::from_path(Path::new("bank/trans.cob")),
        Some(Language::Cobol)
    );
    assert_eq!(
        Language::from_path(Path::new("app/Module.vb")),
        Some(Language::VisualBasic)
    );
    assert_eq!(
        Language::from_path(Path::new("telecom/server.erl")),
        Some(Language::Erlang)
    );
    assert_eq!(
        Language::from_path(Path::new("telecom/header.hrl")),
        Some(Language::Erlang)
    );
    assert_eq!(
        Language::from_path(Path::new("contracts/Token.sol")),
        Some(Language::Solidity)
    );
    assert_eq!(
        Language::from_path(Path::new("infra/main.tf")),
        Some(Language::Terraform)
    );
    assert_eq!(
        Language::from_path(Path::new("infra/main.tofu")),
        Some(Language::Terraform)
    );
    assert_eq!(
        Language::from_path(Path::new("nix/flake.nix")),
        Some(Language::Nix)
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn parses_all_seventeen_new_languages_cleanly() {
    use crate::parser::parse_file;

    let test_cases = [
        (
            "src/main.ets",
            "export class Component { build() { return 1; } }",
        ),
        ("src/legacy.m", "int compute(int x) { return x * 2; }"),
        ("src/legacy.mm", "int compute_cpp(int x) { return x * 3; }"),
        ("shaders/kernel.metal", "kernel void compute() {}"),
        ("cuda/kernel.cu", "__global__ void vectorAdd() {}"),
        (
            "ui/App.svelte",
            "<script>let count = 0;</script><button>{count}</button>",
        ),
        ("ui/App.vue", "<template><div>Hello</div></template>"),
        (
            "pages/index.astro",
            "--- const name = 'world'; --- <div>{name}</div>",
        ),
        (
            "templates/index.liquid",
            "{% assign title = 'Home' %}<h1>{{ title }}</h1>",
        ),
        (
            "math/unit.pas",
            "function Add(a: Integer): Integer; begin end",
        ),
        (
            "game/script.luau",
            "function calculate(n: number): number return n * 2 end",
        ),
        (
            "web/service.cfc",
            "<cfcomponent><cffunction name=\"init\"></cffunction></cfcomponent>",
        ),
        (
            "bank/trans.cbl",
            "IDENTIFICATION DIVISION. PROGRAM-ID. HELLO.",
        ),
        (
            "app/Module.vb",
            "Public Class TestModule\nPublic Sub Run()\nEnd Sub\nEnd Class",
        ),
        (
            "telecom/server.erl",
            "-module(server).\n-export([start/0]).\nstart() -> ok.",
        ),
        (
            "contracts/Token.sol",
            "contract Token { function transfer() public {} }",
        ),
        (
            "infra/main.tf",
            "resource \"aws_s3_bucket\" \"b\" { bucket = \"my-bucket\" }",
        ),
        (
            "nix/flake.nix",
            "{ description = \"flake\"; inputs = {}; outputs = _: {}; }",
        ),
    ];

    for (file_path, source) in test_cases {
        let path = Path::new(file_path);
        let res = parse_file(path, source);
        assert!(res.is_some(), "expected language match for {file_path}");
        assert!(
            res.unwrap().is_ok(),
            "expected successful parse for {file_path}"
        );
    }
}
