---
title: "hKask Core and MCP Review — Repair and Improvement Plan"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-30
version: "0.10.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask core and MCP: findings and execution plan

## 1. Status, authority, and desired outcome

**Execution authorized on 2026-09-18; partially implemented.** The original review implemented no repairs. The subsequent operator instruction authorized bounded execution; §9 now records fresh evidence separately from the historical review. Unfinished packages remain open; this document is not a whole-system completion claim.

**Operator decision: there are no backward-compatibility requirements.** Replace unsafe or misleading interfaces directly, update all callers together, and delete superseded paths. Do not add deprecated APIs, legacy adapters, compatibility flags, dual writes, or parallel implementations solely to preserve old behavior.

The desired outcome is a working human-controlled agent system: every consequential effect has a clear authority source, attributable initiator, observable outcome, and truthful cancellation/recovery semantics. Preserve minimal upstream divergence and useful upstream facilities, not existing interfaces for their own sake.

No backward compatibility does **not** authorize destroying user data, silently changing existing encryption keys, discarding concurrent work, accessing secrets, or launching paid/live services. Data preservation is separate from maintaining old APIs. Resolve genuinely destructive product decisions with the operator.

### Snapshot and epistemic limits

- Review date: 2026-09-18.
- Initial reviewed HEAD: `e3644b290404663f879df17312e8af4276ca43b7`, with existing staged and unstaged work.
- Final review HEAD: `c9ecc55fd4d59ae70252288c5e97c758233b554b`.
- HEAD when this plan was saved: `28288948c3b6753ba6883ef60a60cb1ca7864e70`.
- Other actors changed the checkout during review. Principal dispatch, authorization, training-script, corpus-containment, gallery-deletion, and packaging finding files were rechecked at review close and unchanged from the initial commit. Spreadsheet service code changed externally and was inspected/tested in its later state.
- Findings and line ranges below are **review-snapshot evidence**, not claims that the later tree has been fully re-reviewed. Revalidate each finding and its callers before implementation. Record already-fixed or disproved findings rather than recreating their repairs.
- “Verified defect” means a concrete source-traced failure path or reproduced check failure. It does not mean an end-to-end exploit was executed. Severity and confidence are independent.
- The review made no source/configuration/documentation/test edits, installations, secret reads, live MCP launches, or external/provider calls. This later save changes documentation only.

### Governing references

Read the current rules and these references before executing:

- `.rules`
- `AGENTS.md`
- `DIVERGENCE.md`, especially the applicable seams and §13.1.
- `kask/docs/architecture/core/PRINCIPLES.md:43–79`
- `kask/docs/architecture/core/magna-carta.md:54–98`
- `kask/docs/architecture/functional-interaction-spec.md:135–197`
- `kask/docs/architecture/DOCUMENTATION_STANDARDS.md`

The Magna Carta explicitly distinguishes implemented enforcement from intended consent machinery. Do not treat unimplemented charter types as available APIs or newly discovered regressions. Prompt-level commitments to human oversight are intent, not proof that runtime enforcement exists.

## 2. Architecture and overall assessment

The architecture is coherent, but authority, interruption, and recovery semantics differ between paths. Repair these boundaries before broad module extraction. The fork's “minimal divergence” is a maintenance objective, not a proven property: the reviewed divergence register reached D66 and several orientation documents were stale.

### Inventory measured during review

Offline Cargo metadata reported 299 workspace members, including **19 libraries under `kask/crates/` and 12 MCP server packages**. The measured workspace release was 0.40.0.

| Responsibility | Libraries |
| --- | --- |
| Contracts and vocabulary | `hkask-types`, `hkask-bridge-ontology`, `hkask-tool-port` |
| Persistence and memory | `hkask-storage`, `hkask-event-store`, `hkask-memory`, `hkask-keystore` |
| Execution infrastructure | `hkask-mcp`, `hkask-mcp-server`, `hkask-inference`, `hkask-services-core` |
| Computation and capabilities | `hkask-forecast`, `hkask-lisp`, `hkask-condenser`, `hkask-spreadsheet` |
| Regulation and interaction support | `hkask-regulation`, `hkask-email`, `hkask-steer-core` |
| Zed-side integration exception | `kask_bridge` |

Servers: companies, corpus, curator, kata-kanban, media, portfolio, prediction-markets, research, scenarios, spreadsheet, swarm, training. Discover the execution-time inventory anew from manifests and `kask/crates/kask_bridge/src/mcp_servers.rs:55–539`; do not hardcode these counts in new tests.

### Observed boundaries and paths

| Path / owner | Responsibilities and constraints |
| --- | --- |
| Upstream-derived Zed host | Agent interaction, permissions, GPUI, settings, and language-model registry. Changes belong at documented divergence seams. |
| `kask_bridge` and composition root | Adapt settings, credentials, memory/regulation, parent-held grants, and inference IPC to host facilities. Explicit §13.1 exception. |
| `hkask-mcp::McpRuntime` | Own managed child lifecycle, discovery, metering, dispatch, recovery, and outcome recording. Metering is not authorization. |
| MCP child processes | Typed inputs, tool behavior, output/error framing, and substantial domain behavior in several server libraries. Direct hKask dependencies; host access through IPC, not linkage to `kask_bridge`. |
| Native agent → managed tool | `KaskServerTool` authorizes the tool, then managed source/runtime dispatch to stdio child. |
| Delegated child → tool | Agent-card allowlist narrows request; IPC checks requested list and parent-held grant; runtime meters. |
| Child → inference | IPC returns to the host model registry; inference admission/deadline controls do not automatically bound arbitrary MCP execution. |
| Child → worktree thread | Separate IPC method queues GPUI worktree creation and auto-submits a prompt; F1 concerns its missing grant boundary. |
| Spreadsheet edit | Dedicated engine actor creates immutable revisions and operation records, with a reconciliation operation for uncertain outcomes. |
| Outcomes → regulation | Client-side typed outcomes feed regulation; server stderr tracing is not the same substrate as durable regulation records. |
| User-configured MCP | ContextServerStore remains a separate transport path; F7 concerns its retry semantics. |

The dependency checker and metadata inspection found no direct forbidden local hKask-to-Zed dependency, excluding `kask_bridge`. Server-library edges were kata-kanban → swarm, scenarios → prediction-markets → portfolio, and companies → portfolio. These do not violate §13.1, but qualify the claim that servers are uniformly thin adapters over separate domain crates.

## 3. Prioritized defects

### F1 — Worktree creation bypasses delegated-tool authority

**Class:** verified defect against attenuation intent. **Severity:** High. **Confidence:** High.

**Evidence:**
- `kask/crates/kask_bridge/src/inference_ipc_server.rs:820–856` checks request list and `parent_allows` for ToolInvoke.
- `kask/crates/kask_bridge/src/inference_ipc_server.rs:867–906` independently accepts CreateWorktreeThread.
- `crates/agent_ui/src/agent_panel.rs:4859–4907` sets `auto_submit: true` and creates worktree-backed work.

**Mechanism/impact:** a child with same-user socket access and an available workspace can initiate filesystem work and an agent task with an empty/revoked delegated-tool grant. The child restrictions are not visibly propagated into the new thread.

**Counterevidence/disproof:** this is not an Internet-authentication bypass; subsequent native-agent tool calls still encounter normal permission checks. Those checks do not authorize the initial task/worktree creation. No live reproduction was performed.

**Repair:** require an explicit parent-granted host capability before enqueueing; define propagation of restrictions or an explicit new human authorization. Remove the ambient route rather than accepting old requests.

**Acceptance:** fake-spawner tests yield zero effects for missing/revoked/insufficient grants and one for a valid grant; inspect spawned-thread effective authority.

**Related queued-cancellation defect:** `kask/crates/kask_bridge/src/inference_ipc_server.rs:422–438` uses an unbounded queue and spawns without checking whether the reply receiver closed. A waiting request can execute after disconnect. Add bounded admission and cancellation-aware dequeue; test request B disconnecting while A occupies the consumer. Do not equate cancellation after a committed effect with rollback.

### F2 — Stop can leave managed MCP execution pending

**Class:** verified control-flow defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `crates/agent/src/tools/context_server_registry.rs:574–592` awaits managed invocation without selecting user cancellation.
- `kask/crates/hkask-mcp/src/runtime.rs:1686–1698` calls rmcp without a request deadline.
- `crates/agent/src/thread.rs:2597–2604,3500–3518,6096–6100` signals cancellation but waits for tool results/turn completion.

**Mechanism/impact:** a connected server that never replies can prevent cancellation completion indefinitely. The locked rmcp implementation was inspected: default request options have no timeout.

**Counterevidence/disproof:** startup/discovery deadlines, host inference deadlines, panel-task aborts, and IPC disconnect handling exist, but do not bound this wrapper. No hung live service was launched.

**Repair:** carry cancellation through managed dispatch and define a user-visible execution deadline policy. After possible delivery, preserve an uncertain outcome rather than reporting rollback or inviting automatic retry.

**Acceptance:** a never-resolving fake source cannot prevent Stop completing within a defined local test bound. Commit-before-cancel produces an unknown/committed outcome as appropriate and zero automatic replays.

### F3 — Training model input becomes remote shell source

**Class:** verified input-boundary defect; isolated shell semantics reproduced. **Severity:** High. **Confidence:** High.

**Evidence:**
- `kask/registry/templates/training/axolotl-lora.j2:10` renders model input directly.
- `kask/mcp-servers/hkask-mcp-training/src/providers/runpod.rs:552–557,649–682` uses a fixed config heredoc delimiter and an unquoted manifest heredoc.
- `kask/mcp-servers/hkask-mcp-training/src/providers/nebius.rs:90–118` executes the generated script with Bash.
- `kask/mcp-servers/hkask-mcp-training/src/tools/submit.rs:239–248` does not reject model-resolution failure.

**Mechanism/impact:** newline input can terminate the config heredoc; model input in the manifest heredoc can execute shell substitutions in the training VM/container. Local-editor code execution was not demonstrated.

**Counterevidence/disproof:** quoting the config delimiter prevents expansion inside its body, not premature delimiter termination. Training parameter/dataset checks are not shell-source separation.

**Reproduction:** a file-free Bash probe of the manifest pattern changed literal `org/model$(printf SHELL_EXPANDED)` into `org/modelSHELL_EXPANDED`. No provider or training job was contacted.

**Repair:** replace caller-data interpolation with structured serialization and data transfer; remove the unsafe generator path. Identifier validation is additional defense, not the sole boundary.

**Acceptance:** local generation/harness tests exercise newlines, delimiter lines, quotes, backticks, and `$()`; data round-trips literally and no marker command executes.

### F4 — Corpus containment misses final-component symlinks

**Class:** verified filesystem-boundary defects. **Severity:** High. **Confidence:** High.

**Evidence:**
- `kask/mcp-servers/hkask-mcp-corpus/src/tools/gather.rs:187–210` checks cache directory, appends a filename, then writes it.
- `kask/crates/hkask-mcp-server/src/server/validation.rs:204–231,289–298` reconstructs a non-existing target from its existing ancestor.
- `kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs:72–84` writes conversion output through that boundary.

**Mechanism/impact:** an existing cache leaf symlink redirects a write outside the allowed root. A dangling output symlink returns NotFound during canonicalization, survives reconstruction, then redirects a later write. Neither requires a race; OS write permissions still apply.

**Counterevidence/disproof:** slug validation blocks textual traversal, not symlinks. A stronger helper exists at `kask/mcp-servers/hkask-mcp-corpus/src/path_safety.rs:28–65`, but these paths do not use it. Eight existing shared path tests passed without covering these cases.

**Repair:** replace permissive write interfaces with complete-destination, symlink-resistant opening/publication. Another pre-write canonicalization alone does not solve the race variant.

**Acceptance:** temporary-root tool tests cover existing/dangling leaf links, ancestor links, ordinary new files, and unchanged outside-file bytes. Include platform-specific behavior explicitly.

### F5 — Gallery deletion ignores read-only/copy-on-write mode

**Class:** verified policy-enforcement defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `kask/crates/hkask-storage/src/gallery.rs:37–47` defines preservation policy.
- `kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs:337–356` returns mode without enforcing it.
- `kask/mcp-servers/hkask-mcp-media/src/tools/gallery.rs:1393–1427` calls remove_file when requested without checking mode.

**Mechanism/impact:** a tool-name-authorized caller can remove an original file from a gallery whose policy says originals are preserved.

**Counterevidence/disproof:** `delete_file` is explicit and defaults to preserving the file. This limits accidental activation but is not a documented policy override. No destructive live test was run.

**Repair:** enforce gallery-mode admission at the file-mutation boundary. Only provide an override if the operator explicitly chooses that product behavior; do not infer consent from a model-provided boolean.

**Acceptance:** mode × deletion-request tests assert source bytes, index state, and transcript linkage on success, refusal, and partial failure.

### F6 — Process identity is not authenticated per-call authorship

**Class:** verified attribution defect; broader authorization impact is workflow-dependent. **Severity:** High for ownership/audit semantics. **Confidence:** High.

