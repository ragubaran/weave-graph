#[cfg(feature = "lang-extended")]
mod bash;
mod c;
mod cpp;
#[cfg(feature = "lang-extended")]
mod csharp;
#[cfg(feature = "lang-extended")]
mod css;
#[cfg(feature = "lang-extended")]
mod dart;
mod ecma;
#[cfg(feature = "lang-extended")]
mod elixir;
mod go;
#[cfg(feature = "lang-extended")]
mod html;
mod java;
#[cfg(feature = "lang-extended")]
mod kotlin;
#[cfg(feature = "lang-extended")]
mod lua;
#[cfg(feature = "lang-extended")]
mod php;
#[cfg(feature = "lang-extended")]
mod powershell;
mod python;
#[cfg(feature = "lang-extended")]
mod query_vm;
#[cfg(feature = "lang-extended")]
mod r;
#[cfg(feature = "lang-extended")]
mod ruby;
mod rust;
#[cfg(feature = "lang-extended")]
mod scala;
#[cfg(feature = "lang-extended")]
mod sql;
#[cfg(feature = "lang-extended")]
mod swift;
mod util;
#[cfg(feature = "lang-extended")]
mod zig;

use tree_sitter::Node;

use crate::language::Language;
use crate::model::ParsedFile;

pub(crate) fn extract(language: Language, root: Node, source: &[u8], path: &str) -> ParsedFile {
    match language {
        Language::Rust => rust::extract(root, source, path),
        Language::Python => python::extract(root, source, path),
        Language::JavaScript => ecma::extract(root, source, path, false),
        Language::TypeScript => ecma::extract(root, source, path, true),
        Language::Go => go::extract(root, source, path),
        Language::Java => java::extract(root, source, path),
        Language::C => c::extract(root, source, path),
        Language::Cpp => cpp::extract(root, source, path),
        // Extended extractors are gated like the enum: without `lang-extended`
        // the variants and their modules don't exist, so these arms (and the
        // query_vm fallback below) compile only under the feature.
        #[cfg(feature = "lang-extended")]
        Language::ArkTs => ecma::extract(root, source, path, true),
        #[cfg(feature = "lang-extended")]
        Language::ObjC | Language::Metal | Language::Cuda => cpp::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::CSharp => csharp::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Dart => dart::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Elixir => elixir::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Kotlin => kotlin::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Swift => swift::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Scala => scala::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Zig => zig::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Ruby => ruby::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Php => php::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Bash => bash::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::PowerShell => powershell::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Lua | Language::Luau => lua::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Sql => sql::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Html
        | Language::Svelte
        | Language::Vue
        | Language::Astro
        | Language::Liquid
        | Language::Cfml => html::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::Css => css::extract(root, source, path),
        #[cfg(feature = "lang-extended")]
        Language::R => r::extract(root, source, path),
        // Universal fallback: ANY other language runs through query_vm
        // automatically. Base variants all have bespoke extractors above, so
        // this arm only has work to do when extended variants are present.
        #[cfg(feature = "lang-extended")]
        _ => query_vm::extract(language, root, source, path),
    }
}
