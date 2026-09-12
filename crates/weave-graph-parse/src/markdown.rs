//! Deterministic Markdown extraction for the `docs` feature (`plan.md`
//! §2.1): wikilinks, frontmatter tags/aliases, and backtick code
//! references. `pulldown-cmark`'s pure CPU streaming parser only — no
//! LLM, no network, and (per `AGENTS.md` §3) no regex parser.

use pulldown_cmark::{Event, Parser};

/// A `[[Note Name]]` or `[[Note Name#Section]]` wikilink — Obsidian's own
/// non-standard syntax, invisible to plain CommonMark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wikilink {
    pub target: String,
    pub section: Option<String>,
}

/// Everything extracted from one Markdown file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedMarkdown {
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub links: Vec<Wikilink>,
    /// Heading texts in document order (`# Section` -> "Section") — the
    /// `[[Note#Section]]` anchor targets resolve against these.
    pub headings: Vec<String>,
    /// Raw backtick code-span text, e.g. `AuthService.verify()` — resolving
    /// this against already-indexed code symbols is the caller's job, not
    /// this module's; a Markdown parser has no business knowing the graph.
    pub code_refs: Vec<String>,
}

pub fn parse_markdown(source: &str) -> ParsedMarkdown {
    let (frontmatter, body) = split_frontmatter(source);
    let mut doc = ParsedMarkdown {
        tags: frontmatter
            .map(|fm| extract_yaml_list(fm, "tags"))
            .unwrap_or_default(),
        aliases: frontmatter
            .map(|fm| extract_yaml_list(fm, "aliases"))
            .unwrap_or_default(),
        ..ParsedMarkdown::default()
    };
    // pulldown-cmark splits `[[Note]]` across several adjacent `Text`
    // events (one per bracket run) since `[`/`]` are CommonMark link
    // syntax it tries and fails to match — Obsidian's non-standard
    // wikilink syntax only becomes visible once those fragments are
    // rejoined in document order, so accumulate first and scan once.
    let mut text_buffer = String::new();
    let mut in_heading: Option<String> = None;
    for event in Parser::new(body) {
        match event {
            Event::Code(text) => doc.code_refs.push(text.to_string()),
            Event::Text(text) => match &mut in_heading {
                Some(heading) => heading.push_str(text.as_ref()),
                None => text_buffer.push_str(text.as_ref()),
            },
            Event::Start(pulldown_cmark::Tag::Heading { .. }) => {
                in_heading = Some(String::new());
            }
            Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                if let Some(heading) = in_heading.take() {
                    // CommonMark allows a closing ATX sequence (`## Intro ##`);
                    // pulldown-cmark leaves it in the text events.
                    let heading = heading.trim().trim_end_matches('#').trim().to_string();
                    if !heading.is_empty() {
                        doc.headings.push(heading);
                    }
                }
            }
            _ => {}
        }
    }
    doc.links = scan_wikilinks(&text_buffer);
    doc
}

/// Splits a leading `---\n...\n---` YAML frontmatter block from the body.
/// `(None, source)` when there's no frontmatter fence — not every note has
/// one, and that's not an error.
fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let Some(rest) = source.strip_prefix("---\n") else {
        return (None, source);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, source);
    };
    let frontmatter = &rest[..end];
    let after = &rest[end + 4..];
    let body = after.strip_prefix('\n').unwrap_or(after);
    (Some(frontmatter), body)
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

/// A narrow, deliberate YAML subset — only the two list shapes Obsidian's
/// own frontmatter convention uses for `tags`/`aliases`: `key: [a, b]` and
/// `key:\n  - a\n  - b`. Not a general YAML parser (`AGENTS.md` §3: pure
/// CPU streaming parsers, zero regex — this stays inside that spirit by
/// staying this narrow instead of reaching for a full YAML crate).
fn extract_yaml_list(frontmatter: &str, key: &str) -> Vec<String> {
    let mut lines = frontmatter.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let rest = rest.trim();
        if let Some(inline) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return inline
                .split(',')
                .map(unquote)
                .filter(|s| !s.is_empty())
                .collect();
        }
        if rest.is_empty() {
            let mut items = Vec::new();
            for item_line in lines.by_ref() {
                let Some(item) = item_line.trim_start().strip_prefix("- ") else {
                    break;
                };
                items.push(unquote(item));
            }
            return items;
        }
        return vec![unquote(rest)];
    }
    Vec::new()
}

/// Manual scan for `[[...]]`, not a regex — safe to index by byte since
/// `[`/`]`/`#` are ASCII and never appear as a UTF-8 continuation byte.
fn scan_wikilinks(text: &str) -> Vec<Wikilink> {
    let mut links = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'['
            && bytes[i + 1] == b'['
            && let Some(end) = text[i + 2..].find("]]")
        {
            let inner = text[i + 2..i + 2 + end].trim();
            if !inner.is_empty() {
                let (target, section) = match inner.split_once('#') {
                    Some((t, s)) => (t.trim().to_string(), Some(s.trim().to_string())),
                    None => (inner.to_string(), None),
                };
                if !target.is_empty() {
                    links.push(Wikilink { target, section });
                }
            }
            i += 2 + end + 2;
            continue;
        }
        i += 1;
    }
    links
}

#[cfg(test)]
mod tests;
