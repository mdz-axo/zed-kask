#[derive(Debug, Clone)]
pub struct ConsolidationOutcome {
    pub deleted_count: usize,
    pub failed_count: usize,
    /// The pass was skipped because nothing can have changed since the
    /// last one (the hmems high-water rowid is unchanged) — hang-audit Pr4
    /// over F5: the no-op sweeps (0 rows deleted every observed tick) no
    /// longer pay the pass cost.
    pub skipped: bool,
}
