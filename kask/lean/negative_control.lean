/- Negative control for the truncation-guard spec pin: this file must FAIL
to check (`decide` cannot prove a false equation). A checker that accepts it
is not an oracle. -/
example : 1 = 2 := by decide
