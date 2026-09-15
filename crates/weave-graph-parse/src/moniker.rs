//! SCIP-*inspired* local key, not the real SCIP wire protocol (`scip.proto`
//! is a whole protobuf ecosystem for cross-project lookup) — this crate
//! only needs resolution within one repo, not across repos. Revisit if
//! `federation`/`hub` need to interoperate with real SCIP.

pub fn build(path: &str, qualified_symbol: &str) -> String {
    format!("{path}#{qualified_symbol}")
}

#[cfg(test)]
mod tests;
