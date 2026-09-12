//! Query-expansion boundary (`impl.md` M3.7 Tier 1): static synonym
//! groups and identifier splitting. Pure string transforms, no I/O/SQL —
//! FTS5's own `porter` tokenizer covers stemming, so that part of the
//! original proposal needs no code here at all.

/// Equivalence groups of software terms/abbreviations — a handful of
/// short static arrays, so a linear scan per query word is simpler and
/// cheaper than a perfect-hash map would be at this size.
static SYNONYM_GROUPS: &[&[&str]] = &[
    &[
        "auth",
        "authentication",
        "login",
        "jwt",
        "token",
        "credential",
    ],
    &[
        "ttl",
        "expiry",
        "expiration",
        "timeout",
        "lifetime",
        "deadline",
    ],
    &["calc", "calculate", "compute", "eval", "score"],
    &["db", "database", "store", "repository", "sql", "storage"],
    &["msg", "message", "payload", "event", "packet"],
];

/// Free text -> FTS5 `MATCH` expression: words AND together, each word
/// ORs with the rest of its synonym group (if it belongs to one).
/// Quoted so FTS5 never misreads a word as an operator.
pub fn expand_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            match SYNONYM_GROUPS.iter().find(|g| g.contains(&lower.as_str())) {
                Some(group) => format!(
                    "({})",
                    group
                        .iter()
                        .map(|t| format!("\"{t}\""))
                        .collect::<Vec<_>>()
                        .join(" OR ")
                ),
                None => format!("\"{lower}\""),
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Splits `JwtTokenExpirationHandler` into "Jwt Token Expiration Handler"
/// so FTS5 indexes it as matchable words, not one opaque token. `_`/`-`
/// become spaces; a run of uppercase letters (`HTTPServer`) stays one
/// word until a lowercase letter starts the next.
pub fn split_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' {
            out.push(' ');
            continue;
        }
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let starts_word = match prev {
            Some(p) if c.is_uppercase() && (p.is_lowercase() || p.is_ascii_digit()) => true,
            Some(p)
                if c.is_uppercase() && p.is_uppercase() && next.is_some_and(char::is_lowercase) =>
            {
                true
            }
            _ => false,
        };
        if starts_word && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
