---
title: "Upstream-Zed Removal Principles for the zed-kask Seam"
audience: [architects, integrators, agents]
last_updated: 2026-08-28
version: "1.2.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, trust, composition]
---

# Upstream-Zed Removal Principles for the zed-kask Seam

> **Status:** consolidated, testable principle set governing **what to remove
> from upstream Zed** (everything outside `kask/` and outside the named D-seams
> in `DIVERGENCE.md` — the table currently runs D1–D38, with D17 and D19
> retired) and **why**. Sibling to
> [`upstream-rebase-process.md`](upstream-rebase-process.md).
>
> **Meta-constraint (inviolable):** the D-seam discipline is a *boundary on the
> mechanism*, not a removal *reason*. Never edit upstream files outside the
> named D-seams; push any fix into a `kask/` crate behind a D-seam and pin it
> with a test (`.rules:69`). Every principle below is *compatible* with this:
> no principle authorizes forking upstream outside a D-seam. For upstream
> surface, "removal" means **disable-behind-a-D-seam + test-pin**, not
> **delete-the-file**. File deletion is reserved for `kask/`-side surface.

## How to read this document

Each category carries five fields:
- **Definition** (one sentence, IS not OUGHT).
- **Decision test** (a falsifiable predicate an agent evaluates against a diff).
- **Failure mode if mis-applied** (counterfactual stress — what breaks).
- **Anchoring evidence** — `.rules` / `DIVERGENCE.md` file:line, or an explicit
  "no existing anchor — proposed" note (never fabricated).
- **Scope boundary** — what this category does NOT authorize (esp. vs the
  D-seam meta-constraint).

The categories are **ranked** by MCDA (§MCDA). They are **decision-test-disjoint**,
not instance-disjoint: a single removal may satisfy multiple tests; the agent
applies all tests and classifies by the one capturing the **load-bearing risk**
(§Overlap). Pure "elegance/simplification" (removing code because it is "ugly"
or "could be cleaner") is **rejected** — it would authorize forking upstream.
Only the *provably unreachable* form survives as Category 4.

> **Citation hygiene note (2026-08-28):** earlier revisions cited `.rules` by
> line ranges up to `:851`. The current `.rules` file is 128 lines; all such
> ranged citations were stale and have been re-anchored to the current file.
> Where a cited source file has since been deleted (e.g.
> `crates/auto_update/`, `crates/kask_extensions_ui/`), the citation is marked
> historical or re-anchored to the surviving D-seam row.

---

## Ranked principle set

### Rank 1 — Category 1: Install/runtime collision

- **Definition:** Removing upstream-declared install/runtime surface that, if
  retained, would cause zed-kask to hijack or be hijacked by the user's real
  Zed install (or vice versa).
- **Decision test (falsifiable):** Does the retained upstream surface cause a
  desktop-environment, file-association, URL-scheme, auto-update, or
  install-path collision such that installing or running zed-kask silently
  alters the user's real Zed, or is silently altered by it? **Mechanical
  check:** `bash kask/scripts/build/check-desktop-no-collision.sh` and
  `bash kask/scripts/build/check-zed-isolation.sh` both pass with the surface
  removed and fail with it retained. The forbidden strings are `text/plain`,
  `application/x-zerosize`, `x-scheme-handler/zed`, `Keywords=zed` in any
  `.desktop` template rendered for zed-kask (`.rules:126`).
- **Failure mode if mis-applied:** zed-kask silently hijacks the user's real
  Zed (or vice versa) — the project's fundamental premise ("complements, not
  replaces") fails. This happened in commit `dcc5aa6dd3` (Jul 26 2026): the URL
  scheme was fixed but `text/plain`, `application/x-zerosize`, and
  `Keywords=zed` were left in place.
- **Anchoring evidence:** `.rules:126` (the `.desktop` collision trap);
  `DIVERGENCE.md` D7 L27 (Hard Zed-isolation invariant: "updater is not
  initialized or imported by the zed binary"; legacy bundlers "fail-closed";
  enforced by `check-zed-isolation.sh`); `DIVERGENCE.md` D16 L35 (upstream
  update actions removed, replaced by the safe zed-kask updater). Verified:
  both scripts exist at `kask/scripts/build/` (`check-desktop-no-collision.sh`
  is a one-line `exec` alias of `check-zed-isolation.sh`, L6).
