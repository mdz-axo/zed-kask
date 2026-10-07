/- zed-kask spec pin: re-anchored interpolation invariants for
hkask-mcp-media (from-scratch design review, Phase 4, MF-2).

The Rust side:
- `reanchored_corrected_words`
  (kask/mcp-servers/hkask-mcp-media/src/transcript_layers.rs) slices each
  cardinality-changing edit's replacement tokens equally across its source
  range's span: token k of M gets [start + k·span/M,
  start + (k+1)·span/M] (floor ms), span = the source range's
  end − start. The unit tests pin the behavior on concrete cases
  (`reanchored_insertion_slices_the_source_span_equally`,
  `reanchored_deletion_and_many_to_few_slice_the_span`,
  `reanchored_zero_duration_span_is_degenerate_but_deterministic`); this
  file pins the arithmetic CONTRACT at spec level — the second oracle.

STATED ASSUMPTIONS:
1. Nat models the millisecond offsets (u64 in Rust; the invariants are
   order-theoretic, unaffected by width).
2. `span / count` is Euclidean floor division — Rust's u64 `/` on
   nonnegative operands.
3. count ≥ 1 (an empty replacement contributes no tokens — the Rust path
   emits none), token index k < count.
4. The Rust saturating subtraction in the span computation is not modeled:
   the source range is well-formed (end ≥ start) by layer validation
   upstream, so span = end − start is exact.

The theorems are parametric in span/count/token; `#print axioms` reports
only the standard logical axioms — no `sorryAx`.
-/

/-- The first token's offset starts at zero: 0·span/count = 0 — the
slices begin at the source span's start, exactly. -/
theorem token_zero_starts_at_zero (span count : Nat) : 0 * span / count = 0 := by
  simp

/-- The last token's offset ends at the full span: count·span/count = span —
the slices are CONFINED (never past the source span's end) and the span's
end is exact, never truncated. -/
theorem last_token_ends_at_span (span : Nat) {count : Nat} (h : 1 ≤ count) :
    count * span / count = span := by
  exact Nat.mul_div_cancel_left (by omega) span

/-- Monotone slices: token k's offset ≤ token (k+1)'s offset — the
interpolation never emits a reversed pair, for every token index and every
span/count. -/
theorem slices_monotone (span k : Nat) {count : Nat} (hk : k + 1 ≤ count) :
    k * span / count ≤ (k + 1) * span / count := by
  have hmul : k * span ≤ (k + 1) * span := by
    exact Nat.mul_le_mul_right span (Nat.le_succ k)
  exact Nat.div_le_div_left hmul count

/-- Confined slices: token (k+1)'s offset ≤ span — every slice stays inside
the source range's span, for every token index and every span/count. -/
theorem slices_confined (span k : Nat) {count : Nat} (hk : k + 1 ≤ count) :
    (k + 1) * span / count ≤ span := by
  have hmul : (k + 1) * span ≤ count * span :=
    Nat.mul_le_mul_right span hk
  have hdiv := Nat.div_le_div_left hmul count
  rw [last_token_ends_at_span span (by omega)] at hdiv
  exact hdiv

#print axioms token_zero_starts_at_zero
#print axioms last_token_ends_at_span
#print axioms slices_monotone
#print axioms slices_confined