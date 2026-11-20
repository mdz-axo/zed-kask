/-
zed-kask research pin (2026-11-08, restated 2026-11-12): machine-checked
properties of the direct-model-edit skill's deterministic gates — DME-0
(run settings against one named vendor-card row), DME-2 (the surface
budget), DME-3 (the attribution verdict and its noise floor), DME-4 (the
convergence check).

Core Lean 4 only, no Mathlib (lake-manifest packages = []), like every pin in
this project; checked with the pinned toolchain (leanprover/lean4:v4.34.0).
No sorry, no native_decide. The negative controls
(dme_gates_negative_control.lean) must FAIL to check: decide refutes every
closed false claim there.

STATED ASSUMPTIONS (the modeling boundary):
1. These definitions pin the GATES' DECISION RULES as specified in
   .agents/skills/direct-model-edit/SKILL.md and carried by
   kask/crates/dme-harness/gates/dme-0.lisp and dme-3.lisp — not the
   lisp_eval binary, not dme-measure.jq, and not the input checks those
   forms run first. The deployed forms are checked separately by their
   declared cases (cargo test -p hkask-lisp --test dme_gates). No behavioral
   claim about any model is made or needed here.
2. DME-0 compares nine recorded settings, indexed 0-8 in the form's order
   (enable_thinking, temperature, top_p, top_k, min_p, presence_penalty,
   repetition_penalty, cap, penalty_last_n). The run states every one; the
   card side states a value or is silent (Option). Indices 1-6 are the
   sampling settings, whose card values the measuring module reads from the
   quoted card text. Agreement is an abstract boolean relation: the form's
   equality on the mode flag and its 1e-6 relative tolerance on numbers are
   instances. Nothing is assumed of it — in particular not transitivity,
   which a tolerance does not have.
3. DME-2's 0.1% cap is pinned as exact natural arithmetic (edited * 1000
   <= total, the cross-multiplied form of edited <= total * 0.001). The
   deployed lisp form computes the budget by float multiplication, which
   agrees with this predicate on integer surfaces at the deployed
   magnitudes (752,393,024 -> 752,393.024).
4. DME-3's rule is modeled over three signs (an established loss, no
   established change, an established gain) for the net change, the change
   among answers that finished before the edit, and the change in
   truncation. "Established" is modeled as est: an abstract significance
   test sig AND a net change above 1.0 point of all answers. The pin proves
   the floor, the verdict table and the 1.0-point band on finished answers
   for every sig; it says nothing about the
   sign test's arithmetic, which the Rust test checks against exact
   integers.
5. DME-4's decider models the lisp form's arithmetic exactly — the length
   >= 3 guard, then equality of the lookups at xs[len-1], xs[len-2],
   xs[len-3] — with the lookup equality carried by optEq.
-/

namespace DmeGates

-- ── DME-0: run settings against one named vendor-card row ────────────────────

variable {V : Type}

/-- The nine settings the DME-0 form compares, in the form's order. -/
def fields : List Nat := [0, 1, 2, 3, 4, 5, 6, 7, 8]

/-- A setting has moved when the card row states a value and the run's
value does not agree with it. A setting the card is silent on never moves. -/
def movedAt (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) (i : Nat) : Bool :=
  match card i with
  | none => false
  | some c => !agree (run i) c

/-- The form's third result: the moved settings, in field order. -/
def moved (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) : List Nat :=
  fields.filter (movedAt agree run card)

/-- The form's fourth result: the settings the card row does not state. -/
def silent (card : Nat → Option V) : List Nat :=
  fields.filter (fun i => (card i).isNone)

/-- The six sampling settings, whose card values the measuring module reads
from the quoted card text (indices 1-6). -/
def sampling : List Nat := [1, 2, 3, 4, 5, 6]

/-- The sampling settings the quoted card text does not state. -/
def samplingSilent (card : Nat → Option V) : List Nat :=
  sampling.filter (fun i => (card i).isNone)

/-- The DME-0 verdict (`card-quote-states-no-sampling-setting`,
`settings-match-card-row`, `settings-match-on-stated-settings-only`,
`settings-differ-from-card-row`). -/
inductive SettingsVerdict where
  | statesNoSampling : SettingsVerdict
  | matchesRow : SettingsVerdict
  | matchesStatedOnly : SettingsVerdict
  | differs : List Nat → SettingsVerdict

