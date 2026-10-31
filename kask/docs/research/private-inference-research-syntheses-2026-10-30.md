---
title: "Private-inference research syntheses (D-1)"
audience: [operators, architects, agents]
last_updated: 2026-10-30
version: "1.0.0"
status: "Proposed — delivered for PI review 2026-10-30; three bounded research runs (R-1 private/secure inference stacks, R-2 provider MCP gateways, R-3 product direction, all completed in the run ledger) with per-claim verification states and window discipline 2026-01-30 through 2026-10-30"
domain: "Private inference / research"
mds_categories: [domain, trust]
---

# Private-inference research syntheses (D-1)

- **Status:** delivered for PI review — 2026-10-30. Part of the Private-Inference MCP Server
  planning engagement (requirements, architecture plan, and point-of-view memo are the
  companion deliverables under `kask/docs/plans/`).
- **Method:** three bounded research runs, each ≤8 queries, every source recorded in the
  server-side run ledger with an agent-declared verification state:
  - **R-1** (private/secure inference stacks) — run `5bb6f2787e33a095`, 7 of 8 queries used, status `completed`.
  - **R-2** (provider MCP servers as gateways) — run `47b59ee22d0cd91e`, 5 of 8 queries used, status `completed`.
  - **R-3** (product-direction scan) — run `ede39835b3aa95b1`, 3 of 8 queries used, status `completed`.
- **Window discipline:** 2026-01-30 through 2026-10-30. Every claim below names its date;
  undated or out-of-window sources are marked and never used as window evidence.
- **Verification states** (per the research-run ledger): `verified` = server-recorded source
  whose recorded excerpt states the claim; `partial` = source read but not primary, or
  primary evidence not fully extracted; `inferred` = agent judgment over recorded evidence
  not itself stating the claim; `not_checked` = a dimension the run did not probe. An
  absence of a published source is reported as a finding, never treated as a premise.
- **Verification oracle for this document:** the three run ledgers (IDs above) — every
  citation's excerpt and verification state is re-readable there via `get_research_run`.

---

## R-1 — Private and secure inference stacks

### F1.1 vLLM's built-in API key is not a perimeter

`--api-key` / `VLLM_API_KEY` authenticates only requests under the `/v1`, `/v2`, and
`/inference` path prefixes; other endpoints on the same port (`/invocations`, `/pause`,
`/abort_requests`, the gRPC listener) are unauthenticated by default, and vLLM's own
documentation recommends a reverse proxy that allowlists exposed endpoints as the most
effective additional control. The key must be set via the `VLLM_API_KEY` environment
variable, never a bare CLI argument (process listings / shell history).
**[partial]** — two independent in-window secondary sources citing vLLM's docs
(Decryption Digest, 2026-09-04; GitCode blog, 2026-09-06, referencing vLLM's
`SECURITY.md` + `docs/usage/security.md`); the primary vLLM security page was not
extracted within the query budget.

In-window corroboration that this is a live perimeter concern, not theory: **CVE-2026-22778**
(CVSS 9.8; API-key bypassable; shipped ~302 days; safe floor v0.32.0) **[inferred]** — one
secondary source (Particula, 2026-07-29), not corroborated by a second source in this run.

**Design consequence (→ FR-2):** a private vLLM endpoint must be fronted by an
allowlisting proxy and carry a version floor check; the connection profile cannot treat
"vLLM with --api-key" as sufficient.

### F1.2 Deployment hardening baseline (in-window practitioner literature)

Consensus shape across three in-window guides (Medium, 2026-03-11; Decryption Digest,
2026-09-04; Systems Hardening, 2026-03-18) **[partial — secondary sources]**: TLS-terminating
front proxy (nginx class) adding auth + rate limiting + endpoint allowlist ahead of vLLM;
vLLM bound to localhost/private interface; non-root, read-only filesystem; network
isolation; secrets from a manager, not CLI args. GPU endpoints are stated at $2–30/GPU-hour
unprotected-exposure cost, making authentication a first-order concern (Systems Hardening,
2026-03-18).

### F1.3 Hetzner: private networking exists, encryption does not

