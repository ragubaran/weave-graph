use std::path::Path;

/// Language set: the four from M1.2 (Rust, Python, JS, TS) plus M1.2b's
/// additions (`impl.md` M1.2b) — Go, Java, C, C++, C#, Kotlin, Swift,
/// Scala, Zig, Ruby, PHP, Shell/Bash, PowerShell, Lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Kotlin,
    Swift,
    Scala,
    Zig,
    Ruby,
    Php,
    Bash,
    PowerShell,
    Lua,
    Yaml,
    Toml,
    Json,
    Properties,
    /// Any language with a grammar but no hand-written `extract/<lang>.rs`
    /// — routed through `query_vm`'s generic, kind-name-heuristic
    /// extractor (`impl.md` M1.2c) instead of bespoke Rust per language.
    Elixir,
    Haskell,
    Dart,
    Sql,
    Html,
    Css,
    R,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        #[cfg(feature = "lang-extended")]
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str())
            && file_name.starts_with(".env")
        {
            return Some(Language::Properties);
        }

        match path.extension()?.to_str()? {
            "rs" => Some(Language::Rust),
            "py" => Some(Language::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" => Some(Language::TypeScript),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "c" | "h" => Some(Language::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Language::Cpp),
            #[cfg(feature = "lang-extended")]
            "cs" => Some(Language::CSharp),
            #[cfg(feature = "lang-extended")]
            "kt" | "kts" => Some(Language::Kotlin),
            #[cfg(feature = "lang-extended")]
            "swift" => Some(Language::Swift),
            #[cfg(feature = "lang-extended")]
            "scala" | "sc" => Some(Language::Scala),
            #[cfg(feature = "lang-extended")]
            "zig" => Some(Language::Zig),
            #[cfg(feature = "lang-extended")]
            "rb" => Some(Language::Ruby),
            #[cfg(feature = "lang-extended")]
            "php" => Some(Language::Php),
            #[cfg(feature = "lang-extended")]
            "sh" | "bash" => Some(Language::Bash),
            #[cfg(feature = "lang-extended")]
            "ps1" | "psm1" => Some(Language::PowerShell),
            #[cfg(feature = "lang-extended")]
            "lua" => Some(Language::Lua),
            #[cfg(feature = "lang-extended")]
            "yaml" | "yml" => Some(Language::Yaml),
            #[cfg(feature = "lang-extended")]
            "toml" => Some(Language::Toml),
            #[cfg(feature = "lang-extended")]
            "json" | "jsonc" => Some(Language::Json),
            #[cfg(feature = "lang-extended")]
            "properties" => Some(Language::Properties),
            #[cfg(feature = "lang-extended")]
            "ex" | "exs" => Some(Language::Elixir),
            #[cfg(feature = "lang-extended")]
            "hs" => Some(Language::Haskell),
            #[cfg(feature = "lang-extended")]
            "dart" => Some(Language::Dart),
            #[cfg(feature = "lang-extended")]
            "sql" => Some(Language::Sql),
            #[cfg(feature = "lang-extended")]
            "html" | "htm" => Some(Language::Html),
            #[cfg(feature = "lang-extended")]
            "css" => Some(Language::Css),
            #[cfg(feature = "lang-extended")]
            "r" | "R" => Some(Language::R),
            _ => None,
        }
    }

    pub(crate) fn grammar(self) -> tree_sitter::Language {
        match self {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Swift => tree_sitter_swift::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Scala => tree_sitter_scala::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Zig => tree_sitter_zig::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            #[cfg(feature = "lang-extended")]
            Language::Bash => tree_sitter_bash::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::PowerShell => tree_sitter_powershell::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Lua => tree_sitter_lua::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Json => tree_sitter_json::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Properties => tree_sitter_properties::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Haskell => tree_sitter_haskell::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Dart => tree_sitter_dart::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Sql => tree_sitter_sequel::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Css => tree_sitter_css::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::R => tree_sitter_r::LANGUAGE.into(),
            // Unreachable without `lang-extended`: `from_path` never
            // returns one of these variants in that build, so `grammar`
            // never needs to build a `tree_sitter::Language` for it.
            #[cfg(not(feature = "lang-extended"))]
            Language::CSharp
            | Language::Kotlin
            | Language::Swift
            | Language::Scala
            | Language::Zig
            | Language::Ruby
            | Language::Php
            | Language::Bash
            | Language::PowerShell
            | Language::Lua
            | Language::Yaml
            | Language::Toml
            | Language::Json
            | Language::Properties
            | Language::Elixir
            | Language::Haskell
            | Language::Dart
            | Language::Sql
            | Language::Html
            | Language::Css
            | Language::R => unreachable!(
                "extended language grammar requested without `lang-extended` compiled in"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
