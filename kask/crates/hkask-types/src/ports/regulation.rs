/// Parameters for consolidation. All fields except `limit` optional.
#[derive(Debug, Clone)]
pub struct ConsolidationRequest {
    pub limit: usize,
    pub confidence_floor: Option<f64>,
    /// Maximum h_mem count after pruning — an explicit caller instruction.
    /// When `None`, there is no count cap (count-based pruning is deprecated,
    /// operator ruling 2026-09-04: forgetting is time-based and
    /// distillation-gated).
    pub max_h_mems: Option<usize>,
}

impl Default for ConsolidationRequest {
    fn default() -> Self {
        Self {
            limit: 100,
            confidence_floor: None,
            max_h_mems: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidationOutcome {
    pub deleted_count: usize,
    pub failed_count: usize,
}