Hetzner Cloud Networks provide private layer-3 links over dedicated interfaces, but
**"Traffic between cloud servers inside a Network is private and isolated, but not
automatically encrypted. We recommend you use TLS or similar protocols to encrypt
sensitive traffic."** Also verified on the same page: layer-3 routing via a network
gateway (no direct physical links), IPv4-only, RFC 1918 ranges, and vSwitch coupling for
dedicated (Robot) servers into the same network. **[verified — primary docs, full extract
in ledger]** (Hetzner docs FAQ; page is a living document, last-change stamp 2020-02-06 —
the extract was taken in-window).

**Design consequence (→ FR-2):** provider private networking is an isolation tool, not an
encryption tool. The encrypted-channel requirement stands even when traffic never leaves
the provider's fabric.

### F1.4 RunPod: tenancy tiers by data-center class, not by hardware isolation

RunPod's own docs: Secure Cloud = **T3/T4 data centers**, high redundancy, "best for:
production, sensitive data"; Community Cloud = **peer-to-peer providers**, variable
reliability; and **"Runpod is no longer accepting new hosts for Community Cloud"**
(existing resources remain). Pod creation carries a `Cloud Type: SECURE` parameter.
**[verified — primary docs snippet in ledger]** (Choose a Pod, retrieved in-window).

**Design consequence:** on RunPod, "private by design" at the provider layer means
Secure Cloud + single-tenant pod + keys held by the PI; it is a datacenter-class and
tenancy claim, not a cryptographic one (see F1.6).

### F1.5 ArubaCloud: dedicated-GPU posture, EU provider

Aruba's own page: GPU on Demand **"allows you to integrate dedicated NVIDIA GPUs into your
existing Kubernetes cluster in minutes"** — "Dedicated GPU power, when you need it."
**[verified — primary page snippet]**. Secondary in-window source lists the GPU range as
RTX 4070/4080/L40S up to higher-end, and mentions both "shared environments" and
"reserved GPU servers" in their AI infrastructure **[partial — Server Parts EU, 2026-06-14]**;
per-offering tenancy details were not extracted and are **not_checked**.

### F1.6 TEE/confidential computing: absent at all three target providers (absence finding)

No published TEE / confidential-computing instance offering was found at RunPod, Hetzner,
or ArubaCloud in the window, across all R-1 queries. TEE-class confidential computing for
GenAI is offered at hyperscalers (Azure/GCP/AWS, on Intel TDX / AMD SEV-SNP instances)
**[partial — Fortanix FAQ, undated]**, and in-window research artifacts exist for
confidential Kubernetes (Confidential.ai, C8s paper, April 2026) — none at the three
target providers. This is an **absence finding, not a premise**: the plan must not assume
TEE, and must not claim it is impossible — it reports that no published source was found.

**Design consequence (→ D-4 memo):** private-by-design rests today on dedicated
single-tenancy + encrypted transport + PI-held keys; TEE is a named upgrade path the plan
re-evaluates if any of the three providers publishes an offering.

### F1.7 Connection options menu (what a secure encrypted channel concretely requires)

In-window practice across the three clouds converges on a menu:
- **TLS fronting proxy on the instance** (F1.2) — required regardless of transport because
  of F1.1.
- **WireGuard-style overlay / managed mesh** — multiple in-window practitioner sources
  tunneling self-hosted LLM inference: Tailscale + Ollama GPU tunneling (Keane, 2026-02-15),
  home AI server access via Tailscale (RunAIHome, 2026-05-17), Tailscale Serve for MCP
  endpoints (Plane-CE deployment doc, decision dated 2026-07-12), WireGuard-vs-Tailscale VPS
  guide (virtua.cloud, 2026-03-20) **[partial — practitioner sources]**. Ecosystem signal:
  Tailscale acquired Border0 (2026-03-17) to add SSH/user-level access controls to the mesh
  **[partial — deal-report source]**.
- **SSH tunnels** — available on all three (RunPod docs list SSH connection options;
  nav-level evidence **[not_checked]** for Hetzner/Aruba).
- **Provider private networking** — Hetzner: isolation only, not encryption (F1.3);
  RunPod global networking and Aruba network semantics **[not_checked]** within budget.

**Design consequence (→ FR-2):** the connection profile derives its requirements from
R-1 as: TLS to the endpoint (cert-validated), the vLLM surface restricted to the
OpenAI-compatible `/v1` prefixes through an allowlisting proxy, credentials fail-closed,
and an overlay or tunnel chosen per provider at intake. Hetzner additionally requires
the TLS/overlay even on its private networks.

### F1.8 Tenancy isolation for multiple users

