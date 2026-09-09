mod bash;
mod c;
mod cpp;
mod csharp;
mod css;
mod dart;
mod ecma;
mod elixir;
mod go;
mod html;
mod java;
mod kotlin;
mod lua;
mod php;
mod powershell;
mod python;
mod query_vm;
mod r;
mod ruby;
mod rust;
mod scala;
mod sql;
mod swift;
mod util;
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
        Language::CSharp => csharp::extract(root, source, path),
        Language::Dart => dart::extract(root, source, path),
        Language::Elixir => elixir::extract(root, source, path),
        Language::Kotlin => kotlin::extract(root, source, path),
        Language::Swift => swift::extract(root, source, path),
        Language::Scala => scala::extract(root, source, path),
        Language::Zig => zig::extract(root, source, path),
        Language::Ruby => ruby::extract(root, source, path),
        Language::Php => php::extract(root, source, path),
        Language::Bash => bash::extract(root, source, path),
        Language::PowerShell => powershell::extract(root, source, path),
        Language::Lua => lua::extract(root, source, path),
        Language::Sql => sql::extract(root, source, path),
        Language::Html => html::extract(root, source, path),
        Language::Css => css::extract(root, source, path),
        Language::R => r::extract(root, source, path),
        // Universal fallback: ANY other language runs through query_vm automatically!
        _ => query_vm::extract(language, root, source, path),
    }
}
