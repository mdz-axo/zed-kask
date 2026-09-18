---
title: "hKask Core and MCP Review — Repair and Improvement Plan"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-18
version: "0.2.0"
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

- `/home/mdz-axolotl/Clones/zed-kask/.rules`
- `/home/mdz-axolotl/Clones/zed-kask/AGENTS.md`
- `/home/mdz-axolotl/Clones/zed-kask/DIVERGENCE.md`, especially the applicable seams and §13.1.
- `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/core/PRINCIPLES.md:43–79`
- `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/core/magna-carta.md:54–98`
- `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/functional-interaction-spec.md:135–197`
- `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/DOCUMENTATION_STANDARDS.md`

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

Servers: companies, corpus, curator, kata-kanban, media, portfolio, prediction-markets, research, scenarios, spreadsheet, swarm, training. Discover the execution-time inventory anew from manifests and `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/mcp_servers.rs:55–539`; do not hardcode these counts in new tests.

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
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/inference_ipc_server.rs:820–856` checks request list and `parent_allows` for ToolInvoke.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/inference_ipc_server.rs:867–906` independently accepts CreateWorktreeThread.
- `/home/mdz-axolotl/Clones/zed-kask/crates/agent_ui/src/agent_panel.rs:4859–4907` sets `auto_submit: true` and creates worktree-backed work.

**Mechanism/impact:** a child with same-user socket access and an available workspace can initiate filesystem work and an agent task with an empty/revoked delegated-tool grant. The child restrictions are not visibly propagated into the new thread.

**Counterevidence/disproof:** this is not an Internet-authentication bypass; subsequent native-agent tool calls still encounter normal permission checks. Those checks do not authorize the initial task/worktree creation. No live reproduction was performed.

**Repair:** require an explicit parent-granted host capability before enqueueing; define propagation of restrictions or an explicit new human authorization. Remove the ambient route rather than accepting old requests.

**Acceptance:** fake-spawner tests yield zero effects for missing/revoked/insufficient grants and one for a valid grant; inspect spawned-thread effective authority.

**Related queued-cancellation defect:** `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/inference_ipc_server.rs:422–438` uses an unbounded queue and spawns without checking whether the reply receiver closed. A waiting request can execute after disconnect. Add bounded admission and cancellation-aware dequeue; test request B disconnecting while A occupies the consumer. Do not equate cancellation after a committed effect with rollback.

### F2 — Stop can leave managed MCP execution pending

**Class:** verified control-flow defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/crates/agent/src/tools/context_server_registry.rs:574–592` awaits managed invocation without selecting user cancellation.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:1686–1698` calls rmcp without a request deadline.
- `/home/mdz-axolotl/Clones/zed-kask/crates/agent/src/thread.rs:2597–2604,3500–3518,6096–6100` signals cancellation but waits for tool results/turn completion.

**Mechanism/impact:** a connected server that never replies can prevent cancellation completion indefinitely. The locked rmcp implementation was inspected: default request options have no timeout.

**Counterevidence/disproof:** startup/discovery deadlines, host inference deadlines, panel-task aborts, and IPC disconnect handling exist, but do not bound this wrapper. No hung live service was launched.

**Repair:** carry cancellation through managed dispatch and define a user-visible execution deadline policy. After possible delivery, preserve an uncertain outcome rather than reporting rollback or inviting automatic retry.

**Acceptance:** a never-resolving fake source cannot prevent Stop completing within a defined local test bound. Commit-before-cancel produces an unknown/committed outcome as appropriate and zero automatic replays.

### F3 — Training model input becomes remote shell source

**Class:** verified input-boundary defect; isolated shell semantics reproduced. **Severity:** High. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/kask/registry/templates/training/axolotl-lora.j2:10` renders model input directly.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/providers/runpod.rs:552–557,649–682` uses a fixed config heredoc delimiter and an unquoted manifest heredoc.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/providers/nebius.rs:90–118` executes the generated script with Bash.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/submit.rs:239–248` does not reject model-resolution failure.

**Mechanism/impact:** newline input can terminate the config heredoc; model input in the manifest heredoc can execute shell substitutions in the training VM/container. Local-editor code execution was not demonstrated.

