# hkask-services-onboarding — Onboarding Service

New-user and new-agent onboarding: secrets generation, Matrix registration, initial agent setup, and first-run configuration wizardry.

**Version:** v0.31.0 | **Crate:** `hkask-services-onboarding`

## Modules

| Module | Purpose |
|--------|---------|
| `onboarding_impl` | `OnboardingService` — secrets bootstrap, Matrix account creation, agent init |

## Key Types

- `OnboardingService` — primary service interface
- `OnboardingConfig` — first-run configuration defaults

## Dependencies

- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-keystore` — credential generation and storage
- `hkask-communication` — Matrix homeserver registration
- `hkask-regulation` — Regulation span emission for onboarding events
