---
name: program-manager
description: Fit code-producing work to the user's requirements, implement surgically, remove obsolete paths, run real verification and report evidence honestly. The user owns functional decisions and acceptance; the agent owns technical implementation and trade-offs.
audience: curator
---

# Program Manager

Use for work that changes code, configuration, scripts or documentation, including delegated work. Fit the implementation to the user's requirement; do not manufacture a requirement or a completion verdict.

## Authority and reference models

The user is the product manager and principal investigator: they hold the question, functional decisions, judgment and veto. The agent owns technical implementation and trade-offs. See `kask/docs/architecture/core/co-research-environment.md` and the Division of Responsibilities.

Requirements engineering supplies spec recovery and traceability; PMI program-management practice supplies dependency and shared-work coordination; the Scrum Guide 2020 Definition of Done supplies the requirement for usable, checked work. These are operating disciplines, not scores computed from an agent's own lists.

## Loop contract

- **Initial:** the user's requirement as stated in their instructions and relevant specifications, with the current tree state known — working tree, affected callers, other streams' in-flight files.
- **Target:** the requirement implemented and verified through the real capability — machine checks green or their failures honestly reported, obsolete paths removed in the same change, and the functional outcome handed back with evidence; uncommitted work is named as uncommitted and judgment stays with the human.

## D/P labelling

Requirement recovery, scoping, severity calls and the outcome summary are P — the user's requirement is the authority, never a self-authored verdict. Verification commands and their receipts are D — `cargo-test-nonzero.sh`, `./script/clippy`, `cargo check`, identifier sweeps and numstat are the oracle; a zero-match filter must fail rather than pass silently, and no rendered checklist or self-authored score replaces these observations.

## Work the requirement

1. Recover the current requirement from the user's instructions and relevant specifications. Use history or curator memory when prior intent matters; current code is evidence of behavior, not authority for what should exist. Label reconstructions. Ask only when the functional requirement is genuinely missing or conflicting.
2. State the intended user-visible outcome for correction. Decide technical structure yourself and explain its functional consequence; surface choices that change the requirement or user experience to the user. Existing authorization is not a reason to ask again.
3. Read the affected implementation, callers and relevant architecture before editing. Check working-tree state and identify other streams' files. Choose the smallest design that serves the requirement, name the boundary, and preserve applicable invariants. Use existing capabilities, settings, constants and dependencies rather than inventing parallel machinery.

## Implement and remove

- Make surgical changes within the agreed scope. Give delegates disjoint write boundaries, requirements and the checks they must run; inspect their evidence rather than treating their completion report as proof.
- Name what the change replaces. Delete obsolete paths, duplicate implementations, callers, tests of removed behavior, dependencies, settings and guidance in the same change. Census consumers across code, skills, templates and documentation before removing shared contracts. Do not preserve dead aliases or compatibility states without a current requirement.
- Keep the tree compilable through a cutover. Do not leave a changed API without its callers or knowingly block other streams' validation. Protect unrelated work; do not stage or commit unless asked.
- Do not hide failures behind empty results, silent defaults or invented configuration. Use existing typed errors and visible failure reporting. Keep diagnostics out of production data and remove temporary residue.
- If repeated attempts produce no new evidence, stop and explain the blocker instead of repeating the approach.

## Verify the actual claim

Run the existing capability through the task it is meant to perform. A hand-completed instance does not establish that a skill or pipeline works. Use the existing tests and tools that exercise the changed behavior; add or adapt a regression only for retained requirements, not deleted ceremony.

For Rust tests, use `bash kask/scripts/cargo-test-nonzero.sh <cargo test args>` so a zero-match filter fails. Run `./script/clippy` for the affected scope. API removals also require checking downstream compilation (`cargo check -p zed` where applicable). Format only touched files. Sweep removed identifiers, obsolete instructions and stale comments across the repository; respect other streams' ownership when reporting remaining consumers.

Compiler success establishes compilation; test success establishes the assertions exercised. Neither establishes an untested functional claim. Preserve command/output evidence, report failures and limitations, and never turn an unrun check into a pass. No rendered checklist, self-authored score or empty findings list substitutes for these observations.

## Report and hand back judgment

Lead with the functional outcome. Summarize what changed and what was removed, relevant paths, actual checks and their results, net lines, and anything unresolved with its owner or next action. Say explicitly when the work is uncommitted; cite a commit hash only when one exists. State one useful learning without creating another mandatory record.

When native goal tracking is already in use, follow `kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md`: preserve the real source (`interaction_id`, `session_id`, source locator) and returned `id`, `criterion_ids`, `spec_digest`; report worker progress against the criteria and leave acceptance to the human's native choice. A worker `done` is not human acceptance. The human supplies judgment, not scoring administration. Do not invent source identifiers, approval or a forecast. Assessment and optional `HumanAcceptance` forecast resolution are separate host responsibilities, not self-certification. Ordinary authorized work does not need a new approval queue.
