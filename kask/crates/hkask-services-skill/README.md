# hkask-services-skill — Skill Service

Skill discovery, publishing, hashing, auditing, and bundle composition. Powers the skill registry — the canonical source of truth for all agent capabilities.

**Version:** v0.31.0 | **Crate:** `hkask-services-skill`

## Modules

| Module | Purpose |
|--------|---------|
| `skill_impl` | Skill discovery (`discover_skills`), publishing (`publish_skill`), visibility/namespace parsing, BLAKE3 hashing, userpod name resolution |
| `audit` | `SkillAuditor` — registry health scoring, staleness detection, template validation, deprecation recommendations |
| `bundle` | `BundleService` — LLM-native skill composition into `BundleManifest`, polarity classification, conflict detection, cascade ordering |

## Key Types

- `SkillInfo` — discovered skill metadata (path, name, visibility, namespace, content hash)
- `SkillPublishResult` — result of publishing a skill from private to public zone
- `SkillAuditor` / `SkillAuditReport` / `SkillHealthScore` — registry health audit pipeline
- `SkillStatus` — Active / StaleWarning / Critical / RecommendDeprecation
- `BundleService` / `BundleComposeResult` — skill bundle composition and evolution
- `TemplateSummary` — counts of WordAct / KnowAct / FlowDef templates per skill

## Key Functions

- `discover_skills(zone_dir)` — scan a zone directory for skills
- `publish_skill(root, name)` — publish a skill from private to public zone
- `find_public_skill(root, name)` — locate a namespaced skill in the public zone
- `compute_file_hash(path)` — BLAKE3 hash of a file
- `read_skill_visibility(path)` / `read_skill_namespace(path)` — parse SKILL.md front matter
- `resolve_userpod_name()` — resolve the userpod name for namespacing

## Dependencies

- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-services-context` — `AgentService` context
- `hkask-templates` — template registry, `SkillLoader`, `BundleManifest`
- `hkask-types` — `Visibility`, Regulation span types
- `hkask-ports` — `Skill`, `SkillZone`, `InferencePort`, `SkillRegistryIndex`
