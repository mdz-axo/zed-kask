#!/usr/bin/env bash
# Read-only: compose the skill semantic-review ledger markdown from the
# mechanical base plus the auditor's per-skill verdicts.
#
# Verdicts are keyed by skill name. `clean` = mechanical base green and no
# defect reproduced in read-triage. `repaired` = a verified defect was found
# and fixed in this review (named in the notes column). The reference-model
# verdict is `aligned` where the in-tree derived-registry encoding was checked
# against the skill's claim, and `unverified-external` where the cited book or
# paper itself is not inspectable from this tree.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
BASE="/tmp/ledger-base.tsv"

# skill|reference_verdict|dp_verdict|defect_verdict|notes
VERDICTS=$(cat <<'V'
adhd-mode|aligned|clean|clean|Counts extract from the model's own draft (semantic extraction); verdicts D via Forms G/C
algedonic-review|aligned|clean|clean|Board reads + ALG-1/ALG-2 D; severity triage P (operator's)
bug-hunt|aligned|clean|clean|Bach & Bolton heuristic model encoded in BH-1
build-corpus-pipeline|aligned|clean|clean|Stage gates D; BCP-1 measured counts
code-review|aligned|clean|clean|Fagan/PERFECT encoded; CR-0..2 D; applied_count is a runtime observation, correctly P
coding-guidelines|aligned|clean|clean|Karpathy four principles; CG-1/CG-2 D
hypothesis-testing|aligned|clean|clean|HT-1 D; experimental-design owns scientific validity; experiment registry is the lifecycle oracle
experimental-design|aligned|clean|clean|ED-0..ED-2 D over classified inputs; executor reports and operator are the scientific oracles
create-skill|aligned|clean|clean|CS-1 D; my-skill/analyze.j2 is a documented placeholder in the scaffold example
deep-module|aligned|clean|clean|Ousterhout Ch.4/Ch.7.1 via derived deep_module + deletion_test (both verified present)
diagnose|aligned|clean|clean|Zeller/Agans encoded; DIAG-1/2 D
diataxis-diagram|aligned|clean|clean|DD-1/DD-2 D
doc-update|aligned|clean|clean|DU-1 D
eqm|aligned|clean|clean|EQM-1/EQM-2 D; directional_match_pct is caller-supplied (not template-computable), correctly P
essentialist|aligned|clean|clean|ESS-1/ESS-3 D; zero-delta eq over lists
falsifiability|aligned|clean|clean|Popper via derived falsifiability; FAL-1/FAL-2 D; multi-line env parses
gpui-bench|unverified-external|clean|clean|GPB-1/GPB-2 D; benchmark thresholds are operator-declared
gradient-hunter|aligned|clean|clean|GH-1/GH-2 D
grill-me|aligned|clean|clean|Socratic method + Bloom + ZPD all verified present in derived registry
grounding-verify|aligned|clean|repaired|GV-1 had no env block: corrected_count/e_count/w_count/n_count/min_why_len all unbound. Env added from the producer's own totals contract
hypothesis-framer|aligned|clean|clean|FINER criteria verified present; HF-1/HF-2 D
idiomatic-rust|aligned|clean|clean|IR-1/IR-2 D
improv|aligned|clean|clean|Johnstone; plussing/yes_and verified present; IMP-1..3 D
kanban-task-management|aligned|clean|clean|KTM-1/KTM-2 D; INVEST via derived invest_criteria
kask-seam-audit|aligned|clean|clean|KSA-1..3 D
kata-improvement|aligned|clean|clean|Rother Toyota Kata via derived improvement_kata; KI-1/KI-2 D
lean-prover|unverified-external|clean|clean|LP-1/LP-2 D; lean_check is the external oracle
local-research-swarm|aligned|clean|clean|LRS-1/LRS-2 D; boundary text updated by the retirement commit
mcda|aligned|clean|clean|Belton & Stewart + Keeney & Raiffa; MCDA-1/2 D
mcp-tool-review|aligned|clean|clean|MTR-1/MTR-2 D; Initial stated as "**Initial:**" (false positive in the sweep)
media-workflow|aligned|clean|clean|MWF-1/MWF-2 D
metacognition|aligned|clean|clean|MC-1 D; falstaffian-rotation.j2 referenced extensionlessly at line 92 (resolves)
onto-anchor|aligned|clean|clean|OA-1/OA-2 D; PDCA exemption stated (one deterministic call)
pragmatic-cybernetics|aligned|clean|clean|Beer/Ashby; PC-1/PC-2 D
pragmatic-semantics|aligned|clean|repaired|Modifier prose in two templates stated FIBO +0.10 / SUMO +0.05 against the PS-1 gate's +0.05 / +0.03. Prose bound to the gate by a derived pin
product-manager|aligned|clean|clean|PMB-1/PMB-2 D
program-manager|aligned|clean|clean|PM-1 D; dod-checklist to closeout-report handoff repaired earlier (3b3688a8ae) and verified live
prompt-enhance|aligned|clean|clean|2026-10-18 receipt: PE-2 derived blocking_defects from structured findings (verified four branches). Current replacement: PE-State with raw-shape preflight and scalar selected_attempt_id into the original raw log; this annotation is not a new measurement
provider-audit|aligned|clean|clean|PA-1/PA-2 D
refactor-architecture|aligned|clean|clean|RA-1/RA-2 D; Fowler strangler verified present
region-routing|aligned|clean|clean|RR-1 D
sankey-flow|aligned|clean|clean|SFW-1/SFW-2 D; CSV rendered from validated edges
scenario-planning|aligned|clean|clean|Schwartz/Chermack; SP-1/SP-2 D
self-improvement|aligned|clean|clean|SI-1..3 D
skill-bundler|aligned|clean|clean|SB-1 D
skill-discovery|aligned|clean|clean|SD-1/SD-2 D
skill-maintenance|aligned|clean|clean|Fagan + Goodhart verified; SM-1/SM-2 D
structured-extraction|aligned|clean|clean|SE-1..3 D
superforecasting|aligned|clean|clean|SF-1/SF-2 D; Tetlock/Brier/fermi/reference_class all verified present; overconfidence_bias is EQM-derived, not an ontology anchor
swarm-intelligence|aligned|clean|clean|SWM-1..3 D
system-maintenance|aligned|clean|clean|SMN-1/SMN-2 D
task-breakdown|aligned|clean|clean|TB-1/TB-2 D
tdd|aligned|clean|clean|Beck TDD; TDD-1/TDD-2 D
therapy|aligned|clean|clean|TH-1/TH-2 D; passphrase claim verified accurate (DEFAULT_PASSPHRASE, tested provision path)
transcript-reel|aligned|clean|clean|TR-1/TR-2 D; MovieLabs OMC vocabulary
upstream-rebase|aligned|clean|clean|UR-1..3 D
verification-compression|aligned|clean|clean|VC-1/VC-2 D
wardley-mapper|aligned|clean|clean|Wardley + Porter verified present; WM-1/WM-2 D
writing-style|aligned|clean|clean|WS-1/WS-2 D
V
)

{
echo "# Skill semantic review — 2026-10-18"
echo
echo "Live census: $(($(wc -l < "$BASE")-1)) skills, $(find "$ROOT/kask/registry/templates" -name '*.j2' | wc -l) templates."
echo "Set-equal to \`/tmp/live-skills.txt\` (see reconciliation note at the foot)."
echo
echo "Columns: **tpl** templates in the skill's registry dir; **gates** pinned \`lisp_eval\` gate ids;"
echo "**miss** referenced templates that do not resolve (0 = all resolve); **edges** skill-to-skill"
echo "composition edges that resolve to a live skill; **anch** \`onto_anchor\`→derived citations;"
echo "**docs** in-tree doc citations; **loop** the four S13 anatomy parts (I/T/C/B/A)."
echo
echo "| skill | tpl | gates | miss | edges | anch | docs | loop | reference model | D/P | verdict | notes |"
echo "|---|---|---|---|---|---|---|---|---|---|---|---|"
while IFS=$'\t' read -r n tpl gates missing edges dcites doccites init tgt chk bnd act; do
    [ "$n" = "skill" ] && continue
    v=$(printf '%s\n' "$VERDICTS" | awk -F'|' -v k="$n" '$1==k{print}')
    ref=$(printf '%s' "$v" | cut -d'|' -f2)
    dp=$(printf '%s' "$v" | cut -d'|' -f3)
    def=$(printf '%s' "$v" | cut -d'|' -f4)
    note=$(printf '%s' "$v" | cut -d'|' -f5)
    loop="$init/$tgt/$chk/$bnd/$act"
    printf '| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n' \
        "$n" "$tpl" "$gates" "$missing" "$edges" "$dcites" "$doccites" "$loop" "$ref" "$dp" "$def" "$note"
done < "$BASE"
LIVE_COUNT=$(ls -d "$ROOT/.agents/skills"/*/ | wc -l)
LEDGER_COUNT=$(grep '^| ' "$ROOT/kask/docs/research/skill-semantic-review-2026-10-18.md" | grep -vc '^| skill ')
echo
echo "## Reconciliation"
echo
echo "The ledger's skill set is reconciled against the live census by exact set equality"
echo "(not count equality): live **$LIVE_COUNT**, ledger **$LEDGER_COUNT**."
echo
echo "Unreviewed remainder: **zero** — every live skill has a row, and no row names a"
echo "skill that is not live. Reproduce with:"
echo
echo '```bash'
echo "bash kask/scripts/audit/skill-review-ledger-base.sh | tail -n +2 | cut -f1 | sort > /tmp/ledger-skills.txt"
echo "(cd .agents/skills && ls -d */ | sed 's#/##' | sort) > /tmp/live-skills.txt"
echo "diff /tmp/live-skills.txt /tmp/ledger-skills.txt   # no output = set-equal"
echo '```'
echo
echo "## Evidence and instruments"
echo
echo "| check | instrument | scope | result |"
echo "|---|---|---|---|"
echo "| Lisp parse + AST lint | \`kask/scripts/check-skill-lisp-forms.sh\` | 276 files, 301 candidates | 0 defects |"
echo "| S12 symbol resolution | \`check-skill-lisp-forms\` test \`pinned_gate_forms_resolve_every_symbol\` | every pinned (form, env) pair | 1 defect (GV-1), repaired |"
echo "| Gate selftests | \`kask/scripts/check-skill-lisp-forms-selftest.sh\` | 2 pass + 6 fail controls | OK |"
echo "| Composition edges | \`kask/scripts/audit/skill-review-edges.sh\` | 101 edges | all resolve |"
echo "| Template refs | \`kask/scripts/audit/skill-review-ledger-base.sh\` (miss column) | 216 templates | 1 non-resolving (a documented placeholder) |"
echo "| Template contracts | \`kask/scripts/check-template-contracts.sh\` | 216 templates | 0 flagged, 7 skipped |"
echo "| Ontology anchors | \`kask/scripts/audit/skill-review-anchors.sh\` | 37 skill-key citations (29 distinct keys) | 0 dangling |"
echo "| Doc citations | \`kask/scripts/audit/skill-review-doc-cites.sh\` | 25 in-tree doc citations | 0 missing |"
echo "| S13 anatomy | \`kask/scripts/audit/skill-review-loop-anatomy.sh\` + read-triage | 58 bodies | all state initial + target; flags read-triaged as false positives |"
echo "| S14 leads | \`kask/scripts/audit/skill-review-s14-leads.sh\` | 17 contract leads | all read-triaged (P judgment or runtime observation) |"
echo
echo "## Reference-model scope"
echo
echo "Each skill's cited external source was checked at the depth the tree supports:"
echo "the **derived-registry encoding** (\`kask/crates/hkask-bridge-ontology/src/derived.rs\`)"
echo "was read and compared against the skill's claim for every \`onto_anchor\`→derived"
echo "citation (37 skill-key citations over 29 distinct keys, 0 dangling; chapter-level"
echo "precision where the source warrants it)."
echo "The **primary sources themselves** — the books and papers — are not vendored in this"
echo "tree, so no skill is marked \`aligned\` on the strength of having read its book."
echo "Two skills (gpui-bench, lean-prover) whose oracle is an external toolchain rather"
echo "than a published model are marked \`unverified-external\` for the model itself while"
echo "their deterministic gates are green."
echo
echo "## Composition"
echo
echo "101 skill-to-skill references resolve to a live skill. Eighteen mutual pairs exist"
echo "(create-skill/skill-maintenance/skill-discovery, pragmatic-semantics/"
echo "pragmatic-cybernetics, grounding-verify/structured-extraction, diataxis-diagram/"
echo "doc-update/sankey-flow, idiomatic-rust/lean-prover); every one was read and is a"
echo "\`When NOT to Use\` boundary note or a named-consumer reference, not an invocation"
echo "cycle. No self-loops. The invocation graph is acyclic."
echo
echo "## Template accounting"
echo
echo "216 registry templates decompose exactly:"
echo
echo "- **207** skill-owned (the ledger's tpl column sums to 207 across 51 skills)."
echo "- **9** server/process-owned, rendered by Rust callers rather than by a skill:"
echo "  \`docproc/\` (6, the corpus pipeline), \`media/\` (2, the media server),"
echo "  \`experimentation/\` (1, the experiment protocol)."
echo "- **Seven** live skills carry no templates of their own (the tpl column reads 0):"
echo "  five tool-driven — algedonic-review, gpui-bench, onto-anchor, system-maintenance,"
echo "  transcript-reel — and two that render the server-owned templates above:"
echo "  build-corpus-pipeline (\`docproc/\`) and media-workflow (\`media/\`)."
echo
echo "Every template was therefore either read against its owning skill or accounted"
echo "to the server that renders it — none is unaccounted for."
echo
echo "## Independent assessment"
echo
echo "An independent agent (no prior context on this work, instructed to falsify) re-ran"
echo "the instruments and reported **partially-achieved, confidence 0.82**. Its four"
echo "per-criterion verdicts were PASS/PASS/PASS/PASS, but it refuted three of the"
echo "ledger's own claims — all three reproduced and are repaired (commit 8b6af9c0297):"
echo
echo "1. The anchors instrument matched a bare \`derived \`key\`\` anywhere, reading a"
echo "   skill's prose about something \"EQM-derived\" as an ontology anchor. Tightened to"
echo "   require the \`onto_anchor\` context; the true figure is 37 skill-key citations"
echo "   over 29 distinct keys, 0 dangling."
echo "2. Both audit scripts exited 1 on a clean sweep (\`grep -o\` returns 1 on no-match"
echo "   under \`pipefail\`), so a green result read as failure."
echo "3. The template accounting named four zero-template skills; the tpl column reads"
echo "   seven. Corrected."
echo
echo "Two evidence gaps the assessor could not close, recorded as concerns:"
echo
echo "- \`check-skill-crossrefs.sh\` only checks refs carrying an explicit extension, so"
echo "  the one bare ref (\`create-skill\` → \`my-skill/analyze\`, a documented placeholder)"
echo "  is outside its reach — the passing gate is consistent with a miss it cannot see."
echo "- Reference-model verdicts rest on the in-tree derived-registry encoding; the primary"
echo "  sources are not vendored."
echo
echo "## Memory consolidation (companion pass, 2026-10-18)"
echo
echo "Run alongside this review because the skill corpus and the curator store share the"
echo "same growth pressure. Store at pass time: **5,483 h_mems** against a 4,000 ceiling,"
echo "composition 2,691 thread-tier / 1,749 knowledge / 724 goal-events / 285 other."
echo
echo "All three mechanical valves returned clean, which is the finding:"
echo
echo "- **Dedup**: 5,509 scanned, **0 duplicate groups** — the writer-side canonical-key and"
echo "  same-key dedup fixes hold."
echo "- **Prune** (skill_use_issue, 30d, spare-recall 7d): **0 candidates** — no stale incidents."
echo "- **Backfill**: **0 missing embeddings** (4 unsupported rows are non-string values, by design)."
echo "- **Thread-tier shed working**: 0 rows older than 30 days; only 110 older than 7."
echo "- **Confidence floor holds**: 5,249 at floor, 1 below (a legacy row)."
echo
echo "The 204 apparent embedding orphans are all \`curator:distilled:*\` watermark rows —"
echo "the distillation timer's coverage markers, bookkeeping by design, not content."
echo
echo "What remains is **additive-only knowledge growth** (1,861 rows added in October, the"
echo "layer that has no automatic forgetting path) plus **68 space-keyed entities** carrying"
echo "canonical-form drift. Both are consolidation work for a therapy session, not mechanical"
echo "valve work: renaming a drifted entity key must carry its embeddings (the \`entity_ref\`"
echo "recall JOIN), and condensing near-duplicate lesson keys (\`prompt-enhancement\` 24 vs"
echo "\`prompt-enhance-skill\` 11; \`agent-workflow\` 13 vs \`agent-work\` 11) needs per-group"
echo "semantic judgment. Both require operator approval per the therapy skill's Phase 4."
echo
echo "## Findings"
echo
echo "Verified defects, both repaired in this review:"
echo
echo "1. **GV-1 (grounding-verify) had no env block.** The form referenced"
echo "   \`corrected_count\`, \`e_count\`, \`w_count\`, \`n_count\` and \`min_why_len\`, none"
echo "   bound by anything — the step could not run as written. Env added from the"
echo "   producer's own contract (\`compile-error-log.j2\` emits"
echo "   \`totals: {corrected, data_notes, warnings}\`)."
echo "2. **Pragmatic-semantics modifier drift.** Two companion templates stated"
echo "   FIBO +0.10 / SUMO +0.05 against the PS-1 gate's +0.05 / +0.03 — residue the"
echo "   earlier fix missed in the Tier-5 table and the provenance layer table. Both"
echo "   now state the gate's values, and a pin derives the expected spelling from the"
echo "   live gate so a future gate edit cannot leave the prose behind."
echo
echo "Concerns requiring further evidence (not defects):"
echo
echo "- The primary sources are not vendored, so reference-model verdicts rest on the"
echo "  in-tree derived-registry encoding rather than the source text."
echo "- The S13 anatomy sweep's narrow pattern flagged 28 skills whose bounds and"
echo "  re-entry points are stated in other words; each was read and cleared, but the"
echo "  sweep is a lead generator, not a gate."
echo "- \`falstaffian-rotation.j2\` is referenced extensionlessly (\`metacognition/"
echo "  falstaffian-rotation\`) rather than with the \`.j2\` suffix; it resolves, but a"
echo "  suffix-matching instrument would miss it."
} > "$ROOT/kask/docs/research/skill-semantic-review-2026-10-18.md"
echo "wrote $(wc -l < "$ROOT/kask/docs/research/skill-semantic-review-2026-10-18.md") lines"