**Evidence:**
- `kask/crates/hkask-mcp-server/src/server/transport.rs:91–117` falls back to anonymous identity.
- `kask/crates/kask_bridge/src/mcp_env.rs:46–54` maps curator identity only.
- `kask/crates/hkask-mcp-server/src/server/tool_span.rs:140–168` attributes from server context.
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:885–895` claims tasks as the “authenticated caller” using `self.webid`.

**Mechanism/impact:** distinct initiating agents can be indistinguishable in server actions and ownership checks. A process-scoped WebID is not evidence of the caller's authenticated identity.

**Counterevidence/disproof:** OS-user/data-directory separation, host accounting identities, and parent grants remain. Cross-OS-user access was not established.

**Repair:** introduce one host-authored invocation contract distinguishing owner, initiating actor, delegated actor, server identity, and authority. Replace anonymous production fallback and update all callers together. Ordinary tool arguments must not establish identity. Revalidate ownership behavior before changing persisted identity semantics.

**Acceptance:** two delegated actors produce distinct attributable claims; spoofed input cannot alter effective identity/ownership; missing production identity fails visibly and safely.

### F7 — ContextServerStore retries possibly committed effects

**Class:** verified retry-safety defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `crates/agent/src/tools/context_server_registry.rs:988–1046` retries non-timeout errors.
- `kask/crates/hkask-mcp/src/runtime.rs:1632–1653,1686–1698` distinguishes non-delivery and uncertain delivery.
- `kask/crates/hkask-tool-port/src/tool_port.rs:15–51` defines retry-safe versus interrupted outcomes.

**Mechanism/impact:** a server commits a mutation and loses its response; the host may repeat that mutation. Non-timeout protocol/decoding errors can also enter the retry branch.

**Counterevidence/disproof:** timeouts are excluded and managed runtime behavior is safer. Its semantics do not protect the alternative path; the comment claiming equivalent behavior is stale.

**Repair:** one delivery-state contract across both paths; retry only proven non-delivery. Retain different transports only where they earn their complexity, not different safety rules.

**Acceptance:** commit-then-drop-response fixture receives exactly one mutation; distinguish transport, protocol, decoding, cancellation, and pre-send failures. Unknown effects require reconciliation, not replay.

### F8 — Spreadsheet packaging is incomplete; critical checks are skipped

**Class:** reproduced packaging defect and verified CI coverage gap. **Severity:** High for capability availability; Medium for CI gap. **Confidence:** High.

**Evidence:**
- `kask/crates/kask_bridge/src/mcp_servers.rs:520–538` registers spreadsheet.
- `kask/scripts/build/mcp-servers.txt:12–22` omits it.
- `kask/scripts/build/install-common.sh:32–46` consumes the list.
- `.github/workflows/kask-invariants.yml:75–114` omits the registry check and explicit fixture feature.
- `kask/crates/hkask-mcp/Cargo.toml:32–49` gates reconnect integration tests on `test-fixture`.

**Reproduction/impact:** `check-mcp-servers.sh` failed, reporting missing `hkask-mcp-spreadsheet`. List-driven installation omits an advertised capability; the inspected CI command skips the process-boundary reconnection suite.

**Counterevidence/disproof:** the checker and tests already exist. External automation beyond the inspected workflows was not verified.

**Repair:** synchronize packaging immediately and wire existing checks. Prefer one authoritative inventory or mechanically checked equality over duplicate prose/name lists. Explicitly enable isolated process-fixture tests in CI.

**Acceptance:** manifest/registry/install sets agree; clean temporary installation has the complete current fleet; CI output shows actual reconnect tests, not a zero-test success.

## 4. Design risks and improvement opportunities

### R1 — Public default passphrase weakens copied-database confidentiality

**Class:** design risk reflecting an explicit recovery tradeoff. **Severity:** High if encryption is relied upon against copied-file disclosure. **Confidence:** High.

Evidence: `kask/crates/hkask-keystore/src/passphrase.rs:1–17` and `kask/crates/hkask-keystore/src/keychain.rs:402–424`. First-run provisioning uses a public fixed default. A copied database using it does not require keychain access to decrypt.

Counterevidence: source explicitly prioritizes recoverability; users can change the secret. No deployed database or keychain was inspected. This is not proof that any particular user's database still uses the default.

Plan: make the protection state visible, then obtain the operator's choice of required user secret versus recoverable generated secret. No compatibility requirement justifies preserving the default as a confidentiality claim; it also does not authorize silent rotation or data loss. Test onboarding, recovery, and all-database rotation before changing provisioning.

### R2 — Generic transaction handle does not own its connection

**Current disposition (2026-09-18): removed, not wrapped; implementation committed by another stream in `74df6916b7`.** Caller
inventory at `6edf354cd739a22cc6f7d1b2e565072b84913738` found no production users
of the generic driver transaction facade. Deleted its module, trait hooks,
SQLite forwarding methods and two bridge test-driver forwards. Existing
`rusqlite::Transaction` operations retain one connection. Public HMem batch
test `atomic_batch_commit_failure_is_rolled_back_before_reuse_and_reopen`
checks deferred COMMIT failure, rollback, subsequent reuse and file reopen with
two pooled connections. Storage/bridge tests and scoped clippy passed; detailed
evidence is in the R6 storage entry of the removed Gödel gap-closure plan (git
history; the deletion itself is committed in `74df6916b7`). R3/H1 and broader
P3 recovery are not closed by this deletion. The following evidence is the
**historical pre-deletion finding**, not a claim these APIs still exist.

**Class:** latent API defect/design risk; production use of this API not established. **Severity:** Medium now, High if used for atomic writes. **Confidence:** High.

Evidence:
- `kask/crates/hkask-storage/src/database/driver.rs:57–66`
- `kask/crates/hkask-storage/src/database/transaction.rs:19–38`
- `kask/crates/hkask-storage/src/database/sqlite.rs:270–290,332–337`

BEGIN, operations, and COMMIT independently acquire pooled connections. The handle owns a driver reference, not exclusive connection access; it marks itself committed before commit succeeds. Concurrent pool use can break the promised transaction scope.

Counterevidence: inspected real atomic writes use borrowed rusqlite transactions, including `kask/crates/hkask-storage/src/hmem.rs:323–343`.

Plan: confirm callers; delete the unused misleading facade. If actual callers need a shared API, replace it with connection-owning transaction scope, not a wrapper around the unsafe contract. Validate two-connection concurrency, failed commit, and rollback without partial writes.

### R3 — Spreadsheet reconciliation has a crash-durability gap

**Class:** design risk. **Severity:** Medium. **Confidence:** High on sequence, Medium on deployment likelihood.

Evidence:
- `kask/crates/hkask-spreadsheet/src/artifact_store.rs:106–145,161–172,193–229`
- `kask/crates/hkask-spreadsheet/src/service.rs:585–602`

Revision publication syncs the file and renames it, then operation records use direct writes. A crash can leave a revision without a usable receipt. File sync alone does not establish parent-directory durability.

Counterevidence: absent operation records explicitly mean unknown, not unapplied; immutable bases limit destructive consequences. Thirteen spreadsheet tests passed. No power-loss experiment ran.

Plan: define process-crash versus power-loss guarantees, then replace the publication/receipt contract as needed. Make receipt publication atomic/durable and incomplete publication discoverable. Inject failures between revision and receipt publication; reopened state must never falsely report “not applied” or replay automatically.

**H1 — cached-open digest hypothesis:** `kask/crates/hkask-spreadsheet/src/service.rs:502–525` returns early for cached IDs before comparing the newly supplied digest. Test valid open followed by the same IDs with a bad digest. This was not dynamically reproduced; assess severity after verifying caller reachability and contract.

### R4 — Caller assertion is not operator approval evidence

**Class:** design risk. **Severity:** Medium. **Confidence:** High.

Evidence: `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:420–460`. An agent can supply `operator_confirmed: true` and a note without a host-observed confirmation event.

Counterevidence: this records advice application rather than executing the intervention, and keeps causal attribution unverified.

Plan: replace the boolean with a host-authenticated receipt wherever authenticated confirmation is required. If agent-reported confirmation remains useful, name it as a claim rather than granting it the same evidence status. Test that model arguments alone cannot create the stronger record.

### O1 — Documentation and contract inventory drift

**Class:** improvement opportunity with verified documentation defects. **Severity:** Medium. **Confidence:** High.

Evidence:
- `AGENTS.md:39–47` says 10 servers, describes an old host path, and references an absent per-tool contract document.
- `kask/docs/architecture/zed-host-architecture-plan.md:13–43` says 18 libraries/11 servers.
- `DIVERGENCE.md:234–240` includes a removed library and 11 servers.

Counterevidence: the runtime registry is centralized; the charter distinguishes IS/OUGHT. Missing documentation is not proof that all tool contracts lack tests.

Plan: generate inventory facts, validate links, restore or replace the missing contract reference, and remove obsolete claims. Inventory actual input/output/error behavior, including structured content versus string envelopes. Test category invariants, not hand-maintained tool-name lists.

### O2 — Focus architectural cleanup on behavior, not crate counts

**Class:** optional improvement opportunity. **Severity:** Low unless tied to a defect. **Confidence:** Medium.

Keep the spreadsheet actor: deleting it spreads non-Send engine ownership, executor bridging, staging, and revision management across MCP and GPUI. Keep typed uncertain-delivery errors: they encode essential retry information. The unused connectionless transaction facade has been removed in the later R2 slice; preserve connection-owned domain transactions instead.

Mechanical samples during review: event-store root had 8 public methods and 1 type with approximately 138 nonblank/noncomment pre-test lines; spreadsheet service had 12 public methods and 5 types with approximately 503 such lines. These are navigation signals, not reasons to split cohesive modules.

Server-library dependencies alone do not justify a new service layer. Extract only demonstrated shared behavior whose removal would duplicate invariant enforcement across callers. No compatibility requirement is a license to simplify, not a mandate to rewrite everything.

## 5. Strengths and constraints to preserve

- §13.1 held in inspected local manifests and the repository checker.
- Managed dispatch distinguishes proven non-delivery from uncertain delivery; see F7.
- Child environment clearing and selective injection: `kask/crates/hkask-mcp/src/runtime.rs:649–675`.
- Independently held, revocable grants: `kask/crates/kask_bridge/src/delegation_grants.rs:15–74`.
- Atomic call charging: `kask/crates/hkask-regulation/src/energy.rs:164–193`.
- Research strict transport validates redirect destinations and connect-time DNS and disables proxies: `kask/mcp-servers/hkask-mcp-research/src/research/providers/raw_fetch.rs:41–49,93–106,128–144`.
- Immutable spreadsheet bases, explicit unknown outcomes, evidence-aware regulation, and surfaced degradation.
- Existing real-process lifecycle tests and temporary-installation isolation checks.

Same-UID children are not OS-sandboxed; information flow is taint-unaware by documented decision. Call caps are neither consent nor monetary budgets. Do not convert those limitations into unstated security guarantees.

## 6. Execution program: complete vertical changes, not compatibility stages

Every work package follows: revalidate → reproduce safely → replace/update all callers → delete superseded path → verify → record result. Keep the tree buildable. No half-applied API changes in the shared checkout. “Incremental” means bounded and independently verified, not maintaining old and new implementations together.

### P0 — Re-ground and establish a safe baseline

1. Record HEAD, worktree/index status, and current inventory; preserve unrelated edits. Do not assume the review snapshot still holds.
2. Read applicable rules, principles, and DIVERGENCE seams. Identify current test commands and fixtures.
3. For every F/R/H item, record upheld, already repaired, disproved, or unverified, with current citations.
4. Select a single work package; identify its callers and negative tests before editing. Prefer a branch in the canonical clone under the operator's normal workflow, without discarding or copying unrelated dirty work. Never a second clone, worktree, or copy of this repository (`.rules`, Repository fragmentation).
5. No live provider tests, secrets, installations, paid operations, or changes to real user databases. Use mocks, in-memory stores, and temporary directories. Ask separately before crossing those boundaries.

**Exit:** current evidence matrix, reproducible baseline, and bounded first change. Do not block urgent local repair on exhaustive fleet re-review.

### P1 — Close immediate effect-boundary failures

**Scope:** F1–F5. Work on one complete boundary at a time.

| Package | Primary implementation surface | Required behavior / verification |
| --- | --- | --- |
| P1a: host task creation | `kask_bridge` IPC/grants; host worktree adapter; shared IPC types/client as required | Explicit grant before enqueue; restriction propagation; cancelled queued work never starts; bounded admission. Fake spawner and host-seam tests. |
| P1b: Stop and managed invocation | Agent managed-tool wrapper, tool port, runtime | Cancellation reaches the wait/transport; explicit deadline policy; possible effects remain unknown, never auto-replayed. Never-resolving and commit-before-cancel tests. |
| P1c: training input separation | Training submission, provider script generation, config templates | Caller data cannot become shell syntax. Local adversarial-string tests across every provider consuming the generator. |
| P1d: file policy | Shared write boundary, affected corpus tools, gallery mutation boundary | No escape through complete destinations; gallery originals preserved by mode. Tool-seam temporary-filesystem tests. |

Names in this table identify modules, not authorization to modify every file in a crate. Derive the exact change set from current callers. Coordinate P1a/P1b with P2 so no disposable compatibility machinery is introduced.

### P2 — Replace the invocation and outcome contract end to end

**Scope:** F6, F7, R4 and contract consolidation needed by F1/F2.

Define one host-authored contract with explicit distinctions:

- data owner versus initiating/delegated actor versus executing server;
- granted authority versus the caller's requested narrowing;
- host-observed approval receipt versus an agent-reported claim;
- not delivered versus delivered/outcome unknown versus completed;
- cancellation of waiting versus cancellation of execution versus reversal of effects.

Use only fields that enforce an actual requirement. Do not replace these distinctions with another caller-supplied token/value comparison. Reuse existing authenticated host facts and parent-held grants rather than inventing a parallel identity system.

Update native-agent, widget, delegated IPC, server context, and audit/ownership consumers that actually use the contract. Retain distinct transports only for real host responsibilities. Remove anonymous production fallback, unsafe retries, and superseded argument forms. No dual protocol is required.

**Exit:** two-actor ownership tests, spoofing/empty/revoked authority tests, host approval evidence tests, and commit-then-drop-response tests pass across affected paths. Old authority/retry paths are absent. A compiler pass alone is insufficient.

### P3 — Repair persistence interfaces and recovery

**Scope:** R2, R3, H1; R1 only after the required product decision.

- Delete unused generic transaction machinery; if needed, replace it with connection-owning scope and test concurrency/rollback.
- Specify spreadsheet receipt/revision crash states before changing files. Implement atomic publication/reconciliation appropriate to that contract; schemas and interfaces may change directly.
- Test cached-open digest identity separately; fix only if the hypothesis survives current-source and behavior checks.
- For encryption, first make actual protection status explicit and decide onboarding/recovery policy with the human. Test rotation against disposable databases only. Do not silently rotate real data.

**Exit:** no misleading transaction API; fault-injected recovery reports truthfully; existing user data remains recoverable; no fake success from absent/corrupt evidence.

### P4 — Make packaging, CI, and documentation reflect actual behavior

**Scope:** F8, O1. The small F8 packaging repair may run earlier as an independent vertical change.

- Restore inventory equality and execute the existing check in CI.
- Enable `test-fixture` explicitly for the serial reconnect suite. In a continuation session, obtain authorization for isolated fixture child-process launches if not already allowed; never substitute live installed MCP servers.
- Update affected seam records and behavioral contracts in the same change as implementation. Generate inventories where practical; delete stale references instead of preserving aliases.
- Do not claim every tool is behaviorally covered because a grep found `Parameters(`. Require targeted tests for repaired effects and failure boundaries.

**Exit:** complete temporary installation, actually executed process tests, current documentation, and no manual list silently disagreeing with runtime.

### P5 — Optional simplification and formal models

Only after the preceding behavior is pinned, revisit duplicated dispatch semantics, broad public surfaces, and server-library coupling. Apply the deletion test to each proposed abstraction. Do not undertake a fleet-wide service-layer rewrite without demonstrated caller benefit.

Candidate formal invariants:

1. Accepted child effects are a subset of parent authority; revocation prevents later admission.
2. Automatic retry occurs only from proven non-delivery.
3. Successful charges within a fixed tick without overrides do not exceed the ceiling.
4. An operation identity maps to one committed result or an explicit uncertain state.

Lean/lake were unavailable during the 2026-09-18 review; no proof was attempted and no tools installed. A model proof would not establish that Rust checks every IPC variant, that a filesystem survives power loss, or that a remote process handles cancellation. Prefer implementation-level tests first; attempt formalization only with explicit assumptions, practical tooling, and a mapping from the model to implementation.

### P6 — Separate skill evaluation from skill execution

**Operator ruling (2026-09-24):** skill evaluation runs as part of the algedonic review with the human operator and the Curator — never in the executing thread, a project-bound evaluation thread, or the agent as it runs. Evaluation must be logically separated from execution; otherwise the measure becomes the executor's target and stops measuring (Goodhart's law, Wikidata Q2575082). This was always the specification; no backward compatibility is required. Build to the functional specification, minimally, and delete the superseded self-evaluation paths rather than keeping them alongside.

**Spec provenance:** the ruling above; `gemba-walk` design reference ("kask's separation of skill execution from regulation"); `functional-interaction-spec.md` §7.4 (outcome quality is resolved against named instruments, the operator supplies final ground truth); PRINCIPLES §9.2 (the loop measures activation reliability and operator acceptance, and does not infer work quality from activation). The 2026-09-24 skill-system audit found the violation: `self-improvement`, `skill-maintenance-optimize`, `skill-logic-audit` and `gpa-evolution` instruct the executing agent to evaluate and commit its own skill changes, and `record_skill_feedback` was registered in every native session.

**Invariant:** an executing session can *record what happened* (activation outcome, predictions via `kanban_goal_create`, skill-use issue reports) and can *propose* a change; it cannot *evaluate* a skill or *accept* a skill change. Evaluation verdicts and acceptance are operator decisions taken in the algedonic review with the Curator.

| Slice | Change (no compatibility path) | Checkpoint (falsifiable) |
| --- | --- | --- |
| P6a — evaluation tool authority | `record_skill_feedback` registered only in Curator sessions (`NativeAgent::new_session`); removed from `register_session` | Native model request lacks the tool, Curator request carries it; the native pin fails when the tool is re-registered in `register_session` |
| P6b — review owns evaluation | `algedonic-review` gains a skill-evaluation phase: sense (skill outcomes, operator-feedback history, skill-use issue reports, resolved goal outcomes, pending proposals) → evidence-backed per-skill briefing → operator verdict → `record_skill_feedback` / accept-reject of proposals, receipts retained | A review run over a fixture backlog produces verdicts only from operator decisions with cited evidence; no verdict is recorded without an operator decision |
| P6c — executing skills stop self-evaluating | Delete self-judged commit/select steps from `self-improvement` (`si-commit-or-rollback` Act becomes propose-and-queue), `skill-maintenance-optimize`, `skill-logic-audit` compare/compose/user-choice, `gpa-evolution` self-scored selection, and `metacognition`/`logo-builder` self-judged loops; each ends by filing a proposal for the review. Within-session PDCA loops remain for work *on the task* (operator ruling 2026-09-24: PDCA self-improvement loops are within a session), bounded by iterations, not calendar horizons | Grep of the skill corpus finds no instruction for an executing agent to commit, accept, or score its own skill change; `si-kata-target`/`improvement-step3-target` carry an in-session bound instead of "1 week to 3 months" |
| P6d — ontology anchors | Derived entries for `pdca_cycle` (Wikidata Q820214; LEI lexicon PDCA; Shewhart/Deming), `improvement_kata` and `coaching_kata` (LEI lexicon Kata; Rother 2010, Wikidata Q7830807), `goodharts_law` (Wikidata Q2575082), each citing the 2026-09-24 ruling | `onto_anchor` returns tier `derived` with identity and authority for each; resolver tests pin them. No published lean/kata OWL vocabulary was found in the bundled fixtures or a web search, so the derived rung is the correct ladder rung |
| P6e — docs and seams | DIVERGENCE D1/D59, PRINCIPLES §9.2, skills-and-composition regulation-feedback rows, skill docs that describe direct rating from executing sessions | Full-repo sweep for the old registration and self-evaluation phrasing returns only historical records |

**Resolved 2026-09-24:** the operator ruled to merge `gemba-walk` into `algedonic-review`; "gemba walk" is now the name of the review's skill-evaluation phase. P6b is that merge.

**L1 — producer-named artifact layout (operator ruling 2026-09-24).** Artifacts created by skills and MCP servers are organized clearly and transparently in the data folder (`zk-data` in this install): `{server}-mcp/{type}/`, `skills/{skill}/{date}-{run}/` with a manifest. The Algedonic review board now holds review cards and skill proposals instead of `curator/` proposal/review folders. MCP writes into the tree are confined to the writing server's own folder. Existing corpus runs stay in place and are indexed in `zk-data/INDEX.md` (operator option A: their manifests embed absolute paths under SHA-256 seals). Specified in `standardized-artifact-storage.md` §0.

**N1 — skill consolidation review (operator request 2026-09-24), after P6 and L1:** apply the deletion test across the skill corpus and bring a ranked merge proposal for operator decision; no merge without approval. **Decided 2026-09-24** (historical proposal file retired with the file-backed review worklist): delete `goal-analysis` (superseded by the native `kanban_goal_*` tools; its judge moved to `company-research/thesis-judge`); merge `skill-router` into `skill-discovery` as its route phase (the router and discovery-search templates computed the same 0.50/0.25/0.25 fit score); `self-improvement` renders `kata-improvement` steps 1–3 instead of its own `si-kata-*` copies, and Kata targets are bounded in experiments, not calendar time; `verification-compression` stays a separate skill (operator: no); company-research deep/flash, kata-improvement/kata-coaching and hypothesis-framer/falsifiability stay separate.

**Stop criteria:** P6 is complete when P6a–P6e checkpoints are observed, a live Curator session shows `record_skill_feedback` while a native session does not, and one algedonic review with the operator records at least one skill verdict through the new phase. A structurally green corpus is not completion.

### P7 — Editor responsiveness: remove measured foreground loops

**Operator requirement (2026-09-23/24):** eliminate roughly five-second existing-thread opens, sustained lag while threads run, and spontaneous force-quit prompts. Only live, user-visible measurements count as evidence. No backward compatibility is required; prefer removing fork divergence over adding code (take away, never add).

**Measured live (2026-09-24, build `a4434afc`, one process and project):** the foreground thread ran at about 100% of one core (about 495 of 500 ticks per 5 s) whether or not threads ran; draw took 3.1–3.9 s per 5 s at 70–160 frames; about 1,000 model-picker notifies and about 12 model-discovery completions per 5 s; 420–730 cursor-blink ticks per 5 s; one running-thread spinner requested about 520 animation frames per 5 s. Six existing-thread opens took 0.7–2.2 s from construction to first frame, dominated by load (0.54–2.0 s), not rendering (17–99 ms).

| Slice | Change (no compatibility path) | Checkpoint (falsifiable) |
| --- | --- | --- |
| P7a — discovery loop (D76) | Discovery success notifies only its own entity; providers subscribe the registry to it | `successful_discovery_does_not_restart_itself`: 998 `/models` requests without the fix, 1 with it; live discovery completions drop to 0 per 5 s after startup |
| P7b — retire D15 | `crates/editor/src/blink_manager.rs` restored byte-identical to upstream; its fork-only settings handler re-enabled blinking in unfocused editors | Upstream retirement probe: blurred editor 0 blink notifies per 5 s after settings updates; live blink ticks drop to about 10 per 5 s per focused editor |
| P7c — remeasure | Same process shape, same project, idle and running | Foreground ticks, draw time, notify-caller and animation readings before and after P7a/P7b; probes removed only after the reading |
| P7d — thread open | Decompose the 0.5–2.0 s load phase (threads-DB lock, deserialization, replay) with the existing open probe before changing code | Click-to-first-frame for the same slow thread, before and after |

**Not yet explained:** whether draw cost and the running-thread spinner remain dominant once P7a/P7b remove their notify sources, and the force-quit prompts (no hang over 100 ms was logged in the measured interval).

**P7c observed (2026-09-24 15:47–15:49, build `f310bb05` at `067d836a89`, same project):**

| Reading per 5 s | Before (build `a4434afc`) | After, quiet | After, threads running |
| --- | --- | --- | --- |
| Foreground ticks (of about 500) | 481–499 | 100–168 | 226–352 |
| Draw frames / draw time | 70–160 / 3.1–4.1 s | 88–114 / 1.1–1.3 s | 358–779 / 1.8–2.7 s |
| Foreground runnables time | 0.7–1.5 s | 19–21 ms | 42–63 ms |
| Model-discovery notifies | about 12 | 0 | 0 |
| Cursor-blink runs | 420–1,317 | 0–19 | up to 55 |
| Running-thread spinner frame requests | about 520 | — | 1,426–1,550 |

The self-sustaining loops are gone: with no thread running the editor no longer pins a core. The remaining cost while threads run is draw work driven by the sidebar's running-thread spinner (`crates/ui/src/components/ai/thread_item.rs:346`, upstream code unchanged), which requests a frame every frame and redraws the window. Three opens on this build: 963 entries in 670 ms (load 635 ms), 381 entries in 90 ms, empty in 12 ms, versus 0.7–2.2 s before. The probes were removed after this reading.

**Next (P7e):** measure whether bounding the spinner's frame rate (the existing `Animation::max_fps` facility) reduces running-thread draw cost, before changing upstream code; decompose the remaining 635 ms load for the largest thread under P7d.

**P7e applied (2026-09-28, D84; falsified and escalated 2026-09-29):** before any draw-cost measurement, the live reproduction was strengthened — the GPUI main thread measured 100% of a core over a 5 s jiffy sample during an active turn (500/500), including tool-execution gaps with no tokens streaming, on the user's release build. A first cap (rotate spinners only, 20 fps via `with_max_fps` in `crates/ui/src/traits/animation_ext.rs`) was built and committed — and **falsified by measurement**: on the rebuilt binary the main thread still measured 96% of a core during active turns (same 5 s method, two samples, 2026-09-29). The generating-title pulsator in the same `thread_item.rs` row and every other uncapped `repeat()` loop kept the frame rate at display refresh. The cap therefore moved to the choke point: `Animation::repeat()`/`repeat_synced()` in `crates/gpui/src/elements/animation.rs` now default `max_fps` to `DEFAULT_REPEAT_MAX_FPS` (20 fps, timer-driven), covering every looping indicator in the tree; `animation_ext.rs` is restored byte-identical to upstream. Alongside it, `MaxIdleRetainedThreads` is raised 5 → 16 via the GPUI global in `crates/zed/src/main.rs` (upstream's own override point), so switching among recent idle threads no longer re-pays the DB load + per-entry replay (the 635 ms P7c reading). Pins: `repeating_animations_default_to_capped_redraw` + the updated `test_repeating_animation_schedules_animation_frames` (gpui), `kask_idle_thread_retention_override_is_set_after_agent_ui_init` (zed). **Still open:** the after-measurement on the choke-point build (if the main thread still pins, the animation family is eliminated entirely and the per-frame cost decomposition via the built-in profiler — `instrumentation.performance_profiler.enabled` — becomes mandatory), the P7d load decomposition for first-open of a large thread (retention only removes re-loads, not first loads).

**P7f forward program (2026-09-30; the six-skill review's plan, recorded here because the docs-count cap forbids a new plan file):**

*Current condition (measured):* draw rate fixed (17–21/s, one drawing window — per-window `ui frame health` counters prove the other agent windows draw nothing on Wayland); draw cost 20–38 ms avg on a quiet machine (healthy 2–7 ms) and 83–128 ms during 16-job builds (spikes 388 ms); streaming events ~3 ms × up to 40/s per agent (the parallel `[DIAG-anr]` lane); main thread 96% during active turns.

*The generalized issue — the uncoordinated-multiplier antipattern:* a shared capacity-limited resource (main thread, memory bandwidth, a DB) consumed by N independent actors validated in isolation, multiplying without coordination, invisible because measurement aggregates across actors. Inflation = per-operation cost grows with ambient load (builds → 3–4× draws). Collision = N actors independently trigger the resource (N timers, double pollers, two editor processes on one DB).

*Instance sweep:* A1 per-thread reveal timers — FIXED (D14 grid). A2 capped animation timers unsynchronized — FIXED 2026-09-30 (plan step 3.1: the grid math moved to `gpui::frame_grid`, consumed by both the reveal loop and the `AnimationElement` max_fps path; pins `grid_delay_lands_on_shared_boundaries` + the grid-updated `test_max_fps_schedules_timer_driven_frames`). B observer fan-out (per-event cost × windows) — suspected, F1 measures. C build inflation — correlation confirmed, mechanism unresolved (bandwidth vs file-event storm), step 1.2 discriminates. D1 aggregate counters — FIXED (per-window). D2 kanban double-poll (panel refresh + regulation connector) — suspected, F2. E cross-process DB collision — documented (D8), F3. G blink timers — suspected, F4. H file events × windows — suspected, F5.

*Remaining steps:* 1.1 per-window attribution — DONE (the focused window owns the cost; split ~11 ms conversation / ~15 ms editor+chrome, sidebar ≈ 0). 1.2 build-inflation mechanism discrimination from existing log lines. 2.1 build hygiene (`-j 8`/`nice`) — workflow relief. 3.2 expensive-element fix — GATED on the empty-tab differential (editor content vs chrome) and per-pane data; no speculative optimization. 3.3 P7d first-open replay decomposition — carried. Follow-ups F1–F5 are flagged with their discriminating measurements and are promoted to work only when evidence arrives.

**P7g diagnostic consolidation (2026-10-01; the two-symptom investigation — thread-open latency + input-tracking freeze):**

*Measured (per-window `ui frame health` counters, the live `[DIAG-anr]` event probe, `ps`/load):* on a fresh post-reboot process, quiet machine (load ≈ 2.2), with one heavy conversation streaming, draw cost held at 23.2–50.4 ms avg (max 50–160 ms) at 220–897 draws/30 s — against the 2.3–7.5 ms healthy baseline (P7c, 2026-09-24). Same day, pre-reboot: 24–38 ms avg with agent activity on a quiet machine (two windows: 24–38 / 17–21), 66–115 ms during the heaviest conversation content (load ≈ 3.8, no build), and the P7f 83–128 ms band during a 16-job build (load 19.6, a second clone compiling). `[DIAG-anr]`: 1,319–2,370 events per window at 3.6–4.5 ms avg (max 16.9 ms) — busy 22–66% of the window; H2 (single-event hog) eliminated, H3 (uniformly expensive events) confirmed. threads.db = 338 MB; the 13-server MCP fleet idles at 0.0–0.4% CPU each (eliminated as a CPU factor).

*Confirmed:* the input freeze is **cost-driven, not rate-driven** — the D84/D14/frame-grid caps hold (idle windows draw 0; active 7–30/s) while per-draw cost runs 3–15× baseline and scales with conversation size (23–50 ms fresh/quiet/heavy vs 24–38 ms light content). Eliminated as causes: mimalloc (removed 2026-09-30; the no-mimalloc binary reads the same), the 2026-09-24 upstream sync (verified `git merge-base --is-ancestor`: merge `d2f29c3827` at 07:20 predates the 15:47 baseline build), long-session allocator ratchet (a 10-minute-old process already reads 23–50 ms), build inflation as the baseline cause (real as a 3–4× multiplier only), DB scan on thread open (PK lookup, `db.rs:620`), MCP startup on open (not on the path), summary regeneration on open/list (call-site sweep: `thread_summary` fires only on @-mention / the summary flow / title regeneration). Thread-open latency: mechanism chain code-verified — `open_thread` (`agent.rs:1889`) → blob load + zstd decompress + full-thread parse (`db.rs:615`, background) → `from_db` (`thread.rs:2031`) → foreground `replay` (`thread.rs:1806`) + per-event forwarding (`agent.rs:2505`, the same 3–4.5 ms/event path) — matching the 2026-09-24 probe (0.54–2.0 s load-dominated opens; 635 ms of 670 for a 963-entry thread). The loader is upstream-parity code; the fork's divergence is thread *size* (338 MB DB, whole-thread-per-row blobs re-serialized on every save) and the fork is ahead of upstream on re-opens (retention 16 vs 5).

*Open:* the per-draw cost decomposition (conversation ≈ 11 ms / editor+chrome ≈ 15 ms at the P7f attribution; the content scaling beyond that unexplained) — the built-in profiler is the mandated instrument and has not been run; the P7d phase split (DB parse vs replay vs forwarding) post-D73; connection-mutex contention (`db.rs:489`, one shared connection — a whole-thread blob INSERT at turn end can queue a concurrent open's read behind it).

*Next steps (ranked):* (1) profiler decomposition — operator checklist: set `instrumentation.performance_profiler.enabled: true` in settings, restart, run `zed: open performance profiler` from the command palette, and capture while typing in a heavy conversation, in an empty tab, and during a build; gates the expensive-element fix (3.2). (2) P7d re-run with a bounded open probe, then fix the dominant term (batched replay-forwarding in `agent.rs`, or message-level storage if parse dominates — upstream-reportable). (3) Build hygiene for parallel-clone builds (`HKASK_BUILD_JOBS` / `nice`). (4) A second read-only threads.db connection so opens never queue behind saves. (5) Event-forwarding coalescing at the reveal-grid boundary if the profiler shows observer fan-out (F1). (6) `MALLOC_ARENA_MAX` A/B — weakened as a draw-cost cause by the fresh-process reading; keep as the RSS test. (7) Remove `[DIAG-anr]` with whichever event-path fix lands.

**P7h — startup decomposition (2026-10-02; the "massively slow startup" report on the rebuilt binary):**

*Measured (log timeline, 06:23:23 process start → 06:25:57 fleet stable):* the MCP fleet launch is a restart storm, not slow launching. The deferred task starts a no-op inference IPC server at 06:23:26 (no default model yet — provider credentials still loading) binding `kask-inference-{pid}-{nonce}.sock` (`inference_ipc_server.rs:588-595`; the nonce makes every start unique, pinned by `generate_socket_path_produces_unique_paths`); the first MCP wave (portfolio, companies, corpus, curator, kata-kanban) launches 06:23:26–28 with that socket in env; 25 s later the registry fires (DefaultModelChanged/ProviderStateChanged) → the re-wire (`main.rs:2514-2560`) starts a SECOND IPC server at a NEW nonce path (06:23:51) → `set_inference_socket_path` + both sync paths + the settings observer re-fire (16 "Kask MCP sync fired" in the window) → every child holding the old socket path is killed and respawned: **30 restart events** — portfolio 4× in 13 s (06:23:51, :52, :57, 06:24:04), companies 2× (49 s apart), corpus 2× (8 s), curator 2× (2 s), kata-kanban 1× — the env-diff actors re-fire inside the window where the restarted server's baseline has not yet been recorded, so the same diff restarts again. Later-wave servers (research, scenarios, prediction-markets, swarm, training, media, spreadsheet, evolution — launched 06:23:56–06:25:57 against the new socket) never restart. Fleet stabilization: ~2.5 min; each spawn pays process start + rmcp handshake + SQLCipher DB open (corpus 898 MB, curator 198 MB, threads 338 MB across the fleet). Draw cost during the churn: 56.9 ms avg (vs 23.6 ms in the first 30 s). Also in-window: a 5.9 s background hang (`crates/client/src/client.rs:1510`, the force-quit family) and one "Error in run turn: no language model configured" (a thread running inside the 25 s unconfigured window).

*Eliminated:* skill/template seeding (content-aware — only 10 of 275 templates rewritten at this boot, the ones the rebuild changed); extension loading (one 47-extension load; the other ~100 "extension" lines are rmcp handshake log noise); LSP storms (none in window); MCP fleet CPU after stabilization (0.0–0.4% each); build inflation (no build running at the time).

*Root cause (verified end-to-end):* the inference socket path is per-server-instance (nanosecond nonce), so the no-op→real port transition is an env CHANGE — restart-by-design — instead of a port swap behind one stable socket; and the restart path has multiple independent actors (two sync fns, the SettingsStore observer, the wiring function's internal re-syncs) diffing against a shared baseline that is updated only after a restart records its env, racing each other (the 4× portfolio loop).

*Fix plan (ranked):* (1) **one stable socket per editor process + swappable port** — bind once (pid-keyed path), store the serving port behind a process-global swappable slot next to `INFERENCE_SOCKET_PATH` (`inference_socket.rs`), and make the re-wire swap the port instead of starting a second server; `HKASK_INFERENCE_SOCKET` then never changes after first set → zero restarts at model resolution; the uniqueness pin is replaced by a same-process-reuse pin plus a re-wire-does-not-change-env pin. kask_bridge-side (D8/D9 seam). (2) **Single restart authority + baseline recorded at restart initiation** — so any legitimate env change (credential rotation, the D9 nudge) restarts once, not 2–4×. (3) The 25 s no-op→real gap (provider credential loading — Secret Service latency; D9's unverified comparison) becomes harmless with (1); chase separately only if it persists as a user-visible delay. (4) The `client.rs:1510` 5.9 s hang is a separate item in the force-quit family.

**P7h applied (2026-10-02, same day):** fixes (1) and (2) landed together. (1) `generate_socket_path` is pid-keyed (nonce dropped); the serving backend (inference/embedding/tool ports) lives behind `IpcBackendSlot` (`inference_ipc_server.rs`), swapped in place by `kask_bridge::swap_inference_ipc_ports`; `wire_kask_inference_stack` is swap-first with fresh-start fallback; the no-op start publishes `HKASK_INFERENCE_TIMEOUT_SECS` from the same setting so both env keys are stable across the swap. (2) `sync_kask_mcp_runtime_servers` records the baseline at initiation (decision loop) and removes it on failure — the launch loop's record-before-start discipline. Pins: `generate_socket_path_is_stable_within_a_process`, `backend_slot_swap_replaces_serving_ports_in_place`, `swap_inference_ipc_ports_without_a_server_returns_false_and_keeps_the_socket_path` (kask_bridge), `kask_restart_baseline_is_recorded_at_initiation` (zed). DIVERGENCE: D8 (stable socket + swappable backend) + D45 (baseline at initiation). Validation: `cargo check -p kask_bridge` / `-p zed` clean; kask_bridge lib 260/260; the four zed pins green. **Verification pending on a rebuilt binary:** the startup log should show one `IPC server started (no-op port)` line, one `ports swapped in place` line at model resolution, zero `env changed — restarting` lines attributable to `HKASK_INFERENCE_SOCKET`/`_TIMEOUT_SECS`, and fleet registration completing in the first launch pass.

### Kata experiment table

| Order | Current condition | Target | Next experiment | Success criterion |
| --- | --- | --- | --- | --- |
| 1 | Worktree branch bypasses tool grants; queued work survives disconnect | Authorized, cancellable admission | Fake spawner with empty/revoked grants and disconnected request B | Zero unauthorized or cancelled-before-start effects |
| 2 | Managed wait can outlive Stop | Predictable interruption with truthful outcome | Never-resolving source; then commit-before-cancel | Stop meets local bound; zero automatic replays |
| 3 | Model text enters shell source | Literal data only | Hostile-string local generator harness | Exact data round-trip; no marker execution |
| 4 | File/path policies incompletely enforced | Complete containment and mode enforcement | Symlink fixtures and gallery-mode matrix | Outside bytes unchanged; forbidden deletes refused |
| 5 | Callers collapse into server identity | Distinct owner/actor attribution | Two-actor kanban workflow | Distinct records; input cannot spoof identity |
| 6 | Retry rules differ by transport | One delivery-state rule | Commit then drop response | Exactly one mutation on each path |
| 7 | Installer/registry differ; fixture suite skipped | Complete fleet and real boundary verification | Inventory check and explicit CI fixture job | Equal sets and nonzero executed fixture tests |
| 8 | Receipt/revision crash states incompletely bounded | Truthful recovery | Fault injection at publication boundaries | No false unapplied result or automatic replay |
| 9 | Documentation repeats stale facts | Checked inventory and links | Registry-derived facts plus link checks | No dangling contracts or false enforcement claims |

Suggested first target: P0 and the P1 experiments within one week of authorized execution, adjusted to actual test feedback. After each experiment record prediction, observation, and next obstacle; revise the theory if the test disproves it. No improvement metric is recorded as achieved before the change and test exist.

### P8 — Evolution reference model and program (2026-09-30; recorded here because the docs-count cap forbids a new plan file)

**Provenance.** Composed 2026-09-30 from the David Ha research run (research run `9c47de77a42b8ed8`, completed) — insight 3: evolution and population-based search as a first-class optimizer with small, structurally interpretable controllers; insight 4: the improve loop as itself an automatable, evolvable system — plus same-day tree probes: the GEPA sub-loop census, the experiment-template census, and the registered-MCP-surface census. Operator decisions locked 2026-09-30 (§P8.7). **Promotion path:** split into `research/evolution-reference-model.md` at the next docs condensation (the tree sits at 74 files under the formal fewer-than-75 count gate, with a working 60-file target).

#### P8.1 The design pattern: vary locally, select centrally, retain durably

The capability to evolve is already sharded across zed-kask's layers and stays sharded: each layer's genotype medium differs, so each layer owns its variation operators. What is shared is the ledger (comparable fitness and selection records), the selection surface (one human-chaired gate), and this named pattern.

| Element | In zed-kask |
| --- | --- |
| Genotype (small, legible, versioned) | SKILL.md **plus its `.j2` templates as one composite genotype** (§P8.7-Q2); agent cards (system_prompt, model_params, evaluators); MCP tool schemas; regulation thresholds; LoRA adapters; goal/task definitions. Structure over weights: prefer evolving text/config to fine-tuning when both could serve — cheaper, reviewable, revertible. |
| Phenotype | A running session/agent/server expressing a genotype configuration. Fitness is measured on phenotypes; selection edits genotypes. |
| Variation (sharded) | Mutation (GEPA prompt evolution; skill edits; `evolve_mcp_tool_schema` directives), recombination (adapter merging under merge contracts; skill bundling; roster composition), continuous calibration (thresholds — the scalar ES analog). |
| Fitness (grounded only) | Deterministic evaluators, verifier gates, Brier scores on resolved outcomes, calibration readings, algedonic verdicts. Never LLM-only (the anti-gaming rule). Recorded runs only — never simulated (the GEPA house rule). |
| Selection (central, human-chaired) | Automated pre-selection only within declared noise bands (GEPA's 10% cost band is the precedent); final selection at the algedonic review (§P8.7-Q4) or a recorded grant. Nothing auto-adopts. |
| Retention & lineage | Git is the germline (proposal → delegated implementation → verification). The experiment registry is the fossil record, including rejected variants with reasons. Curator memory holds acquired traits, feeding variation through therapy's reification (the Baldwin channel). |

Two zed-kask-specific syntheses:

1. **The pre-registered, Brier-scored prediction is the tiny controller.** The experiment machinery (swarm rollouts, eval harnesses, training jobs) is the big shared commodity part; the hypothesis plus prediction is the small, legible, evolvable part; the Brier score is the selection signal on the experimenter, not just the artifact. This is the advantage over the source setting: Ha's fitness functions are external benchmarks; this system scores its own hypotheses.
2. **D/P labelling is the legibility constraint that makes variants selectable.** The variant carries its own deterministic gates, declared evaluators, and lineage — it explains itself, so selection is auditable.

#### P8.2 Current state — the shard map (probe evidence, 2026-09-30)

| Layer | Genotype | Variation today | Fitness today | Gap |
| --- | --- | --- | --- | --- |
| Prompt/template | system prompts, `.j2` | GEPA sub-loop (mutation+crossover, Pareto frontier, pinned D forms) | `swarm_eval_agent_local` on disjoint feedback/selection sets; 10% cost noise band | "Prompts only (v1)"; no cross-run lineage |
| Weights (FM) | LoRA adapters | retrain mode; verdict-bridged rollouts (SFT/DPO) | held-out eval; A/B on job completion | runs not registered or comparable |
| Skill | SKILL.md + templates | skill-maintenance edits; therapy reification | fixed-task comparisons; pin tests | ad hoc A/Bs, no records |
| Agent/swarm | agent cards | reconfigure; roster composition | `swarm_eval_agent_local`; task boards | no variant populations or retention |
| Tool schema | MCP schemas | `evolve_mcp_tool_schema` directive | skill-use reports | fitness loop thin (issues, not outcomes) |
| Regulation scalars | thresholds, budgets | `calibrate_threshold` directives | variety deficit, Brier, calibration readings | calibration by judgment, not recorded fitness |
| Memory | h_mems, rules | insert/update; therapy | recall/decay, contradictions | acquired vs heritable not distinguished |

Evidence notes:

- GEPA sub-loop: `.agents/skills/self-improvement/SKILL.md` ("Prompt evolution (GEPA)" section; templates `gpa-sample-trajectories.j2`, `gpa-reflect.j2`, `gpa-propose-mutations.j2`, `gpa-test-variants.j2`, `gpa-frontier-update.j2`); dominance and convergence forms executed live through `lisp_eval` (receipts in the skill-audit records). Scoped "Prompts only (v1)"; disjoint feedback/selection sets; recorded runs only.
- Experiment-concept shards (each skill its own notion, no shared record): `kask/registry/templates/eqm/eqm-imp-experiment.j2`, `kask/registry/templates/kata-improvement/coaching-q4-experiment.j2`, `kask/registry/templates/kata-improvement/improvement-step4-experiment.j2`, `kask/registry/templates/metacognition/meta-experiment.j2`, `kask/registry/templates/verification-compression/experiment.j2`.
- No evolution/experiment MCP server registered (list_mcp_tools census, 2026-09-30).
- GEPA run logs persist per-run at `~/Documents/zk-data/skills/self-improvement/gepa/{date}-{run}/` — directories, not a queryable registry.

#### P8.3 The generalized experiment protocol

Generalizes GEPA (prompts-only v1) to the artifact classes. **The skill genotype is composite** (§P8.7-Q2): SKILL.md process text and its `.j2` template resources evolve together; a skill experiment's variants are (SKILL.md, templates) pairs, and pin suites re-run in the same change.

| Artifact class | Eval-set requirement | Fitness | Noise band / gates |
| --- | --- | --- | --- |
| Skill (composite: SKILL.md + templates) | Fixed task set plus the skill's pin tests | Evaluator pass rates on the fixed set; pin suites green | Pin-suite gate mandatory; D/P labels preserved |
| Agent card | Fixed task set with declared evaluators | `swarm_eval_agent_local` pass_rate, total_tokens | 10% cost band (GEPA precedent) |
| Standalone prompt/template | Runnable eval set, disjoint feedback/selection | Eval-set scores | GEPA dominance/convergence forms |
| Tool schema | Skill-use reports over a window | Issue counts, resolution outcomes | Curator directive ledger |
| Regulation scalar | Calibration/Brier records over a window | Calibration delta | Curator thresholds |
| LoRA adapter | Held-out task set | `training_evaluate` scores | lora-training gates G-M1..G-Q5 |

Protocol steps, every experiment:

1. **Declare** — hypothesis, genotype config, eval set, fitness function, noise band, pre-registered prediction with confidence, energy budget → `experiment_propose` (creates the linked kanban goal; the prediction is Brier-scored at verdict).
2. **Vary** — mutation/crossover per the layer's operator; each variant registered with its parent (lineage).
3. **Test** — fitness from recorded runs only; feedback and selection sets disjoint wherever the operator is a model. **Headroom pre-check first (§P8.7-Q6):** the baseline variant runs alone before any challenger spend — a baseline at 1.0 (100% of the test set) voids the experiment at design time as `rejected` with a no-headroom reason, and the eval set is redesigned (harder discriminators, never weaker evaluators) and re-declared; the pre-check is budgeted auto-run (§P8.7-Q7).
4. **Select** — deterministic pre-selection within noise bands; final selection at the algedonic review (§P8.7-Q4). Deterministic-fitness experiments auto-run within the declared budget (§P8.7-Q1); human-judged-fitness experiments route through the operator before running.
5. **Retain** — selected variants land in git through the proposal → delegation → verification path (proposes, never commits); rejected variants persist as fossils with reasons; lineage recorded.

#### P8.4 The server: `hkask-mcp-evolution` (§P8.7-Q3 — build now)

A new `hkask-mcp-evolution` crate alongside the existing hkask-mcp-* server crates — the 13th managed MCP server — landing together with the protocol. Tool surface:

| Tool | Record shape (summary) |
| --- | --- |
| `experiment_propose` | hypothesis, layer, genotype_refs, eval_set, fitness_fn, noise_band, prediction, confidence, budget → experiment_id (status: proposed) plus a linked kanban goal |
| `variant_register` | experiment_id, genotype_config (AnyJsonValue), parent_variant_id → variant_id |
| `fitness_record` | experiment_id, variant_id, runs (recorded report refs only), per-objective scores → immutable fitness row |
| `selection_record` | experiment_id, verdict, selected_variant_id, reject_reasons, algedonic_reference → fossil (selected and rejected both retained) |
| `lineage_read` | artifact_ref → ancestry chain with fitness and selection events |
| `population_query` | layer, status, window → variants and frontier state |

Implementation contracts: SQLCipher storage with the canonical passphrase helper (`hkask_mcp_server::server::resolve_db_passphrase`); per-variant error classification (never blanket `internal`); `AnyJsonValue` for genotype configs; `unwrap_tool_envelope`; missing credentials surface as `permission_denied` naming the env var. The registry is a regulated domain (§P8.9, wired 2026-09-30): the per-experiment `budget.max_runs` set point is server-enforced at `fitness_record` (D-1), the cybernetics loop senses stuck experiments through the evolution health source (D-3: 7-day stale set point), and the executing agents' tool calls remain under the existing call-cap meter — the curator's `OverrideEnergyBudget` directive adjusts an agent's ceiling when a cycle needs more or less room. `evolve_mcp_tool_schema` remains the schema-mutation path for the server's own tools. DIVERGENCE.md D-seam entry plus tests in the same change; the MDS 12-server count becomes 13; the `reference/mcp-servers/` registry entry needs a docs slot (see Phase 1). No compatibility surface: the registry is the only record path from day one — no legacy-import path for historical GEPA log directories ships (§P8.7-Q5); historical directories on disk are data, untouched.

#### P8.5 The re-phased program (Q3=B: the server lands with the protocol)

- **Phase 0 — design authority (this section, 2026-09-30).** Pattern, shard map, protocol, server schemas, decisions, re-phased plan. *Acceptance:* this section exists with the decision log complete. *Replaces:* nothing yet — this section is the program's first artifact.
- **Phase 1 — the server and protocol build (replacement-first; §P8.7-Q5).** `hkask-mcp-evolution` per §P8.4; `experiment-protocol.j2` registry template as the **single** experiment-record path; routing edits that replace the old paths in the same change, never running alongside them (**wired 2026-09-30**): the GEPA sub-loop declares and registers before iteration 1, records `fitness_record` after Test, and records `selection_record` at the card verdict (the deciding session writes it — the algedonic verdict is the selection, §P8.7-Q4; a session that files and exits leaves the experiment `running`), and the per-run log-directory retention instructions are deleted from the skill (historical log directories on disk are data, untouched — the mechanism is replaced); the five per-skill experiment templates (`eqm/eqm-imp-experiment.j2`, `kata-improvement/coaching-q4-experiment.j2`, `kata-improvement/improvement-step4-experiment.j2`, `metacognition/meta-experiment.j2`, `verification-compression/experiment.j2`) are audited in the same change — **audit outcome 2026-09-30: none duplicates the shared record format; the five are domain process renderers (kata PDCA on work obstacles, coaching dialogue, calibration application, EQM rationale rewriting, Lean-compression verdicts) for experiment classes outside the registry's six artifact layers — retained, and the registry governs artifact-variant experiments**; skill-maintenance comparisons route through the protocol (the optimize loop declares via `experiment_propose`, registers candidates as variants, records `fitness_record` from the measured runs, and records the card verdict). Pin-test re-runs (`cargo test -p agent --lib lisp_eval_tool`); D-seam plus tests; MDS 12→13 (direct replacement, no transitional count); the mcp-servers reference doc (needs a docs slot — run the docs condensation first, or fold into the registry README). *Acceptance:* the server builds with tests green; one experiment passes proposed → variants registered → fitness recorded → selection recorded end-to-end through the server's tools; zero superseded record paths remain in the skills. *Replaces:* the GEPA per-run log-directory retention mechanism and the per-skill experiment-record formats — deleted in the same change, not kept as fallbacks.
- **Phase 2 — first scope through the loop (§P8.7-Q2: skills-as-composite plus agent cards).** At least two registered experiments across the first-scope genotypes with pre-registered Brier-scored predictions; one full select/reject with fossil; budgeted auto-run operating per §P8.7-Q1. *Acceptance:* the run-one-through — a skill (SKILL.md plus templates) or card experiment completes the full protocol with a recorded verdict.
- **Phase 3 — the experiment-designer agenda loop.** Extend self-improvement's Plan step into an agenda generator: reads regulation signals (skill-use issues, escalations, variety deficits) plus the registry's fitness/Brier history → prioritized experiment queue as algedonic proposals with pre-registered predictions; delegated runs within budget; proposes, never commits. *Acceptance:* one full cycle (signal → registered experiment → verdict → algedonic decision → retention) completes with human touch only at review; experiment-prediction Brier is computed and reported. *Replaces:* the human assembling experiments by hand from signals. **Wired and first-cycled 2026-09-30:** the si-agenda-generator extension (SKILL.md Plan-step section + `si-agenda-generator.j2`) landed with pins/prescreen/contract audits green; the first cycle ran signal → declared experiment (E3 `exp_694719decaa04494b042bbb5b9769e48`, the E2B fossil's designed follow-up on a §P8.7-Q6-hardened eval set) → headroom pre-check (baseline 5/9 — headroom demonstrated, the gate's first live operation) → challenger (3/9) → rejected fossil `sel_8f5bec05` with a measured reversal (the first non-saturation refutation; prediction Brier 0.25) → review card on Algedonic review. Registry prediction record: 0/3 claims held — the next agenda's confidences calibrate against a demonstrated hit rate of zero.

#### P8.6 Guardrails, meta-metrics, research anchors

**Guardrails** (each tied to a project rule): grounded fitness only — the anti-gaming trio (real sources, load-bearing properties preserved, external ground truth); algedonic selection — nothing auto-adopts; proposes-never-commits; D-seam entries plus tests in the same change; SKILL.md edits re-run pin suites; scripts are bash; new crates declare `[lib] path`; no `mod.rs`; no `unwrap()`; no backward compatibility (§P8.7-Q5) — replace superseded paths directly and delete them in the same change: no deprecated APIs, legacy adapters, compatibility flags, dual writes, or parallel implementations; every phase names its replaces; energy budgets price the loop.

**Meta-metrics:** experiment-prediction Brier (primary — the system learning to predict its own improvements; computed from the registry, reported at algedonic review); retention rate (selected variants still active after N weeks); throughput (proposals → registered → completed → selected/rejected per cycle); variety (genotype distribution in active use — the curator's variety-deficit metric extended to evolution).

**Research anchors** (all retrieved in research run `9c47de77a42b8ed8`): population search over hill-climbing — EvoJAX (Tang, Tian & Ha, 2022); Recurrent World Models (Ha & Schmidhuber, 2018). Small legible controllers — Weight Agnostic Neural Networks (Gaier & Ha, 2019); World Models (Ha & Schmidhuber, 2018). Structural interpretability — Neuroevolution of Self-Interpretable Agents (Tang, Nguyen & Ha, 2020). The loop as the target — The AI Scientist (Lu et al., 2024); The AI Scientist-v2 (Yamada et al., 2025); Towards end-to-end automation of AI research (Yamada et al., Nature, 2026). Recombination — Evolutionary optimization of model merging recipes (Akiba et al., 2024). Adversarial and open-ended search — Digital Red Queen (Kumar et al., 2026); Automating the Search for Artificial Life with Foundation Models (Kumar et al., 2024).

#### P8.7 Decision log (operator rulings, 2026-09-30)

| Decision | Ruling | Consequence |
| --- | --- | --- |
| Q0 — docs slot | Not answered in the locking reply; proceeded with the host-as-section recommendation (this section, per the P7f precedent) to keep the locked decisions moving. Promotion path recorded above; an operator override (cap raise or condensation-first) splits this section into `research/evolution-reference-model.md` at any time. | The reference model is durable today; standalone-file promotion pending a slot. |
| Q1 — loop autonomy | **b — budgeted auto-run.** Deterministic-fitness experiments auto-run within a declared energy budget; the operator chairs selection at the algedonic review with itemized spends; human-judged-fitness experiments route through the operator before running. | The loop runs between reviews; the operator keeps the selection chair and the budget lever. |
| Q2 — first scope | **a+b — skills and templates together, plus agent cards.** The skill genotype is composite: SKILL.md and its `.j2` templates evolve as one unit ("you need to evolve the skill.md with the jinja2 templates" — operator, 2026-09-30). | Skill experiments mutate process text and templates in one variant; pin suites re-run in the same change. |
| Q3 — server shape | **B — build `hkask-mcp-evolution` now.** Demand and requirement are proven by the existing GEPA machinery and the active work thread (operator, 2026-09-30); the server lands together with the protocol. | Phase 1 builds the 13th managed server; the program is re-phased around its schema (§P8.5). |
| Q4 — human gate | **A — algedonic review as the single selection surface.** | One board, one fossil record; selection concentrates in the existing gemba walk. |
| Q5 — compatibility posture | **No backward compatibility — pre-release** (operator, 2026-09-30; the plan-level §1 ruling applied to this program). Replace superseded paths directly, update all callers together, delete old paths in the same change; no deprecated APIs, legacy adapters, compatibility flags, dual writes, or parallel implementations; no legacy-import surface in the server. | Phase 1 is replacement-first: the GEPA retention mechanism and per-skill record formats are deleted as the registry lands, not kept as fallbacks. |
| Q6 — eval-set headroom | **A — baseline headroom pre-check** (operator, 2026-09-30). Before any challenger spend, the baseline variant runs alone; a baseline pass rate of 1.0 (100% of the test set) voids the experiment at design time as `rejected` with a no-headroom reason, and the eval set is redesigned — harder discriminators (conservation checks, exact labels), never weaker evaluators — before re-declaring. | Saturation discovery costs one variant's spend, not two (the trap was observed twice in Phase 2, §P8.8); wired into §P8.3 step 3 and the Phase 3 agenda generator. |
| Q7 — pre-check autonomy | **A — the pre-check is budgeted auto-run** (operator, 2026-09-30). The baseline-only measurement is deterministic-fitness work inside the declared budget under the Q1=b ruling; the operator chairs selection, not baseline measurement. | The Phase 3 agenda loop self-screens its queue; the operator reviews only screened experiments. |

#### P8.8 Phase 2 outcome record (2026-09-30 — first real experiments)

Two first-scope experiments ran end-to-end through the registry; both resolved **rejected** on measured refutation — the saturated-baseline trap, observed twice in the first generation.

| Experiment | Layer | Variants | Measured result | Fossil |
| --- | --- | --- | --- | --- |
| E2A `exp_60c11d32269f4121b05f3806b0a2b2fa` | agent_card | temp 0.7 baseline vs temp 0.2 challenger; 3 exact-reply tasks × 4 repeats | both 12/12 (pass_rate 1.0); cost tie 918 vs 920 tokens (inside the 10% band) | `sel_5eb85ad924934a8bbcd28bd97657d260` rejected |
| E2B `exp_6dfe3e8e8bdd4ab8b67293e14943dfaf` | skill (composite) | shipped sankey-flow body vs body + output-contract recap; 3 sankey tasks × 3 repeats | both 9/9 (pass_rate 1.0); challenger ~11% cheaper (67,311 vs 75,845 tokens) — post-hoc observation, not the registered claim | `sel_f943800e277d4e73b8e7a3cffd55df1e` rejected |

Prediction calibration: E2A confidence 0.6 → Brier 0.36; E2B confidence 0.55 → Brier 0.3025 (both claims refuted). Fitness records `fit_bc5a9a24ca854574a4a33d542bfdaffd`, `fit_fe38ac3c8e0a476294c79291cefcb85d`, `fit_07a66de4f6914d46b350b5441a2852fe`, `fit_4813238f4db347c39a0244db0b598eb2` reference harness reports and genotype card snapshots saved under `~/Documents/zk-data/evolution-mcp/reports/`; the four temp cards were removed after the runs.

**Protocol finding — ruled 2026-09-30 (§P8.7-Q6/Q7).** Both refutations share one root cause: the eval sets had no headroom — each baseline saturated at 1.0, so no challenger could demonstrate the predicted improvement (the failure mode the self-improvement gate already documents). The ruled gate (wired into §P8.3 step 3 and the Phase 3 experiment-designer): a **baseline headroom pre-check** — run the baseline first; if its fitness is at ceiling (100% of the test set), void the experiment at design time as `rejected` with a no-headroom reason and redesign the eval set (harder discriminators — conservation checks, exact node labels — never weaker evaluators) before spending challenger budget. This converts the saturation discovery from a two-variant spend into a one-variant spend; the pre-check itself is budgeted auto-run (§P8.7-Q7).

**Operational note.** A `swarm_eval_agent_local` call whose runtime exceeds the 300 s MCP reply window loses only its reply — the runs complete server-side and are fully recoverable from the swarm events DB (`verdict` and `harness_summary` events carry pass/fail, task/repeat indices, harness IDs, and token usage). E2B's baseline was recovered this way after its 9-run call lost its reply; the events-DB recovery path is the canonical fallback for slow eval sets, with per-task calls as the within-window alternative.

#### P8.9 Connecting the registry to the three-layer loop system (CNS reference model)

The registry is already an expectation-carriage machine in CNS terms (`kask/docs/research/cybernetic-nervous-system-reference-model.md`): every experiment stores its expectation before measurement (the pre-registered prediction — INV2), records the observation (fitness from recorded runs), and propagates the delta (claim held/refuted; Brier — the prediction-error signal). Its reporting is surprise-gated by construction (INV3): the headroom pre-check voids eval sets that cannot produce prediction error, and the agenda generator turns regulation deviations — the system's own prediction errors — into candidates. Selection is model revision at the top (INV5): a selected variant changes an artifact, which changes what Layers A and B expect; a rejected variant persists as a fossil so the model remembers. The gaps below are the unwired pathways.

**Connection map** (registry element ↔ CNS layer ↔ pathway ↔ status):

| Registry element | Layer (loop) | Pathway | Status |
| --- | --- | --- | --- |
| Agenda signal reads (`curator_semantic_search`, `curator_algedonic_log`, `curator_status`, `population_query`) | B, sensing A and C (L2, L16) | afferent into B | wired (Phase 3) |
| `experiment_propose` / `variant_register` / `fitness_record` / `selection_record` from sessions | B (L1 tool dispatch → L4 server) | efferent (in-thread actuation) | wired (Phases 1–3; enters the session surface at the next editor restart, retiring the stdio helper) |
| Headroom pre-check (§P8.7-Q6) | B | INV2/INV3 gate | wired (E3 — first live operation) |
| Algedonic review card → operator verdict → `selection_record` | C (L16) | efferent (authority down) | wired (Q4; E3 card `435b33f7`) |
| Goal-loop Brier scoring of experiment predictions; calibrated confidences descend to the next agenda | C (L9 + L10) | afferent outcome up, efferent calibration down | wired (0/3 record feeds the agenda template) |
| Regulation loop sensing of the evolution server's experiment health | A (L2) | afferent | wired (2026-09-30, D87 — `EvolutionStuckExperiments` sensor + bridge source over `EvolutionStore::health_snapshot()`; live advisory is launch-gated) |
| The declared budget as a Layer-A set point with local actuation | A (L2) | efferent | wired (D-1: `budget.max_runs` required at declaration, server-enforced at `fitness_record` with typed `BudgetExhausted`) |
| Discarded-signal persistence and recurring-discard escalation | C (L16) | INV4 | wired (skill text step 5: every discard persists in the cycle card; a two-cycle recurring discard escalates as its own algedonic card) |
| Selection → adoption → artifact-version join | C → A/B | INV5 efferent | wired as the documented path (card commit hash → `algedonic_reference` → `lineage_read`); E5 is the first selected experiment (sel_85d477fc, 2026-10-01) — the join completes when the operator approves adoption and the commit hash lands on card 19eeea47 |
| Curator energy budget regulating evolution spend (§P8.4 intent) | A + C (L2) | efferent | wired (D-2: `max_runs` set point + existing call-cap meter + health sensor; curator status surfaces it) |

**Next steps (phased; 1–2 first — every later step reports through them):**

1. **Layer-A afferent — the `reg.outcome.evolution` sensor span.** The regulation cycle reads the registry each cycle: experiments by status, declared-vs-recorded run counts, rolling resolved-claim Brier, no-headroom void count. Set points: budget-utilization ceiling; stale-running age (an experiment `running` past the set point is a deviation → advisory → L16 escalation). *Acceptance:* the span appears in the regulation outcome; an injected stale-running experiment produces a visible advisory; pinned by a regulation test.
2. **Layer-A efferent — server-side budget accounting.** The budget field gains a machine-readable `max_runs` (direct replacement of the free-text convention — no compatibility surface, §P8.7-Q5); `fitness_record` counts recorded runs against it and fails with a typed `budget_exhausted` error naming the experiment and the ceiling — never a silent overshoot. *Acceptance:* a test pins the typed refusal at the boundary. **Ruled 2026-09-30 (D-1): enforce at the server** — wired: `budget.max_runs` required at declaration (typed `BudgetMissingMaxRuns` refusal otherwise); `fitness_record` carries `run_count` (the rollouts its reports cover) and the store refuses past the ceiling with `BudgetExhausted` (failed_precondition); exactly-at-ceiling passes. Pinned by store + tool-seam tests.
3. **INV4 closure — discarded-signal persistence.** The agenda protocol requires every discarded signal in the cycle card (E3 did this de facto; make it protocol), and a signal discarded in N consecutive cycles escalates as its own algedonic card — a recurring discard is a model gap, not noise. *Acceptance:* the next cycle card lists discards; the recurring-discard rule is in the skill text.
4. **INV5 efferent — the adoption join.** The selected variant's proposal card carries the commit hash (existing proposal-card discipline); `selection_record.algedonic_reference` → the card; `lineage_read` then traces genotype → verdict → card → commit → deployed artifact. No schema change — document the path in the skill's Act step. *Acceptance:* the first selected experiment records the join end-to-end.
5. **Layer-C join — the linked goal ID in the declaration.** `experiment_propose` records the linked kanban goal ID (the session creates the goal before declaring); the gemba walk then reads `population_query` alongside the board with one explicit join. *Acceptance:* the next experiment's declaration carries its goal ID.
6. **The curator energy budget (§P8.4 intent).** Register the evolution server as a regulated domain in the curator's energy budget so evolution spend is a first-class regulated quantity; the agenda generator's budget declaration reads it. *Acceptance:* the curator status surfaces the evolution budget. **Ruled 2026-09-30 (D-2): wire now** — the regulation loop senses the registry through the evolution health source (step 1), the per-experiment `max_runs` ceiling is the regulated spend quantity (step 2), and the executing agents' tool calls remain under the existing call-cap meter (the curator's `OverrideEnergyBudget` directive adjusts an agent's ceiling when a cycle needs more or less room). The §P8.4 prose is corrected below to name this mechanism.

**Stale-running set point — ruled 2026-09-30 (D-3): 7 days.** `DEFAULT_EVOLUTION_STALE_DAYS = 7` in the regulation crate — an experiment unresolved after a week is a deviation worth an advisory, not a crisis.

**Wired 2026-09-30 (all six steps, DIVERGENCE.md D87):** the `EvolutionStuckExperiments` metric + `EvolutionHealthSensor` + `EvolutionHealthSource` trait landed in the regulation crate (three sensor tests: stuck → deviation, healthy → homeostatic zero, broken source → None + warn); the bridge source opens the same SQLCipher registry the MCP child serves and reads `EvolutionStore::health_snapshot()` (stuck = stale past D-3 OR budget spent with no verdict; `resolved_claim_brier()` excludes voids on the literal markers); the composition root (`crates/zed/src/main.rs`) wires `set_evolution_health_source` in the deferred task beside the memory source — a broken open warns, never fabricates a zero. Steps 3–5 landed in the skill text and templates: discards persist in the cycle card with a two-cycle recurring-discard escalation, the adoption join is documented (card commit hash → `algedonic_reference` → `lineage_read`), and the declaration carries `budget.max_runs` + `linked_goal_id`. The live end-to-end advisory (an injected stale experiment producing a visible regulation alert) fires at the next editor restart, when the composition root wires the source — the sensor, source, setter, and wiring are landed and pinned by tests; the launch-gated verification is the same class as the other composition-root wirings.

**Follow-up fixes and cycle records (2026-10-01):**

1. **Selection-record race closed.** `record_selection` (insert + conditional resolve) is now ONE transaction: a `TransactionOps` trait with a required `transaction` method on `DatabaseDriver`, `BEGIN IMMEDIATE` over a pinned connection (`kask/crates/hkask-storage/src/database/{driver,sqlite}.rs`). A concurrent loser sees the winner's committed status and rolls back its insert, surfacing `ExperimentResolved` — never a double-recorded verdict. Pinned by a 4-thread concurrent-selection test on a file-backed pool (the in-memory helper's `max_size(1)` pool serializes and cannot reproduce the race).
2. **Measurement-void marker.** `resolved_claim_brier()` now excludes infrastructure voids on the literal `measurement-void` first-reason marker, with the same mechanics as `no-headroom` (`is_void_marker`, `kask/mcp-servers/hkask-mcp-evolution/src/types.rs`): a claim whose measurement never happened (rollouts errored with no model output) is never scored as refuted, so provider noise cannot pollute the calibration record the agenda generator calibrates against. Pinned by the extended `snapshot_stuck_and_void_exclusion_rules`. The convention is documented in the self-improvement agenda steps 1 and 5, `si-agenda-generator.j2` (calibration), and `evolution/experiment-protocol.j2` (Retain step).
3. **E4 — Phase 3 cycle 2 (`exp_2d620193fcb94b78adff33f0e6f557bc`, goal `b213c8f1`).** The Output Format quantity-fidelity delta was NOT tested. Headroom pre-check passed (baseline 6/9 = 0.667, 79,551 tokens, 9/9 requests ok — headroom demonstrated, challenger unlocked); then 7 of 9 challenger rollouts died at the provider (status error, `usage.total_tokens` 0, ~330s timeout each) during 17:54–18:23 UTC under 3-concurrent-harness dispatch against the degraded deepinfra backend, while the baseline's 9 sequential requests had all succeeded; the 2 valid challenger rollouts both PASSED. Resolved `rejected` with the `measurement-void` marker (selection `sel_c9a111afbdca4399bc2e547aefcf168d`, algedonic card `728752e4-c80a-498d-b22f-62fbf2fa89a1`); the claim is excluded from the calibration record. Budget 18/18 consumed — no re-measure within E4; the operator ruled re-run (option a) on card `728752e4`. Operational lessons recorded on the cycle card: dispatch harness calls sequentially, not concurrently (sequential 9/9 vs concurrent 2/9 on the same backend in the same hour); probe provider health before spend; and single-quote heredocs when baking phenotype cards (a `$5,` sequence was shell-expanded away in the challenger card's delta example — the snapshot preserves exactly what ran, so the measurement is faithful to the recorded genotype).
4. **E5 — Phase 3 cycle 3 (`exp_22addbfa51a1477090125eda89b9f8ae`, goal `6c6e932a`, the operator's re-run ruling).** The quantity-fidelity mechanism was tested validly this time and WON: challenger 7/9 (0.778) vs baseline 5/9 (0.556) on the identical hardened set — income 3/3 vs 1/3, marketing 2/3 vs 1/3, pipeline 2/3 vs 3/3; cost +6.2% tokens (79,904 vs 75,213), inside the 10% band. The pre-registered claim HELD at confidence 0.5 (Brier 0.25) — the registry's first held claim; calibration record 1/4 (E2A/E2B/E3 refuted, E4 excluded as void). Selection `sel_85d477fc` recorded `selected` (the first), algedonic card `19eeea47` carrying the proposed adoption diff; adoption into the shipped SKILL.md awaits the operator's approval — nothing auto-adopts. Budget 18/18 consumed exactly at ceiling; three transient 0-token provider hangs (one baseline, two challenger — a 22:49–22:59 UTC large-request degradation while small probes succeeded) were re-dispatched after a health probe and disclosed in the fitness records: the sequential-dispatch + probe pattern carried the measurement through a degraded hour that would have voided E4's concurrent shape. Goal `6c6e932a` judged done (all three criteria met).

## 7. Validation record from the review

These are historical review results, **not fresh execution results at plan-save time**. Commands ran from `/home/mdz-axolotl/Clones/zed-kask`. Cargo was offline and locked; no live MCP server or external service was used.

| Command | Observed result |
| --- | --- |
| `timeout 180s cargo test --offline --locked -p hkask-forecast -p hkask-lisp --lib --jobs 2` | 53 forecast + 17 Lisp tests passed |
| `timeout 180s cargo test --offline --locked -p hkask-spreadsheet --lib --jobs 2` | 13 passed |
| `timeout 180s cargo test --offline --locked -p hkask-mcp-server --lib server::validation::tests --jobs 2 -- --test-threads=1` | 8 passed |
| `timeout 180s cargo test --offline --locked -p hkask-event-store --lib --jobs 2` | 18 passed |
| `timeout 180s cargo test --offline --locked -p hkask-regulation --lib energy::tests --jobs 2` | Zero matched, 90 filtered out; not behavioral validation |
| `timeout 180s cargo test --offline --locked -p hkask-regulation --lib cap_exhaustion_is_detected_before_the_reset_replenishes --jobs 2 -- --test-threads=1` | 1 passed, 89 filtered out |

**Total: 110 executed tests passed.** This is not a fleet-wide correctness claim.

| Check | Observed result |
| --- | --- |
| `cargo metadata --offline --locked --no-deps --format-version 1` plus jq inventory/local-edge inspection | 19 libraries, 12 servers, no direct forbidden local edges outside the bridge |
| `bash kask/scripts/check-hkask-no-zed-deps.sh` | Passed |
| `bash kask/scripts/check-mcp-tool-tests.sh` | Passed, zero gaps; token heuristic, not complete behavioral coverage |
| `bash kask/scripts/check-reg-canonical.sh` | Passed |
| `bash kask/scripts/check-mcp-servers.sh` | Failed: spreadsheet absent from installer list |
| `bash kask/scripts/build/check-zed-isolation.sh` | Passed using fake temporary installation; emitted a missing retired-source-path grep warning |
| `bash kask/scripts/check-string-errors.sh` | Passed in library-code scope |
| `bash kask/scripts/check-version-sync.sh` | Passed for release 0.40.0 |
| Isolated unquoted-heredoc probe below | Model-string shell substitution demonstrated |

The harmless shell probe used no files, network, credentials, or services:

```bash
bash <<'REVIEW_PROBE'
set -eu
model='org/model$(printf SHELL_EXPANDED)'
printf 'cat <<MANIFEST\n{"base_model":"%s"}\nMANIFEST\n' "$model" | bash
REVIEW_PROBE
```

Observed output: `{"base_model":"org/modelSHELL_EXPANDED"}`. This demonstrates the shell mechanism, not the complete training submission path.

### Coverage and omissions

- All 19 library and 12 server manifests inventoried.
- Deepest inspection: runtime/tool port, shared server bootstrap/validation, delegation IPC, native-agent wrapper, and alternative dispatch.
- Targeted domain inspection: training script generation, corpus output paths, gallery policy, kanban attribution, spreadsheet persistence, event store, regulation charging/advice.
- Delegated source/test skims covered all server responsibilities, especially swarm, research, corpus, training, media, and kanban.
- One delegated persistence review failed at startup with an agent-service authentication error. Direct sampling only partially replaced it.
- Limited: full memory retention/consolidation, rotation, email, ontology semantics, condenser quality, complete financial workflows, cloud/A2A authorization, media parsers, deployed providers.
- Not executed: full workspace build/clippy, full server suites, GUI tests, real-process MCP reconnect tests, power-loss experiments, malicious-input end-to-end tools.

Coding-guidelines, metacognition, deep-module, bug-hunt, refactor-architecture, code-review, falsifiability, lean-prover, and kata-improvement were activated during review. `grill-me` was unavailable; labeled adversarial questions were used instead. Historical calibration was unavailable; no forecast store was contacted or artificial calibration score reported.

### Verification requirements during execution

Use the current project-prescribed `./script/clippy` interface rather than bypassing it with a different clippy invocation; inspect its arguments first. Run affected tests, applicable seam pinning tests, and the relevant dependency/packaging/isolation checks. Do not run unknown scripts before checking for real-service or user-data effects. Never install dependencies or go online merely because an offline build cannot resolve them.

The feature-gated reconnect suite is a **future validation command**, not a completed test:

```bash
cargo test --offline --locked -p hkask-mcp --features test-fixture --test reconnect_integration -- --test-threads=1
```

It launches temporary fixture child processes. Run only once that scope is authorized. Keep fixture tests serial and prove that they clean up their own processes. Never target live installed servers or unrelated PIDs.

## 8. Open decisions and hypotheses

1. **Resolved 2026-09-18:** operator chose automatic execution with inherited restrictions for subagents/worktree agents. This does not authorize broader tools, a fresh root authority, or treating same-user access as delegation.
2. Is destructive deletion ever an allowed override of read-only gallery mode? Default recommendation: no implicit override.
3. Which human/agent identities must persisted ownership and action attribution represent? Default recommendation: distinguish owner and initiating/delegated actors.
4. Define spreadsheet process-crash and power-loss guarantees separately; validate H1 independently.
5. Choose first-run secret/recovery behavior without silently modifying existing user data.
6. Verify whether any external CI currently executes feature-gated process tests.
7. Resolve active-project roots versus application-launch CWD for filesystem authority.
8. Define cancellation reporting after remote jobs or filesystem effects have already started.

Ask for decisions only when they block a correct intervention. Resolve technical details autonomously within the operator's constraints, explain their functional consequences, and do not inflate hypotheses into confirmed defects.

## 9. Progress record and completion criteria

At save time all packages were **not started**. The following execution record supersedes that snapshot, not the historical evidence in §7.

| Package | Status | Finding disposition / evidence / commit |
| --- | --- | --- |
| P0 revalidation | Baseline established | All finding dispositions below; source-only for unrepaired items |
| P1a host task authority | Implemented tool-ceiling/admission slice; integration verification below | Operator chose auto-run with inherited restrictions. Native ceilings persist; IPC requires explicit host grant; queued disconnect/revocation deny. General per-invocation MCP identity remains P2; native kanban spawning is denied rather than borrowing a server grant |
| P1b interruption | Partial; two of three cancellation gaps closed, behaviorally tested — the third (deadline firing against a stuck peer) was subsequently closed and pinned by the independent Gödel stream | F2: `KaskServerTool::run` and `ContextServerTool::run_inner`'s reconnect-wait loops now select on `event_stream.cancelled_by_user()`, closing the specific case where `Thread::cancel()` could block indefinitely on a managed MCP call that never replies. A new `McpRuntime::dispatch` execution deadline (`DEFAULT_CALL_TIMEOUT`, `HKASK_MCP_CALL_TIMEOUT_SECS`) reports `DispatchError::Interrupted`, never a proven failure, on timeout — config-plumbing pinned, but the actual timeout-firing-against-a-stuck-peer behavior is NOT executed here; it needs either the fixture-gated reconnect suite (permission-gated) or an in-process duplex-transport test (follow-up, not built this session). `ContextServerTool`'s existing request-level cancellation (already present pre-session at the two `cancelled_by_user()` sites guarding the tool call itself) was reviewed, not modified. Update 2026-09-19: the independent Gödel stream repaired the off-runtime dispatch panic (a `tokio_util::context::TokioContext` adapter in `McpRuntime::dispatch`) and executed the full feature-gated reconnect suite — 16 passed, including `off_runtime_deadline_and_drop_do_not_replay_effects` (`kask/crates/hkask-mcp/tests/reconnect_integration.rs`); the deadline/no-replay behavior is now pinned at the process boundary. |
| P1c training input | Repaired; focused verification passed | F3; changes observed in externally created commits `e21e3e5d2d` and `8a1bd7877b`; six fresh regression tests passed |
| P1d filesystem/gallery policy | Partial; F4 narrowed, not closed | F5 repaired and lifecycle suite passed (prior session). F4: `corpus_cache_work` (`kask/mcp-servers/hkask-mcp-corpus/src/tools/gather.rs`) validated only `cache_dir`, then joined and wrote through an unvalidated leaf — an existing symlink at the exact `{slug}.txt` leaf redirected the write outside the cache dir with no rejection. Fixed by re-running `contain_for_write` on the full joined leaf path, closing the "existing symlink leaf" case (verified: RED on the original code, GREEN on the fix, both times against a target genuinely outside all three allowed roots). The live TOCTOU race — a symlink planted in the gap between this check and the later `std::fs::write` — remains open; closing it needs a symlink-resistant atomic open (`O_NOFOLLOW`) shared across corpus/gallery call sites, which is a larger, cross-platform primitive deliberately not built under this session's time/lock pressure. Another actor's concurrent, unrelated edits to `hkask-mcp-corpus`'s OCR modules (`ocr/llm_ocr.rs`, `services/convert.rs`, `tools/document_tests.rs`) were observed causing 9 failing tests in that crate's full `--lib` run during this review; those failures are outside this session's F2/F4/P1a scope and were not touched. |
| P2 invocation/outcome contract | Open; F6 upheld, F7 retry repaired, R4 surface removed | F6 attribution upheld (anonymous startup fallback `kask/crates/hkask-mcp-server/src/server/transport.rs:91–104`; `self.webid` task claims `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:907–914`). F7's ContextServerStore retry repaired to proven-non-delivery-only (see F7 disposition below). R4's cited curator surface removed with the escalation queue (`2ae9c2cff9`). The host-authored invocation/identity contract itself is not yet replaced |
| P3 persistence/recovery | Partial; R2 facade removed (uncommitted) | R2: unused connectionless transaction API deleted, connection-owned commit-failure/reopen test passes. R3/H1 recovery remains open; R1 decision-gated. |
| P4 packaging/CI/docs | Partial — F8 repaired, fixture suite now executed; O1 closed | F8 inventory/install checks repaired (commit `429812b116`); the reconnect fixture suite was subsequently executed by the independent Gödel stream (16 passed; see P1b update). O1 docs drift closed 2026-09-19 by the kask/docs realignment |
| P5 optional simplification/formalization | Deferred | O2; only after behavior is pinned |
| L1 producer-named artifact layout | Implemented; committed in `3e581287b0` by another actor | `set_artifact_owner` confines MCP writes to `{server}-mcp/`; `artifact_writes_are_confined_to_the_owning_server` observed RED with the rule disabled and GREEN restored; route helpers pinned by `skill_and_curator_routes_name_their_producer` (that pin and its `skill_run_dir`/`curator_review_dir` helpers were later removed as unused in `6d13e5f3b3`; the confinement rule and its pin remain — `kask/crates/hkask-mcp-server/src/server/validation.rs:262,455`); clippy clean on 9 crates; corpus 199, training/companies/media/portfolio/spreadsheet suites green. `zk-data/INDEX.md` and the move log written; loose video and `media-mcp/analysis/` moved to `media-mcp/imported/` and re-indexed (one stale gallery row is unreachable through the tools — reported as a skill-use issue) |
| P6 skill evaluation separated from execution | P6a–P6d implemented; committed by another actor in `9338b493b9`, `12a2ac65c4`, `9e28b13fab`, `ea8824cf88`, `e47661d75e`. The `skill-logic-audit` wording follow-up was superseded: that skill was folded back into `skill-maintenance` as its template-logic audit (commit `39ed9d1ba2`), so no separate uncommitted wording remains. P6a re-verified in the 2026-09-28 tree (Curator-only `record_skill_feedback` at `crates/agent/src/agent.rs:4814`; pin tests at `:10093` and `:10161`); no live-session recheck is claimed | P6a: `record_skill_feedback` moved from `register_session` to the Curator overlay in `NativeAgent::new_session` (`crates/agent/src/agent.rs`); `test_native_session_reads_curator_status_without_curator_mutations` observed RED with the tool re-registered in `register_session` and GREEN after the move; `test_curator_session_registers_directive_tool` asserts the Curator model request carries it (2 passed). DIVERGENCE D1/D59 updated. The orphan media templates (audit Q4) were removed by another actor in `4acadc4ac2` |

For each completed package record: current-source finding disposition; exact changed files; old paths deleted; test command and counts; failures/limitations; functional outcome; and commit hash if committed, otherwise explicitly “uncommitted.” Do not commit automatically or include another actor's staged work.

Completion means repaired behavior demonstrated at the relevant boundary, no superseded unsafe route remaining, applicable invariants passing, current docs, and a truthful account of residual risk. A compile-only result, zero-test filter, mock that removes the capability under test, or an empty degraded result is not completion.

### Execution baseline and finding dispositions — 2026-09-18

Initial HEAD was `7f1c83e558df951eec7f74d43fb623b02717c945`; the index was empty. Existing unstaged work comprised the docs portal, six corpus implementation files (including `index.rs`), prediction-markets tests, the review task, and this untracked plan. Those changes were preserved. Offline locked metadata returned 299 members, 19 `kask/crates/` libraries and 12 MCP packages. Read current root/agent rules, AGENTS, the whole plan including §10, principles, charter, interaction specification, documentation standard, and applicable D3/D8/D23 seams. No upstream Rust files were edited by this session.

Other actors advanced HEAD and staged/committed shared files during execution, including the repairs. The implementing agents issued **no stage or commit commands**. Initial verification base was `8a1bd7877b63f0cb2c71a57cc96ab84e69302f2a`, plus gallery/doc edits. Later observed HEAD `acbefb647bcb8011cfcf4dfc4ba4e1ad3d2d4d5e` contains the gallery repair and strengthened tests, externally committed alongside unrelated regulation work. The progress record, portal status and media-reference metadata remain **uncommitted**. Unrelated regulation/bridge changes and all index contents were left untouched. This is a moving-checkout record, not an assertion that every check ran on an immutable tree.

Source citations below are relative to ``; they record execution-time inspection, not new exploit reproductions.

| Item | Current disposition and evidence |
| --- | --- |
| F1 | Upheld at initial baseline; repaired admission and native tool-ceiling paths in the P1a continuation below. Broader per-call MCP identity is not claimed repaired. |
| F2 | Upheld at baseline; subsequently closed by the independent Gödel stream (see P1b): the reconnect-wait loops now select on `cancelled_by_user()` (`crates/agent/src/tools/context_server_registry.rs:726,1080,1177`), and `McpRuntime::dispatch` runs under `DEFAULT_CALL_TIMEOUT` / `HKASK_MCP_CALL_TIMEOUT_SECS` (`kask/crates/hkask-mcp/src/runtime.rs:106`), reporting `DispatchError::Interrupted` on timeout — pinned by `off_runtime_deadline_and_drop_do_not_replay_effects` (`kask/crates/hkask-mcp/tests/reconnect_integration.rs:54`). |
| F3 | Upheld at baseline, repaired below. Both providers use the repaired shared generator. Prior TRL removal was retained; only Axolotl/Ludwig are in scope. |
| F4 | Upheld: `kask/mcp-servers/hkask-mcp-corpus/src/tools/gather.rs:199–210` validates the directory then writes a leaf; shared `validation.rs:289–312` reconstructs missing suffixes before `tools/document.rs:72–89` writes. Existing relative-basename and QA-specific link fixes are not race-resistant publication and were not recreated. |
| F5 | Reproduced and repaired below. Existing transcript-detachment and unlink-failure identity preservation were retained. |
| F6 | Upheld: `kask/crates/hkask-mcp-server/src/server/transport.rs:91–104` retains anonymous startup fallback; `server/tool_span.rs:140–169` attributes process context; kanban task claim at `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:907–914` still uses `self.webid`. No cross-user compromise demonstrated. |
| F7 | Upheld at baseline; subsequently repaired. The ContextServerStore path now retries only a request proven never delivered — `is_provably_not_delivered` (`crates/agent/src/tools/context_server_registry.rs:849–851`) gates the restart-and-retry at `:1126–1160`, replacing the "retry unless proven timeout" denylist; pinned by `timeout_and_transport_death_classify_into_distinct_retry_verdicts` (`crates/agent/src/tools/context_server_registry.rs:2085`) and `test_mcp_tool_timeout_does_not_retry` (`crates/agent/src/tests/mod.rs:5737`). Managed typed non-delivery behavior remains intact. |
| F8 | Reproduced inventory failure; repaired inventory and CI wiring below. Fixture process tests remain permission-gated and unexecuted. |
| R1 | Upheld by source-only inspection of the provisioning policy at `kask/crates/hkask-keystore/src/passphrase.rs:1–17` and `keychain.rs:402–424`; no keychain or deployed database inspected, no secret rotated. Onboarding/recovery decision remains open. |
| R2 | Historical finding upheld; subsequently closed by deletion in the 2026-09-18 working tree (see R2 current disposition). The connectionless facade and hooks no longer exist; safe borrowed-connection transactions remain, with commit-failure/reopen coverage. |
| R3 | Upheld design risk: `kask/crates/hkask-spreadsheet/src/artifact_store.rs:106–145,193–229` separates revision publication and direct receipt writes. No fault injection or power-loss guarantee established. |
| R4 | Upheld at baseline; the cited surface no longer exists. The escalation/management tools carrying caller-supplied `operator_confirmed` were removed with the escalation queue in commit `2ae9c2cff9` (2026-09-26); no `operator_confirmed` field remains in `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`. The caller-assertion-versus-host-receipt distinction remains part of P2's open invocation contract. |
| H1 | Source-supported, dynamic test still outstanding: cached return at `kask/crates/hkask-spreadsheet/src/service.rs:502–525` precedes digest comparison; public `open` returns the supplied reference. Fresh apply independently verifies its base. |
| O1 | Closed 2026-09-19: the kask/docs realignment re-verified counts, citations, and links across the corpus (12 servers, 19 crates, D1–D69), and the `AGENTS.md` server inventory is current (12 servers, registry-authoritative pointer, no per-tool-contracts-doc reference). No stale-count claims remain in the named locations. |
| O2 | Deferred guidance, not a defect: preserve the spreadsheet actor and typed delivery outcomes; no fleet-wide service extraction undertaken. |

### P1c / F3 — caller data is no longer shell source

Changed files: `kask/mcp-servers/hkask-mcp-training/src/{providers/runpod.rs,providers/nebius.rs,huggingface.rs,tools/submit.rs,hkask_mcp_training.rs}` and `kask/registry/templates/training/{axolotl-lora.j2,ludwig-lora.j2}`. Deleted fixed config/unquoted manifest heredocs, manual cloud-init serialization, unquoted model scalars, and the ignored model-resolution-error path. Config bytes use octal transfer; argv values are encoded; static manifest metadata and cloud-init are serialized; both templates JSON-quote the model scalar. Invalid model identifiers fail before dataset/credential/provider effects. No legacy generator remains.

Prediction: hostile delimiters, substitutions and quotes round-trip as data without marker execution. Delegated execution observed RED (three boundary tests failed; a separate submission test failed) then GREEN. Independent rerun: `timeout 180s cargo test --offline --locked -p hkask-mcp-training --lib f3_ --jobs 2 -- --test-threads=1` — **6 passed, 0 failed, 23 filtered**. Tests run only local transfer/argument-recorder snippets, never installation/training/upload commands. The delegated `manifest` filter additionally passed 2 tests (one overlaps F3; not counted as distinct here).

Residual: no deployed provider/cloud-init test; identifier validity does not establish repository existence; octal encoding increases payload size. Upload-argument regression uses the shared Axolotl path; config/manifest transport tests cover both providers and both harnesses. No external service was contacted.

### P1d / F5 — preservation modes now refuse original-file deletion

Changed files: `kask/mcp-servers/hkask-mcp-media/src/{tools/gallery.rs,types.rs,hkask_mcp_media.rs}` and `kask/docs/reference/mcp-servers/media.md`. Replaced unconditional unlink with destructive-mode admission. Kept index-only deletion in every mode. After unlink, a database error explicitly states that the source was deleted and requests reconciliation; it never implies rollback. Removed the old read-only setup from the filesystem-failure test so it still exercises an authorized unlink failure.

Prediction: all six mode × delete-flag cases preserve the specified source/index/transcript state; a seventh injected database failure exposes partial effects. RED: `gallery_lifecycle_tests::gallery_deletion_mode_matrix` executed **1 test, 1 failed**, because read-only deletion returned success with `file_deleted:true`. An earlier malformed patch caused a compile error, was corrected immediately, and is not counted as behavioral RED. GREEN: `timeout 240s cargo test --offline --locked -p hkask-mcp-media --lib gallery_lifecycle_tests --jobs 2 -- --test-threads=1` — **10 passed, 0 failed, 274 filtered**. The matrix includes exact retained source bytes, index presence, transcript linkage/availability, successful deletion and injected post-unlink failure. Final strengthened unlink-failure assertions are subject to the final verification entry below.

Final rerun after adding source-path and transcript-availability assertions to the unlink-failure case used the same lifecycle command: **10 passed, 0 failed, 274 filtered**, exit **0**, 28.10 seconds of test execution. Observed in `acbefb647b` after external commit; no test or implementation remains half-applied.

Residual: filesystem unlink and SQLite deletion are not one atomic transaction. This slice surfaces partial failure, not crash rollback. Containment races (F4), cross-process policy changes, and authenticated policy-setting (P2) are not solved by this local admission check.

### P4 / F8 — packaging equality and explicit CI coverage

Changed files: `kask/scripts/build/mcp-servers.txt` and `.github/workflows/kask-invariants.yml`; observed externally committed as `429812b116`. Added spreadsheet to the installer list and wired the existing equality checker/self-test plus an explicit serial `--features test-fixture --test reconnect_integration` CI command. No duplicate inventory or replacement test framework was added.

Prediction/observation: existing `bash kask/scripts/check-mcp-servers.sh` failed with exit **1**, naming the missing spreadsheet binary, then passed with **12 matching servers**. `bash kask/scripts/check-mcp-servers-selftest.sh` passed **both negative cases** (drift and empty sets). `bash kask/scripts/build/check-zed-isolation.sh` passed a complete **12-server fake temporary installation**, rollback and confinement checks; it emitted a pre-existing missing `crates/auto_update/src/auto_update.rs` grep warning. No real installation or MCP process occurred. Reconnect CI wiring is inspected, not an executed/green CI claim; separate fixture-process permission remains required.

### Cross-slice checks and next experiment

- Final observation: another actor committed the progress/portal/media docs as `700e6f3c1342cd7659ff76309f4d5f2be017dd16`; later verification additions to this record are **uncommitted**. The implementing agents still issued no stage/commit commands. An unrelated `kask/crates/kask_bridge/src/memory.rs` edit was left untouched.
- `timeout 240s env CARGO_NET_OFFLINE=true HKASK_BUILD_JOBS=2 HKASK_SCCACHE_DIR=/nonexistent GITHUB_ACTIONS=true ./script/clippy --offline --locked -p hkask-mcp-media -p hkask-mcp-training` — **exit 0**, all targets/features with warnings denied. `GITHUB_ACTIONS=true` skips optional local machete/buf extras; those are not claimed as run. No tools installed.
- Final clippy rerun after the strengthened assertions, same environment/packages with `timeout 180s`, also finished **exit 0** (51.45 seconds including Cargo-lock contention).
- `bash kask/scripts/check-hkask-no-zed-deps.sh` — passed. Scoped `check-mcp-tool-tests.sh` over media/training — **0 violations, 0 gaps** (heuristic, not behavioral coverage). `bash -n kask/scripts/build/install-common.sh` and staged/unstaged `git diff --check` passed.
- Targeted media `rustfmt --check` passed. All three edited documentation files passed required-metadata and relative-file-link checks; `kask/docs/` contains **67 files**. This is not a full documentation-health pass: the prescribed `verify-docs.sh` does not exist. No diagram or additional document was introduced.
- Focused static review found no blocker in inspected F3/F5 paths; a missing unlink-failure availability assertion was added. No compatibility paths, new dependencies, upstream Rust changes or broad refactors were introduced. Full workspace build/tests, live MCP, reconnect fixtures and deployed providers were not run.
- Learning: data/source separation closes shell substitution independently of identifier validation; file policy needs assertions over persisted relationships and partial effects, not just the error return. The P1a decision was subsequently resolved below. Next priorities are P1b bounded Stop/unknown-outcome behavior and F4 destination-handle containment. P2–P3 remain substantive unfinished work, not compiler-only follow-ups.

### P1a continuation — auto-run under inherited tool ceilings

**Operator decision:** subagents and worktree agents auto-run with inherited restrictions. No extra spawn-confirmation prompt is added; normal tool permission and sandbox approvals remain. This continuation began at `700e6f3c1342cd7659ff76309f4d5f2be017dd16`, with unstaged bridge memory/alert/OCR and progress-record work, empty index. Another actor committed those baseline edits as `c86faf6a59525f29d411713d0e81e633ec114be1`; the P1a changes are **uncommitted**, with no stage/commit commands from the implementing agents. Concurrent native/UI edits were inspected and completed rather than overwritten. One delegated implementation attempt returned an authentication error; a later attempt succeeded. No provider or MCP service was launched.

**Implemented behavior:**

- `Thread::new_subagent` and `NativeThreadEnvironment::create_sibling_thread` derive host-owned exact tool identities from the parent's enabled tool set. A persisted `DelegationAuthority` can only intersect/narrow, never widen when a profile changes. `AnyAgentTool::delegation_identity` distinguishes native builtins from both MCP transports' exact server/original tool names, independently of model aliases. Advertising and execution check the ceiling. Resume verifies actual parent lineage and narrows again. `DbThread` and `SharedThread` preserve ceilings through save/load and export/import without deleting existing data.
- Sibling creation without authority fails before workspace creation; external-agent destinations are refused rather than entrusted with unenforceable native policy. `AgentInitialContent::DelegatedSibling` applies the ceiling before the first auto-submit, preserving its envelope across connection retries. Ordinary user-created root threads remain unchanged. Delegated children cannot recursively call native `create_thread`/`spawn_agent`, preserving the existing depth-one delegation limit across sibling paths too.
- IPC `CreateWorktreeThread` now needs `host/create_worktree_thread` in the parent-owned server grant plus an explicit requested MCP narrowing. Its child receives the intersection, not caller-provided authority or ambient native tools. A 32-entry bounded queue rejects overflow; the actual dequeue consumer skips disconnected callers and rechecks revocation before starting effects. Cancellation after admission does not imply rollback. Fake-spawner/socket-pair tests do not launch MCP fixture processes.
- Deleted kanban's `spawn_via_local_runtime`, unused `local_runtime` field/construction and fallback behavior. The selected agent card's `mcp_tools` narrows the host grant. An authorization refusal is permission-denied; uncertain transport outcomes remain unavailable and retain an idempotency claim (when a key was supplied), preventing same-key duplicate spawning. Known pre-effect validation failures still release their claim. Queued worktree responses say pending and no longer advance the task to InProgress before asynchronous session startup succeeds.
- **P2 boundary exposed, not papered over:** the existing MCP grant identifies a server, not the initiating thread. Native `kata-kanban/kanban_task_spawn` is denied—including root native callers—until that per-invocation authority exists. Native agents use the bound `create_thread`/`spawn_agent` paths. Explicit server-origin worktree delegation remains possible under its configured server grant. This is not an implementation of general MCP per-call attribution.

**Exact changed files (relative to ``):**

- Native: `crates/agent/src/{delegation_authority.rs,agent.rs,thread.rs,db.rs,thread_store.rs,tests/mod.rs,tools/context_server_registry.rs,tools/create_thread_tool.rs}`.
- UI/host: `crates/agent_ui/src/{agent_panel.rs,agent_ui.rs,conversation_view.rs,conversation_view/thread_view.rs,thread_metadata_store.rs}`; `crates/zed/src/{main.rs,visual_test_runner.rs}`. Metadata/visual-test edits only initialize the new persisted field. All upstream edits are recorded in `DIVERGENCE.md` D23/D8 continuation.
- IPC: `kask/crates/kask_bridge/src/{delegation_grants.rs,inference_ipc_server.rs,settings.rs}`; `kask/crates/hkask-types/src/{inference_ipc.rs,ports/inference_port.rs}`; `kask/crates/hkask-inference/src/{hkask_inference.rs,inference_ipc_client.rs}`.
- Kanban: `kask/mcp-servers/hkask-mcp-kata-kanban/src/{hkask_mcp_kata_kanban.rs,idempotency.rs,kanban/service_impl/service.rs,kanban/service_impl/spawn.rs,kanban/service_impl/tests.rs}` and `tests/{board_rename.rs,idempotent_creates.rs}`.
- Documentation: `DIVERGENCE.md`, `kask/docs/reference/kask-settings.md`, and this plan. No dependency or compatibility adapter added. Although cross-seam integration exceeds 50 lines, production scope is one creation/authority contract; most additions are behavioral tests. The old fallback and its unused constructor dependency are removed, not retained in parallel.

**Verification evidence:** all Cargo commands offline/locked, jobs 2, temporary data under checkout-local `target/p1a-tmp` because default `/tmp` quota was exhausted. No real database/keychain/provider is required by the repair tests. Counts below are actual executions, not summed repeated tests.

| Command (from repository root) | Result |
| --- | --- |
| `timeout 360s cargo test --offline --locked -p agent -p kask_bridge -p hkask-mcp-kata-kanban --lib --jobs 2 -- --test-threads=1` | **911 agent passed, 11 ignored; 223 bridge passed; 56 kanban passed**, exit 0. Includes nine native delegation regressions and the shared-thread round trip |
| `timeout 360s cargo test --offline --locked -p kask_bridge --lib worktree_ --jobs 2 -- --test-threads=1` | **6 passed**, 217 filtered: no-port/closed-queue, invalid authority, valid intersection/revoked dequeue, B disconnect while A runs, and queue overflow |
| `timeout 360s cargo test --offline --locked -p agent_ui --lib sibling_ --jobs 2 -- --test-threads=1` | **4 passed**, 434 filtered: native-only destination, missing-authority denial, before-first-completion ceiling, external startup/refused retry |
| `timeout 300s cargo test --offline --locked -p hkask-mcp-kata-kanban --tests --jobs 2 -- --test-threads=1` | **85 passed** (56 unit, 5 board, 23 idempotency, 1 removal pin), exit 0; final run includes unknown-spawn same-key refusal and pending response assertions |
| `timeout 240s cargo test --offline --locked -p hkask-mcp-kata-kanban --tests --jobs 2 -- --test-threads=1` | Post-cleanup **83 passed** (54 unit, 5 board, 23 idempotency, 1 removal pin), exit 0. Removed the orphaned fallback result writer and its two tests; persisted historical result fields remain readable |
| `timeout 300s cargo check --offline --locked -p zed --jobs 2` | Initial and final integrated checks passed, **exit 0**; final 2m33s includes lock contention |
| `timeout 300s env CARGO_NET_OFFLINE=true HKASK_BUILD_JOBS=2 HKASK_SCCACHE_DIR=/nonexistent GITHUB_ACTIONS=true ./script/clippy --offline --locked -p agent -p agent_ui -p kask_bridge -p hkask-inference -p hkask-mcp-kata-kanban` | Final **exit 0**, all targets/features, warnings denied, 51.81s. Initial pass caught the orphan fallback result writer; deleted it and reran rather than adding a dead-code suppression. Optional local machete/buf extras skipped by `GITHUB_ACTIONS=true` |
| `check-hkask-no-zed-deps.sh`; `check-no-new-dead-code-allows.sh`; scoped `check-mcp-tool-tests.sh`; `check-string-errors.sh` | Passed; scoped tool gate **0 violations/0 gaps** (heuristic, not behavioral proof) |

All changed Rust files passed targeted `rustfmt --check`, staged/unstaged diffs passed whitespace checks, and both changed `kask/docs/` documents passed metadata and relative-file-link checks. Documentation count remains 67. No dependency changes, compatibility flags, legacy adapters, additional dead-code suppressions, installations, commits, live MCP processes or real database modifications. The full removal-symbol sweep finds no remaining fallback executor/result-writer references in Rust; historical task-log mentions are retained as historical evidence. Coding-guidelines self-audit: zero critical violations; scope expansion was required by actual persistence, alias, UI-before-submit and fallback callers, not a fleet-wide redesign. Remaining test-process, broader identity and OS-isolation limitations are explicit rather than marked passed.

The regression suite is behavioral, but **no pre-repair behavioral RED execution was retained for this continuation**; initial tests were added around a concurrently implemented native/UI seam and bridge tests followed implementation. Do not label that as a completed RED→GREEN cycle. Earlier native test execution encountered `/tmp` quota exhaustion, not a product failure; checkout-local temporary storage resolved it. The first UI cold build took 4m45s and executed 2 tests; the final expanded UI run executed 4. Full end-user git-worktree creation, live providers and MCP fixture processes were not run.

**Review and limitations:** second-pass inspection found no remaining direct bypass in the repaired tool-ceiling paths after closing server-grant exchange, recursive siblings, shared export/import, retry-envelope loss and false running reports. This is a **snapshot of enabled tool identities**, not continuous revocation of running descendants, an OS sandbox, or machine enforcement of natural-language restrictions. Existing global tool/sandbox approval mechanisms remain in force; they were not replaced with blanket allow. Generic MCP delegation/per-call identity, active-workspace authority, cancellation after admitted filesystem effects and broader uncertain-delivery contracts remain P2/P1b work. A no-key caller still has no idempotency guarantee. No real keys were provisioned/rotated and no user data migrated destructively.

## 10. Continuation prompt

Copy the following into an implementation-authorized session:

```text
Execute the evidence-led repair and improvement plan at:
kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md

This instruction authorizes implementation of the plan's bounded repairs, not
speculative rewrites or decision-gated destructive operations. There are NO
backward-compatibility requirements. Replace incorrect contracts directly,
update all callers and tests together, and delete superseded paths. Do not add
legacy adapters, deprecated APIs, compatibility flags, or dual implementations.
Preserve user data, human agency, and minimal upstream divergence.

Begin with P0: read the complete plan, current .rules/AGENTS.md, principles, and
applicable DIVERGENCE.md seams; inspect HEAD and staged/unstaged changes. The plan
records an older, concurrently changing review snapshot. Revalidate findings
against today's source and callers. Mark already-fixed/disproved items honestly;
do not recreate repairs. Preserve all unrelated work.

Activate coding-guidelines before code, then use tdd/bug-hunt and code-review
for each bounded change. Apply deep-module/refactor-architecture only where they
reduce demonstrated complexity. Use falsifiability to challenge suspected bugs
and kata-improvement to record prediction, observation, and the next obstacle.
If a requested skill is unavailable, disclose it rather than claiming execution.

Work through P1 in complete vertical slices, then P2–P4 subject to the plan's
dependencies. The small F8 packaging/check repair can be an independent early
slice. Coordinate P1 with the final invocation contract: no temporary legacy
support. Keep the shared tree buildable; never leave an API half-updated.

For each slice: show a short plan; establish a regression test at the real public
or integration boundary; implement the smallest complete replacement; delete
the obsolete path; run affected tests and applicable seam/static checks; review
the diff; update the plan's progress record. Tests must demonstrate authority,
attribution, cancellation, effect/retry safety, or persistence behavior—not just
compilation. Report exact counts and do not count zero-test runs as validation.

Use offline, locked Cargo commands where feasible and the repository's
./script/clippy entrypoint. Do not install tools, access secrets, contact external
or paid services, launch live MCP services, or modify real user databases. Use
mocks, in-memory stores, and temporary files. Ask separately before launching
the feature-gated isolated MCP fixture processes if that permission has not
already been granted. Never silently rotate keys or reset persisted data.

Ask the operator only for blocking product/security choices identified in the
plan; proceed on independent work meanwhile. Do not claim a formal proof without
an executed proof checker and explicit model/implementation limitations.

Finish with repaired user-visible outcomes, finding dispositions, changed files,
deleted paths, exact test/check results, residual risks, and next steps. Record
commit hashes only if commits actually exist; otherwise label work uncommitted.
Do not commit, stage unrelated work, or expand scope without instruction.
```
