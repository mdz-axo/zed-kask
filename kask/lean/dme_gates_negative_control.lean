/- Negative control for the DME-gates pin: this file must FAIL to check. It
is the closed false claim family of the pin — one per gate. A checker that
accepts this file is not an oracle. `decide` must disprove each:
  - DME-2: the first surface over the anchor's cap passes (800,001 exceeds
    the 800,000 budget of an 800,000,000-weight model);
  - DME-4: two counts satisfy the length guard (SKILL.md's regression case:
    two counts can never satisfy the form — [12, 12] never converges);
  - DME-0: two distinct config fields are the same field (indices 0 and 1). -/
example : 800001 ≤ 800000 := by decide

example : 3 ≤ ([12, 12] : List Nat).length := by decide

example : (0 : Nat) = 1 := by decide