**Counterevidence/disproof:** quoting the config delimiter prevents expansion inside its body, not premature delimiter termination. Training parameter/dataset checks are not shell-source separation.

**Reproduction:** a file-free Bash probe of the manifest pattern changed literal `org/model$(printf SHELL_EXPANDED)` into `org/modelSHELL_EXPANDED`. No provider or training job was contacted.

**Repair:** replace caller-data interpolation with structured serialization and data transfer; remove the unsafe generator path. Identifier validation is additional defense, not the sole boundary.

**Acceptance:** local generation/harness tests exercise newlines, delimiter lines, quotes, backticks, and `$()`; data round-trips literally and no marker command executes.

### F4 — Corpus containment misses final-component symlinks

**Class:** verified filesystem-boundary defects. **Severity:** High. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-corpus/src/tools/gather.rs:187–210` checks cache directory, appends a filename, then writes it.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp-server/src/server/validation.rs:204–231,289–298` reconstructs a non-existing target from its existing ancestor.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs:72–84` writes conversion output through that boundary.

**Mechanism/impact:** an existing cache leaf symlink redirects a write outside the allowed root. A dangling output symlink returns NotFound during canonicalization, survives reconstruction, then redirects a later write. Neither requires a race; OS write permissions still apply.

**Counterevidence/disproof:** slug validation blocks textual traversal, not symlinks. A stronger helper exists at `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-corpus/src/path_safety.rs:28–65`, but these paths do not use it. Eight existing shared path tests passed without covering these cases.

**Repair:** replace permissive write interfaces with complete-destination, symlink-resistant opening/publication. Another pre-write canonicalization alone does not solve the race variant.

**Acceptance:** temporary-root tool tests cover existing/dangling leaf links, ancestor links, ordinary new files, and unchanged outside-file bytes. Include platform-specific behavior explicitly.

### F5 — Gallery deletion ignores read-only/copy-on-write mode

**Class:** verified policy-enforcement defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/gallery.rs:37–47` defines preservation policy.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs:337–356` returns mode without enforcing it.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-media/src/tools/gallery.rs:1393–1427` calls remove_file when requested without checking mode.

**Mechanism/impact:** a tool-name-authorized caller can remove an original file from a gallery whose policy says originals are preserved.

**Counterevidence/disproof:** `delete_file` is explicit and defaults to preserving the file. This limits accidental activation but is not a documented policy override. No destructive live test was run.

**Repair:** enforce gallery-mode admission at the file-mutation boundary. Only provide an override if the operator explicitly chooses that product behavior; do not infer consent from a model-provided boolean.

**Acceptance:** mode × deletion-request tests assert source bytes, index state, and transcript linkage on success, refusal, and partial failure.

### F6 — Process identity is not authenticated per-call authorship

**Class:** verified attribution defect; broader authorization impact is workflow-dependent. **Severity:** High for ownership/audit semantics. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp-server/src/server/transport.rs:91–117` falls back to anonymous identity.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/mcp_env.rs:46–54` maps curator identity only.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp-server/src/server/tool_span.rs:140–168` attributes from server context.
- `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:885–895` claims tasks as the “authenticated caller” using `self.webid`.

**Mechanism/impact:** distinct initiating agents can be indistinguishable in server actions and ownership checks. A process-scoped WebID is not evidence of the caller's authenticated identity.

**Counterevidence/disproof:** OS-user/data-directory separation, host accounting identities, and parent grants remain. Cross-OS-user access was not established.

**Repair:** introduce one host-authored invocation contract distinguishing owner, initiating actor, delegated actor, server identity, and authority. Replace anonymous production fallback and update all callers together. Ordinary tool arguments must not establish identity. Revalidate ownership behavior before changing persisted identity semantics.

**Acceptance:** two delegated actors produce distinct attributable claims; spoofed input cannot alter effective identity/ownership; missing production identity fails visibly and safely.

### F7 — ContextServerStore retries possibly committed effects

**Class:** verified retry-safety defect. **Severity:** High. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/crates/agent/src/tools/context_server_registry.rs:988–1046` retries non-timeout errors.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:1632–1653,1686–1698` distinguishes non-delivery and uncertain delivery.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-tool-port/src/tool_port.rs:15–51` defines retry-safe versus interrupted outcomes.