vLLM ships single-key authentication; multi-user authorization in front of self-hosted
vLLM is a gateway concern (the in-window gateway pattern: Inference Gateway, LiteLLM-class
proxies — see R-2). **[inferred]** — no primary multi-tenant vLLM document was extracted
within budget. Consequence: the group-access question (PI question 3) is answered by the
gateway/proxy layer and the PI's boundary decision, not by vLLM itself.

---

## R-2 — Provider MCP servers as gateways

### F2.1 RunPod ships official MCP servers — infrastructure control planes

Verified from RunPod's primary docs (page last modified 2026-10-26 — actively maintained)
and the vendor announcement blog (2026-09-13, in-window):

- **Two servers:** (1) the **RunPod API MCP server** — manage **Pods, Serverless endpoints,
  templates, network volumes, container registries** through the REST API; (2) the
  **RunPod docs MCP server** — documentation search, no authentication. **[verified]**
- **Distribution and auth posture:** hosted at `https://mcp.runpod.io/` (Streamable HTTP)
  with OAuth "Sign in with Runpod" producing a session-scoped API key — **"Nothing is
  stored on disk"** — or a local `npx @runpod/mcp-server` stdio server with
  `RUNPOD_API_KEY`. **[verified — primary docs extract]**
- **License and source:** Apache-2.0, `github.com/runpod/runpod-mcp`. **[verified — vendor
  blog snippet]**
- **Usage shape:** "create and manage Pods, deploy Serverless endpoints, submit and monitor
  jobs, browse GPU availability and pricing, and pull up your spend history, all through
  plain conversation"; usage examples include pod creation with `Cloud Type: SECURE`.
  **[verified]**
- RunPod also ships **agent skills** for coding tools alongside the MCP servers
  (nav-level evidence, **[not_checked]**).

### F2.2 The MCP-to-inference seam: control plane, not data path

The engagement's opening expectation was that provider MCP servers act as provisioning/
policy **control planes**, with inference routing staying in the provider layer (the
zk-ref-open openrouter routing model: a model-routing gateway sits between the caller and
many providers). The falsifier probe — a provider MCP server operating as a **data-path
inference gateway** — returned **no instance**:

- RunPod's servers manage infrastructure and search docs; model tokens do not flow
  through them. **[verified]**
- The one in-window gateway found that exposes MCP (Inference Gateway, `POST /mcp`,
  speaking only MCP 2026-07-28's `server/discover` / `tools/list` / `tools/call`) is an
  **aggregator of MCP tools** — its inference routing remains direct HTTP to providers.
  **[partial — README preview]**
- GPUClouds' MCP server (2026-10, filed 2 days before this writing) is a **price
  comparison** service. **[partial — issue snippet]**

**Expectation verdict: held.** The architecture plan adopts the control-plane design on
this evidence, and the memo records what would reverse it.

### F2.3 The MCP specification evolved in-window: 2025-11-25 → 2026-07-28

Verified from the primary changelog (modelcontextprotocol.io, revision 2026-07-28,
retrieved in-window; corroborated by two in-window migration accounts — Haning 2026-07-30,
Apify 2026-08-18) **[verified — primary extract]**:

- **Stateless protocol:** the `initialize` handshake is removed; every request carries
  protocol version and client capabilities in `_meta`; servers needing cross-call state
  use **explicit, server-minted handles passed as ordinary tool arguments** (SEP-2575,
  SEP-2567).
- **Multi Round-Trip Requests (MRTR, SEP-2322)** replace server-initiated requests
  (`elicitation/create`-class interactions) with `input_required` results + retry.
- **Roots, Sampling, and Logging are deprecated** (SEP-2577) — the suggested Sampling
  migration is **"integrate directly with LLM provider APIs"**: the spec itself steers
  servers away from mediating model tokens, independent corroboration of F2.2.
- **Tighter authorization:** `iss` validation (SEP-2468), credentials bound to the issuing
  authorization server (SEP-2352), RFC 9207.
- **Caching-aware lists** (`ttlMs`/`cacheScope`, SEP-2549) and a SHOULD for deterministic
  `tools/list` ordering to improve prompt-cache hit rates.

**Design consequence:** the guided workflow's multi-step approvals fit the stateless
server-minted-handle pattern exactly (an approval token is an ordinary tool argument —
the design already wanted this; the spec now prescribes it). The kask server stack's
(`hkask-mcp-server`/rmcp) protocol-version posture against 2026-07-28 is an implementation-
engagement verification item, not assumed here.

### F2.4 Hetzner and ArubaCloud: no official MCP server found (absence finding)

No official MCP server from Hetzner or ArubaCloud surfaced in this run's searches. Hetzner
maintains a mature IaC surface (Terraform provider, `hetznercloud/terraform-provider-hcloud`;
version-compare page retrieved in-window **[partial]**) and a Cloud API; ArubaCloud's API
ecosystem was **not probed** within budget **[not_checked]**. Reported as findings: the
provisioning integrations for those two providers will be provider-API clients (or IaC
wrappers), not MCP consumers.

### F2.5 RunPod Organizations (Beta) — group-access evidence at the provider layer

RunPod's docs tree carries an "Organizations (Beta)" section (overview, access, roles,
groups, resource tagging, billing) — nav-level evidence only, **[not_checked]**. Relevant
to PI question 3 (group membership/access model): a provider-side group construct exists
at least at RunPod; its semantics were not read in this run.

