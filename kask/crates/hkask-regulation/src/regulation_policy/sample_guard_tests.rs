use super::*;

#[test]
fn small_or_noisy_samples_cannot_block() {
    // 3/3 → 1/3: too few attempts.
    assert!(insufficient_block_sample(1.0, 1.0 / 3.0, Some((3, 3))).is_some());
    // Unknown sizes are insufficient, never large.
    assert!(insufficient_block_sample(1.0, 0.0, None).is_some());
    // 12/12 → 4/12 (E4's observed runs): a real, significant drop.
    assert!(insufficient_block_sample(1.0, 4.0 / 12.0, Some((12, 12))).is_none());
    // 0.60 → 0.45 at n=12: within noise.
    assert!(insufficient_block_sample(0.60, 0.45, Some((12, 12))).is_some());
}