def dme0Verdict (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) :
    SettingsVerdict :=
  if samplingSilent card = sampling then .statesNoSampling
  else
    match moved agree run card with
    | [] => if samplingSilent card = [] then .matchesRow else .matchesStatedOnly
    | m => .differs m

theorem mem_fields {i : Nat} (h : i < 9) : i ∈ fields := by
  have hd : i = 0 ∨ i = 1 ∨ i = 2 ∨ i = 3 ∨ i = 4 ∨ i = 5 ∨ i = 6 ∨ i = 7 ∨ i = 8 := by omega
  rcases hd with rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl <;> decide

theorem fields_lt {i : Nat} (h : i ∈ fields) : i < 9 := by
  simp [fields] at h
  omega

theorem movedAt_iff (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) (i : Nat) :
    movedAt agree run card i = true ↔ ∃ c, card i = some c ∧ agree (run i) c = false := by
  unfold movedAt
  cases card i with
  | none => simp
  | some c => simp

/-- The moved list names exactly the settings the card row states and the
run does not agree with — no more, no less. -/
theorem mem_moved_iff (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) {i : Nat} :
    i ∈ moved agree run card ↔ i < 9 ∧ ∃ c, card i = some c ∧ agree (run i) c = false := by
  simp only [moved, List.mem_filter]
  constructor
  · rintro ⟨hf, hm⟩
    exact ⟨fields_lt hf, (movedAt_iff agree run card i).mp hm⟩
  · rintro ⟨hlt, hm⟩
    exact ⟨mem_fields hlt, (movedAt_iff agree run card i).mpr hm⟩

/-- The silent list names exactly the settings the card row does not state. -/
theorem mem_silent_iff (card : Nat → Option V) {i : Nat} :
    i ∈ silent card ↔ i < 9 ∧ card i = none := by
  simp only [silent, List.mem_filter]
  constructor
  · rintro ⟨hf, hs⟩
    exact ⟨fields_lt hf, Option.isNone_iff_eq_none.mp hs⟩
  · rintro ⟨hlt, hn⟩
    exact ⟨mem_fields hlt, Option.isNone_iff_eq_none.mpr hn⟩

/-- Both lists keep the form's field order. -/
theorem moved_sublist (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) :
    (moved agree run card).Sublist fields :=
  List.filter_sublist

theorem silent_sublist (card : Nat → Option V) : (silent card).Sublist fields :=
  List.filter_sublist

/-- A setting the vendor does not state is never reported as moved (the
case the six-field gate crashed on). -/
theorem moved_not_silent (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V)
    {i : Nat} (hm : i ∈ moved agree run card) : i ∉ silent card := by
  intro hs
  obtain ⟨_, c, hc, _⟩ := (mem_moved_iff agree run card).mp hm
  have hn := ((mem_silent_iff card).mp hs).2
  rw [hn] at hc
  cases hc

/-- Nothing moved exactly when the run agrees with every setting the card
row states. -/
theorem moved_nil_iff (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) :
    moved agree run card = [] ↔
      ∀ i, i < 9 → ∀ c, card i = some c → agree (run i) c = true := by
  constructor
  · intro h i hlt c hc
    cases hb : agree (run i) c with
    | true => rfl
    | false =>
      have hm : i ∈ moved agree run card := (mem_moved_iff agree run card).mpr ⟨hlt, c, hc, hb⟩
      rw [h] at hm
      cases hm
  · intro h
    cases hl : moved agree run card with
    | nil => rfl
    | cons d ds =>
      have hm : d ∈ moved agree run card := by
        rw [hl]
        exact List.mem_cons_self
      obtain ⟨hlt, c, hc, hb⟩ := (mem_moved_iff agree run card).mp hm
      rw [h d hlt c hc] at hb
      cases hb

/-- A full match needs every sampling setting stated by the quote and none
moved: a quote that states only some settings can never return it. -/
theorem dme0_matchesRow_iff (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V) :
    dme0Verdict agree run card = SettingsVerdict.matchesRow ↔
      samplingSilent card = [] ∧ moved agree run card = [] := by
  unfold dme0Verdict
  by_cases h1 : samplingSilent card = sampling
  · simp [h1, sampling]
  · cases hm : moved agree run card with
    | nil =>
      by_cases h2 : samplingSilent card = []
      · simp [h2, sampling]
      · simp [h1, h2]
    | cons d ds => simp [h1]

