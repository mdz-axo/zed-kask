/- Negative control for the embed-to-adapter pin (research run
4d38e9250a4edaa4): this file must FAIL to check. It is the zero-input
contradiction that `no_linear_edit_is_constant` derives, stated as a
closed false claim: a constant offset c = 1 on top of a linear edit
forces `0 = 1` at the zero input (x = 0, since any linear edit maps 0 to
0). `decide` must disprove it — a checker that accepts this file is not
an oracle. -/
example : (0 : Nat) = 0 + 1 := by
  decide
