//! SCIP-*inspired* local key, not the real SCIP wire protocol (`scip.proto`
//! is a whole protobuf ecosystem for cross-project lookup) — Phase 1 only
//! needs resolution within one repo (`plan.md` §2.3 covers cross-repo).
//! Revisit if `federation`/`hub` need to interoperate with real SCIP.

pub fn build(path: &str, qualified_symbol: &str) -> String {
    format!("{path}#{qualified_symbol}")
}

#[cfg(test)]
mod tests;