/-- A quote that states no sampling setting is refused, whatever the run. -/
theorem dme0_statesNoSampling_iff (agree : V → V → Bool) (run : Nat → V)
    (card : Nat → Option V) :
    dme0Verdict agree run card = SettingsVerdict.statesNoSampling ↔
      samplingSilent card = sampling := by
  unfold dme0Verdict
  by_cases h1 : samplingSilent card = sampling
  · simp [h1]
  · cases hm : moved agree run card with
    | nil =>
      by_cases h2 : samplingSilent card = []
      · simp [h2, sampling]
      · simp [h1, h2]
    | cons d ds => simp [h1]

/-- No false alarm: a `differs` verdict carries the moved list, and that
list is nonempty. -/
theorem dme0_differs_is_moved (agree : V → V → Bool) (run : Nat → V) (card : Nat → Option V)
    {m : List Nat} (h : dme0Verdict agree run card = SettingsVerdict.differs m) :
    m = moved agree run card ∧ m ≠ [] := by
  unfold dme0Verdict at h
  by_cases h1 : samplingSilent card = sampling
  · simp [h1] at h
  · cases hl : moved agree run card with
    | nil =>
      by_cases h2 : samplingSilent card = []
      · simp [h2, hl, sampling] at h
      · simp [h1, h2, hl] at h
    | cons d ds =>
      simp [h1, hl] at h
      refine ⟨h.symm, ?_⟩
      rw [← h]
      simp

-- ── DME-2: the 0.1% surface budget ────────────────────────────────────────────

/-- The DME-2 surface predicate in exact natural arithmetic: the
cross-multiplied form of `edited ≤ total * 0.001` (the 0.1% cap). -/
def withinBudget (total edited : Nat) : Prop := edited * 1000 ≤ total

/-- The anchor regression case (SKILL.md, Regression case): 131,072 of the
measured 752,393,024 weights passes — kernel-checked arithmetic. -/
theorem anchor_withinBudget : withinBudget 752393024 131072 := by
  show 131072000 ≤ 752393024
  decide

