/-
zed-kask spec pin: event-tree marginalization invariants for
hkask-mcp-scenarios (from-scratch design review, Phase 4).

The Rust side:
- `ScenarioEvent::validate` (kask/mcp-servers/hkask-mcp-scenarios/src/types.rs:654-708)
  rejects probabilities outside [0,1] and conditional tables whose length
  is not 2^|parents| before any computation runs.
- `hkask_forecast::marginalize` (kask/crates/hkask-forecast/src/hkask_forecast.rs:149-168)
  computes, for parent marginals ps and a bitmap-ordered table c,
  marginal = SUM_assignment (PROD_j (bit j set ? ps[j] : 1-ps[j])) * c[assignment],
  with bit j of the assignment index corresponding to parent j.
- `combine_independent_channels` (kask/mcp-servers/hkask-mcp-scenarios/src/superforecast/math.rs:122-127)
  combines multiple dependency groups by noisy-OR: 1 - PROD_g (1 - P_g).

This file pins the invariants at spec level over an abstract ordered field F.
STATED ASSUMPTIONS:
1. F models the OrdField axioms below — standard ordered-field facts (each
   provable from the usual ordered-field axioms; R and Q are the intended
   models). The theorems are parametric in F, so `#print axioms` reports
   only the standard logical axioms — the OrdField fields are the stated
   assumptions, carried by the typeclass, not hidden axioms.
2. Parent independence: the assignment mass factorizes as a product of
   per-parent terms — the same assumption the Rust marginalizer makes. The
   property layer pins its approximation character when parents share
   ancestry (`correlated_parents_keep_the_documented_independence_approximation`,
   kask/mcp-servers/hkask-mcp-scenarios/src/property_tests.rs:293-325).
3. Exact arithmetic: the Rust f64 clamp (math.rs:109) is NOT modeled —
   these are exact-arithmetic invariants.
4. The `none` of this model corresponds to the server's validation
   rejection (types.rs:684-695). The math crate's zero-fill leniency for
   short tables (hkask_forecast.rs:163-165, pinned by
   `marginalize_short_table_contributes_zero_for_missing_entries`) is a
   DIFFERENT, deliberately lenient contract, out of this model's scope: the
   server never reaches it with an invalid table.

A proof here establishes the invariant under these assumptions — not
fitness to the reference models, and not the Rust implementation (which is
pinned by its own tests). Core Lean 4 only (no Mathlib); checked with the
project's pinned Lake toolchain via lean_check.
-/

namespace EventTreeSpec

/-- An ordered semifield: the stated assumptions, as standard facts. -/
class OrdField (F : Type) where
  add : F → F → F
  mul : F → F → F
  sub : F → F → F
  zero : F
  one : F
  le : F → F → Prop
  add_comm : ∀ {a b : F}, add a b = add b a
  add_assoc : ∀ {a b c : F}, add (add a b) c = add a (add b c)
  add_zero : ∀ {a : F}, add a zero = a
  add_right_cancel : ∀ {a b c : F}, add b a = add c a → b = c
  mul_comm : ∀ {a b : F}, mul a b = mul b a
  mul_assoc : ∀ {a b c : F}, mul (mul a b) c = mul a (mul b c)
  mul_one : ∀ {a : F}, mul a one = a
  one_mul : ∀ {a : F}, mul one a = a
  mul_zero : ∀ {a : F}, mul a zero = zero
  left_distrib : ∀ {a b c : F}, mul a (add b c) = add (mul a b) (mul a c)
  sub_add_cancel : ∀ {a b : F}, add (sub a b) b = a
  sub_zero : ∀ {a : F}, sub a zero = a
  sub_nonneg_iff_le : ∀ {a b : F}, le zero (sub a b) ↔ le b a
  sub_le_sub_left : ∀ {a b c : F}, le b c → le (sub a c) (sub a b)
  le_refl : ∀ {a : F}, le a a
  le_trans : ∀ {a b c : F}, le a b → le b c → le a c
  zero_le_one : le zero one
  add_le_add : ∀ {a b c d : F}, le a b → le c d → le (add a c) (add b d)
  add_nonneg : ∀ {a b : F}, le zero a → le zero b → le zero (add a b)
  mul_nonneg : ∀ {a b : F}, le zero a → le zero b → le zero (mul a b)
  mul_le_mul_left : ∀ {m a b : F}, le zero m → le a b → le (mul m a) (mul m b)