**Mechanism/impact:** a server commits a mutation and loses its response; the host may repeat that mutation. Non-timeout protocol/decoding errors can also enter the retry branch.

**Counterevidence/disproof:** timeouts are excluded and managed runtime behavior is safer. Its semantics do not protect the alternative path; the comment claiming equivalent behavior is stale.

**Repair:** one delivery-state contract across both paths; retry only proven non-delivery. Retain different transports only where they earn their complexity, not different safety rules.

**Acceptance:** commit-then-drop-response fixture receives exactly one mutation; distinguish transport, protocol, decoding, cancellation, and pre-send failures. Unknown effects require reconciliation, not replay.

### F8 — Spreadsheet packaging is incomplete; critical checks are skipped

**Class:** reproduced packaging defect and verified CI coverage gap. **Severity:** High for capability availability; Medium for CI gap. **Confidence:** High.

**Evidence:**
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/mcp_servers.rs:520–538` registers spreadsheet.
- `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/build/mcp-servers.txt:12–22` omits it.
- `/home/mdz-axolotl/Clones/zed-kask/kask/scripts/build/install-common.sh:32–46` consumes the list.
- `/home/mdz-axolotl/Clones/zed-kask/.github/workflows/kask-invariants.yml:75–114` omits the registry check and explicit fixture feature.
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/Cargo.toml:32–49` gates reconnect integration tests on `test-fixture`.

**Reproduction/impact:** `check-mcp-servers.sh` failed, reporting missing `hkask-mcp-spreadsheet`. List-driven installation omits an advertised capability; the inspected CI command skips the process-boundary reconnection suite.

**Counterevidence/disproof:** the checker and tests already exist. External automation beyond the inspected workflows was not verified.

**Repair:** synchronize packaging immediately and wire existing checks. Prefer one authoritative inventory or mechanically checked equality over duplicate prose/name lists. Explicitly enable isolated process-fixture tests in CI.

**Acceptance:** manifest/registry/install sets agree; clean temporary installation has the complete current fleet; CI output shows actual reconnect tests, not a zero-test success.

## 4. Design risks and improvement opportunities

### R1 — Public default passphrase weakens copied-database confidentiality

**Class:** design risk reflecting an explicit recovery tradeoff. **Severity:** High if encryption is relied upon against copied-file disclosure. **Confidence:** High.

Evidence: `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-keystore/src/passphrase.rs:1–17` and `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-keystore/src/keychain.rs:402–424`. First-run provisioning uses a public fixed default. A copied database using it does not require keychain access to decrypt.

Counterevidence: source explicitly prioritizes recoverability; users can change the secret. No deployed database or keychain was inspected. This is not proof that any particular user's database still uses the default.

Plan: make the protection state visible, then obtain the operator's choice of required user secret versus recoverable generated secret. No compatibility requirement justifies preserving the default as a confidentiality claim; it also does not authorize silent rotation or data loss. Test onboarding, recovery, and all-database rotation before changing provisioning.

### R2 — Generic transaction handle does not own its connection

**Class:** latent API defect/design risk; production use of this API not established. **Severity:** Medium now, High if used for atomic writes. **Confidence:** High.

Evidence:
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/database/driver.rs:57–66`
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/database/transaction.rs:19–38`
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/database/sqlite.rs:270–290,332–337`

BEGIN, operations, and COMMIT independently acquire pooled connections. The handle owns a driver reference, not exclusive connection access; it marks itself committed before commit succeeds. Concurrent pool use can break the promised transaction scope.

Counterevidence: inspected real atomic writes use borrowed rusqlite transactions, including `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-storage/src/hmem.rs:323–343`.

Plan: confirm callers; delete the unused misleading facade. If actual callers need a shared API, replace it with connection-owning transaction scope, not a wrapper around the unsafe contract. Validate two-connection concurrency, failed commit, and rollback without partial writes.

### R3 — Spreadsheet reconciliation has a crash-durability gap

**Class:** design risk. **Severity:** Medium. **Confidence:** High on sequence, Medium on deployment likelihood.

Evidence:
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-spreadsheet/src/artifact_store.rs:106–145,161–172,193–229`
- `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-spreadsheet/src/service.rs:585–602`