---

## R-3 — Product-direction scan

### F3.1 Open-weight models: a real selection space with license/VRAM/context trade-offs

From the self-hosted leaderboard (Onyx, updated 2026-10-13; table rows verbatim in ledger)
**[verified]** and the October 2026 ranking snapshot (BenchLM: Kimi K3 at 69.3 leads, ahead
of Ling 3.1 Flash 65.4 and DeepSeek V4.1 Flash 65.2) **[partial — single board]**:

| Example model (2026-10) | Params | Context | License | Weights / VRAM |
|---|---|---|---|---|
| Devstral-2-123B (Mistral) | 123B | 256K | Modified MIT | 65 GB / 246 GB |
| Mistral Small 4 | 119B | 262K | Apache 2.0 | 60 GB / 238 GB |
| Nemotron 3 Super (Nvidia) | 120B | 1M | NVIDIA Open Model | 60 GB / 240 GB |
| Kimi K2.6 (Moonshot) | 1T | 262K | Modified MIT | 500 GB / 2000 GB |
| DS-R1-Distill-Qwen-14B | 14B | 128K | MIT | 8 GB / 28 GB |

**Design consequences (→ FR-5, D-4):** (1) the five model-selection dimensions
(benchmarks, licensing, VRAM fit, context length, maintenance status) are real and
independent — licenses alone span MIT/Apache-2.0/Modified MIT/Llama License/NVIDIA Open;
(2) frontier open-weight models are multi-GPU cloud-scale (≈0.5–2 TB VRAM), while
capable mid-size models fit single modern GPUs — the crossover is the routing policy's
subject matter; (3) leaderboards are volatile — rankings here are 2026-10 snapshots, and
the model-brief tool must re-source at run time, not hardcode.

### F3.2 Hybrid local/cloud inference is the field's direction — with privacy as a named driver

In-window sources, all **[partial — practitioner/research sources]**:
- Local-first agents with cloud-edge split for **privacy, latency, and cost** (Zylos
  Research, 2026-05-10).
- Hybrid local-and-cloud LLM router for token-cost balancing at high volume (nasscom,
  2026-06-27).
- Multi-LLM routing with explicit policy across hosted providers and self-hosted Qwen
  (API7, 2026-03-02).
- On-device-first routing ("cloud when it counts") as a productized router (oneinfer-edge,
  2026-06-03); on-device-first hybrid inference on AI-PCs (IEEE, in-window).

The repo's own state matches the local leg: `ollama` is a wired provider in
`hkask-inference` (`kask/crates/hkask-inference/src/config.rs`), and a RunPod serverless
vLLM endpoint precedent exists (D29) — the hybrid is not speculative on this codebase.

### F3.3 Agent harnesses: single-model era ending; routing becomes a harness feature

