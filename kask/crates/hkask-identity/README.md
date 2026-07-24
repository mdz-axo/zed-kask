# hkask-identity — Human Identity & Authentication Types

Human users, userpod identities, sessions, invites, and registration — the Access Guard boundary (Cybernetics subloop 6.1: who can access what). Extracted from `hkask-services-core` (see `tasks/plan-core-scope-contraction.md`, Task 2.2).

**Version:** v0.31.0 | **Crate:** `hkask-identity`

## Exports

| Type | Purpose |
|------|---------|
| `HumanUser` | Human user — owns contact info (email/phone for recovery); `new` |
| `UserPodIdentity` | In-system persona users log in as; `derive_webid`, `new` |
| `UserSession` | Active session; `is_expired` |
| `Invite` / `InviteStatus` | Multi-user invitation record + status enum (Pending/Accepted/Revoked/Expired); `Display`, `FromStr` |
| `RegistrationRequest` | New-userpod registration payload |
| `RegistrationError` | Registration validation errors (`thiserror`) |
| `Role` / `OAuthProvider` | Re-exported from `hkask_types::identity` (orphan rule) |

## Dependencies

- `hkask-types` — `WebID`, `UserID`, `WalletId`, `Role`, `OAuthProvider`
- `serde` — (de)serialization
- `chrono` — `Utc::now()` timestamps (stored as Unix `i64`)
- `thiserror` — `RegistrationError`
- No coupling back to `hkask-services-core`; `WalletId` comes from `hkask_types::id` (not `hkask-wallet-types`)