Revision publication syncs the file and renames it, then operation records use direct writes. A crash can leave a revision without a usable receipt. File sync alone does not establish parent-directory durability.

Counterevidence: absent operation records explicitly mean unknown, not unapplied; immutable bases limit destructive consequences. Thirteen spreadsheet tests passed. No power-loss experiment ran.

Plan: define process-crash versus power-loss guarantees, then replace the publication/receipt contract as needed. Make receipt publication atomic/durable and incomplete publication discoverable. Inject failures between revision and receipt publication; reopened state must never falsely report “not applied” or replay automatically.

**H1 — cached-open digest hypothesis:** `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-spreadsheet/src/service.rs:502–525` returns early for cached IDs before comparing the newly supplied digest. Test valid open followed by the same IDs with a bad digest. This was not dynamically reproduced; assess severity after verifying caller reachability and contract.

### R4 — Caller assertion is not operator approval evidence

**Class:** design risk. **Severity:** Medium. **Confidence:** High.

Evidence: `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:420–460`. An agent can supply `operator_confirmed: true` and a note without a host-observed confirmation event.

Counterevidence: this records advice application rather than executing the intervention, and keeps causal attribution unverified.

Plan: replace the boolean with a host-authenticated receipt wherever authenticated confirmation is required. If agent-reported confirmation remains useful, name it as a claim rather than granting it the same evidence status. Test that model arguments alone cannot create the stronger record.

### O1 — Documentation and contract inventory drift

**Class:** improvement opportunity with verified documentation defects. **Severity:** Medium. **Confidence:** High.

Evidence:
- `/home/mdz-axolotl/Clones/zed-kask/AGENTS.md:39–47` says 10 servers, describes an old host path, and references an absent per-tool contract document.
- `/home/mdz-axolotl/Clones/zed-kask/kask/docs/architecture/zed-host-architecture-plan.md:13–43` says 18 libraries/11 servers.
- `/home/mdz-axolotl/Clones/zed-kask/DIVERGENCE.md:234–240` includes a removed library and 11 servers.

Counterevidence: the runtime registry is centralized; the charter distinguishes IS/OUGHT. Missing documentation is not proof that all tool contracts lack tests.

Plan: generate inventory facts, validate links, restore or replace the missing contract reference, and remove obsolete claims. Inventory actual input/output/error behavior, including structured content versus string envelopes. Test category invariants, not hand-maintained tool-name lists.

### O2 — Focus architectural cleanup on behavior, not crate counts

**Class:** optional improvement opportunity. **Severity:** Low unless tied to a defect. **Confidence:** Medium.

Keep the spreadsheet actor: deleting it spreads non-Send engine ownership, executor bridging, staging, and revision management across MCP and GPUI. Keep typed uncertain-delivery errors: they encode essential retry information. Question the transaction facade because it hides ownership incorrectly.

Mechanical samples during review: event-store root had 8 public methods and 1 type with approximately 138 nonblank/noncomment pre-test lines; spreadsheet service had 12 public methods and 5 types with approximately 503 such lines. These are navigation signals, not reasons to split cohesive modules.

Server-library dependencies alone do not justify a new service layer. Extract only demonstrated shared behavior whose removal would duplicate invariant enforcement across callers. No compatibility requirement is a license to simplify, not a mandate to rewrite everything.

## 5. Strengths and constraints to preserve

- §13.1 held in inspected local manifests and the repository checker.
- Managed dispatch distinguishes proven non-delivery from uncertain delivery; see F7.
- Child environment clearing and selective injection: `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-mcp/src/runtime.rs:649–675`.
- Independently held, revocable grants: `/home/mdz-axolotl/Clones/zed-kask/kask/crates/kask_bridge/src/delegation_grants.rs:15–74`.
- Atomic call charging: `/home/mdz-axolotl/Clones/zed-kask/kask/crates/hkask-regulation/src/energy.rs:164–193`.
- Research strict transport validates redirect destinations and connect-time DNS and disables proxies: `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-research/src/research/providers/raw_fetch.rs:41–49,93–106,128–144`.
- Immutable spreadsheet bases, explicit unknown outcomes, evidence-aware regulation, and surfaced degradation.
- Existing real-process lifecycle tests and temporary-installation isolation checks.