**[partial — mix of research and vendor sources]**: "The era of the single-model agent is
ending" (Zylos, 2026-03-02); harness SDKs gaining explicit model-routing design documents
(strands-agents harness-sdk PR #3217, 2026-07); model lock-in by leading harnesses driving
external router layers (OpenHands blog, 2026: Claude Code is Anthropic-model-only, no
BYOK — teams wanting cross-model routing add "a separate gateway or router layer");
harness evolution measured with models routed via official APIs or OpenRouter
(HarnessDev, arXiv:2609.01437, 2026-09).

**Design consequence (→ D-4):** private inference is not a bolt-on at odds with the
field's direction — the field is already multi-provider and privacy-motivated; a
PI-controlled routing boundary rides the trend rather than fighting it.

### F3.4 GPU-cloud economics snapshot

RunPod at $120M ARR as of January 2026, raised ~$20M (ChatForest review, 2026-05-06)
**[partial]** — one signal of provider durability among the three targets; a 2026-07-25
GPU-cloud benchmark (AWS vs RunPod vs Hyperbolic) surfaced but was not extracted
**[not_checked]**.

---

## Verified / inferred / not-checked — separation summary

**Verified (primary-source excerpts in the run ledgers):**
Hetzner private networks not automatically encrypted (F1.3); RunPod Secure/Community
tenancy table + Community-sunset (F1.4); Aruba dedicated-GPU offering (F1.5); RunPod's
two MCP servers, their auth posture, and active maintenance (F2.1); MCP 2026-07-28
statelessness/MRTR/deprecations/authz tightening (F2.3); the self-hosted model table
with license/VRAM/context (F3.1).

**Inferred (agent judgment over recorded evidence):** CVE-2026-22778 details (F1.1);
vLLM single-key authz + gateway-fronted multi-user pattern (F1.8); the control-plane
seam verdict synthesizing F2.1–F2.3 (F2.2 — the expectation held; no counterexample
surfaced).

**Not checked (named, not assumed):** vLLM primary security docs (budget — F1.1 stays
partial); Hetzner/Aruba network and API semantics beyond F1.3/F1.5; RunPod global
networking + security-and-compliance page; ArubaCloud API ecosystem; RunPod Organizations
pages (nav-level only); Hetzner/ArubaCloud MCP absence is an absence finding, not proof
(F2.4); GPU-cloud economics benchmark (F3.4).

**Absence findings (reported, never premised):** (1) no TEE/confidential-computing
offering found at any of the three providers in-window (F1.6); (2) no official
Hetzner/ArubaCloud MCP server found in-window (F2.4); (3) no data-path provider-MCP
inference gateway found in-window (F2.2).

## Citations (APA, generated from run-ledger sources)

R-1: Hetzner (n.d.). *FAQ — Hetzner Cloud Networks.*; Runpod (n.d.). *Choose a Pod —
Secure Cloud vs Community Cloud.*; Hannecke, M. (2026). *vLLM in Production: A Security
Hardening Guide…*; Decryption Digest (2026). *vLLM Security Hardening for Production…*;
Mondragon, S. (2026). *vLLM Security Hardening: Safe Version After CVE-2026-22778*;
Aruba Cloud (n.d.). *GPU on Demand for Kubernetes*; Server Parts EU (2026). *GPU as a
Service in Italy…*; Fortanix (n.d.). *Trusted Execution Environments (FAQ)*; Nadeau, J.
(2026). *Homelab networking… Tailscale and Caddy*; virtua.cloud (2026). *WireGuard vs
Tailscale on a Linux VPS.* (Full URLs in run ledger `5bb6f2787e33a095`.)

R-2: Model Context Protocol (2026). *Key Changes — MCP Specification Revision
2026-07-28*; Runpod (2026). *Use Runpod's MCP servers*; Runpod (2026). *…introducing the
Runpod MCP server*; Runpod (n.d.). *runpod/runpod-mcp*; Inference Gateway (n.d.).
*inference-gateway*; Haning (2026). *The MCP 2026-07-28 Specification Revision*; Apify
(2026). *Migrating the Apify MCP Server to the new stateless spec.* (Run ledger
`47b59ee22d0cd91e`.)

R-3: Onyx AI (2026). *Best Self-Hosted LLM Leaderboard 2026*; BenchLM.ai (2026).
*Open-Source LLM Leaderboard 2026*; LMSA (2026). *Open-Weight LLM Leaderboard*; Zylos
Research (2026). *Local-First AI Agents…*; Zylos Research (2026). *AI Agent Model
Routing…*; Yadlapalli, P. (2026). *Balancing AI Token Costs, Part 2…*; API7 (2026).
*Multi-LLM Routing with Qwen and Cloud Models*; OpenHands (2026). *The Best AI for
Coding…*; Rombaut et al. (2026). *HarnessDev* (arXiv:2609.01437). (Run ledger
`ede39835b3aa95b1`.)