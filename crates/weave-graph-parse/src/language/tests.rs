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

/// The `lang-extended` set (`impl.md` M1.2b's widened ten) — split out
/// from the core test above so the core assertions still run, unchanged,
/// in a `--no-default-features` ("mini") build.
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