Same-UID children are not OS-sandboxed; information flow is taint-unaware by documented decision. Call caps are neither consent nor monetary budgets. Do not convert those limitations into unstated security guarantees.

## 6. Execution program: complete vertical changes, not compatibility stages

Every work package follows: revalidate → reproduce safely → replace/update all callers → delete superseded path → verify → record result. Keep the tree buildable. No half-applied API changes in the shared checkout. “Incremental” means bounded and independently verified, not maintaining old and new implementations together.

### P0 — Re-ground and establish a safe baseline

1. Record HEAD, worktree/index status, and current inventory; preserve unrelated edits. Do not assume the review snapshot still holds.
2. Read applicable rules, principles, and DIVERGENCE seams. Identify current test commands and fixtures.
3. For every F/R/H item, record upheld, already repaired, disproved, or unverified, with current citations.
4. Select a single work package; identify its callers and negative tests before editing. Prefer an isolated branch/worktree under the operator's normal workflow, without discarding or copying unrelated dirty work.
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

Lean/lake were unavailable during review; no proof was attempted and no tools installed. A model proof would not establish that Rust checks every IPC variant, that a filesystem survives power loss, or that a remote process handles cancellation. Prefer implementation-level tests first; attempt formalization only with explicit assumptions, practical tooling, and a mapping from the model to implementation.

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
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-hkask-no-zed-deps.sh` | Passed |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-mcp-tool-tests.sh` | Passed, zero gaps; token heuristic, not complete behavioral coverage |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-reg-canonical.sh` | Passed |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-mcp-servers.sh` | Failed: spreadsheet absent from installer list |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/build/check-zed-isolation.sh` | Passed using fake temporary installation; emitted a missing retired-source-path grep warning |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-string-errors.sh` | Passed in library-code scope |
| `bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-version-sync.sh` | Passed for release 0.40.0 |
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
| P1b interruption | Open; not repaired | F2 upheld; no cancellation/deadline implementation or behavioral test this session |
| P1c training input | Repaired; focused verification passed | F3; changes observed in externally created commits `e21e3e5d2d` and `8a1bd7877b`; six fresh regression tests passed |
| P1d filesystem/gallery policy | Partial | F5 repaired and lifecycle suite passed; F4 containment still open |
| P2 invocation/outcome contract | Open; not repaired | F6, F7, R4 upheld; no replacement invocation contract yet |
| P3 persistence/recovery | Open; not repaired | R2/R3 upheld; H1 source-supported, not dynamically tested; R1 decision-gated |
| P4 packaging/CI/docs | Partial | F8 inventory/install checks repaired; reconnect suite wired but NOT executed. O1 partially repaired concurrently; remaining drift below |
| P5 optional simplification/formalization | Deferred | O2; only after behavior is pinned |

For each completed package record: current-source finding disposition; exact changed files; old paths deleted; test command and counts; failures/limitations; functional outcome; and commit hash if committed, otherwise explicitly “uncommitted.” Do not commit automatically or include another actor's staged work.

Completion means repaired behavior demonstrated at the relevant boundary, no superseded unsafe route remaining, applicable invariants passing, current docs, and a truthful account of residual risk. A compile-only result, zero-test filter, mock that removes the capability under test, or an empty degraded result is not completion.

### Execution baseline and finding dispositions — 2026-09-18

Initial HEAD was `7f1c83e558df951eec7f74d43fb623b02717c945`; the index was empty. Existing unstaged work comprised the docs portal, six corpus implementation files (including `index.rs`), prediction-markets tests, the review task, and this untracked plan. Those changes were preserved. Offline locked metadata returned 299 members, 19 `kask/crates/` libraries and 12 MCP packages. Read current root/agent rules, AGENTS, the whole plan including §10, principles, charter, interaction specification, documentation standard, and applicable D3/D8/D23 seams. No upstream Rust files were edited by this session.

