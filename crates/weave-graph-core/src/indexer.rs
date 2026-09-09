/// Reindex configuration knobs (`plan.md` §1.2a, §0.3). Both values are
/// configurable so a tiny repo never bails out on a 3-file change.
#[derive(Debug, Clone)]
pub struct ReindexConfig {
    /// Minimum changed-file count before bailout is considered.
    pub bailout_floor: usize,
    /// Fraction of total indexed files above which a full rebuild is cheaper.
    pub bailout_ratio: f64,
}

impl Default for ReindexConfig {
    fn default() -> Self {
        Self {
            bailout_floor: 100,
            bailout_ratio: 0.10,
        }
    }
}

/// Returns true when a full rebuild is cheaper than per-file diff.
/// Bail if `changed > max(bailout_floor, bailout_ratio * total_indexed)`.
pub fn should_bail_out(changed: usize, total_indexed: usize, cfg: &ReindexConfig) -> bool {
    let threshold = cfg
        .bailout_floor
        .max((cfg.bailout_ratio * total_indexed as f64) as usize);
    changed > threshold
}

#[cfg(test)]
mod tests;
