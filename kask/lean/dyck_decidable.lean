/- Deterministic-pole exemplar for
   kask/docs/research/syntax-semantic-probabilistic-deterministic-space.md:
   decidable membership for a formal language (the Dyck language of balanced
   parentheses over a two-symbol alphabet). The Bool-valued membership
   predicate is a decision procedure by construction; the theorems pin
   positive and negative instances, including the prefix-dip trap that a
   total-sum-only check misses. Core Lean 4 only (no Mathlib). Checked with
   the project's pinned Lake toolchain via lean_check. -/

inductive Paren where
  | open : Paren
  | close : Paren

/-- Walk the word with a running depth; reject when depth would go negative. -/
def walk : List Paren → Int → Bool
  | [], d => decide (d = 0)
  | Paren.open :: t, d => walk t (d + 1)
  | Paren.close :: t, d => if d ≤ 0 then false else walk t (d - 1)

/-- Membership in the Dyck language: balanced parentheses. -/
def dyck (w : List Paren) : Bool := walk w 0

theorem dyck_empty : dyck [] = true := by decide

theorem dyck_balanced : dyck [Paren.open, Paren.close] = true := by decide

/-- Prefix-dip trap: total depth is zero but the word dips below zero. -/
theorem dyck_prefix_dip :
    dyck [Paren.open, Paren.close, Paren.close, Paren.open] = false := by decide

/-- A close with nothing open is rejected. -/
theorem dyck_close_first : dyck [Paren.close, Paren.open] = false := by decide
