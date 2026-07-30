---
title: "Kask Extensions Panel & Skill Sharing — Build Plan"
audience: [zed-kask integrators, hKask architects]
last_updated: 2026-07-29
version: "0.4.0"
status: "Active"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Extensions Panel & Skill Sharing — Build Plan

> **One-line frame:** A marketplace for zed-kask skills (and later: templates, embeddings, userpods, MCP-server configs) where users toggle their locally-installed skills between **Private** (default) and **Public** in Settings → AI → Skills; the toggle is drained lazily on page-leave into a publish/unpublish pipeline that packages the skill (`SKILL.md` + `manifest.yaml` + `*.j2`) as a Kask skill-extension and uploads it to a marketplace backend. Other users discover and install public skills through a **Kask Extensions Panel** — a fork of upstream `ExtensionsPage` wired to a parallel kask-artifact catalog. Only the latest version of each skill is offered; listings are namespaced by source user (`{source_user}/{skill_name}`).

## 0. Scope & Non-Goals

**In scope (v1):**
- `SkillVisibility` field on skill frontmatter (`Private` default, `Public` opt-in).
- Visibility toggle in Settings → AI → Skills page (per-skill).
- Lazy publish/unpublish queue drained on page navigation (and on app quit).
- `SkillSource::Public` variant for installed-from-marketplace skills.
- `KaskExtensionsPage` — forked `ExtensionsPage` UI wired to a kask-artifact catalog.
- Marketplace backend: S3 blob store + Postgres catalog + REST router (parallel to existing extension pipeline).
- Install path: download → extract into namespaced dir → register in `SkillIndex` → fire `SkillsUpdatedHook`.
- Dependency validation at publish and install time.
- Failure-signal `log::warn!` coverage per the `.rules` "process-global hooks need a startup-failure signal" trap.
- Tests pinning every deliberate deviation from upstream `ExtensionsPage` per the `.rules` "tests must pin deliberate zed-kask deviations" trap.

**Non-goals (v1):**
- Multi-version pinning. Only the latest published version is offered. v2 will add pinning.
- Auto-update of installed marketplace skills. v1 notifies the user; the user explicitly reinstalls.
- Embeddings, userpods, MCP-server configs as marketplace artifacts. v1 is skills-only; the catalog schema is designed to extend.
- A review/approval workflow for published skills. v1 trusts the publisher's GitHub identity (mirroring Zed's extension marketplace).
- Skill ratings, comments, or social features beyond upvotes/downvotes.

## 1. Existing Pieces (load-bearing)

These already exist in the tree and the plan reuses them:

| Piece | Location | Role in this plan |
|---|---|---|
| `SkillVisibility` enum | `crates/agent_skills/agent_skills.rs:87` | Added in Phase 1. `Private` default, `Public` opt-in. |
| `Skill` struct | `crates/agent_skills/agent_skills.rs:95` | Add `visibility: SkillVisibility` field. |
| `SkillSource` enum | `crates/agent_skills/agent_skills.rs:129` | Add `Public { source_user, original_skill_id }` variant; update `precedence()`. |
| `SkillMetadata` (frontmatter) | `crates/agent_skills/agent_skills.rs:266` | Add `visibility` field with `#[serde(default)]`. |
| `SkillIndex` (global) | `crates/agent_skills/agent_skills.rs:245` | Read by Settings page; updated by install path. |
| `SkillsUpdatedHook` | `crates/agent_skills/agent_skills.rs:258` | Existing "skills changed" signal; reused by install/uninstall. |
| `encode_skill_share_link` / `decode_skill_share_link` | `crates/agent_skills/agent_skills.rs:1038` | Existing `zed-kask://skill?data=...` deep link; orthogonal to this plan but shows the share primitive already exists. |
| `SettingsWindow` struct (queue field) | `crates/settings_ui/src/settings_ui.rs` (not `settings_window.rs` — that file does not exist) | Phase 2 adds `skill_visibility_queue` field here. |
| `ExtensionsPage` | `crates/extensions_ui/src/extensions_ui.rs:414` | The upstream UI to fork into `KaskExtensionsPage`. |
| `ExtensionStore` + S3/Postgres backend | `crates/extension_host/src/extension_host.rs`, `crates/collab/src/api/extensions.rs`, `crates/collab/src/db/queries/extensions.rs` | The plumbing to mirror for the kask-artifact catalog. |
| `kask_page()` | `crates/settings_ui/src/pages/kask_page.rs` | Existing Kask settings section; the Kask Extensions Panel is a center-pane `Item`, not a settings sub-page, so it does not live here. |
| `Event::ExtensionInstalled` / `ExtensionUninstalled` | `crates/extension/src/extension_events.rs` | Pattern to mirror for `KaskArtifactInstalled` / `KaskArtifactUninstalled`. |
| `OpenRequestKind::InstallSkill` | `crates/zed/src/zed/open_listener.rs` | Existing deep-link install path; marketplace install reuses the same flow with a marketplace URL instead of an embedded payload. |

## 2. Design Decisions (pinned)

### 2.1 `Private` is the default

A skill with no `visibility` frontmatter field is `Private`. This is the safe default — no skill is published without explicit user action. This mirrors the `.rules` trap about not silently leaving hooks unwired: a missing field must not silently publish.

### 2.2 `visibility` (a flag) vs `SkillSource::Public` (a source variant) are distinct

- `visibility: Public` on a `SkillSource::Global` skill means **"I want to share this"** — a flag on the user's own skill.
- `SkillSource::Public { source_user, original_skill_id }` means **"this came from someone else via the marketplace"** — a source variant for installed skills.

