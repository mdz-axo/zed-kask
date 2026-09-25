# Company research verification regressions

These are synthetic fixtures, not a company report or cached financial data.
They exercise the verification stage shared by company-research-flash and
company-research-deep without external search, credentials or Exa Agent.

## Run through the skills

1. Load the two research SKILL.md bodies and `verification.json`. Render
   `company-research/verification-handoff` with `packet` as its context.
   Verify that the complete target and original source records survive rendering.
2. Execute the original-source check and the Lisp gate **extracted from the
   current handoff template**. The gate fixtures consume the derived source
   status, not an authored `checked` field. The test also varies source bytes,
   discovery/extraction log, material-disclosure inventory and frozen forecasts;
   it checks omission and forbidden mutation against a clean and rationale-only
   control. A returned official-domain search URL with no extracted original,
   an absent search-results field, a mismatched result count, or a mismatched
   original URL must leave coverage `not_checked` even with a perfect fact
   score. The same source-check result separately preserves a known material
   omission when another discovery hit is unresolved; a corrected target
   removes only the omission, not the unresolved hit.
   Search fixtures are synthetic; they do not attest real tool provenance.
   Do not duplicate either decision form in test code.
3. For a live process check, separately invoke `grounding-verify` in a
   `spawn_agent` with the rendered packet and let it search public issuer and
   regulator disclosures using only public identifiers. Inspect the actual
   tool-call record, its source review and its claim-level mechanical results.
   Synthetic `example.invalid` records must not be represented as live discovery.
   The old `report_cases` were prose expectations with no executed oracle; they
   were removed rather than counted as passing tests. A generated-summary-only
   source and a material omission are now executable negative controls for the
   shared source check, but no test here proves full claim extraction.
4. Execute flash's final confidence/publication form against `publication_cases`.
   Its adjustment applies once to unadjusted confidence, not once per retry.
   Preserve the separate confidence band and the original ENTER eligibility.
5. Exercise correction: an unsupported source claim must re-enter collection
   or its producing stage, regenerate affected dependent outputs, and get a new
   verification record. Earlier findings stay unchanged and outside current
   score denominators. Three unsuccessful iterations end with a labelled draft.

Also check the actual source contracts: the two collection templates and the
semantic classifier must render retained source records, not just generated
summaries. A quote found only in generated prose cannot become an observation
or tool_verified through any of those handoffs. Any new summary belongs in the
final verification target.

Run the deterministic source check, gate and final publication cases (steps 2 and 4) with
`cargo test -p hkask-mcp-companies --test company_verification_gate`. The test
extracts the two current handoff forms and flash's publication form instead of maintaining copies of their rules.
The `missing_source_packet` case supplies `checks_complete=false` as the
caller's recorded preflight result; these scalar gate fixtures do not prove
that a caller computed that flag honestly from retained source bytes.
Rendered-contract checks, independent full semantic/grounding exercises, and
correction/retry behavior (steps 1, 3 and 5) remain separate checks. A passing
fixture test does not show that an agent ran the verifier or followed the
workflow, and these fixtures are not an end-to-end live equity research run.
