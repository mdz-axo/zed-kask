# Minimalist Refactor — Todo

## Slice 1 — `EnergyEstimator` trait deletion test
- [x] Done. Verdict: **remove**. 92+10 tests green.

## Slice 2 — `EscalationPort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 3 — `LedgerStoragePort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 4 — `EmbeddingPort` trait deletion test
- [x] Done. Verdict: **remove**. 72+79 tests green. Zero dyn-consumers.

## Slice 5 — `WalletBudgetPort` + `WalletBackedBudget` dead path deletion test
- [x] Done. Verdict: **remove**. 91 tests green (1 test removed with deleted production code).
  `register_wallet_budget` had zero call sites; entire `WalletBackedBudget` →
  `wallet_budgets` map → sensor fallback chain was dead.

## Slice 6 — `SkillReader` trait deletion test
- [x] Done. Verdict: **remove**. 130 tests green. Single impl, no test mock
  despite doc claim.

## Slice 7 — `RuntimePolicy` trait deletion test
- [x] Done. Verdict: **remove**. 91+130 tests green. Consumer already depended
  on impl crate directly.

## Final report
- [x] `tasks/final-report.md` written with before/after code graph, edge delta,
      deletion-test verdicts, and suggested .rules additions.