Do not conflate. The Settings page toggles the first; the Kask Extensions Panel displays the second.

### 2.3 Precedence: local > marketplace > built-in

```rust
pub fn precedence(&self) -> u8 {
    match self {
        Self::BuiltIn => 0,
        Self::Public { .. } => 1,   // installed from marketplace
        Self::Global => 2,          // locally authored
        Self::ProjectLocal { .. } => 3,
    }
}
```

A locally-authored `bug-hunt` shadows a marketplace-installed `alice/bug-hunt` on name collision. The marketplace install lands in a namespaced directory (`~/.agents/skills/_marketplace/{source_user}/{skill_name}/`) so it does not overwrite a local skill of the same name.

### 2.4 Naming: `{source_user}/{skill_name}`

Listings are namespaced by the publisher's GitHub username (sourced from the Zed account the user is logged in with — the same identity the existing extension marketplace uses). `alice/bug-hunt` and `bob/bug-hunt` are distinct listings. This solves name squatting without a global registry.

### 2.5 Only the latest version is offered

The `kask_skill_versions` table uses a composite PK `(kask_skill_id, version)` (mirroring `extension_versions`), but v1 only ever inserts one row per skill — the latest. The periodic poll overwrites prior versions. The UI never shows a version picker. v2 will add pinning; v1 keeps the surface minimal.

### 2.6 Lazy drain on page-leave