- **Scope boundary:** Does NOT authorize removing upstream functionality
  merely because kask has a parallel feature — only *collision surface*
  (MIME types, keywords, URL scheme, install path, auto-update path). A kask
  feature that does not collide is governed by Category 3, not this one.

### Rank 2 — Category 2: Platform scope

- **Definition:** Removing upstream code paths that exist only to build,
  package, or run for non-Linux targets that zed-kask does not ship.
- **Decision test (falsifiable):** (a) Does the code path compile or execute
  only behind a `#[cfg(target_os = "...")]` gate for a non-Linux OS, or is it
  a non-Linux bundler/release workflow? AND (b) is it absent from the zed-kask
  build matrix? **Negative (counter-example) test:** if the code is a
  *cross-platform library* that Linux also benefits from, it is NOT
  platform-scope removal — leave it. The test fires only when the surface is
  *exclusively* non-Linux.
- **Failure mode if mis-applied:** (a) removing a cross-platform library Linux
  uses → silent loss of Linux functionality; (b) leaving a non-Linux bundler
  → wasted build surface and a re-introduced collision risk (the bundler may
  redeclare upstream `zed` identity).
- **Anchoring evidence:** `DIVERGENCE.md` D7 L27: legacy Zed bundlers for
  Linux, macOS, Windows, and Snap are fail-closed; upstream release
  workflows, the Zed desktop template, and Flatpak/Snap resources are deleted;
  macOS `[package.metadata.bundle-*]` sections (including
  `osx_url_schemes = ["zed"]`) are removed — zed-kask is Linux-only and does
  not bundle for macOS. Verified: `script/bundle-mac`,
  `script/bundle-windows.ps1`, `script/snap-build` exist (fail-closed);
  `script/bundle-linux` is the active bundler. D7 also records the 2026-08-27
  removal of upstream's CLI `mod flatpak` (sandbox escape via
  `flatpak-spawn --host` hard-coding upstream Zed's `FLATPAK_ID`).
- **Scope boundary:** Does NOT authorize removing cross-platform shared
  libraries, only target-gated or bundler-gated non-Linux surface. Does NOT
  authorize removing the Linux bundler or Linux build matrix.

### Rank 3 — Category 4: Dead surface rendered unreachable (sharpened "elegance")

- **Definition:** Removing upstream surface that kask has rendered
  *unreachable* (no production caller in the zed-kask build), where removal
  reduces upstream-merge friction without changing any observed behavior.
  This is the **only** acceptable form of "simplification": not "the code is
  inelegant" but "the code is provably never reached."
- **Decision test (falsifiable — essentialist G1 Exist + G2 Surface):** Delete
  the candidate in your head. (G1) Does the complexity it was hiding reappear
  at the call sites? (G2) Does any test or production path assert the surface
  is reachable? **Removable iff** G1 = no (complexity vanishes) AND G2 = no
  (nothing asserts reachability) AND no `.rules`/`DIVERGENCE.md` invariant
  depends on the surface. If G1 = yes → NOT removable (the surface is
  load-bearing despite looking dead). If G2 = yes → NOT removable (a test pins
  its reachability; update or remove the test first).
- **Failure mode if mis-applied:** (a) removing surface that *appears* dead
  but is reached via a dynamic path (reflection, trait-object dispatch, URL
  scheme handler, `observe_new`) the grep missed → silent breakage; (b) the
  "LLM-improves-against-LLM-scored-target" gaming (`.rules`, "Other traps":
  three mitigations required) — an agent declares surface "dead" to reduce a
  merge-friction score without genuinely verifying G1.