/-- Shrinking the surface preserves passing (SKILL.md Step 4: "shrink the
surface ... or stop"). -/
theorem withinBudget_mono (t e1 e2 : Nat) (h2 : e2 ≤ e1) (h1 : withinBudget t e1) :
    withinBudget t e2 :=
  Nat.le_trans (Nat.mul_le_mul_right 1000 h2) h1

/-- The cap is a ratio property: uniform scaling of model and surface
preserves the verdict. -/
theorem withinBudget_scale (k t e : Nat) (hk : 0 < k) :
    withinBudget (k * t) (k * e) ↔ withinBudget t e := by
  simp only [withinBudget]
  constructor
  · intro h
    have h' : k * (e * 1000) ≤ k * t := by
      rw [← Nat.mul_assoc]
      exact h
    exact Nat.le_of_mul_le_mul_left h' hk
  · intro h
    have h' : k * (e * 1000) ≤ k * t := Nat.mul_le_mul_left k h
    rwa [Nat.mul_assoc]

-- ── DME-3: the attribution verdict and its noise floor ───────────────────────

/-- What a paired comparison establishes: a loss, nothing, or a gain. -/
inductive Sign where
  | down : Sign
  | flat : Sign
  | up : Sign
  deriving DecidableEq

/-- The forms' `est`: of `n` answers, `b` changed for the better and `c` for
the worse. A change is established when the significance test `sig` passes
AND the net change is more than 1.0 point of all answers
(`100 * |b - c| > n`, with `|b - c|` written in truncated subtraction). -/
def est (sig : Nat → Nat → Bool) (b c n : Nat) : Sign :=
  if sig b c = true ∧ n < 100 * ((b - c) + (c - b)) then
    (if c < b then Sign.up else Sign.down)
  else Sign.flat

/-- The noise floor: a net change within 1.0 point of all answers is never
established, whatever the significance test says. -/
theorem est_floor (sig : Nat → Nat → Bool) (b c n : Nat)
    (h : 100 * ((b - c) + (c - b)) ≤ n) : est sig b c n = Sign.flat := by
  unfold est
  split
  · rename_i hc
    exact absurd hc.2 (Nat.not_lt.mpr h)
  · rfl

/-- Predeclared case F13: 20 rescues and no regressions in 2,000 answers is
exactly 1.0 point — not established, for every significance test. -/
theorem est_floor_f13 (sig : Nat → Nat → Bool) : est sig 20 0 2000 = Sign.flat :=
  est_floor sig 20 0 2000 (by decide)

/-- An established gain has more answers improving than worsening. -/
theorem est_up_lt (sig : Nat → Nat → Bool) (b c n : Nat) (h : est sig b c n = Sign.up) :
    c < b := by
  unfold est at h
  split at h
  · split at h
    · assumption
    · cases h
  · cases h

/-- Nothing is established without the significance test. -/
theorem est_needs_sig (sig : Nat → Nat → Bool) (b c n : Nat) (h : sig b c = false) :
    est sig b c n = Sign.flat := by
  unfold est
  split
  · rename_i hc
    rw [h] at hc
    cases hc.1
  · rfl

/-- The DME-3 verdicts, in the form's order. -/
inductive Attribution where
  | netChangeNotEstablished : Attribution
  | netLossEstablished : Attribution
  | gainIncludesFinishedAnswers : Attribution
  | netGainSourceUnresolved : Attribution
  | unblockingWithLossOnFinishedAnswers : Attribution
  | mostlyUnblocking : Attribution
  | unblockingWithUnresolvedChangeOnFinishedAnswers : Attribution
  deriving DecidableEq

/-- The form's `cond`, clause for clause: `net` over all answers, `fin` over
answers that finished before the edit, `unb` over truncation, and `band`:
the finished-answer net change is within 1.0 point of all answers. -/
def attributeGain (net fin unb : Sign) (band : Bool) : Attribution :=
  match net, fin, unb, band with
  | .flat, _, _, _ => .netChangeNotEstablished
  | .down, _, _, _ => .netLossEstablished
  | .up, .up, _, _ => .gainIncludesFinishedAnswers
  | .up, .down, .up, _ => .unblockingWithLossOnFinishedAnswers
  | .up, .flat, .up, true => .mostlyUnblocking
  | .up, .flat, .up, false => .unblockingWithUnresolvedChangeOnFinishedAnswers
  | .up, _, _, _ => .netGainSourceUnresolved

/-- The whole rule over the four paired counts the gate reads: answers that
went truncated→correct (`hc`), finished-wrong→correct (`wc`),
correct→truncated (`ch`), correct→finished-wrong (`cw`), of `n` answers. -/
def judge (sig : Nat → Nat → Bool) (hc wc ch cw n : Nat) : Attribution :=
  attributeGain (est sig (hc + wc) (ch + cw) n) (est sig wc cw n) (est sig hc ch n)
    (decide (100 * ((wc - cw) + (cw - wc)) ≤ n))

/-- `mostly-unblocking` exactly when the net gain and the truncation gain are
established, the finished-answer change is not established, AND that change
is within the 1.0-point band. -/
theorem attribute_mostly_iff (net fin unb : Sign) (band : Bool) :
    attributeGain net fin unb band = Attribution.mostlyUnblocking ↔
      net = Sign.up ∧ unb = Sign.up ∧ fin = Sign.flat ∧ band = true := by
  cases net <;> cases fin <;> cases unb <;> cases band <;> decide

/-- Without an established net gain, no verdict that names a gain is
returned. -/
theorem attribute_no_gain (net fin unb : Sign) (band : Bool) (h : net ≠ Sign.up) :
    attributeGain net fin unb band = Attribution.netChangeNotEstablished ∨
      attributeGain net fin unb band = Attribution.netLossEstablished := by
  cases net <;> cases fin <;> cases unb <;> cases band <;> simp_all [attributeGain]

/-- An established loss among finished answers is never hidden under
`mostly-unblocking`. -/
theorem attribute_loss_visible (net unb : Sign) (band : Bool) :
    attributeGain net Sign.down unb band ≠ Attribution.mostlyUnblocking := by
  cases net <;> cases unb <;> cases band <;> decide

/-- A finished-answer change outside the band is never read as "no change":
60 gained against 42 lost of 500 answers is 3.6 points, so the verdict is
not `mostly-unblocking` for any significance test. -/
theorem judge_outside_band_not_mostly (sig : Nat → Nat → Bool) :
    judge sig 6 60 0 42 500 ≠ Attribution.mostlyUnblocking := by
  intro h
  have hb := ((attribute_mostly_iff _ _ _ _).mp h).2.2.2
  simp at hb

/-- The anchor's published counts — 576 truncated answers rescued, 3
finished-wrong answers rescued, 5 regressions, of 1,152 — are
`mostly-unblocking` for every significance test that accepts 579 against 5
and 576 against 0. The finished-answer change (3 against 5) is inside the
band, so no assumption about it is needed. -/
theorem anchor_mostlyUnblocking (sig : Nat → Nat → Bool)
    (h1 : sig 579 5 = true) (h2 : sig 576 0 = true) :
    judge sig 576 3 0 5 1152 = Attribution.mostlyUnblocking := by
  simp [judge, est, h1, h2, attributeGain]

-- ── DME-4: the convergence check ─────────────────────────────────────────────

/-- The form's equality on nth-lookups: `some` values compare by their
payload (the lisp `=` on the values nth returns); anything else is not
equal. -/
def optEq : Option Nat → Option Nat → Bool
  | some a, some b => a == b
  | none, none => true
  | _, _ => false

/-- DME-4's convergence spec: the recorded open-findings counts end in
three equal values (the lisp form: converged when the last three counts
are equal). -/
def converged (xs : List Nat) : Prop := ∃ pre a, xs = pre ++ [a, a, a]

/-- The DME-4 decider — the lisp form's arithmetic exactly: the length >= 3
guard, then equality of the lookups at xs[len-1], xs[len-2], xs[len-3]
(the guard makes all three lookups `some`, which convergedDec_iff proves
is faithful). -/
def convergedDec (xs : List Nat) : Bool :=
  decide (3 ≤ xs.length)
  && optEq xs[xs.length - 1]? xs[xs.length - 2]?
  && optEq xs[xs.length - 2]? xs[xs.length - 3]?

/-- Convergence requires three counts (SKILL.md, Regression case: "two
counts can never satisfy the form"). -/
theorem converged_length {xs : List Nat} (h : converged xs) : 3 ≤ xs.length := by
  obtain ⟨pre, a, hpa⟩ := h
  rw [hpa, List.length_append]
  have h3 : ([a, a, a] : List Nat).length = 3 := rfl
  rw [h3]
  omega

/-- The regression instance: [12, 12] never converges. -/
theorem not_converged_two : ¬converged [12, 12] := by
  intro h
  have hh := converged_length h
  have h2 : ([12, 12] : List Nat).length = 2 := rfl
  omega

/-- The form's index arithmetic shifts with a cons: with three or more
counts below it, the last three lookups of `x :: R` are the last three
lookups of `R`. -/
theorem get?_cons_shift (x : Nat) (R : List Nat) (j : Nat) (hj : j + 1 ≤ R.length) :
    (x :: R)[R.length - j]? = R[R.length - 1 - j]? := by
  obtain ⟨m, hm⟩ : ∃ m, R.length - j = m + 1 := ⟨R.length - j - 1, by omega⟩
  have hm2 : R.length - 1 - j = m := by omega
  rw [hm, hm2]
  simp

/-- With three or more counts under it, a cons does not change the verdict —
the form's indices track the tail. -/
theorem convergedDec_cons (x : Nat) (R : List Nat) (hR : 3 ≤ R.length) :
    convergedDec (x :: R) = convergedDec R := by
  have h1 : (x :: R)[R.length]? = R[R.length - 1]? :=
    get?_cons_shift x R 0 (by omega)
  have h2 : (x :: R)[R.length - 1]? = R[R.length - 2]? :=
    get?_cons_shift x R 1 (by omega)
  have h3 : (x :: R)[R.length - 2]? = R[R.length - 3]? :=
    get?_cons_shift x R 2 (by omega)
  have ha : R.length + 1 - 1 = R.length := by omega
  have hb : R.length + 1 - 2 = R.length - 1 := by omega
  have hc : R.length + 1 - 3 = R.length - 2 := by omega
  simp only [convergedDec, List.length_cons, ha, hb, hc]
  rw [h1, h2, h3]
  have hd1 : decide (3 ≤ R.length + 1) = true := decide_eq_true (by omega)
  have hd2 : decide (3 ≤ R.length) = true := decide_eq_true (by omega)
  rw [hd1, hd2]

/-- The main correctness theorem: the lisp form's index arithmetic decides
the spec exactly — `convergedDec xs = true` iff the last three counts are
equal. -/
theorem convergedDec_iff (xs : List Nat) : convergedDec xs = true ↔ converged xs := by
  induction xs with
  | nil =>
    constructor
    · intro h
      simp [convergedDec] at h
    · intro h
      exact absurd (converged_length h) (by have hl : ([] : List Nat).length = 0 := rfl; omega)
  | cons x rest ih =>
    cases rest with
    | nil =>
      constructor
      · intro h
        simp [convergedDec] at h
      · intro h
        exact absurd (converged_length h)
          (by have hl : ([x] : List Nat).length = 1 := rfl; omega)
    | cons y rest2 =>
      cases rest2 with
      | nil =>
        constructor
        · intro h
          simp [convergedDec] at h
        · intro h
          exact absurd (converged_length h)
            (by have hl : ([x, y] : List Nat).length = 2 := rfl; omega)
      | cons z rest3 =>
        by_cases h3 : rest3 = []
        · subst h3
          -- the three-count case: the form's comparisons at [x, y, z]
          have hlen : ([x, y, z] : List Nat).length = 3 := rfl
          constructor
          · intro hd
            simp [convergedDec, hlen, optEq, beq_iff_eq] at hd
            obtain ⟨hzy, hyx⟩ := hd
            exact ⟨[], y, by simp [List.nil_append, hzy, ← hyx]⟩
          · rintro ⟨pre, a, hpa⟩
            cases pre with
            | nil =>
              -- [x, y, z] = [a, a, a]
              injection hpa with hx ht
              injection ht with hy ht2
              injection ht2 with hz _
              have hd : convergedDec [x, y, z] = true := by
                simp [convergedDec, hlen, optEq, beq_iff_eq]
                omega
              exact hd
            | cons p ps =>
              -- length contradiction: 3 = 1 + (ps.length + 3)
              have hl : ([x, y, z] : List Nat).length
                  = ((p :: ps) ++ [a, a, a]).length := by rw [hpa]
              simp only [List.length_cons, List.length_nil, List.length_append] at hl
              exact absurd hl (by omega)
        · obtain ⟨w, rest4, rfl⟩ : ∃ w rest4, rest3 = w :: rest4 := by
            cases rest3 with
            | nil => exact absurd rfl h3
            | cons w rest4 => exact ⟨w, rest4, rfl⟩
          -- rest has length >= 3: the cons shifts onto the tail
          rw [convergedDec_cons x (y :: z :: w :: rest4)
            (by simp only [List.length_cons]; omega)]
          constructor
          · intro h
            obtain ⟨pre, a, hpa⟩ := ih.mp h
            exact ⟨x :: pre, a, by simp [List.cons_append, hpa]⟩
          · rintro ⟨pre, a, hpa⟩
            cases pre with
            | nil =>
              -- x :: R = [a, a, a] forces R = [a, a]: length contradiction
              have h5 : (x :: y :: z :: w :: rest4).length
                  = ([a, a, a] : List Nat).length := by rw [hpa, List.nil_append]
              simp only [List.length_cons, List.length_nil] at h5
              exact absurd h5 (by omega)
            | cons p ps =>
              -- x :: R = (p :: ps) ++ [a, a, a]: peel the head
              simp only [List.cons_append, List.cons.injEq] at hpa
              obtain ⟨_, hR⟩ := hpa
              exact ih.mpr ⟨ps, a, hR⟩

/-- Once three equal counts are recorded, appending the same count again
keeps convergence (SKILL.md Step 7 — the converged state is stable under
repetition of the converged value). -/
theorem converged_stable (pre : List Nat) (a : Nat) :
    converged ((pre ++ [a, a, a]) ++ [a]) :=
  ⟨pre ++ [a], a, by simp [List.append_assoc]⟩

/-- A changed count breaks convergence (SKILL.md Step 7: the remaining
instability is reported, not iterated). -/
theorem converged_break (pre : List Nat) (a b : Nat) (h : a ≠ b) :
    ¬converged ((pre ++ [a, a, a]) ++ [b]) := by
  have key : ∀ (l : List Nat), convergedDec (l ++ [a, a, a, b]) = true → False := by
    intro l
    induction l with
    | nil =>
      intro hd
      have hlen : ([a, a, a, b] : List Nat).length = 4 := rfl
      simp [convergedDec, hlen, optEq, beq_iff_eq] at hd
      have hba : b = a := by omega
      exact h hba.symm
    | cons p l' ih =>
      intro hd
      have hd' : convergedDec (l' ++ [a, a, a, b]) = true := by
        rw [← convergedDec_cons p (l' ++ [a, a, a, b])
          (by simp only [List.length_append, List.length_cons, List.length_nil]; omega)]
        exact hd
      exact ih hd'
  intro hc
  have hd : convergedDec ((pre ++ [a, a, a]) ++ [b]) = true :=
    (convergedDec_iff _).mpr hc
  have hxs : (pre ++ [a, a, a]) ++ [b] = pre ++ [a, a, a, b] := by
    simp [List.append_assoc]
  rw [hxs] at hd
  exact key pre hd

end DmeGates