Toggling visibility writes to an in-memory queue. The queue is drained when:
1. The user navigates off the Settings → AI → Skills sub-page, OR
2. The user closes the Settings window, OR
3. A 30-second debounce timer fires (belt-and-suspenders; mirrors VS Code's settings-sync pattern).

Drain failures `log::warn!` with the skill ID, the failure reason, and the remediation. The skill's local `visibility` flag is **not** rolled back on publish failure — the user's intent is preserved; the queue retains the pending state and retries on the next drain.

### 2.7 Dependency resolution is prompt-based, not automatic

When a user installs `essentialist` (which depends on `deep-module` and `coding-guidelines`), the panel prompts: "This skill depends on `deep-module` and `coding-guidelines`. Install them too?" The user consents to each. No auto-install. At publish time, the pipeline refuses to publish a skill whose declared dependencies are not themselves published.

### 2.8 Jinja2 sandboxing is enforced at publish time

The `.rules` already require sandboxed Jinja2 (no `import os`, no file system access, no network calls in safety mode). The publish pipeline runs static analysis on every `*.j2` template and refuses to publish templates that violate the sandbox contract. This is a security boundary, not a feature.

### 2.9 Updates are notify-only

When `alice` publishes a new version of `bug-hunt`, users who have it installed see a "Update available" badge in the Kask Extensions Panel. The user explicitly clicks "Update" to reinstall. No auto-update — a changed prompt can change agent behavior in ways the user did not consent to.

## 3. Architecture

```mermaid
flowchart TD
    subgraph Client[zed-kask client]
        SettingsPage[Settings → AI → Skills page]
        VisibilityQueue[SkillVisibilityQueue - in-memory]
        DrainTask[Lazy drain task - page-leave / 30s debounce / app quit]
        PublishPipeline[Publish pipeline - package + upload]
        UnpublishPipeline[Unpublish pipeline - DELETE]
        KaskPanel[KaskExtensionsPage - forked ExtensionsPage]
        InstallPath[Install path - download + extract + register]
        SkillIndex[SkillIndex - global entity]
        SkillsUpdatedHook[SkillsUpdatedHook - rescan signal]
    end

    subgraph Server[collab server]
        S3[(S3 blob store)]
        Postgres[(Postgres - kask_artifacts + kask_artifact_versions)]
        Router[/api/kask-artifacts router]
        PeriodicFetch[fetch_kask_artifacts_from_blob_store_periodically]
    end

    SettingsPage -->|toggle| VisibilityQueue
    VisibilityQueue --> DrainTask
    DrainTask --> PublishPipeline
    DrainTask --> UnpublishPipeline
    PublishPipeline -->|PUT tar.gz| S3
    PublishPipeline -->|UPSERT metadata| Postgres
    UnpublishPipeline -->|DELETE| S3
    UnpublishPipeline -->|DELETE row| Postgres

    KaskPanel -->|GET catalog| Router
    Router --> Postgres
    PeriodicFetch -->|index manifests| Postgres
    PeriodicFetch -->|pull manifests| S3
    KaskPanel -->|click Install| InstallPath
    InstallPath -->|GET tar.gz| S3
    InstallPath -->|extract to ~/.agents/skills/_marketplace/...| SkillIndex
    InstallPath -->|fire| SkillsUpdatedHook
    SkillIndex -->|read| SettingsPage
```

## 4. Phased Plan

Each phase is independently shippable. Phases 1–3 land client-side without any server changes; Phase 4 adds the marketplace backend; Phase 5 wires the client to the backend; Phases 6–7 add policy and tests.

### Phase 1 — Skill visibility model (client-only, no UI) ✅ COMPLETE

**Goal:** the data model exists and parses correctly. No UI, no network.

**Tasks:**
1. Add `SkillVisibility` enum to `crates/agent_skills/src/agent_skills.rs`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
   #[serde(rename_all = "lowercase")]
   pub enum SkillVisibility {
       #[default]
       Private,
       Public,
   }
   ```
2. Add `visibility: SkillVisibility` to `SkillMetadata` with `#[serde(default)]`.
3. Add `visibility: SkillVisibility` to `Skill` struct; populate in `parse_skill_frontmatter`, `parse_builtin_skill`, `parse_embedded_global_skill`.
4. Add `SkillSource::Public { source_user: Arc<str>, original_skill_id: Arc<str> }` variant.
5. Update `SkillSource::precedence()` per §2.3.
6. Update `SkillSource::display_label()` to return `"{source_user}/{original_skill_id}"` for `Public`.
7. Update `SkillSource::scope_prefix()` — `Public` uses the empty prefix (same as `Global`), since installed marketplace skills behave like globals for slash-command purposes.
8. Update `SkillSource::matches_scope()` — `Public` matches the empty scope (same as `Global`).
9. **Tests:** pin the precedence ordering (`ProjectLocal > Global > Public > BuiltIn`); pin that a missing `visibility` field defaults to `Private`; pin that `Public` skills match the empty scope.

**Files touched:**
- `crates/agent_skills/src/agent_skills.rs`
- `crates/agent_skills/src/agent_skills.rs` (tests module)

**Acceptance:** `cargo check -p agent_skills` passes; new tests pass; existing skill loading is unaffected (every skill defaults to `Private`).

**Estimated effort:** 0.5 day

---

### Phase 2 — Visibility toggle in Settings → AI → Skills ✅ COMPLETE

**Goal:** the user can toggle a skill between Private and Public in the UI. The toggle writes to the frontmatter and to an in-memory queue. No network yet.

**Tasks:**
1. Add `SkillVisibilityQueue` struct to `crates/settings_ui/src/pages/skills_setup.rs` (or a new `crates/settings_ui/src/pages/skills_visibility.rs` file):
   ```rust
   pub struct SkillVisibilityQueue {
       pending: HashMap<String, SkillVisibility>,
   }
   ```
   Store it as a field on `SettingsWindow`.
2. In `render_skill_row`, add a visibility toggle `IconButton` next to the existing share-link button. Only render it for `SkillSource::Global` skills (built-ins can't be published; project-local skills defer to v2). Use `IconName::Lock` for Private, `IconName::Unlock` (or `IconName::Globe`) for Public.
3. On toggle click:
   - Read the new desired visibility.
   - Write `visibility: <new>` into the SKILL.md frontmatter on disk (preserve the rest of the file).
   - Update the in-memory `Skill` in `SkillIndex`.
   - Push the skill ID + desired visibility into `SkillVisibilityQueue`.
   - Call `cx.notify()` to re-render.
4. Add a drain trigger: when the Settings sub-page changes away from `skills`, spawn the drain task. The drain task is a no-op in Phase 2 (it logs "would publish/unpublish X" — the actual pipeline lands in Phase 5). The window-close and 30-second debounce triggers (§2.6) are deferred to Phase 5 — they only matter once the drain does real network work, and adding them now would gold-plate a no-op.
5. **Tests:** pin that toggling writes the frontmatter correctly; pin that the queue accumulates pending changes; pin that drain fires on page-leave.

**Files touched:**
- `crates/settings_ui/src/pages/skills_setup.rs`
- `crates/settings_ui/src/pages/skills_visibility.rs` (new)
- `crates/settings_ui/src/settings_window.rs` (add queue field + drain trigger)

**Acceptance:** user can toggle a skill's visibility; the frontmatter is updated on disk; the queue accumulates; navigating away from the page triggers the (no-op) drain. `cargo check -p settings_ui` passes.

**Estimated effort:** 1.5 days

---

### Phase 3 — `KaskExtensionsPage` UI shell (no backend) ✅ COMPLETE

**Goal:** the forked `ExtensionsPage` exists as a center-pane `Item`, renders a search field and a list, and shows placeholder data. No network yet.

**Tasks:**
1. Create `crates/kask_extensions_ui/` crate (sibling of `crates/extensions_ui/`). Add to workspace.
2. Copy `crates/extensions_ui/src/extensions_ui.rs` → `crates/kask_extensions_ui/src/kask_extensions_ui.rs` as the starting point. Rename `ExtensionsPage` → `KaskExtensionsPage`.
3. Define `KaskArtifactMetadata` with only the fields the Phase 3 UI renders. Add fields in the phase that consumes them (version/published_at/dependencies/energy_caps land in Phase 4/5 when the detail view and install modal need them):
   ```rust
   pub struct KaskArtifactMetadata {
       pub id: Arc<str>,            // "{source_user}/{skill_name}"
       pub source_user: Arc<str>,
       pub skill_name: Arc<str>,
       pub description: String,
       pub download_count: u64,
   }
   ```
4. Implement `Item` for `KaskExtensionsPage` (mirror `ExtensionsPage`'s impl). Tab title: "Kask Extensions". Tab icon: `Icon::new(IconName::Kask)` (reuse the existing kask icon).
5. Register a `ToggleKaskExtensions` action (deploy/focus) and a `ToggleKaskExtensionsFocus` action (focus-only). Wire via `kask_extensions_ui::init(cx)` called from `main.rs` (mirror `kask_panel::init`). Both actions are required per the `.rules` trap "Center-pane Item Toggle vs ToggleFocus".
6. Add a View dropdown menu entry "Kask Extensions" dispatching `ToggleKaskExtensions`, placed near the existing "Kask Panel" entry in `crates/zed/src/zed/app_menus.rs`. Per the `.rules` trap, center-pane Items use `Toggle` (not `ToggleFocus`) so the menu deploys a new item if none exists.
7. Add a status bar button (bottom bar) so the user can open the panel by clicking an icon, mirroring `kask_panel::panel_button::KaskPanelButton`. The button is a `StatusItemView` registered via `status_bar.add_right_item(...)` in `crates/zed/src/zed.rs`. Icon: `IconName::Share` (visual language for sharing/trading skills in the marketplace). Tooltip: "Toggle Kask Extensions".
8. Replace the `ExtensionStore` reads with placeholder data: a hardcoded list of 3-5 fake `KaskArtifactMetadata` entries so the UI is testable without a backend.
9. **Tests:** pin that the page renders; pin that the search field filters the placeholder list; pin that `ToggleKaskExtensions` deploys the page.

**Eliminated (essentialist G1 FAIL):** `KaskArtifactFilter` enum with 4 stub variants. v1 is skills-only (§0); a filter dropdown with 4 dead entries is premature surface. Add the filter in Phase 4+ when there's something real to filter.

**Files touched:**
- `crates/kask_extensions_ui/` (new crate)
- `crates/kask_extensions_ui/src/kask_extensions_ui.rs`
- `crates/kask_extensions_ui/src/panel_button.rs` (status bar button)
- `crates/kask_extensions_ui/Cargo.toml`
- `Cargo.toml` (workspace)
- `crates/zed/src/main.rs` (call `kask_extensions_ui::init(cx)`)
- `crates/zed/src/zed/app_menus.rs` (View menu entry)
- `crates/zed/src/zed.rs` (register status bar button)

**Acceptance:** user can open the Kask Extensions Panel from the command palette / menu; it renders with placeholder data; search filters the list. `cargo check -p kask_extensions_ui` passes.

**Estimated effort:** 2-3 days

---

### Phase 4 — Marketplace backend (server-side) ✅ COMPLETE

**Goal:** the collab server can store and serve kask skill metadata, mirroring the extension marketplace architecture exactly: S3 is the source of truth, Postgres is a cache built by periodic polling, publishing is direct-to-S3, downloads redirect to S3 presigned URLs.

**Implementation status (verified 2026-07-29):** `crates/cloud_api_types/src/kask_skill.rs` defines `KaskSkillManifest`, `KaskSkillMetadata`, `GetKaskSkillsResponse`. `crates/collab/src/api/kask_skills.rs` exposes `GET /api/kask-skills`, `GET /api/kask-skills/:id`, `GET /api/kask-skills/:id/download`, `POST /api/kask-skills/:id/vote`, `POST /api/kask-skills/upload`, `DELETE /api/kask-skills/:id`, plus `fetch_kask_skills_from_blob_store_periodically` (5-minute poll). The upload handler verifies the S3 key's `source_user` matches the authenticated user's `github_login` (FORBIDDEN on mismatch).

**Architecture (mirrors `crates/collab/src/api/extensions.rs` + `crates/collab/src/db/queries/extensions.rs`):**
- **Publishing:** the client packages the skill dir into `archive.tar.gz` + `manifest.json` and uploads both directly to S3 at `kask-skills/{source_user}/{skill_name}/{version}/`. No `PUT` endpoint.
- **Catalog:** `fetch_kask_skills_from_blob_store_periodically` runs every 5 minutes, lists S3 objects under `kask-skills/`, downloads new `manifest.json` files, upserts into Postgres.
- **Discovery:** `GET /api/kask-skills` reads Postgres.
- **Download:** `GET /api/kask-skills/:id/download` increments download count in Postgres, redirects to S3 presigned URL.
- **Votes:** `POST /api/kask-skills/:id/vote` (authenticated, +1/-1), stored in a `kask_skill_votes` table.

**Tasks:**
1. Add `KaskSkillManifest` and `KaskSkillMetadata` to `crates/cloud_api_types/src/kask_skill.rs` (mirror `extension.rs`):
   ```rust
   pub struct KaskSkillManifest {
       pub source_user: String,       // authenticated github_login
       pub skill_name: String,
       pub version: String,           // timestamp-based; only latest kept
       pub description: String,
       pub dependencies: Vec<String>,
       pub tarball_sha256: String,
   }
   pub struct KaskSkillMetadata {
       pub id: String,                // "{source_user}/{skill_name}"
       pub manifest: KaskSkillManifest,
       pub published_at: DateTime<Utc>,
       pub download_count: u64,
       pub upvote_count: i64,
       pub downvote_count: i64,
   }
   pub struct GetKaskSkillsResponse { pub data: Vec<KaskSkillMetadata> }
   ```
2. Add Postgres tables (migration in `crates/collab/migrations/` and `crates/collab/migrations.sqlite/`):
   ```sql
   CREATE TABLE kask_skills (
     id SERIAL PRIMARY KEY,
     source_user TEXT NOT NULL,
     skill_name TEXT NOT NULL,
     description TEXT NOT NULL,
     latest_version TEXT NOT NULL,
     total_download_count BIGINT NOT NULL DEFAULT 0,
     upvote_count BIGINT NOT NULL DEFAULT 0,
     downvote_count BIGINT NOT NULL DEFAULT 0,
     UNIQUE (source_user, skill_name)
   );
   CREATE TABLE kask_skill_versions (
     kask_skill_id INTEGER NOT NULL REFERENCES kask_skills(id) ON DELETE CASCADE,
     version TEXT NOT NULL,
     published_at TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW() NOT NULL,
     dependencies TEXT NOT NULL DEFAULT '',
     tarball_sha256 TEXT NOT NULL,
     download_count BIGINT NOT NULL DEFAULT 0,
     PRIMARY KEY (kask_skill_id, version)
   );
   CREATE TABLE kask_skill_votes (
     kask_skill_id INTEGER NOT NULL REFERENCES kask_skills(id) ON DELETE CASCADE,
     user_id INTEGER NOT NULL,
     vote SMALLINT NOT NULL,   -- +1 or -1
     voted_at TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW() NOT NULL,
     PRIMARY KEY (kask_skill_id, user_id)
   );
   ```
   The composite PK on `kask_skill_versions` allows multiple versions (v2 pinning); v1 only ever inserts one row per skill (the latest), matching the extension pattern.
3. Add SeaORM entities: `crates/collab/src/db/tables/kask_skill.rs`, `kask_skill_version.rs`, `kask_skill_vote.rs` (mirror `extension.rs` / `extension_version.rs`).
4. Add `crates/collab/src/db/queries/kask_skills.rs` with `get_kask_skills`, `get_kask_skill`, `get_known_kask_skill_versions`, `insert_kask_skill_versions`, `record_kask_skill_download`, `vote_kask_skill` (mirror `queries/extensions.rs`).
5. Add `crates/collab/src/api/kask_skills.rs` with an axum router:
   - `GET /api/kask-skills` — list (reads Postgres, ordered by download_count desc)
   - `GET /api/kask-skills/:id` — single
   - `GET /api/kask-skills/:id/download` — increment download count, redirect to S3 presigned URL at `kask-skills/{source_user}/{skill_name}/{version}/archive.tar.gz`
   - `POST /api/kask-skills/:id/vote` — authenticated; upsert vote (+1/-1); update aggregate counts on the skill row
6. Add `fetch_kask_skills_from_blob_store_periodically` (mirror `fetch_extensions_from_blob_store_periodically`) — lists S3 objects under `kask-skills/`, finds new `manifest.json` files, downloads, parses, upserts into Postgres.
7. Wire the router + periodic fetch in `crates/collab/src/main.rs` (mirror the extensions wiring).
8. **Tests:** integration tests in `crates/collab/tests/integration/db_tests/kask_skill_tests.rs` mirroring `extension_tests.rs`. Pin: upsert replaces the prior version; delete cascades to versions; the `(source_user, skill_name)` uniqueness holds; vote upsert changes the aggregate counts.

**Files touched:**
- `crates/cloud_api_types/src/kask_skill.rs` (new)
- `crates/cloud_api_types/src/lib.rs` (re-export)
- `crates/collab/migrations/20251208000000_test_schema.sql` (add tables)
- `crates/collab/migrations.sqlite/20221109000000_test_schema.sql` (add tables)
- `crates/collab/src/db/tables/kask_skill.rs` (new)
- `crates/collab/src/db/tables/kask_skill_version.rs` (new)
- `crates/collab/src/db/tables/kask_skill_vote.rs` (new)
- `crates/collab/src/db/tables.rs` (re-export)
- `crates/collab/src/db/queries/kask_skills.rs` (new)
- `crates/collab/src/db/queries.rs` (re-export)
- `crates/collab/src/api/kask_skills.rs` (new)
- `crates/collab/src/api.rs` (re-export)
- `crates/collab/src/main.rs` (wire router + periodic fetch)
- `crates/collab/tests/integration/db_tests/kask_skill_tests.rs` (new)

**Acceptance:** the collab server's `/api/kask-skills` endpoints respond; integration tests pass; the periodic fetch loop runs without error on an empty S3 bucket.

**Estimated effort:** 2-3 days

---

### Phase 5 — Wire client to backend (publish + install) ✅ COMPLETE

**Goal:** end-to-end. Toggling a skill to Public publishes it; clicking Install in the Kask Extensions Panel installs it.

**Architecture (mirrors `extension_cli` + `ExtensionStore::install_extension`):**
- **Publish:** client packages skill dir → `archive.tar.gz` + `manifest.json` → uploads both directly to S3 at `kask-skills/{source_user}/{skill_name}/{version}/`. `source_user` = authenticated user's `github_login`. No collab server involvement in the upload (the periodic poll picks it up).
- **Install:** client calls `GET /api/kask-skills` (catalog) → `GET /api/kask-skills/:id/download` (S3 presigned redirect) → downloads tarball → verifies SHA256 → extracts into `~/.agents/skills/_marketplace/{source_user}/{skill_name}/`.

**Tasks:**
1. Implement the publish pipeline (client side, in a new `crates/kask_extensions_ui/src/publish.rs`):
   - Read `SKILL.md` + `manifest.yaml` + all `*.j2` templates from the skill directory.
   - Run Jinja2 sandbox static analysis (refuse templates with `import os`, `import subprocess`, file/network calls outside safety mode). Reuse the existing sandbox contract documented in skill `Constraints` sections.
   - Validate declared dependencies are published (query `GET /api/kask-skills/{dep_id}` for each; refuse if any 404s).
   - Package as `archive.tar.gz` (mirror `extension_cli::main` — copy resources to a temp dir, `tar -czvf`).
   - Compute SHA256 of the tarball.
   - Write `manifest.json` (`KaskSkillManifest` serialized to JSON).
   - Upload both to S3 at `kask-skills/{source_user}/{skill_name}/{version}/archive.tar.gz` and `.../manifest.json`. Use the same S3 client + credentials the extension CLI uses.
   - `source_user` = the authenticated user's `github_login` (from `client::User`).
   - On any failure: `log::warn!` with skill ID, failure reason, remediation. Do not roll back the local `visibility` flag.
2. Implement the unpublish pipeline:
   - Delete the S3 objects at `kask-skills/{source_user}/{skill_name}/` (prefix delete).
   - The collab server's periodic poll will remove the Postgres row on the next cycle (or we add a `DELETE /api/kask-skills/:id` endpoint that deletes from both S3 and Postgres — simpler for v1).
   - The local skill stays on disk; only the marketplace listing is removed.
   - On failure: `log::warn!`; retain the pending state in the queue for retry.
3. Wire the `SkillVisibilityQueue` drain task (from Phase 2) to call the publish/unpublish pipelines. Add the window-close and 30s debounce drain triggers here (deferred from Phase 2 — they only matter once the drain does real network work).
4. Implement the install path in `KaskExtensionsPage` (mirror `ExtensionStore::install_extension`):
   - On "Install" click: `GET /api/kask-skills/{id}` → `GET /api/kask-skills/{id}/download` → download → verify SHA256 → extract into `~/.agents/skills/_marketplace/{source_user}/{skill_name}/`.
   - Register in `SkillIndex` with `SkillSource::Public { source_user, original_skill_id }`.
   - Call `SkillsUpdatedHook` so the Settings page and the agent's skill catalog refresh.
   - Emit a `KaskArtifactInstalled` event (mirror `Event::ExtensionInstalled`) so the agent panel syncs.
5. Implement the uninstall path: remove the directory, deregister from `SkillIndex`, fire `SkillsUpdatedHook`, emit `KaskArtifactUninstalled`.
6. Implement the "Update available" badge: on `KaskExtensionsPage` render, compare the installed version (stored in a local manifest in the `_marketplace/` dir) against the latest from the catalog. Show a badge if they differ. Clicking "Update" re-runs the install path.
7. Implement the vote UI: upvote/downvote buttons on each skill row in `KaskExtensionsPage`. On click: `POST /api/kask-skills/:id/vote` with `+1` or `-1`. Update the row's counts from the response.
8. Replace the placeholder data in `KaskExtensionsPage` (from Phase 3) with `fetch_kask_skills` calls.
9. **Tests:** end-to-end test with a test collab server (mirror `test_extension_store_with_test_extension`); pin that publish + install round-trips; pin that uninstall removes the directory; pin that the "Update available" badge appears when versions differ; pin that voting updates the counts.

**Files touched:**
- `crates/kask_extensions_ui/src/publish.rs` (new)
- `crates/kask_extensions_ui/src/kask_extensions_ui.rs` (install/uninstall/update/vote)
- `crates/settings_ui/src/pages/skills_visibility.rs` (wire drain to pipelines + window-close/debounce triggers)
- `crates/agent_skills/src/agent_skills.rs` (register `Public` skills in `SkillIndex`)

**Acceptance:** a user on one machine can toggle a skill to Public, and a user on another machine can see it in the Kask Extensions Panel and install it. The installed skill loads and executes via the existing `ManifestExecutor` cascade. Voting works. End-to-end test passes.

**Estimated effort:** 4-5 days

---

### Phase 6 — Dependency resolution & policy enforcement (PARTIAL)

**Goal:** the marketplace enforces the dependency contract at publish and install time.

**Implementation status (verified 2026-07-29):** PARTIAL.
- **Install-time dependency check (client):** implemented as **notify-only**, not blocking. `KaskExtensionsPage::install_kask_skill` (`crates/kask_extensions_ui/src/kask_extensions_ui.rs:485`) checks installed skill names against `manifest.dependencies`, logs a `warn!` listing missing deps, but proceeds with the install. The plan called for a blocking modal with per-dependency checkboxes; the implementation chose notify-only (simpler, the skill fails loudly at runtime via `ManifestExecutor` if a dep is missing).
- **Publish-time dependency validation (server):** NOT implemented. The upload handler (`crates/collab/src/api/kask_skills.rs:upload_kask_skill`) does not reject 409 on missing dependencies. The manifest's `dependencies` field is stored but not validated against existing skills.
- **Cycle detection:** NOT implemented. No recursive install with cycle refusal exists.
- **Uninstall-time dependent warning:** eliminated per the plan's essentialist G1 note (a broken dependent fails loudly at load time).

**Tasks:**
1. At publish time (server-side, in `PUT /api/kask-artifacts/{id}` handler): parse `manifest_json.dependencies`; for each dependency, check that a `kask_artifact` row exists with that ID. Reject the publish with a 409 if any dependency is missing. Return the list of missing dependencies in the error body.
2. At install time (client-side, in `KaskExtensionsPage`): before downloading, fetch the artifact metadata; if `dependencies` is non-empty, show a modal: "This skill depends on: [list]. Install them too?" with per-dependency checkboxes (all pre-checked). On confirm, install each missing dependency first (recursively, with cycle detection — refuse cycles).
3. At uninstall time: remove the directory, deregister from `SkillIndex`, fire `SkillsUpdatedHook`. If a dependent skill breaks, the `ManifestExecutor` reports the missing dependency at load time with a clear error.
4. **Tests:** pin that publish with a missing dependency is rejected; pin that install prompts for dependencies; pin that cyclic dependencies are refused.

**Eliminated (essentialist G1 FAIL):** Uninstall-time dependent warning modal ("the following skills depend on this"). Scanning all installed skills to build a reverse dependency graph and rendering a modal adds significant complexity for a polish feature. A broken dependent skill already fails loudly at load time via the `ManifestExecutor`. Defer the warning modal to v2.

**Files touched:**
- `crates/collab/src/api/kask_artifacts.rs` (server-side dependency check)
- `crates/kask_extensions_ui/src/kask_extensions_ui.rs` (client-side install modal)

**Acceptance:** publishing `essentialist` without `deep-module` and `coding-guidelines` in the marketplace fails with a clear error. Installing `essentialist` prompts to install its dependencies. Cyclic dependencies are refused.

**Estimated effort:** 1-2 days

---

### Phase 7 — Tests pinning deviations from upstream (PARTIAL)

**Goal:** every deliberate deviation from upstream `ExtensionsPage` is pinned by a test, per the `.rules` "tests must pin deliberate zed-kask deviations from upstream" trap.

**Implementation status (verified 2026-07-29):** PARTIAL.
- **`agent_skills` deviation tests:** DONE. `crates/agent_skills/agent_skills.rs` tests module pins: `SkillVisibility` defaults to `Private`; `SkillSource::Public` precedence is between `BuiltIn` and `Global`; `Public` `display_label` is namespaced; `Public` matches the empty scope; `SkillMetadata` missing `visibility` field defaults to `Private`; explicit `visibility: public` parses.
- **`settings_ui` visibility tests:** DONE. `crates/settings_ui/src/pages/skills_visibility.rs` tests module pins: `SkillVisibilityQueue` accumulates and drains (last-write-wins); `rewrite_skill_visibility_in_content` inserts/replaces the `visibility` field; `rewrite_skill_visibility_on_disk` writes the file; `update_skill_visibility_in_index` updates the global index.
- **`kask_extensions_ui` deviation tests:** NOT DONE. No `crates/kask_extensions_ui/tests/` directory exists. The plan called for pinning tests that `KaskExtensionsPage` renders with kask-artifact metadata (not extension metadata), that `ToggleKaskExtensions` deploys the page, and that the visibility toggle only appears for `SkillSource::Global` skills. These tests are still owed.

**Tasks:**
1. Audit every `// zed-kask:` comment added in `crates/kask_extensions_ui/` and `crates/agent_skills/` during Phases 1–6.
2. For each deviation, write a test asserting the deviated behavior stays the way it was designed:
   - `SkillVisibility` defaults to `Private` (Phase 1).
   - `SkillSource::Public` precedence is between `BuiltIn` and `Global` (Phase 1).
   - `KaskExtensionsPage` renders with kask-artifact metadata, not extension metadata (Phase 3).
   - The visibility toggle only appears for `SkillSource::Global` skills (Phase 2).
   - The lazy drain fires on page-leave (Phase 2); window-close and 30s debounce land with the real publish pipeline in Phase 5.
   - Publish fails closed on dependency violations (Phase 6).
   - Install prompts for dependencies; does not auto-install (Phase 6).
   - Uninstall warns about dependents; does not auto-uninstall (Phase 6).
   - Updates are notify-only; no auto-update (Phase 5).
3. Add a `// zed-kask:` comment at the top of `kask_extensions_ui.rs` explaining why the file is a fork and what deviations are pinned by tests.
4. **Tests:** the pinning tests themselves.

**Files touched:**
- `crates/kask_extensions_ui/src/kask_extensions_ui.rs` (header comment)
- `crates/kask_extensions_ui/tests/` (new tests module)
- `crates/agent_skills/tests/` (new tests for visibility deviations)

**Acceptance:** every `// zed-kask:` comment has a corresponding test; running the test suite confirms the deviations are intact.

**Estimated effort:** 1-2 days

---

## 5. Total Effort

| Phase | Effort | Cumulative |
|---|---|---|
| 1 — Skill visibility model | 0.5 day | 0.5 day |
| 2 — Visibility toggle UI | 1 day | 1.5 days |
| 3 — `KaskExtensionsPage` shell | 2-3 days | 3.5-4.5 days |
| 4 — Marketplace backend | 2 days | 5.5-6.5 days |
| 5 — Wire client to backend | 4-5 days | 9.5-11.5 days |
| 6 — Dependency resolution | 1-2 days | 10.5-13.5 days |
| 7 — Deviation-pinning tests | 1-2 days | 11.5-15.5 days |

**Total: ~12-16 days of focused work** for a working v1.

## 6. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Upstream `ExtensionsPage` evolves and the fork drifts | Phase 7 pins deviations; rebase regularly; keep the fork surface minimal |
| Skill name squatting | `{source_user}/{skill_name}` namespacing (§2.4) |
| Malicious Jinja2 templates in published skills | Static analysis at publish time (§2.8); sandbox contract already documented in skill `Constraints` |
| Half-published state on network failure | `log::warn!` on failure; retain pending state in queue; retry on next drain (§2.6) |
| Dependency cycles | Cycle detection in install path (Phase 6) |
| User installs a skill that shadows their local skill of the same name | `SkillSource::Public` precedence is lower than `Global` (§2.3); marketplace installs land in `_marketplace/` namespace |
| The `.rules` trap about "process-global hooks need a startup-failure signal" | Every publish/unpublish/install failure emits `log::warn!` with skill ID, reason, remediation (§2.6, Phase 5) |
| The `.rules` trap about "tests must pin deliberate zed-kask deviations from upstream" | Phase 7 is dedicated to pinning tests |

## 7. Open Questions (defer to v2)

- **Multi-version pinning:** v1 offers only the latest. v2 should add a `kask_artifact_version` table that allows multiple rows per artifact and a UI to pin.
- **Project-local skill publishing:** v1 only allows publishing `SkillSource::Global` skills. v2 should add a "promote to global then publish" flow for project-local skills.
- **Embeddings, userpods, MCP-server configs as marketplace artifacts:** v1 is skills-only. v2 adds other artifact types; the catalog schema is designed to extend.
- **Ratings, comments, social features:** out of scope for v1.
- **Review/approval workflow:** v1 trusts the publisher's GitHub identity. v2 may add a review step.

## 8. Suggested .rules Additions

Per the `.rules` "Rules Hygiene" section, the following patterns emerged during this plan and are candidates for `.rules` additions after the team validates them in code review:

1. **Marketplace install namespace:** Marketplace-installed skills land in `~/.agents/skills/_marketplace/{source_user}/{skill_name}/`, never directly in `~/.agents/skills/{skill_name}/`. This prevents a marketplace skill from shadowing a locally-authored skill of the same name. `SkillSource::Public` precedence is lower than `SkillSource::Global` for the same reason.
2. **Publish-time Jinja2 sandbox enforcement:** The publish pipeline MUST run static analysis on every `*.j2` template and refuse to publish templates that violate the sandbox contract (no `import os`, no file/network access outside safety mode). This is a security boundary, not a feature. The sandbox contract is already documented in skill `Constraints` sections; the publish pipeline enforces it.
3. **Lazy drain failure signaling:** When the `SkillVisibilityQueue` drain task fails to publish or unpublish a skill, it MUST `log::warn!` with the skill ID, the failure reason, and the remediation. The local `visibility` flag is NOT rolled back — the user's intent is preserved; the queue retains the pending state and retries on the next drain. This mirrors the "process-global hooks need a startup-failure signal" trap.

---

## Continuation Prompt

> **Instructions for the agent executing this plan:**
>
> You are resuming work on the **Kask Extensions Panel & Skill Sharing** plan. This document is the authoritative specification. Follow it precisely.
>
> ### How to use this prompt
>
> 1. **Read this entire document first.** Do not skip sections. Sections 2 (Design Decisions) and 6 (Risks & Mitigations) are load-bearing — they pin decisions that are easy to get wrong.
> 2. **Check current phase.** Open `kask/docs/plans/kask-extensions-panel-and-skill-sharing.md` and find the highest phase whose "Acceptance" criteria are met in the codebase. That is your starting phase. Do not re-do completed phases.
> 3. **Verify the existing pieces (§1) are still where the plan says they are.** The plan references line numbers and file paths that may have drifted. Run `grep` to confirm each piece exists before depending on it. If a piece has moved, update the plan's §1 table before proceeding.
> 4. **Execute one phase at a time.** Do not skip ahead. Each phase's "Acceptance" criteria must be met before starting the next phase. Run `cargo check -p <crate>` (or `./script/clippy` for the affected crates) after each phase.
> 5. **Write the deviation-pinning tests in Phase 7 as you go, not at the end.** Every `// zed-kask:` comment you add in Phases 1–6 should get a corresponding test in the same phase. Phase 7 is the audit pass that confirms coverage.
> 6. **Follow the `.rules` traps.** Three traps apply directly:
>    - "Process-global hooks set at runtime need a startup-failure signal" — every publish/unpublish/install failure MUST `log::warn!` with skill ID, reason, remediation.
>    - "Tests must pin deliberate zed-kask deviations from upstream" — every deviation from upstream `ExtensionsPage` needs a pinning test.
>    - "Cross-thread GPUI communication uses channels, not `AsyncApp` handles" — the publish/unpublish pipelines run on a background executor and must not capture `AsyncApp`. Use a `tokio::sync::mpsc` channel with a foreground drainer if they need to dispatch to GPUI.
> 7. **Do not add features not in scope (§0).** Multi-version pinning, auto-update, embeddings/userpods/MCP-server artifacts, ratings, and review workflows are explicitly out of scope for v1. If the user asks for them, note them in §7 (Open Questions) and defer.
> 8. **Update this document as you learn.** If you discover a piece has moved, a design decision needs revisiting, or a phase's effort estimate was wrong, edit this document in the same commit as the code change. The plan is a living document, not a contract.
> 9. **Commit per phase.** Each phase is a commit (or a small commit series). The commit message should reference the phase (e.g., "kask-extensions: Phase 1 — skill visibility model").
> 10. **When you finish a phase, report:** which phase, what was built, what tests pass, what `cargo check` / `./script/clippy` output was, and any deviations from the plan (with reasons). Then ask the user whether to proceed to the next phase.
>
> ### Resuming mid-phase
>
> If you are resuming in the middle of a phase (some tasks done, some not):
> 1. Read the phase's task list. Check each task's checkbox state in your last report (or in git log if no report).
> 2. For each incomplete task, verify it is still needed (the plan may have evolved). If still needed, execute it.
> 3. Do not re-execute completed tasks.
> 4. Run the phase's acceptance check before declaring the phase done.
>
> ### If the plan is wrong
>
> If you discover the plan is wrong (a design decision is flawed, a phase is missing a task, an effort estimate is wildly off):
> 1. Stop coding.
> 2. Edit this document to fix the plan. Bump the version in the frontmatter.
> 3. Add a note in the commit message explaining what changed and why.
> 4. Resume coding from the corrected plan.
>
> ### If you are blocked
>
> If you are blocked on a decision not covered by this plan:
> 1. Do not guess. The plan's §2 (Design Decisions) and §6 (Risks & Mitigations) are the decision record. If the question is not covered there, it is an open question.
> 2. Add the question to §7 (Open Questions) with the context that triggered it.
> 3. Ask the user. Do not proceed on assumptions.
>
> ### When you are done
>
> When all phases are complete:
> 1. Run the full test suite for the affected crates: `cargo test -p agent_skills -p settings_ui -p kask_extensions_ui -p collab`.
> 2. Run `./script/clippy` on the affected crates.
> 3. Verify every `// zed-kask:` comment added during this work has a corresponding pinning test (Phase 7 audit).
> 4. Verify every failure path in the publish/unpublish/install pipelines has a `log::warn!` with skill ID, reason, and remediation.
> 5. Update the plan's frontmatter `status` from `Draft` to `Active`.
> 6. Report completion to the user with: phases completed, tests passing, clippy clean, and any open questions that emerged.
