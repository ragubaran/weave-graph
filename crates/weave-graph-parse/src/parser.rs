use std::path::Path;

use thiserror::Error;
use tree_sitter::{InputEdit, Tree};

use crate::extract;
use crate::language::Language;
use crate::model::ParsedFile;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("tree-sitter grammar for {0:?} is incompatible with this tree-sitter version")]
    IncompatibleGrammar(Language),
    #[error("tree-sitter parse was cancelled")]
    Cancelled,
}

/// One-shot parse for files with no prior state (initial bulk index).
/// Returns `None` if `path`'s extension isn't a supported language.
pub fn parse_file(path: &Path, source: &str) -> Option<Result<ParsedFile, ParseError>> {
    let language = Language::from_path(path)?;
    Some(SourceParser::new(language).and_then(|mut p| p.parse(&path.to_string_lossy(), source)))
}

/// Wraps a `tree_sitter::Parser` and its last-parsed tree so an edited
/// file can be reparsed incrementally (`impl.md` M1.2: "use tree-sitter's
/// native incremental reparse API from the start") instead of rebuilding
/// parse state from scratch on every keystroke-sized change.
pub struct SourceParser {
    language: Language,
    parser: tree_sitter::Parser,
    tree: Option<Tree>,
}

impl SourceParser {
    pub fn new(language: Language) -> Result<Self, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.grammar())
            .map_err(|_| ParseError::IncompatibleGrammar(language))?;
        Ok(Self {
            language,
            parser,
            tree: None,
        })
    }

    pub fn parse(&mut self, path: &str, source: &str) -> Result<ParsedFile, ParseError> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or(ParseError::Cancelled)?;
        let parsed = extract::extract(self.language, tree.root_node(), source.as_bytes(), path);
        self.tree = Some(tree);
        Ok(parsed)
    }

    /// `edit` must describe the byte/position range that changed in the
    /// *previous* source this parser saw; `new_source` is the file's full
    /// text after the edit. Falls back to a fresh parse if this is the
    /// first call (nothing cached yet to reuse).
    pub fn reparse(
        &mut self,
        path: &str,
        new_source: &str,
        edit: InputEdit,
    ) -> Result<ParsedFile, ParseError> {
        if let Some(old_tree) = self.tree.as_mut() {
            old_tree.edit(&edit);
        }
        let tree = self
            .parser
            .parse(new_source, self.tree.as_ref())
            .ok_or(ParseError::Cancelled)?;
        let parsed = extract::extract(self.language, tree.root_node(), new_source.as_bytes(), path);
        self.tree = Some(tree);
        Ok(parsed)
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

#[cfg(test)]
mod tests;