open OrdField

variable {F : Type} [OrdField F]

/-- Even-indexed sublist: the entries whose assignment bit 0 (first
parent) is clear. -/
def evens : List F → List F
  | [] => []
  | [a] => [a]
  | a :: _ :: rest => a :: evens rest

/-- Odd-indexed sublist: the entries whose assignment bit 0 (first
parent) is set. -/
def odds : List F → List F
  | [] => []
  | [_] => []
  | _ :: b :: rest => b :: odds rest

/--
Marginalization over independent parents. `marginalize ps c` consumes the
parent list one parent at a time against the bitmap-ordered table: the
first parent splits `c` into its even indices (parent false) and odd
indices (parent true). `none` when the table length is not 2^|ps| — the
model of the server's validation rejection.
-/
def marginalize : List F → List F → Option F
  | [], [c] => some c
  | [], _ => none
  | p :: ps, c =>
      match marginalize ps (evens c), marginalize ps (odds c) with
      | some m0, some m1 => some (add (mul (sub one p) m0) (mul p m1))
      | _, _ => none

-- ── Membership flows into the halves ────────────────────────────────────────

theorem mem_of_mem_evens (x : F) : ∀ (c : List F), x ∈ evens c → x ∈ c
  | [], h => by simp [evens] at h
  | [_], h => by simpa [evens] using h
  | a :: _ :: rest, h => by
      rcases List.mem_cons.1 h with rfl | hmem
      · exact List.mem_cons_self
      · exact List.mem_cons_of_mem _ (List.mem_cons_of_mem _ (mem_of_mem_evens x rest hmem))

theorem mem_of_mem_odds (x : F) : ∀ (c : List F), x ∈ odds c → x ∈ c
  | [], h => by simp [odds] at h
  | [_], h => by simp [odds] at h
  | _ :: b :: rest, h => by
      rcases List.mem_cons.1 h with rfl | hmem
      · exact List.mem_cons_of_mem _ List.mem_cons_self
      · exact List.mem_cons_of_mem _ (List.mem_cons_of_mem _ (mem_of_mem_odds x rest hmem))

-- ── Length bookkeeping for the even/odd split ──────────────────────────────

