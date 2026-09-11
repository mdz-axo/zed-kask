# Company research verification regressions

These are synthetic fixtures, not a company report or cached financial data.
They exercise the verification stage shared by company-research-flash and
company-research-deep without external search, credentials or Exa Agent.

## Run through the skills

1. Load the two research SKILL.md bodies and `verification.json`. Render
   `company-research/verification-handoff` with `packet` as its context.
   Verify that the complete target and original source records survive rendering.
2. Execute the Lisp gate **extracted from the current handoff template** with
   each `gate_cases` record as its environment. Compare its returned symbol
   against `expected`; all cases must match. Do not duplicate the gate logic
   in a test implementation.
3. In a separate `spawn_agent`, load `grounding-verify`, execute its checks on
   the rendered synthetic packet, and return mechanical results and source
   assignments. Exercise each `report_cases` variation separately against the
   same sources (adding its additional_source and corresponding fixture-log
   record when present). Do not fetch example.invalid or claim live retrieval.
   Use the calling research skill's gate and correction instructions, rather
   than asking the verifier to approve publication itself.
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

Report separately: rendered-contract checks, deterministic gate cases, and
independent semantic/grounding exercises. A render or grep pass is not evidence
that a model followed the workflow, and these fixtures are not an end-to-end
live equity research run.