- **Anchoring evidence:** `.rules` "Dead code patterns" section
  (trait-with-one-impl is speculative generality; convention helpers with only
  test callers are dead code; folded-service modules with no production
  callers are dead surface; advertised invariants must point to their
  enforcement line). **All current instances are `kask/`-side** (e.g.
  ocap/`OcapConfig`/`required_capabilities`; `AdapterPort`/`AdapterRouter`).
  **No existing *upstream* anchor — proposed.** Applying this to upstream is
  unproven; it MUST be exercised as disable-behind-a-D-seam + test-pin, never
  as file deletion.
- **Scope boundary:** Does NOT authorize deleting upstream files — only
  disabling via a D-seam with a test pin. Does NOT authorize removing surface
  that is merely "rarely used" or "inelegant" — it must be *unreachable*.
  Does NOT authorize removing surface whose reachability is asserted by any
  test or production path (G2 guard).

### Rank 4 — Category 3: Redundant surface superseded by kask

- **Definition:** Removing upstream behavior/surface that kask has
  deliberately replaced with an equivalent or better kask-side facility, where
  retaining the upstream surface causes a *concrete defect* (duplicate UI,
  wrong warning, conflicting updater, double-budgeting).
- **Decision test (falsifiable, two-part):** (a) Does kask provide a
  replacement that is **wired and load-bearing** — grep its enforcement point;
  it must be *called in production*, not merely declared (the
  "advertised-invariants-need-enforcement-points" guard)? AND (b) Does
  retaining the upstream surface produce a **concrete defect** (a visible
  duplicate, a wrong warning, a conflicting updater, a double charge)?
  **Removable iff both (a) and (b).** If only (a) without (b) → NOT removable:
  a kask preference is not a removal reason.