Other actors advanced HEAD and staged/committed shared files during execution, including the repairs. The implementing agents issued **no stage or commit commands**. Initial verification base was `8a1bd7877b63f0cb2c71a57cc96ab84e69302f2a`, plus gallery/doc edits. Later observed HEAD `acbefb647bcb8011cfcf4dfc4ba4e1ad3d2d4d5e` contains the gallery repair and strengthened tests, externally committed alongside unrelated regulation work. The progress record, portal status and media-reference metadata remain **uncommitted**. Unrelated regulation/bridge changes and all index contents were left untouched. This is a moving-checkout record, not an assertion that every check ran on an immutable tree.

Source citations below are relative to `/home/mdz-axolotl/Clones/zed-kask/`; they record execution-time inspection, not new exploit reproductions.

| Item | Current disposition and evidence |
| --- | --- |
| F1 | Upheld at initial baseline; repaired admission and native tool-ceiling paths in the P1a continuation below. Broader per-call MCP identity is not claimed repaired. |
| F2 | Upheld: `crates/agent/src/tools/context_server_registry.rs:574–592` still awaits the managed source without selecting cancellation; runtime call at `kask/crates/hkask-mcp/src/runtime.rs:1686–1698` has no execution deadline. |
| F3 | Upheld at baseline, repaired below. Both providers use the repaired shared generator. Prior TRL removal was retained; only Axolotl/Ludwig are in scope. |
| F4 | Upheld: `kask/mcp-servers/hkask-mcp-corpus/src/tools/gather.rs:199–210` validates the directory then writes a leaf; shared `validation.rs:289–312` reconstructs missing suffixes before `tools/document.rs:72–89` writes. Existing relative-basename and QA-specific link fixes are not race-resistant publication and were not recreated. |
| F5 | Reproduced and repaired below. Existing transcript-detachment and unlink-failure identity preservation were retained. |
| F6 | Upheld: `kask/crates/hkask-mcp-server/src/server/transport.rs:91–118` retains anonymous startup fallback; `server/tool_span.rs:140–169` attributes process context; kanban task claim at `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:885–895` still uses `self.webid`. No cross-user compromise demonstrated. |
| F7 | Upheld: `crates/agent/src/tools/context_server_registry.rs:986–1046` still retries non-timeout errors after possible delivery. Managed typed non-delivery behavior remains intact. |
| F8 | Reproduced inventory failure; repaired inventory and CI wiring below. Fixture process tests remain permission-gated and unexecuted. |
| R1 | Upheld by source-only inspection of the provisioning policy at `kask/crates/hkask-keystore/src/passphrase.rs:1–17` and `keychain.rs:402–424`; no keychain or deployed database inspected, no secret rotated. Onboarding/recovery decision remains open. |
| R2 | Upheld: `kask/crates/hkask-storage/src/database/driver.rs:29–35,57–66` and `database/transaction.rs:19–38` retain the connectionless facade; production usage not established. Safe borrowed-connection transactions remain. |
| R3 | Upheld design risk: `kask/crates/hkask-spreadsheet/src/artifact_store.rs:106–145,193–229` separates revision publication and direct receipt writes. No fault injection or power-loss guarantee established. |
| R4 | Upheld: `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:428–460` still accepts caller confirmation assertions, not host approval receipts. |
| H1 | Source-supported, dynamic test still outstanding: cached return at `kask/crates/hkask-spreadsheet/src/service.rs:502–525` precedes digest comparison; public `open` returns the supplied reference. Fresh apply independently verifies its base. |
| O1 | Partially repaired by other actors: AGENTS/server-reference counts and missing reference improved. Remaining stale claims include launcher wording in `AGENTS.md:41` and `kask/docs/README.md:13`, plus 18/11 counts in `kask/docs/architecture/zed-host-architecture-plan.md:43`. The advertised `docs/ci/verify-docs.sh` is absent at both root and `kask/` locations. |
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

**Exact changed files (relative to `/home/mdz-axolotl/Clones/zed-kask/`):**

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
/home/mdz-axolotl/Clones/zed-kask/kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md

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
