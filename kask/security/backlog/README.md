# Security backlog

Entries here are **not** CI-enforced regressions. This directory holds
security work that is tracked but not yet enforceable:

- `status: proposed` items whose mitigation is an unbuilt feature (a
  regression gate cannot test for a mechanism that does not exist yet).
- Accepted risks recorded for visibility.

When the mechanism lands, write the regression with a real detection
(cargo-test preferred; grep only when the pattern is the *vulnerable* idiom,
not the *presence of the type*), give it the next RR number, and place it in
`../regressions/` with `status: enforced`.
