---
name: provider-audit
description: "Two-directional provider contract audit over kask-adopted vendors: inbound (provider alignment with principles P1–P4 and values V1–V8) and outbound (our code's alignment with provider capabilities), over the profile layer at kask/registry/providers/. Carries the provider lifecycle: adoption gate, drift trigger, scheduled cadence, announcement trigger, and a retirement pass."
audience: platform
---

# Provider Audit

Two-directional provider contract audit over kask-adopted vendors — inbound (does the provider meet our principles?) and outbound (does our code use the provider's capabilities correctly and currently?) — over the machine-walkable profile layer at `kask/registry/providers/`, with the adoption, drift, scheduled, announcement, and retirement lifecycle.

## Reference models

- **The provider-contract canon (project rulings).** D5 one-key-one-location keychain discipline — `kask/crates/kask_bridge/src/inference_providers.rs` module docs; the `McpToolError` per-variant taxonomy — a missing caller-named resource is `not_found` naming the thing, a missing credential is `permission_denied` naming the env var (`.rules`, operator ruling 2026-10-05); closed-vocabulary field normalization; `BuiltinMcpServer` per-server allowlist-alignment test discipline — `kask/crates/kask_bridge/src/mcp_servers.rs` tests.
- **Magna Carta P1–P4** (`kask/docs/architecture/core/magna-carta.md`) and **values V1–V8** (`kask/docs/architecture/core/magnifica-humanitas.md`) — the inbound check set's source of authority.
- **`DIVERGENCE.md`** — seam accounting for any zed-side integration surface a finding touches.
- **Audit as verification discipline:** assertion-orientation with an evidence hierarchy (the `grounding-verify` skill's ISA-style reference set) — every check names its assertion and cites dated evidence; a check that was not performed is `unverified`, never passed.
- **`onto_anchor`:** "audit" resolves at tier `domain_supplement` (`dpv:Audit`, W3C DPV 2.3) — state as of 2026-11-01; anchored on the published definition, never a private one.

## D/P labelling (P8.4)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Enumerate (derive) | D | `bash kask/scripts/check-provider-profiles.sh` — the adopted set derived from tree anchors, never memory; its exit code is the gate (`PA-1`) |
| Inbound B1–B6 | P | the operator; every verdict cites dated evidence (doc URL + extraction date, probe receipt, or tree file:line) |
| Outbound O1–O4 | P (O1 grep receipts are D-assisted) | the operator; grep receipts pinned to file:line; `PA-2` reads both renders' `checks` |
| Calibration | D | `lisp_eval` gate `PA-3` over the two retrodiction renders |
| Findings routing | P | the operator accepts/rejects/overrides |
| Adoption recommendation | P | the operator decides; contested merits route to `mcda` |
| Convergence | D | `lisp_eval` gate `PA-4` over the renders and the re-entry log |

## Initial and target condition

- **Initial condition:** the trigger (adoption | drift | scheduled | announcement | retirement), the vendor(s) in scope, the gate script's derived adopted set, and the profile layer state (`first_audit_pending: true` markers on fields no run has read yet).
- **Target condition:** a findings artifact with severity and file:line-or-URL evidence per finding; routing complete (blocking → the operator immediately, major → a kanban repair task, minor → the hygiene backlog); the retrodictive calibration receipts recorded (one known-good vendor passes, one known-bad instance from the `.rules` record flags); profiles updated (dated terms, consumed capabilities, deliberate non-consumption); and — for adoption runs — a go/no-go recommendation delivered to the operator as a decision, never adopted.

## When to Use

- A new provider/vendor is proposed for adoption — audit **before** integration (the gate; the Tinker decision package of 2026-10-13 was the first instance).
- An incident of a class the audit checks (credential misrouting, silent fallback, error misclassification, stale doc) fires a drift trigger → targeted re-audit of that vendor.
- Scheduled cadence: a quarterly outbound pass (vendor capability/deprecation waves are quarterly) and an annual inbound terms re-read (data terms drift).
- A vendor announces a capability or a deprecation → an outbound check of that vendor.
- A vendor leaves the adopted set → the retirement pass.

## When NOT to Use

- Auditing MCP *tool surfaces* against reference models — `mcp-tool-review`.
- Auditing D-seams against upstream — `kask-seam-audit`.
- A one-off vendor question with no adoption or lifecycle stake — answer it directly.
- Choosing between providers on contested merits — `mcda`; this skill feeds it evidence, it does not rank.

## Instructions

### Step 1 — Trigger and scope

Fix the trigger (adoption | drift | scheduled | announcement | retirement) and the vendor(s) in scope. For an adoption run, the audited vendor is the candidate; for drift, the vendor behind the incident; for scheduled, the full derived set.

### Step 2 — Enumerate (D)

1. Run `bash kask/scripts/check-provider-profiles.sh` via `terminal`. It derives the adopted set from four tree anchors — inference descriptor ids (`INFERENCE_PROVIDERS`), media provider structs, research provider modules, and `DATA_SERVICES` credential keys — and pins the structural contract: a profile per adopted vendor, required keys present, id/status consistency, no zombie retirements, no active orphans.
2. `PA-1` is the script's own exit: run it as `bash kask/scripts/check-provider-profiles.sh; echo "exit=$?"` and keep the output verbatim in the run directory (`~/Documents/zk-data/skills/provider-audit/{date}-{run}/enumerate.txt`). The script refuses an empty derived set, a missing profile, a zombie retirement and an active orphan by name; a nonzero exit is a finding before any semantic reading begins, and its `OK:` line carries the derived vendor count. No count or list is retyped from it.

### Step 3 — Load profiles

Read `kask/registry/providers/<vendor>.yaml` per scoped vendor. Fields marked `first_audit_pending: true` (data terms, status surface, pricing) are read-and-dated **this run** — never assumed, never smoothed.

### Step 4 — Inbound pass (B1–B6), per vendor

Render `provider-audit/inbound` with the vendor, its profile, and the trigger; produce one dated-evidence verdict per check:

- **B1 credential contract:** one key, one location (D5) — the profile's slot matches the tree's reads; an unset key surfaces `permission_denied` naming the env var; no silent fallback.
- **B2 error taxonomy:** failures classify per-variant (auth, rate, not-found), never blanket `internal`.
- **B3 data terms:** no default training on customer data; retention published; weights export (training vendors). Read the vendor's terms this run and date the extraction.
- **B4 status surface:** status page; SLA presence; incident history.
- **B5 pricing transparency:** published; machine-readable source named.
- **B6 values posture:** open weights; portability; V1–V8.

A check not performed this run is `unverified` — never passed.

### Step 5 — Outbound pass (O1–O4), per vendor

Render `provider-audit/outbound` with the vendor, its profile, and the capability sources; produce:

- **O1 stale docs:** grep the tree for claims about the vendor; every comment/doc describing vendor behavior is current or a finding (the `providers.rs` "fixed to Runpod" class).
- **O2 unconsumed capabilities:** the vendor's published capability list vs the profile's `consumed_capabilities`; a gap with no `deliberate_non_consumption` entry is a silent-gap finding; a recorded entry is a choice.
- **O3 churn:** deprecation/retirement waves vs our constants (`kask/crates/hkask-inference/src/model_constants.rs`, the descriptor tables).
- **O4 degradation honesty:** missing capabilities surface loudly, never silently fall back (the `.rules` failure-signals class).

After completing `outbound` and before composing `findings`, run deterministic `lisp_eval` gate `PA-2` over the two renders as returned:

- form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (lambda (r) (if (listp (assoc "checks" r)) (assoc "checks" r) (list)))) (define checks (append (rows inbound) (rows outbound))) (define pick (lambda (pred xs) (if (is_null xs) (list) (if (pred (car xs)) (cons (text-of (car xs) "id") (pick pred (cdr xs))) (pick pred (cdr xs)))))) (define known (list "pass" "fail" "unverified")) (define uncited (pick (lambda (c) (and (string= (text-of c "verdict") "fail") (= (length (text-of c "evidence")) 0))) checks)) (define unverified (pick (lambda (c) (string= (text-of c "verdict") "unverified")) checks)) (define malformed (pick (lambda (c) (or (= (length (text-of c "id")) 0) (not (member (text-of c "verdict") known)))) checks)) (define failed (pick (lambda (c) (string= (text-of c "verdict") "fail")) checks)) (define replacements (if (listp replacement_providers) (length replacement_providers) 0)) (define retiring (and (stringp trigger) (string= trigger "retirement"))) (define problems (append (if (is_null checks) (list "no_checks") (list)) (if (is_null malformed) (list) (list "malformed_checks")) (if (is_null uncited) (list) (list "uncited_failures")) (if (and retiring (= replacements 0)) (list "retirement_without_replacement") (list)))) (list (list "verdict" (if (is_null problems) "pass" "blocked")) (list "problems" problems) (list "uncited" uncited) (list "malformed" malformed) (list "failed" failed) (list "unverified" unverified) (list "checks" (length checks))))`
- env: `{ "inbound": <the inbound render output>, "outbound": <the outbound render output>, "trigger": <Step 1's trigger>, "replacement_providers": <the replacement provider ids named in the outbound render's notes on a retirement run, else []> }`
- `pass` or `blocked` with `problems`: `no_checks`, `malformed_checks` (an id missing or a verdict outside `pass|fail|unverified`), `uncited_failures` (a `fail` whose `evidence` is blank — an evidence-less finding is not a finding), `retirement_without_replacement`. `failed` and `unverified` list the check ids for the report; an `unverified` check is reported as such, never as a pass.

### Step 6 — Calibration (Check, D)

Before a clean report is trusted, retrodict both directions:

- one **known-good** vendor must PASS (OpenRouter B1: slot `https://openrouter.ai/api/v1` + allowlist alignment pinned in-tree);
- one **known-bad** instance from the `.rules` record must FLAG (the SERPAPI pre-normalization key — a vendor key no allowlist carried, `kask/crates/kask_bridge/src/mcp_servers.rs` comment; KiloCode's 503-not-402 payment masquerade under B2).

Each retrodiction is an `inbound` render over that vendor's profile (OpenRouter as adopted; the SERPAPI pre-normalization or KiloCode instance as recorded). Check via `lisp_eval` (`PA-3`) over the two renders as returned:

- form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (lambda (r) (if (listp (assoc "checks" r)) (assoc "checks" r) (list)))) (define verdict-of (lambda (id xs) (if (is_null xs) "" (if (string= (text-of (car xs) "id") id) (text-of (car xs) "verdict") (verdict-of id (cdr xs)))))) (define good-b1 (verdict-of "B1" (rows known_good))) (define bad-b1 (verdict-of "B1" (rows known_bad))) (define bad-b2 (verdict-of "B2" (rows known_bad))) (define good-pass (string= good-b1 "pass")) (define bad-flag (or (string= bad-b1 "fail") (string= bad-b2 "fail"))) (list (list "verdict" (if (and good-pass bad-flag) "calibrated" "uncalibrated")) (list "known_good_b1" good-b1) (list "known_bad_b1" bad-b1) (list "known_bad_b2" bad-b2)))`
- env: `{ "known_good": <the OpenRouter inbound render output>, "known_bad": <the known-bad instance's inbound render output> }`
- `calibrated` when the known-good render's B1 is `pass` and the known-bad render's B1 or B2 is `fail`; the three verdicts it read are returned beside the result.

`uncalibrated` → the report is not trusted; re-enter Step 4/5 for the miscalibrated class before delivering anything.

### Step 7 — Findings and routing

Render `provider-audit/findings` with the per-vendor findings and the calibration receipts; route:

- **blocking** (B1/B3 values-or-security failures) → the operator immediately. On adoption runs, integration **stops** until the operator's explicit override (decision D-3, 2026-10-13).
- **major** → a kanban repair task on the consuming board.
- **minor** → the hygiene backlog.

### Step 8 — Profile updates

Record this run's readings into the profiles: dated `data_terms`, `status_surface`, `pricing`; new `consumed_capabilities`; new `deliberate_non_consumption` entries with reasons. Clear `first_audit_pending` only on the fields actually read.

### Step 9 — Retirement pass (retirement trigger only)

Profile → `status: retired` (archived in place); keychain slot and settings UI row purged; D-seam updated; the tree no longer adopts the vendor (the gate's derived set reconciles). Deletions clean up what they orphan (`.rules`).

### Step 10 — Convergence (D)

Call `lisp_eval` (`PA-4`) over the renders and the run's re-entry log (`<run_dir>/reentries.jsonl`, one line appended per re-entry: `jq -n -c --arg step <step> --arg item <open item> '$ARGS.named' >> <run_dir>/reentries.jsonl`):

- form: `(begin (define max-reentries 2) (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define ids (lambda (xs) (if (is_null xs) (list) (cons (if (stringp (car xs)) (car xs) (text-of (car xs) "id")) (ids (cdr xs)))))) (define declared (lambda (rs) (if (is_null rs) (list) (append (ids (if (listp (assoc "blocking_failures" (car rs))) (assoc "blocking_failures" (car rs)) (list))) (declared (cdr rs)))))) (define routed (ids (if (and (listp (assoc "routed" findings)) (listp (assoc "blocking" (assoc "routed" findings)))) (assoc "blocking" (assoc "routed" findings)) (list)))) (define unrouted (lambda (xs) (if (is_null xs) (list) (if (member (car xs) routed) (unrouted (cdr xs)) (cons (car xs) (unrouted (cdr xs))))))) (define open (unrouted (declared inbound_renders))) (define reentries (if (listp log) (length log) 0)) (define calibrated (and (listp calibration) (string= (text-of calibration "verdict") "calibrated"))) (list (list "verdict" (cond ((and (is_null open) calibrated) "done") ((>= reentries max-reentries) "stop-and-report") (t "iterate"))) (list "blocking_unrouted" open) (list "calibrated" calibrated) (list "reentries" reentries)))`
- env: `{ "inbound_renders": <every vendor's inbound render output, as a list>, "findings": <the findings render output>, "calibration": <the PA-3 result>, "log": <jq -s . <run_dir>/reentries.jsonl, [] before the first re-entry> }`
- `blocking_unrouted` is every id in any inbound render's `blocking_failures` that is absent from the findings render's `routed.blocking`. `done` → deliver the report; `iterate` → append a log line and re-enter the step owning the open item; `stop-and-report` (two re-entries already logged) → deliver with the open items listed — the decision is the operator's, never the loop's.

## Regression case

- Gate: `bash kask/scripts/check-provider-profiles.sh` passes on the seeded tree; delete any profile → it fails naming the vendor; restore.
- Selftest: `bash kask/scripts/check-provider-profiles-selftest.sh` proves the gate fails on a synthetic missing profile, a zombie retirement, and an active orphan, and passes on a clean synthetic tree.
- Calibration retrodiction: OpenRouter passes B1 (slot + in-tree allowlist pins); the SERPAPI pre-normalization instance flags under B1; KiloCode's 503-not-402 flags under B2.
- `bash kask/scripts/check-skill-lisp-forms.sh` evaluates the three authored gates (`scientific_gate_pa_`): `PA-2` over cited and uncited failures, an `unverified` check, a mis-cased verdict, empty renders, and a retirement run with and without a replacement; `PA-3` over B1-pass/B1-fail, B1-pass/B2-fail, both passing, the known-good failing, and empty renders; `PA-4` over all blocking routed (done), one unrouted (iterate), one unrouted after two logged re-entries (stop-and-report), and an uncalibrated run (iterate).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `inbound.j2` | Read one vendor's inbound contract — credential slot, error taxonomy, data terms, status surface, pricing, values posture — against the project's principles, producing dated evidence for every check. |
| `outbound.j2` | Read one vendor's outbound alignment — stale docs, unconsumed capabilities, churn, degradation honesty — against the tree, distinguishing recorded choice from silent gap. |
| `findings.j2` | Compose the audit's findings artifact with severity, file:line or URL evidence, routing, and the retrodictive calibration receipts. |

To render a template, call `render_template` with the ref (e.g. `provider-audit/inbound`):
- `inbound.j2`: `vendor`, `profile`, `trigger`
- `outbound.j2`: `vendor`, `profile`, `capability_sources`
- `findings.j2`: `vendor`, `findings`, `calibration`

## Constraints

- The adopted set is **derived from the tree** (the gate script's anchors), never enumerated from memory — a hardcoded vendor list silently misses every addition (`.rules`, cross-seam test trap).
- `first_audit_pending` fields are read-and-dated in the run that reads them; a field not read this run stays pending — never assumed, never smoothed.
- Blocking findings stop adoption pending the operator's explicit override; this skill recommends, it never adopts. Route contested merit questions to `mcda`.
- Every check names its evidence: doc URL + extraction date, probe receipt, or tree file:line. `unverified` is a valid verdict; `passed-by-default` is not.
- Profiles are extraction-dated — terms drift; an inbound pass older than a year is stale.
- The gate script owns structural pins; this skill owns semantic reading; the in-tree allowlist-alignment tests own env-var-vs-reads alignment — no duplication.