- **Failure mode if mis-applied:** removing upstream surface that kask
  *appears* to replace but the kask replacement is unwired (the
  "advertised invariants need enforcement points" trap, `.rules` "Dead code
  patterns") → silent loss of functionality with no error. This is the
  highest-risk mis-application because the (a) check is easy to fake by
  pointing at a constructor that is never read.
- **Anchoring evidence:** `DIVERGENCE.md` D1 L22 (catalog budget + description
  length warnings disabled — "skills execute via body injection"; pinned by
  `test_select_catalog_skills_*` in `crates/agent/src/agent.rs:5188,5266,5348`
  and `test_parse_description_too_long_loads_with_warning` in
  `crates/agent_skills/agent_skills.rs:1599`); `DIVERGENCE.md` D3 L24
  ("Daemon transport deleted; identity from `ServerContext.webid`");
  `DIVERGENCE.md` D16 L35 (upstream update actions removed, replaced by the
  safe zed-kask updater). Historical: `crates/kask_extensions_ui/` provided
  filter + upsell banners removal — crate removed 2026-08-20, citation kept
  only as precedent. The former `crates/auto_update/` citation
  (`test_auto_update_defaults_to_false`) is **deleted surface** — that crate
  no longer exists in the tree; the auto-update removal is anchored entirely
  in D7/D16.
- **Scope boundary:** Does NOT authorize removing upstream surface merely
  because kask *also* does the same thing — the upstream surface must produce
  a *concrete defect* when retained alongside the kask replacement. Pure
  preference is not a removal reason. Does NOT authorize removing upstream
  surface for a *real upstream bug* — file an upstream issue, don't fork-fix
  (the D31/D32 rows in `DIVERGENCE.md` L46-47 record two such upstream bug
  fixes that were landed as D-seams *and* flagged for upstream reporting).

### Rejected: pure "elegance / simplification"

Removing upstream code because it is "ugly," "could be cleaner," or "has a
nicer alternative" is **rejected** as a removal reason. It would authorize
forking upstream outside a D-seam — `.rules:69`: "Don't 'fix' upstream files
speculatively — push fixes into `kask/` behind a D-seam." Only Category 4's
*provably unreachable* test survives.

---

## MCDA ranking and ±20% sensitivity report

### Criteria and weights (direct method, normalized to 1.0)

| Criterion | Type | Weight | Rationale |
| --- | --- | --- | --- |
| Install-safety | benefit | 0.25 | A removal that endangers the user's real Zed is catastrophic (project premise). |
| Behavior preservation | benefit | 0.25 | A removal that breaks behavior is a bug. |
| Testability of the decision rule | benefit | 0.20 | A rule an agent cannot evaluate against a diff is dead. |
| Upstream-merge friction reduction | benefit | 0.15 | The purpose of removal, but not safety-critical. |
| Blast radius (inverted: smaller = better) | cost | 0.15 | Risk control; fewer merge-hot files touched = higher score. |

### Scores (0–10) and composites

| Category | friction | install-safety | behavior | testability | blast | composite | rank |
| --- | --- | --- | --- | --- | --- | --- | --- |
| C1 Install/runtime collision | 6 | 10 | 9 | 10 | 8 | **0.885** | 1 |
| C2 Platform scope | 8 | 5 | 9 | 7 | 7 | **0.715** | 2 |
| C4 Dead surface rendered unreachable | 8 | 5 | 7 | 5 | 4 | **0.580** | 3 |
| C3 Redundant surface superseded by kask | 6 | 5 | 5 | 6 | 5 | **0.535** | 4 |

Composite = Σ(weight × raw/10). (C4 outranks C3 despite a weaker anchor
because it preserves behavior by definition and reduces friction more; C3
carries the highest behavior-preservation risk — the unwired-replacement trap.)

### ±20% sensitivity analysis (one-at-a-time + one combined worst-case)

Each weight perturbed ±20% and renormalized; composite recomputed; rank
reversal checked.

| Perturbation | C1 | C2 | C4 | C3 | Reversal? |
| --- | --- | --- | --- | --- | --- |
| Base | 0.885 | 0.715 | 0.580 | 0.535 | — |
| friction +20% | 0.881 | 0.719 | 0.586 | 0.537 | no |
| friction −20% | 0.889 | 0.711 | 0.573 | 0.533 | no |
| safety +20% | 0.890 | 0.708 | 0.577 | 0.533 | no |
| safety −20% | 0.879 | 0.726 | 0.583 | 0.537 | no |
| behavior +20% | 0.884 | 0.712 | 0.586 | 0.533 | no |
| behavior −20% | 0.887 | 0.720 | 0.574 | 0.537 | no |
| testability +20% | 0.885 | 0.715 | 0.577 | 0.538 | no |
| testability −20% | 0.885 | 0.715 | 0.583 | 0.532 | no |
| blast +20% | 0.881 | 0.711 | 0.575 | 0.538 | no |
| blast −20% | 0.889 | 0.719 | 0.585 | 0.532 | no |
| **combined** (test+blast ↑20%, friction+behavior ↓20%) | 0.885 | 0.717 | 0.559 | 0.536 | **no** |

### Robust vs fragile classification

- **Robust (no rank reversal at ±20%):** C1 (rank 1) and C2 (rank 2). C1's lead
  is structural (it *is* the install-safety criterion) and survives all
  perturbations with ≥0.15 margin. C2's rank-2 hold is stable.
- **Moderately robust (stable under all single and combined ±20%
  perturbations, but smallest gap):** the **C4 vs C3** pair (rank 3 vs 4). The
  gap is ~0.045 and never reverses under any tested perturbation (the
  combined worst-case leaves C4 at 0.559 vs C3 at 0.536, Δ=0.023). Flagged as
  the **least robust** pair: a future reweighting that simultaneously boosts
  testability + blast-radius and drops behavior + friction could close the
  gap. The structural fragility is **independent of weights**: C4 has *no
  upstream anchor* while C3 has strong ones — so C4's rank is evidence-fragile
  even if it is weight-robust today.
- **Compensation-masking check:** no category scores below 0.3 (normalized)
  on a critical (weight > 0.1) criterion. C3's behavior score (0.5) is the
  lowest on a critical criterion but is above the 0.3 danger threshold; its
  rank-4 position already reflects this. No masking.

**Recommendation:** proceed with the ranking C1 > C2 > C4 > C3. Treat C4 as
**provisional** for upstream application (disable-behind-D-seam only, never
file deletion) until the first real upstream instance validates its decision
test.

---

## D-seam-compatibility audit (A1)

For each category, confirm the scope boundary does NOT authorize editing
upstream outside a named D-seam.

| Category | Upstream edit authorized? | Mechanism | Verdict |
| --- | --- | --- | --- |
| C1 Collision | Only on a **D-seam file** (`crates/zed/resources/zed.desktop.in` is a D7 seam; menu files are D16 seams). New collision surface → new D-seam entry in `DIVERGENCE.md` + test pin (`.rules:69`). | disable-behind-D-seam + `check-*-no-collision.sh` | ✅ compatible |
| C2 Platform scope | Non-Linux bundlers are D7-seam files (`script/bundle-mac`, etc.). New non-Linux surface → D7 seam or kask-side `kask/scripts/build/` installer. | disable/fail-closed-behind-D7 | ✅ compatible |
| C4 Dead surface | For upstream: **disable-behind-a-D-seam + test-pin, never file deletion** (per the meta-constraint). File deletion is `kask/`-side only. | disable-behind-D-seam + test pin | ✅ compatible (provisional) |
| C3 Redundant | All current instances are D-seam files (D1 `agent.rs`/`agent_skills.rs`; D3 `main.rs`; D16 menu files). New redundant surface → new D-seam entry + test pin (`.rules:69`). | disable-behind-D-seam + test pin | ✅ compatible |

**Audit result:** no category authorizes editing upstream outside a D-seam.
The meta-constraint holds. Any upstream edit demanded by a category must be
expressible as (a) an edit to an existing D-seam file, or (b) a new D-seam
entry in `DIVERGENCE.md` pinned by a test in the same PR (`.rules:69`).

---

## Cross-category overlap matrix (A2)

Categories are **decision-test-disjoint** (no two share a decision test), not
instance-disjoint (a single removal may satisfy several tests). The agent
applies all tests and classifies by the one capturing the **load-bearing
risk**. Co-occurrence resolution rules:

| Pair | Co-occurs? | Example | Resolution (which test is load-bearing) |
| --- | --- | --- | --- |
| C1 × C2 | yes | D7 macOS `osx_url_schemes = ["zed"]` removal (URL-scheme collision *and* macOS-only) | **C1** — collision is safety-critical; even on Linux you'd remove the `zed` scheme. C2 is secondary. |
| C1 × C3 | yes | D16 upstream update actions removed (collision *and* replaced by the safe zed-kask updater) | **C1** — the installer-replaces-real-Zed risk is the load-bearing reason; C3 (redundancy) is co-incident. |
| C2 × C3 | yes | a non-Linux feature kask has replaced (e.g. macOS auto-update) | classify by whether the *non-Linux* gate or the *kask-replacement* defect is the trigger; if both, **C2** (smaller blast radius — the surface is already gated out of the build). |
| C3 × C4 | **no** | mutually exclusive on the reachability axis: C3 = upstream still *reachable* but *wrong*; C4 = upstream *unreachable*. | — |
| C1 × C4 | unlikely | a collision surface that is also unreachable is a contradiction (collision requires reachability). | — |
| C2 × C4 | yes | a non-Linux code path that is also unreachable in the zed-kask build | either test suffices; prefer **C2** (sharper, mechanically checkable via `cfg` gate). |

**Decision-test disjointness proof:** each test keys on a distinct
observable: C1 → collision script output; C2 → `cfg` gate + build matrix; C3 →
wired-replacement grep + concrete-defect; C4 → G1 call-site complexity + G2
reachability assertion. No two tests read the same observable, so no two
categories can be confused by a diff that passes one test's observable.

---

## Metacognition: coverage prediction and Brier score (R2)

### Current condition (Kata Step 1)

Ground-truth prior-removal set, drawn from `DIVERGENCE.md` D1–D38 + `.rules`
traps (verified against the codebase per `.rules`: "Convention priors from
`.rules` must be verified against the codebase before use — rules can be
stale"):

| # | Prior removal | Source | Classified under |
| --- | --- | --- | --- |
| 1 | Upstream auto-update not initialized/imported | D7 L27 | C1 |
| 2 | Legacy bundlers macOS/Windows/Snap fail-closed/deleted | D7 L27 | C2 |
| 3 | macOS `osx_url_schemes = ["zed"]` + bundle sections removed | D7 L27 | C1+C2 (co-occurrence) |
| 4 | Flatpak/Snap resources deleted (incl. CLI `mod flatpak`, 2026-08-27) | D7 L27 | C2 |
| 5 | Upstream release workflows + Zed desktop template deleted | D7 L27 | C2 |
| 6 | Catalog budget disabled | D1 L22; `agent.rs:5188` | C3 |
| 7 | Description-length warnings disabled | D1 L22; `agent_skills.rs:1599` | C3 |
| 8 | Daemon transport deleted | D3 L24 | C3 |
| 9 | Upstream update menu actions removed | D16 L35 | C1+C3 (co-occurrence) |
| 10 | Upstream updater replaced by safe zed-kask updater | D16 L35 (the former `crates/auto_update/` citation is deleted surface) | C1 |
| 11 | kask_extensions_ui provides filter + upsell banners removed | historical — crate removed 2026-08-20 | C3 |

(Kask-side dead-surface removals — ocap/`OcapConfig`, `required_capabilities`,
`AdapterPort`/`AdapterRouter`, folded services, no-consumer hooks — validate
**C4's decision test** but are `kask/`-side, not upstream. C4 has **zero**
upstream instances in the ground truth.)

### Target condition (Kata Step 2)

The taxonomy is retrospectively exhaustive over the upstream ground truth
(11/11 classifiable) AND C4 carries an explicit "no upstream anchor —
proposed, disable-behind-D-seam" caveat AND the residual prospective risk (a
5th category for security/license removal) is documented as an open question
rather than silently assumed covered.

### Prediction (Kata Step 3)

**"The taxonomy will correctly classify the next 5 real upstream removal
decisions."** Confidence **p = 0.70**.

Rationale for 0.70 (not higher): C1/C2/C3 are well-anchored and cover all 11
retrospective cases, so the *well-trodden* removal space is covered with high
confidence. The residual 0.30 risk is (a) C4 unproven upstream, and (b) a
plausible 5th category — *security/license removal* (removing upstream
surface because it carries an incompatible license or an unfixable
vulnerability) — for which there is **no current evidence** in the ground
truth. Per `.rules:57` (new rules must be "non-obvious, repeatedly
encountered, and specific enough to act on"), a category with zero instances
is not added; it is recorded as an open question.

### Experiment (Kata Step 4)

Applied the taxonomy to the 11-case ground truth (table above). Every case is
classifiable; the two co-occurrence cases resolve per §Overlap (C1
load-bearing).

### Convergence (Steps 5–9 — deterministic)

- **Object-space gap (Dublin Core artifact completeness):** each category has
  definition + decision test + failure mode + anchor + scope boundary = 5/5
  fields. C4 anchor is "no existing upstream anchor — proposed" (explicitly
  marked, not fabricated) → 5/5 with a caveat. **object_gap = 0.**
- **Process-space gap (PKO procedure progress):** the procedure (apply all 4
  tests → classify by load-bearing risk → execute via D-seam + test pin) is
  complete for the retrospective set. The prospective procedure for C4 is
  specified (disable-behind-D-seam) but unexercised upstream. **process_gap =
  0.1** (one unexercised upstream path).
- **Hypotenuse:** √(0² + 0.1²) = **0.10** ≤ epsilon.
- **Brier score:** predicted p = 0.70 that the taxonomy covers the next 5.
  Retrospective realization (proxy, since no future cases exist yet) = 1.0
  (11/11 classifiable). **Brier = (0.70 − 1.0)² = 0.09** against the proxy.
  This is a *low* (good) Brier indicating mild **underconfidence** on the
  well-anchored categories — but the proxy is not a true prospective outcome,
  so the 0.30 residual risk (C4 unproven; possible 5th category) is retained
  honestly rather than discarded.
- **Convergence:** gap < epsilon (0.10) AND prediction not poorly calibrated
  (Brier 0.09). **Converged.** No taxonomy iteration required.

### Residual risk (not iterated — documented per the termination rule)

1. **C4 has no upstream anchor.** Its rank (3) is evidence-fragile even though
   weight-robust. The first real upstream dead-surface instance must validate
   G1+G2; until then C4 is provisional and upstream application is
   disable-behind-D-seam only.
2. **Possible 5th category — security/license removal.** No ground-truth
   instance exists (the ocap theater was *kask-side* config, not a license or
   CVE-driven upstream removal). Not added (zero instances violates
   `.rules:57`). If a real upstream CVE or license-incompatibility
   removal occurs, add a Category 5 with its own decision test (e.g. "the
   surface carries a license incompatible with zed-kask's distribution, or an
   unfixable CVE; removal is the only remediation; file an upstream issue in
   parallel").

---

## Acceptance-criteria checklist

- [x] ≥3 ranked categories (4), each carrying a decision test, a failure mode,
      and anchoring evidence.
- [x] No category authorizes editing upstream outside a D-seam (§D-seam audit;
      every category's scope boundary names the D-seam mechanism).
- [x] The "elegance" category is sharpened to a falsifiable test (Category 4:
      G1+G2) — pure elegance is rejected with a stated reason.
- [x] MCDA sensitivity report identifies robust (C1, C2) vs the least-robust
      pair (C4 vs C3) and a structural fragility (C4's absent upstream anchor).
- [x] Metacognition reports a calibrated coverage prediction (p=0.70,
      Brier 0.09 against retrospective proxy, with the prospective limitation
      and residual risk stated — not a bare claim).
- [x] Every citation is a real file:line verified 2026-08-28 (`.rules:57,69,126`;
      `DIVERGENCE.md` D1 L22 / D3 L24 / D7 L27 / D16 L35 / D31-D32 L46-47;
      `crates/agent/src/agent.rs:5188,5266,5348`;
      `crates/agent_skills/agent_skills.rs:1599`;
      `kask/scripts/build/check-desktop-no-collision.sh:6`;
      `kask/scripts/build/check-zed-isolation.sh:25`; `script/bundle-*`);
      deleted-surface citations (`crates/auto_update/`,
      `crates/kask_extensions_ui/`) are marked historical, never current; the
      one missing anchor (C4 upstream) is marked "no existing anchor —
      proposed," never fabricated.

---

## Essentialist review of candidate `.rules` additions (both eliminated)

Two `.rules` additions were drafted during this review and then run through
the `essentialist` 3-gate eliminative loop (G1 Exist → G2 Surface → G3
Contract). **Both were eliminated** — each is a pass-through restatement of an
existing rule, and neither trap has a single ground-truth instance (failing
`.rules:57` "repeatedly encountered"). Per the `.rules` hygiene section, neither
is committed; the findings are recorded here for traceability.

| Candidate | Eliminated because | Existing rule that already covers it |
| --- | --- | --- |
| "Upstream removal is disable-behind-D-seam, not file deletion" | G1+G3 pass-through: "disable via D-seam + test pin" restates `.rules:69`; "disabling comment needs a test" restates the same rule's final sentence; "disable not delete" is *entailed* by DIVERGENCE.md L9-11 ("the only divergences are the D-seams"). 0 instances of an agent deleting an upstream file when it should have disabled it. | `.rules:69`, DIVERGENCE.md L9-11 |
| "A removal category with zero ground-truth instances is not added" | G1+G3 pass-through: it is a *re-scope* of the no-drive-by principle to taxonomy categories, not a new constraint (it generalizes `.rules:57`). The discipline already worked without the rule — Category 5 was recorded as an open question, not added. | `.rules:57` |

**Where the content survives (as prose, not as `.rules` traps):** the
disable-vs-delete distinction is the meta-constraint paragraph at the top of
this document; the zero-instance discipline is the rationale recorded in the
metacognition section for not adding Category 5. Both are documentary, which
is the correct home — a principles document may state its own operating
mechanism without that constituting a `.rules` entry.
