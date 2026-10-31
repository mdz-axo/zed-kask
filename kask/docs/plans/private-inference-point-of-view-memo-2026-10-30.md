---
title: "Private-inference point-of-view memo (D-4)"
audience: [operators, architects, agents]
last_updated: 2026-10-30
version: "1.0.0"
status: "Proposed — delivered for PI review 2026-10-30; the view is a judgment (P) checked by its falsifier list; every claim carries its citation and verification state from the research syntheses; not yet authorized"
domain: "Private inference / product direction"
mds_categories: [domain, trust]
---

# Private-inference point-of-view memo (D-4)

- **Status:** delivered for PI review — 2026-10-30. Evidence base: research run
  `ede39835b3aa95b1` (R-3, product direction), with R-1/R-2 findings where they bear
  (syntheses: `kask/docs/research/private-inference-research-syntheses-2026-10-30.md`).
- **Verification oracle for this document:** every claim carries its citation and
  verification state from the research syntheses; the memo's *view* is a judgment (P) —
  its check is the falsifier list at the end: each entry names the observation that
  would change the view.

## The view

**Build the hybrid: a local leg that is private by default, a private-cloud leg that is
private by design, one explicit routing policy the PI owns, and an MCP server that is a
control plane — never a token path.**

Four positions, each argued from in-window evidence:

### 1. The local leg is private by default — and it is already half-built

`ollama` is a wired provider in `hkask-inference` (qualified-name routing,
`kask/crates/hkask-inference/src/config.rs`); the 2026 field direction is local-first with
cloud escalation, with privacy named as a driver alongside latency and cost (Zylos,
2026-05-10 [partial]; on-device-first routing as product, oneinfer-edge 2026-06-03
[partial]). A capable 14B–mid-size model runs on a single modern GPU or workstation
(self-hosted leaderboard, 2026-10-13 [verified]: 8 GB/28 GB for a 14B distill; 65–60 GB
weights for 119–123B). Private-by-default means: unmarked threads stay where they are;
threads the PI marks private resolve to the local leg **when the model fits**, at zero
cloud exposure. The local leg should exist in the routing policy from day one even if
the cloud leg is the headline.

### 2. The private-cloud leg is private by design — on today's honest boundary

Private-by-design, per `dpv:PrivacyByDesign` (W3C DPV 2.3 [verified anchor]), means
protection built into the design. What the research supports as the actual boundary at
RunPod/Hetzner/ArubaCloud in 2026-10: **dedicated single tenancy** (RunPod Secure Cloud
T3/T4 [verified]; Aruba dedicated GPUs [verified]; Hetzner dedicated/bare-metal),
**encrypted transport** (TLS everywhere; Hetzner private networks explicitly not
encrypted [verified]), **PI-held keys** (fail-closed chains), and an **allowlisted
surface** (vLLM's key is not a perimeter [partial]). It does **not** include TEE: no
confidential-computing offering was found at any of the three providers in-window
[absence finding]. The memo therefore rejects any confidentiality claim stronger than
"the provider hosts your single-tenant instance and sees the host, not your keys" —
P11.1's corrected-claim discipline applied to the cloud leg. The frontier open-weight
models that justify the cloud leg are real (Kimi K3 tops October boards [partial]; 1T-class
models need ~0.5–2 TB VRAM [verified table]) — the cloud leg exists for work the local
leg cannot hold, not for its own sake.

### 3. The MCP server is a control plane; routing stays in the provider layer

The zk-ref-open openrouter routing model (a model-routing gateway sits between the caller
and many providers; the prefix is the routing key) is realized in this codebase by the
provider registry (`hkask-inference` + zed's `LanguageModelRegistry`). The engagement's
falsifier probe — a provider MCP server operating as a data-path inference gateway —
found **none**: RunPod's official MCP servers manage pods/endpoints/volumes
[verified], the one gateway that exposes MCP does it as a tools aggregator [partial], and
the MCP 2026-07-28 spec itself deprecates Sampling with the migration "integrate directly
with LLM provider APIs" [verified]. The industry's own harnesses are adding routing
layers for cost and choice (Zylos 2026-03 [partial]; OpenHands on Claude Code lock-in
[partial]; nasscom's hybrid router 2026-06 [partial]). So: the new MCP server provisions,
verifies, and holds policy; inference flows through the registry it already flows
through. Anything else adds a trust boundary and latency to every token for no capability.

### 4. Guided, PI-decided workflows are the product, not decoration

RunPod's MCP servers prove provider-side control planes are a 2026 product category
(released 2026-09-13, actively maintained [verified]) — but they serve *any* MCP client,
unopinionated about privacy, memory, or consent. The differentiator here is the thesis
charter: every state-changing step bound to explicit PI approval (FR-1), the routing
policy PI-owned and agent-unwritable (FR-3), memory segmented with named falsifiers
(FR-4), model selection a guided research process where the PI decides (FR-5). The
capability deepens the PI's seat at the bench precisely because the cloud is the
convenience and the PI remains the authority.

## What would change the view (falsifiers, each named)

1. **TEE maturation at the target providers.** If RunPod/Hetzner/ArubaCloud (or their
   peers at comparable price) publish confidential-computing GPU instances in the
   verified style of Azure/GCP/AWS TDX/SEV-SNP, the cloud leg's boundary claim upgrades
   from tenancy+transport+keys to attestation — and the design should adopt it rather
   than argue against it. (Today: absence finding F1.6.)
2. **Local capability stagnation vs the group's real workloads.** If the models the
   group actually needs stay above the single-GPU band while mid-size capability plateaus
   (the 2026-10 table is a snapshot, not a trend line), the local leg demotes from
   default-private to private-for-small-tasks — a routing-policy recalibration, visible
   in FR-5 briefs over time.
3. **Provider MCP commoditization of guided provisioning.** If a provider ships the
   guided, approval-bound, privacy-aware provisioning workflow itself (RunPod's servers
   are close on infrastructure, absent on consent/memory), our server should thin into
   policy + privacy layers over theirs — the deletion test in the architecture plan is
   the standing instrument for that decision.
4. **A data-path provider MCP gateway.** If provider MCP servers begin serving model
   tokens (the F2.2 counterexample), the control-plane/data-plane split needs rework —
   the architecture plan records this as the design's central assumption, with the
   2026-07-28 Sampling deprecation as current evidence against it.
5. **VRAM economics shift.** A collapse in per-GB GPU cost (or the group acquiring a
   large local GPU) moves the crossover point and weakens the cloud leg's premise;
   a rise strengthens it. The routing policy is settings-borne precisely so this is a
   recalibration, not a rebuild.

## The values check

V4 (subsidiarity): the PI holds routing, credentials, and every approval — no capability
moves a decision up. V7 (the limit as positive): the private boundary is a designed
limit, surfaced rather than hidden. V2 (truth as a common good): the model brief cites
its sources and labels what it did not check. The discernment test: a group of
researchers gaining private frontier-model inference with their judgment — not their
data — as the cost, makes the work more human, not less.