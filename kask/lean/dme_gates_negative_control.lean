/- Negative control for the DME-gates pin: this file must FAIL to check. Each
claim is false and is stated over the pin's OWN definitions (inlined below
by the check script's convention: this file is checked alone, so the four
definitions it needs are repeated verbatim from dme_gates.lean; the Rust
test `the_negative_control_repeats_the_pin_verbatim` keeps them equal).
`decide` must disprove each:
  - DME-2: the first surface over the cap passes (752,394 of 752,393,024);
  - DME-3: a change of exactly 1.0 point is established (20 of 2,000);
  - DME-3: 60 gained against 42 lost among finished answers reads as
    mostly-unblocking;
  - DME-0: the nine settings are eight. -/

def withinBudget (total edited : Nat) : Prop := edited * 1000 ≤ total

inductive Sign where
  | down : Sign
  | flat : Sign
  | up : Sign
  deriving DecidableEq

def est (sig : Nat → Nat → Bool) (b c n : Nat) : Sign :=
  if sig b c = true ∧ n < 100 * ((b - c) + (c - b)) then
    (if c < b then Sign.up else Sign.down)
  else Sign.flat

def fields : List Nat := [0, 1, 2, 3, 4, 5, 6, 7, 8]

example : 752394000 ≤ 752393024 := by decide

example : est (fun _ _ => true) 20 0 2000 = Sign.up := by decide

example : decide (100 * ((60 - 42) + (42 - 60)) ≤ 500) = true := by decide

example : fields.length = 8 := by decide
