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
            "cs" => Some(Language::CSharp),
            "kt" | "kts" => Some(Language::Kotlin),
            "swift" => Some(Language::Swift),
            "scala" | "sc" => Some(Language::Scala),
            "zig" => Some(Language::Zig),
            "rb" => Some(Language::Ruby),
            "php" => Some(Language::Php),
            "sh" | "bash" => Some(Language::Bash),
            "ps1" | "psm1" => Some(Language::PowerShell),
            "lua" => Some(Language::Lua),
            "yaml" | "yml" => Some(Language::Yaml),
            "toml" => Some(Language::Toml),
            "json" | "jsonc" => Some(Language::Json),
            "properties" => Some(Language::Properties),
            "ex" | "exs" => Some(Language::Elixir),
            "hs" => Some(Language::Haskell),
            "dart" => Some(Language::Dart),
            "sql" => Some(Language::Sql),
            "html" | "htm" => Some(Language::Html),
            "css" => Some(Language::Css),
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
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Language::Swift => tree_sitter_swift::LANGUAGE.into(),
            Language::Scala => tree_sitter_scala::LANGUAGE.into(),
            Language::Zig => tree_sitter_zig::LANGUAGE.into(),
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Language::Bash => tree_sitter_bash::LANGUAGE.into(),
            Language::PowerShell => tree_sitter_powershell::LANGUAGE.into(),
            Language::Lua => tree_sitter_lua::LANGUAGE.into(),
            Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Language::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Language::Json => tree_sitter_json::LANGUAGE.into(),
            Language::Properties => tree_sitter_properties::LANGUAGE.into(),
            Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            Language::Haskell => tree_sitter_haskell::LANGUAGE.into(),
            Language::Dart => tree_sitter_dart::LANGUAGE.into(),
            Language::Sql => tree_sitter_sequel::LANGUAGE.into(),
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            Language::Css => tree_sitter_css::LANGUAGE.into(),
            Language::R => tree_sitter_r::LANGUAGE.into(),
        }
    }
}

#[cfg(test)]
mod tests;