/-- An even-length list's halves each carry half its length. -/
theorem evens_odds_length (k : Nat) : ∀ (c : List F), c.length = 2 * k →
    (evens c).length = k ∧ (odds c).length = k := by
  induction k with
  | zero =>
      intro c h
      rcases c with _ | ⟨a, rest⟩
      · exact ⟨rfl, rfl⟩
      · simp only [List.length_cons] at h
        omega
  | succ k ih =>
      intro c h
      rcases c with _ | ⟨a, rest⟩
      · simp only [List.length_nil] at h
        omega
      rcases rest with _ | ⟨b, rest'⟩
      · simp only [List.length_cons, List.length_nil] at h
        omega
      · have hr : rest'.length = 2 * k := by
          simp only [List.length_cons] at h
          omega
        obtain ⟨he, ho⟩ := ih rest' hr
        have hE : evens (a :: b :: rest') = a :: evens rest' := rfl
        have hO : odds (a :: b :: rest') = b :: odds rest' := rfl
        constructor
        · rw [hE, List.length_cons]; omega
        · rw [hO, List.length_cons]; omega

/-- Halves of equal length force the even total: the converse split. -/
theorem lengths_evens_odds : ∀ (c : List F) (k : Nat),
    (evens c).length = k → (odds c).length = k → c.length = 2 * k
  | [], _, he, ho => by
      simp only [evens, odds, List.length_nil] at he ho
      simp only [List.length_nil]
      omega
  | [_], _, he, ho => by
      simp only [evens, odds, List.length_singleton, List.length_nil] at he ho
      omega
  | a :: b :: rest, k, he, ho => by
      have hE : evens (a :: b :: rest) = a :: evens rest := rfl
      have hO : odds (a :: b :: rest) = b :: odds rest := rfl
      rw [hE, List.length_cons] at he
      rw [hO, List.length_cons] at ho
      have ih := lengths_evens_odds rest (k - 1) (by omega) (by omega)
      simp only [List.length_cons, List.length_cons]
      omega

-- ── The CPT-length invariant (types.rs:684-695) ────────────────────────────

/-- CPT completeness: a table that marginalizes has exactly 2^|parents|
entries. This is the model of the server's validation — a wrong-length
table never produces a marginal. -/
theorem marginalize_length (ps : List F) : ∀ (c : List F) (m : F),
    marginalize ps c = some m → c.length = 2 ^ ps.length := by
  induction ps with
  | nil =>
      intro c m h
      rcases c with _ | ⟨a, rest⟩
      · simp [marginalize] at h
      rcases rest with _ | ⟨b, rest'⟩
      · simp only [List.length_cons, List.length_nil]
      · simp [marginalize] at h
  | cons p ps ih =>
      intro c m h
      simp only [marginalize] at h
      split at h
      · next m0 m1 h0 h1 =>
          have he := ih (evens c) m0 h0
          have ho := ih (odds c) m1 h1
          have hlen := lengths_evens_odds c _ he ho
          rw [List.length_cons, Nat.pow_succ]
          omega
      · next => simp at h

/-- CPT soundness: a table of exactly 2^|parents| entries marginalizes.
Together with completeness, the model is total exactly on valid tables —
the validation neither over-accepts nor over-rejects. -/
theorem marginalize_some (ps : List F) : ∀ (c : List F),
    c.length = 2 ^ ps.length → ∃ m, marginalize ps c = some m := by
  induction ps with
  | nil =>
      intro c h
      rcases c with _ | ⟨a, rest⟩
      · simp only [List.length_nil, Nat.pow_zero] at h
        omega
      rcases rest with _ | ⟨b, rest'⟩
      · exact ⟨a, rfl⟩
      · simp only [List.length_cons, List.length_cons, List.length_nil, Nat.pow_zero] at h
        omega
  | cons p ps ih =>
      intro c h
      have h2 : c.length = 2 * 2 ^ ps.length := by
        rw [List.length_cons, Nat.pow_succ] at h
        omega
      obtain ⟨he, ho⟩ := evens_odds_length (2 ^ ps.length) c h2
      obtain ⟨m0, h0⟩ := ih (evens c) he
      obtain ⟨m1, h1⟩ := ih (odds c) ho
      refine ⟨add (mul (sub one p) m0) (mul p m1), ?_⟩
      simp only [marginalize, h0, h1]

-- ── The bitmap contract (bit j of the index ↔ parent j) ─────────────────────

/-- Single-parent bitmap: conditionals[0] = P(E | parent false),
conditionals[1] = P(E | parent true). -/
theorem single_parent_bitmap (p c0 c1 : F) :
    marginalize [p] [c0, c1] = some (add (mul (sub one p) c0) (mul p c1)) := rfl

/-- Two-parent bitmap: the marginal is the full joint enumeration
(1-p)(1-q)c00 + (1-p)q c10 + p(1-q)c01 + p q c11, with bit 0 = the first
parent and bit 1 = the second — the wire contract of
`conditionals[i] = P(E | parent truth assignment i)`. -/
theorem two_parent_bitmap (p q c00 c01 c10 c11 : F) :
    marginalize [p, q] [c00, c01, c10, c11]
      = some (add
          (mul (sub one p) (add (mul (sub one q) c00) (mul q c10)))
          (mul p (add (mul (sub one q) c01) (mul q c11))) ) := rfl

-- ── The probability-bounds invariant (types.rs:655, 697-704) ────────────────

/-- Marginalization preserves the probability bounds: parents and table
entries in [0,1] yield a marginal in [0,1]. -/
theorem marginal_bounds (ps : List F) : ∀ (c : List F) (m : F),
    (∀ x ∈ ps, le zero x ∧ le x one) →
    (∀ x ∈ c, le zero x ∧ le x one) →
    marginalize ps c = some m →
    le zero m ∧ le m one := by
  induction ps with
  | nil =>
      intro c m hps hc h
      rcases c with _ | ⟨a, rest⟩
      · simp [marginalize] at h
      rcases rest with _ | ⟨b, rest'⟩
      · simp only [marginalize, Option.some.injEq] at h
        subst h
        exact hc a (by simp)
      · simp [marginalize] at h
  | cons p ps ih =>
      intro c m hps hc h
      obtain ⟨hp0, hp1⟩ := hps p (by simp)
      have hps' : ∀ x ∈ ps, le zero x ∧ le x one := fun x hx => hps x (by simp [hx])
      have hce : ∀ x ∈ evens c, le zero x ∧ le x one :=
        fun x hx => hc x (mem_of_mem_evens x c hx)
      have hco : ∀ x ∈ odds c, le zero x ∧ le x one :=
        fun x hx => hc x (mem_of_mem_odds x c hx)
      simp only [marginalize] at h
      split at h
      · next m0 m1 h0 h1 =>
          obtain ⟨b0, b1⟩ := ih (evens c) m0 hps' hce h0
          obtain ⟨d0, d1⟩ := ih (odds c) m1 hps' hco h1
          simp only [Option.some.injEq] at h
          subst h
          -- (1-p) in [0,1] from p in [0,1]
          have e0 : le zero (sub one p) := sub_nonneg_iff_le.mpr hp1
          have e1 : le (sub one p) one := by
            have hs : le (sub one p) (sub one zero) := sub_le_sub_left hp0
            rwa [sub_zero] at hs
          -- nonnegativity of both terms, then of the sum
          have t0 := mul_nonneg e0 b0
          have t1 := mul_nonneg hp0 d0
          have m_ge_0 := add_nonneg t0 t1
          -- each term bounded by its channel weight, sum bounded by (1-p)+p = 1
          have u0 : le (mul (sub one p) m0) (sub one p) := by
            have hu := mul_le_mul_left e0 b1
            rwa [mul_one] at hu
          have u1 : le (mul p m1) p := by
            have hu := mul_le_mul_left hp0 d1
            rwa [mul_one] at hu
          have hone : add (sub one p) p = one := sub_add_cancel
          have hs2 := add_le_add u0 u1
          rw [hone] at hs2
          exact ⟨m_ge_0, hs2⟩
      · next => simp at h

-- ── Mass conservation under parent independence ────────────────────────────

theorem evens_replicate (k : Nat) :
    evens (List.replicate (2 * k) (one : F)) = List.replicate k one := by
  induction k with
  | zero => rfl
  | succ k ih =>
      have h1 : List.replicate (2 * (k + 1)) (one : F)
          = one :: one :: List.replicate (2 * k) one := by
        have h2 : 2 * (k + 1) = (2 * k + 1) + 1 := by omega
        rw [h2, List.replicate_succ, List.replicate_succ]
      rw [h1]
      show one :: evens (List.replicate (2 * k) one) = one :: List.replicate k one
      rw [ih]

theorem odds_replicate (k : Nat) :
    odds (List.replicate (2 * k) (one : F)) = List.replicate k one := by
  induction k with
  | zero => rfl
  | succ k ih =>
      have h1 : List.replicate (2 * (k + 1)) (one : F)
          = one :: one :: List.replicate (2 * k) one := by
        have h2 : 2 * (k + 1) = (2 * k + 1) + 1 := by omega
        rw [h2, List.replicate_succ, List.replicate_succ]
      rw [h1]
      show one :: odds (List.replicate (2 * k) one) = one :: List.replicate k one
      rw [ih]

/-- Mass conservation: a certain child (all-ones table) marginalizes to
one whatever the parents — the total assignment mass is one under parent
independence. This is the normalization that makes the marginal a proper
convex combination of the table entries. -/
theorem certainty_preserved (ps : List F) :
    marginalize ps (List.replicate (2 ^ ps.length) one) = some one := by
  induction ps with
  | nil => rfl
  | cons p ps ih =>
      have hlen : 2 ^ (p :: ps).length = 2 * 2 ^ ps.length := by
        rw [List.length_cons, Nat.pow_succ]
        exact Nat.mul_comm _ _
      rw [hlen]
      have hE : evens (List.replicate (2 * 2 ^ ps.length) (one : F))
          = List.replicate (2 ^ ps.length) one := evens_replicate _
      have hO : odds (List.replicate (2 * 2 ^ ps.length) (one : F))
          = List.replicate (2 ^ ps.length) one := odds_replicate _
      simp only [marginalize, hE, hO, ih]
      rw [mul_one, mul_one, sub_add_cancel]

-- ── Noisy-OR combination of independent channels (math.rs:122-127) ──────────

/-- Survival: the probability that NO channel fires. -/
def survival : List F → F
  | [] => one
  | c :: cs => mul (sub one c) (survival cs)

/-- Noisy-OR: P(E) = 1 - PROD_g (1 - P_g). -/
def noisyOr (cs : List F) : F := sub one (survival cs)

/-- Single-channel identity: in exact arithmetic 1-(1-p) = p. The Rust
comment's "within 1 ULP" (math.rs:119-121) is a floating-point artifact,
not a semantic difference. -/
theorem noisyOr_single (p : F) : noisyOr [p] = p := by
  show sub one (mul (sub one p) one) = p
  rw [mul_one]
  have h1 : add (sub one (sub one p)) (sub one p) = one := sub_add_cancel
  have h2 : add p (sub one p) = one :=
    (add_comm (a := p) (b := sub one p)).trans sub_add_cancel
  exact add_right_cancel (h1.trans h2.symm)

/-- Noisy-OR preserves the probability bounds. -/
theorem noisyOr_bounds (cs : List F) (h : ∀ x ∈ cs, le zero x ∧ le x one) :
    le zero (noisyOr cs) ∧ le (noisyOr cs) one := by
  have hs : le zero (survival cs) ∧ le (survival cs) one := by
    induction cs with
    | nil =>
        show le zero one ∧ le one one
        exact ⟨zero_le_one, le_refl⟩
    | cons c rest ih =>
        obtain ⟨hc0, hc1⟩ := h c (by simp)
        obtain ⟨b0, b1⟩ := ih (fun x hx => h x (by simp [hx]))
        have e0 : le zero (sub one c) := sub_nonneg_iff_le.mpr hc1
        have e1 : le (sub one c) one := by
          have hs2 : le (sub one c) (sub one zero) := sub_le_sub_left hc0
          rwa [sub_zero] at hs2
        have hu : le (mul (sub one c) (survival rest)) (sub one c) := by
            have hw := mul_le_mul_left e0 b1
            rwa [mul_one] at hw
        constructor
        · exact mul_nonneg e0 b0
        · exact le_trans hu e1
  have e0 : le zero (sub one (survival cs)) := sub_nonneg_iff_le.mpr hs.2
  have e1 : le (sub one (survival cs)) one := by
    have hs2 : le (sub one (survival cs)) (sub one zero) :=
      sub_le_sub_left hs.1
    rwa [sub_zero] at hs2
  exact ⟨e0, e1⟩

end EventTreeSpec

#print axioms EventTreeSpec.marginalize_length
#print axioms EventTreeSpec.marginalize_some
#print axioms EventTreeSpec.single_parent_bitmap
#print axioms EventTreeSpec.two_parent_bitmap
#print axioms EventTreeSpec.marginal_bounds
#print axioms EventTreeSpec.certainty_preserved
#print axioms EventTreeSpec.noisyOr_single
#print axioms EventTreeSpec.noisyOr_bounds
