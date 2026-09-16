use std::path::Path;

/// Language set: the original four (Rust, Python, JS, TS) plus the
/// widened set added later — Go, Java, C, C++, C#, Kotlin, Swift, Scala,
/// Zig, Ruby, PHP, Shell/Bash, PowerShell, Lua.
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
    #[cfg(feature = "lang-extended")]
    CSharp,
    #[cfg(feature = "lang-extended")]
    Kotlin,
    #[cfg(feature = "lang-extended")]
    Swift,
    #[cfg(feature = "lang-extended")]
    Scala,
    #[cfg(feature = "lang-extended")]
    Zig,
    #[cfg(feature = "lang-extended")]
    Ruby,
    #[cfg(feature = "lang-extended")]
    Php,
    #[cfg(feature = "lang-extended")]
    Bash,
    #[cfg(feature = "lang-extended")]
    PowerShell,
    #[cfg(feature = "lang-extended")]
    Lua,
    #[cfg(feature = "lang-extended")]
    Yaml,
    #[cfg(feature = "lang-extended")]
    Toml,
    #[cfg(feature = "lang-extended")]
    Json,
    #[cfg(feature = "lang-extended")]
    Properties,
    /// Any language with a grammar but no hand-written `extract/<lang>.rs`
    /// — routed through `query_vm`'s generic, kind-name-heuristic
    /// extractor instead of bespoke Rust per language.
    #[cfg(feature = "lang-extended")]
    Elixir,
    #[cfg(feature = "lang-extended")]
    Haskell,
    #[cfg(feature = "lang-extended")]
    Dart,
    #[cfg(feature = "lang-extended")]
    Sql,
    #[cfg(feature = "lang-extended")]
    Html,
    #[cfg(feature = "lang-extended")]
    Css,
    #[cfg(feature = "lang-extended")]
    R,
    #[cfg(feature = "lang-extended")]
    ArkTs,
    #[cfg(feature = "lang-extended")]
    ObjC,
    #[cfg(feature = "lang-extended")]
    Metal,
    #[cfg(feature = "lang-extended")]
    Cuda,
    #[cfg(feature = "lang-extended")]
    Svelte,
    #[cfg(feature = "lang-extended")]
    Vue,
    #[cfg(feature = "lang-extended")]
    Astro,
    #[cfg(feature = "lang-extended")]
    Liquid,
    #[cfg(feature = "lang-extended")]
    Pascal,
    #[cfg(feature = "lang-extended")]
    Luau,
    #[cfg(feature = "lang-extended")]
    Cfml,
    #[cfg(feature = "lang-extended")]
    Cobol,
    #[cfg(feature = "lang-extended")]
    VisualBasic,
    #[cfg(feature = "lang-extended")]
    Erlang,
    #[cfg(feature = "lang-extended")]
    Solidity,
    #[cfg(feature = "lang-extended")]
    Terraform,
    #[cfg(feature = "lang-extended")]
    Nix,
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
            #[cfg(feature = "lang-extended")]
            "ets" => Some(Language::ArkTs),
            #[cfg(feature = "lang-extended")]
            "m" | "mm" => Some(Language::ObjC),
            #[cfg(feature = "lang-extended")]
            "metal" => Some(Language::Metal),
            #[cfg(feature = "lang-extended")]
            "cu" | "cuh" => Some(Language::Cuda),
            #[cfg(feature = "lang-extended")]
            "svelte" => Some(Language::Svelte),
            #[cfg(feature = "lang-extended")]
            "vue" => Some(Language::Vue),
            #[cfg(feature = "lang-extended")]
            "astro" => Some(Language::Astro),
            #[cfg(feature = "lang-extended")]
            "liquid" => Some(Language::Liquid),
            #[cfg(feature = "lang-extended")]
            "pas" | "pp" => Some(Language::Pascal),
            #[cfg(feature = "lang-extended")]
            "luau" => Some(Language::Luau),
            #[cfg(feature = "lang-extended")]
            "cfm" | "cfc" => Some(Language::Cfml),
            #[cfg(feature = "lang-extended")]
            "cbl" | "cob" => Some(Language::Cobol),
            #[cfg(feature = "lang-extended")]
            "vb" => Some(Language::VisualBasic),
            #[cfg(feature = "lang-extended")]
            "erl" | "hrl" => Some(Language::Erlang),
            #[cfg(feature = "lang-extended")]
            "sol" => Some(Language::Solidity),
            #[cfg(feature = "lang-extended")]
            "tf" | "tofu" => Some(Language::Terraform),
            #[cfg(feature = "lang-extended")]
            "nix" => Some(Language::Nix),
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
            #[cfg(feature = "lang-extended")]
            Language::ArkTs => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            #[cfg(feature = "lang-extended")]
            Language::ObjC => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Metal => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Cuda => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Svelte => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Vue => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Astro => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Liquid => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Pascal => tree_sitter_lua::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Luau => tree_sitter_lua::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Cfml => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Cobol => tree_sitter_bash::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::VisualBasic => tree_sitter_c_sharp::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Erlang => tree_sitter_elixir::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Solidity => tree_sitter_javascript::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Terraform => tree_sitter_ruby::LANGUAGE.into(),
            #[cfg(feature = "lang-extended")]
            Language::Nix => tree_sitter_elixir::LANGUAGE.into(),
        }
    }
}

#[cfg(test)]
mod tests;
