//! Derived concepts — the derived rung of the fallback ladder (P8.3):
//! recorded compositions over anchored constituents, sitting between the
//! domain supplements (rung 1) and the SUMO upper ontology (rung 3) in
//! term resolution (operator ruling 2026-09-10).
//!
//! A derived concept is NOT a fabricated ontology URI: it is a recorded
//! composition whose identity is machine-checkable and whose authority is
//! cited (an operator ruling with its date, or a published standard).
//! Constituents resolve through the ladder themselves — published term,
//! another derived concept, or a ruling — so a derived entry never
//! fabricates a constituent anchor.
//!
//! The ladder invariant (axis.rs P8.3): nothing is ever untagged. A term
//! with no published anchor resolves here; a term with no derived entry
//! either is pending its first ruling (a routing state, never a terminal
//! verdict) or falls to the upper/general layers. "Unanchored" as a
//! terminal state is the failure mode this registry exists to close
//! (observed 2026-09-10: "net margin" — the most-used term in the
//! financial-analysis vocabulary — resolved to a void because the then-loaded
//! FIBO rung published no ratio terms, and a pre-interest operating formula
//! wore the label through three operator corrections).

/// A derived concept: a recorded composition over anchored constituents.
pub struct DerivedConcept {
    /// Canonical term name (snake_case).
    pub term: &'static str,
    /// Resolution aliases (spaced and hyphenated forms resolve via
    /// normalization; list only genuinely different words).
    pub aliases: &'static [&'static str],
    /// The machine-checkable identity — the derivation the term denotes.
    pub identity: &'static str,
    /// The load-bearing semantics, stated so a violation is nameable.
    pub definition: &'static str,
    /// Constituent term names, each resolved through the ladder itself.
    pub constituents: &'static [&'static str],
    /// The authority record: an operator ruling (with date) or a
    /// published standard. An entry with no resolvable authority fails
    /// the build (same discipline as the published-term fixture).
    pub authority: &'static str,
}

/// The derived-concept registry. Seeded from the operator rulings of
/// 2026-09-10 (the DuPont capability envelope and the expectations-gap
/// definition) — the session where every one of these terms was
/// contested, corrected, and ratified.
pub const DERIVED_CONCEPTS: &[DerivedConcept] = &[
    DerivedConcept {
        term: "net_margin",
        aliases: &["net profit margin", "npm"],
        identity: "net income / revenue",
        definition: "Net income — post-interest, post-tax, the equity holder's claim — divided by revenue. Never a pre-interest operating formula: interest is the line where debt holders are paid first, and omitting it removes exactly what makes net income the equity holder's number.",
        constituents: &["net income", "revenue"],
        authority: "operator ruling 2026-09-10 (stated three times)",
    },
    DerivedConcept {
        term: "gross_margin",
        aliases: &["gross profit margin"],
        identity: "(revenue - cost of revenue) / revenue",
        definition: "Gross profit over revenue. Safe for enterprise-value calculations in acquisition scenarios where fixed costs will be restructured; never a reported expectations quantity for equity value.",
        constituents: &["revenue", "cost of revenue"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "operating_margin",
        aliases: &["operating profit margin"],
        identity: "operating income / revenue",
        definition: "Operating income (EBIT) over revenue — pre-interest, pre-tax. A margin of the enterprise, not of the equity holder.",
        constituents: &["operating income", "revenue"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "return_on_equity",
        aliases: &["roe"],
        identity: "net income / equity = net margin x asset turnover x equity multiplier",
        definition: "The DuPont identity: ROE is the product of net profit margin, asset turnover, and the equity multiplier. The identity holds per period; medians hold approximately. The headline profitability measure for financial-sector companies.",
        constituents: &[
            "net income",
            "equity",
            "net margin",
            "asset turnover",
            "equity multiplier",
        ],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "asset_turnover",
        aliases: &["total asset turnover"],
        identity: "revenue / total assets",
        definition: "Revenue generated per unit of total assets — the efficiency component of the DuPont decomposition.",
        constituents: &["revenue", "total assets"],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "equity_multiplier",
        aliases: &["financial leverage", "leverage multiplier"],
        identity: "total assets / equity",
        definition: "Total assets over equity — the leverage component of the DuPont decomposition.",
        constituents: &["total assets", "equity"],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "retention",
        aliases: &["retention ratio", "plowback ratio"],
        identity: "1 - dividends paid / net income, clamped to [0, 1]",
        definition: "The share of earnings retained in the business. A year paying dividends above earnings demonstrates zero retained funding, never negative — the clamp is part of the identity.",
        constituents: &["dividends paid", "net income"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "sustainable_growth_rate",
        aliases: &["sgr", "self-funding growth rate"],
        identity: "return on equity x retention",
        definition: "The Higgins sustainable growth rate: the growth a company can self-fund from internally generated earnings without external financing. The demonstrated-capability anchor of the growth leg of the expectations gap.",
        constituents: &["return on equity", "retention"],
        authority: "operator ruling 2026-09-10; Higgins (1977), 'Financial Management'",
    },
    DerivedConcept {
        term: "price_to_book",
        aliases: &["p/b", "pb ratio", "market to book"],
        identity: "price per share / book value per share",
        definition: "Market price over book equity per share. The equity-based valuation surface for financial-sector companies whose FCF is not meaningful.",
        constituents: &["book value per share"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "implied_roe",
        aliases: &["market implied roe"],
        identity: "price_to_book x (cost of equity - growth) + growth",
        definition: "The ROE the price demands, from the justified price-to-book identity P/B = (ROE - g)/(COE - g) — residual income on equity, the standard reverse solve for financial-sector companies.",
        constituents: &["price to book", "cost of equity"],
        authority: "operator ruling 2026-09-10; Damodaran, Applied Corporate Finance, Ch. 19",
    },
    DerivedConcept {
        term: "inference_resilience_boundary",
        aliases: &["inference resilience"],
        identity: "inference admission + deadline + provider outcome + circuit state machine + observation seam",
        definition: "The local inference-dispatch boundary that applies bounded non-spending circuit intervention and exposes coherent snapshots and receipts to central regulation. It owns enforcement; central regulation observes and escalates rather than duplicating the actuator.",
        constituents: &[
            "inference",
            "admission control",
            "circuit breaker",
            "observation",
        ],
        authority: "operator approval 2026-09-15 of the Inference Regulation Loop Completion plan; Nygard, Release It!; Beer (1972), Viable System Model",
    },
    DerivedConcept {
        term: "intervention_receipt",
        aliases: &["inference intervention receipt"],
        identity: "monotonic intervention id + transition kind + occurrence time",
        definition: "A durable observation that a bounded control-state transition actually occurred. A proposal or recommendation is not an intervention receipt.",
        constituents: &["intervention", "event identifier", "time"],
        authority: "operator approval 2026-09-15 of the Inference Regulation Loop Completion plan",
    },
    DerivedConcept {
        term: "observed_recovery",
        aliases: &["inference observed recovery"],
        identity: "later fresh observation reports return from degraded control state to healthy control state",
        definition: "A later fresh observation that the regulated state returned to its healthy range. It records progress while keeping causal attribution unverified unless an independent causal design exists.",
        constituents: &["observation", "recovery", "causal attribution"],
        authority: "operator approval 2026-09-15 of the Inference Regulation Loop Completion plan; Rother (2010), Toyota Kata",
    },
    DerivedConcept {
        term: "brier_score",
        aliases: &["brier scoring", "brier"],
        identity: "(p - o)^2 for a binary forecast of probability p with outcome o in {0, 1}",
        definition: "The mean squared-error scoring rule for probabilistic forecasts: 0 is perfect, 1 maximally wrong for a binary event, and a constant 0.5 forecast scores 0.25 — the dart-throwing baseline. Strictly proper: the expected score is minimized only by stating true probabilities, so the score measures calibration, not luck.",
        constituents: &["probability", "outcome"],
        authority: "operator ruling 2026-09-18; Brier, Verification of forecasts expressed in terms of probability, Monthly Weather Review 78(1):1-3 (1950)",
    },
    DerivedConcept {
        term: "forecast_calibration",
        aliases: &["calibration"],
        identity: "stated confidences match observed frequencies across many forecasts",
        definition: "The honesty of stated confidence: across many forecasts made at probability p, the event occurs about p of the time. Measured by calibration curves and Brier decomposition over a sample — a single outcome never measures calibration.",
        constituents: &["forecast", "confidence"],
        authority: "operator ruling 2026-09-18; Tetlock & Gardner, Superforecasting: The Art and Science of Prediction (2015)",
    },
    DerivedConcept {
        term: "test_harness",
        aliases: &["testing harness"],
        identity: "the runner, fixtures, and reporting that execute a suite of automated checks",
        definition: "The machinery that executes a codebase's automated checks and reports their outcomes — distinct from the checks themselves. A harness change alters what runs and how failures surface, never what is asserted.",
        constituents: &["test", "reporting"],
        authority: "operator ruling 2026-09-18; IEEE 829 test-documentation lineage; ISTQB Foundation syllabus usage",
    },
    DerivedConcept {
        term: "property_based_testing",
        aliases: &["property testing", "proptest"],
        identity: "a declared invariant over a declared input domain, checked on generated inputs with shrinking",
        definition: "Testing by stating a falsifiable invariant over an input domain and letting the tool generate random inputs to refute it; a failing case is shrunk to a minimal counterexample. Sampled evidence, not proof — a passing run is evidence over the generated sample, never exhaustive coverage.",
        constituents: &["invariant", "generated input", "counterexample"],
        authority: "operator ruling 2026-09-18; Claessen & Hughes, QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs, ICFP 2000",
    },
    DerivedConcept {
        term: "cybernetic_feedback_loop",
        aliases: &[
            "feedback loop",
            "negative feedback",
            "negative feedback loop",
        ],
        identity: "sense -> compare against a set point -> act to reduce the deviation -> the effect is sensed again",
        definition: "The self-regulation pattern: a system measures its own state, compares the measurement against a reference, and acts to reduce the deviation, with the action's effect entering the next measurement. Broken when any leg is missing, delayed beyond usefulness, or silent.",
        constituents: &["observation", "set point", "correction"],
        authority: "operator ruling 2026-09-18; Wiener, Cybernetics (1948); Ashby, An Introduction to Cybernetics (1956)",
    },
    DerivedConcept {
        term: "bounded_model_checking",
        aliases: &["model checking", "kani"],
        identity: "exhaustive verification of all executions up to a stated bound",
        definition: "Mechanical verification that proves or refutes a property over every execution up to a stated bound — an unwinding depth or an input size. Exhaustive within the bound, silent beyond it: a passed run is proof up to the stated bound, never beyond.",
        constituents: &["verification", "bound"],
        authority: "operator ruling 2026-09-18; Biere, Cimatti, Clarke & Zhu, Bounded Model Checking (2003); Clarke, Grumberg & Peled, Model Checking (1999)",
    },
    DerivedConcept {
        term: "godel_machine",
        aliases: &["godel machine"],
        identity: "a self-referential architecture that rewrites its own code only when a proof guarantees the rewrite is beneficial",
        definition: "A provably-optimal self-improvement architecture: an agent that searches for a proof that a candidate self-rewrite is beneficial before applying it. The kask program adopts the self-improvement loop discipline only; strict proof-search self-modification stays out of scope (Gödel plan ruling).",
        constituents: &["self-improvement", "proof"],
        authority: "operator ruling 2026-09-18; Schmidhuber, Gödel machine: self-referential universal problem solvers (2003, 2006)",
    },
    DerivedConcept {
        term: "risk_premium",
        aliases: &["risk premia", "equity risk premium"],
        identity: "expected return on the risky asset minus the known return on the risk-free asset",
        definition: "The minimum amount by which the expected return on a risky asset must exceed the known return on a risk-free asset (Wikidata Q523022, verbatim). Compensation demanded for bearing risk, never the raw expected return — a quote naming a total return a 'premium' is the nameable violation. In the gap program, the first wedge leg: the compensation layer between a probability-market price and a traditional discount rate.",
        constituents: &["expected return", "risk free rate"],
        authority: "operator ruling 2026-09-19; Wikidata Q523022; Mehra & Prescott, The Equity Premium: A Puzzle, Journal of Monetary Economics 15(2) (1985)",
    },
    DerivedConcept {
        term: "pdca_cycle",
        aliases: &[
            "pdca",
            "plan do check act",
            "plan-do-check-act",
            "plan do check adjust",
            "deming cycle",
            "shewhart cycle",
            "pdsa",
        ],
        identity: "plan -> do -> check -> act, repeated: propose a change, implement it, measure the result against the target, then standardize or begin again",
        definition: "The improvement cycle based on the scientific method (Lean Enterprise Institute lexicon): Plan determines goals and needed changes; Do implements them; Check evaluates results against the target; Act standardizes the change or begins the cycle again. In zed-kask a PDCA loop runs within one session, and its Check measures work on the task — it never evaluates the skill that ran it (operator rulings 2026-09-24). The nameable violation: a loop whose Check is the executor's own verdict on itself.",
        constituents: &["plan", "change", "measurement", "target"],
        authority: "operator ruling 2026-09-24; Lean Enterprise Institute lexicon, 'Plan, Do, Check, Act (PDCA)'; Wikidata Q820214; Shewhart (1939); Deming (1950s, JUSE 1951)",
    },
    DerivedConcept {
        term: "improvement_kata",
        aliases: &["improvement kata routine"],
        identity: "challenge -> grasp the current condition -> set the next target condition -> experiment (PDCA) against one obstacle, repeated",
        definition: "The repeating four-step routine by which a learner improves a process (Rother 2010; Lean Enterprise Institute lexicon): create a challenge, grasp the current condition with facts and data, set the next target condition, and run PDCA experiments against obstacles. Human practice sets the target about two weeks out; an agent practices it within one session, so its horizon is counted in bounded experiments (operator ruling 2026-09-24).",
        constituents: &[
            "challenge",
            "current condition",
            "target condition",
            "pdca cycle",
        ],
        authority: "operator ruling 2026-09-24; Lean Enterprise Institute lexicon, 'Kata'; Rother, Toyota Kata (2010), Wikidata Q7830807",
    },
    DerivedConcept {
        term: "coaching_kata",
        aliases: &["coaching kata questions"],
        identity: "five questions a coach asks a learner practicing the Improvement Kata, at the place where the work is done",
        definition: "The routine by which a coach teaches the Improvement Kata (Rother 2010; Lean Enterprise Institute lexicon): five questions that provoke and reinforce PDCA thinking, with procedural guidance rather than solutions. The coach is a separate role from the learner.",
        constituents: &["improvement kata", "coach", "learner"],
        authority: "operator ruling 2026-09-24; Lean Enterprise Institute lexicon, 'Kata'; Rother, Toyota Kata (2010), Wikidata Q7830807",
    },
    DerivedConcept {
        term: "gemba_walk",
        aliases: &["gemba", "genba", "going to the gemba", "genchi gembutsu"],
        identity: "go to the actual place where value is created, observe the work directly and ask questions, before taking action",
        definition: "A management practice for grasping the current situation through direct observation and inquiry before taking action (Lean Enterprise Institute lexicon; gemba, 'actual place'). In zed-kask it names the algedonic review's phase where the operator and the Curator inspect recorded skill execution and the operator evaluates skills — the only place skill evaluation happens (operator ruling 2026-09-24).",
        constituents: &["observation", "current condition", "review"],
        authority: "operator ruling 2026-09-24; Lean Enterprise Institute lexicon, 'Gemba'",
    },
    DerivedConcept {
        term: "goodharts_law",
        aliases: &["goodhart law", "goodhart's law", "goodharts law"],
        identity: "when a measure becomes a target, it ceases to be a good measure",
        definition: "The adage that a measure an agent optimizes stops tracking what it was meant to measure (Wikidata Q2575082). In zed-kask it is the reason skill evaluation is logically separated from skill execution: the executing session records outcomes and proposes changes, and only the operator, in the algedonic review, evaluates (operator ruling 2026-09-24).",
        constituents: &["measure", "target"],
        authority: "operator ruling 2026-09-24; Wikidata Q2575082; Goodhart, Problems of Monetary Management (1975)",
    },
    DerivedConcept {
        term: "viable_system_model",
        aliases: &["viable system model", "vsm"],
        identity: "five necessary and sufficient subsystems (S1 operations, S2 coordination, S3 control, S4 intelligence, S5 policy) for an organization to remain viable",
        definition: "Stafford Beer's model of the organizational structure of any autonomous, viable system (Brain of the Firm, 1972). In zed-kask the operator is the policy level (S5) and algedonic signals are the escalation path from operations to policy.",
        constituents: &["subsystem", "recursion", "algedonic signal"],
        authority: "operator ruling 2026-09-25; Beer, Brain of the Firm (1972)",
    },
    DerivedConcept {
        term: "algedonic_signal",
        aliases: &["algedonic signal", "algedonic alert", "algedonic"],
        identity: "a pain or pleasure signal that bypasses normal channels to alert the policy level when a subsystem breaches threshold",
        definition: "In Beer's Viable System Model (Brain of the Firm, 1972), an alarm channel from System 1 to System 5 that carries only the signal that something is wrong or right. In zed-kask the cybernetics loop escalates threshold breaches to a durable queue that the operator reviews in algedonic-review.",
        constituents: &["viable system model", "threshold"],
        authority: "operator ruling 2026-09-25; Beer, Brain of the Firm (1972)",
    },
    DerivedConcept {
        term: "definition_of_done",
        aliases: &["definition of done", "dod"],
        identity: "a formal description of the state of an increment when it meets the quality measures required for the product",
        definition: "The Scrum commitment for the Increment (Schwaber & Sutherland, The Scrum Guide, 2020): work that does not meet the Definition of Done is not part of the increment. In zed-kask it is program-manager's Phase 4 checklist.",
        constituents: &["increment", "quality measure"],
        authority: "operator ruling 2026-09-25; Schwaber & Sutherland, The Scrum Guide (2020)",
    },
    DerivedConcept {
        term: "requirements_engineering",
        aliases: &["requirements engineering"],
        identity: "the interdisciplinary function that elicits, specifies, verifies and manages stakeholder and system requirements across the life cycle",
        definition: "As defined by ISO/IEC/IEEE 29148:2018 (Systems and software engineering, Life cycle processes, Requirements engineering). In zed-kask a requirement is the operator's falsifiable outcome claim and its acceptance criteria are its verification plan.",
        constituents: &["requirement", "verification"],
        authority: "operator ruling 2026-09-25; ISO/IEC/IEEE 29148:2018",
    },
    DerivedConcept {
        term: "prompt_engineering",
        aliases: &["prompt engineering"],
        identity: "the systematic design of prompts along four aspects: profile and instruction, knowledge, reasoning and planning, and reliability",
        definition: "The taxonomy of Liu et al., 'A comprehensive taxonomy of prompt engineering techniques for large language models', Frontiers of Computer Science 20(3) 2003601 (2026), doi:10.1007/s11704-025-50058-z. prompt-enhance's seven prompt types are this project's mapping onto the four aspects.",
        constituents: &["prompt", "taxonomy"],
        authority: "operator ruling 2026-09-25; Liu et al., Frontiers of Computer Science 20(3) 2003601 (2026)",
    },
    DerivedConcept {
        term: "diataxis",
        aliases: &["diataxis", "diátaxis", "diataxis framework"],
        identity: "documentation organized into four kinds by user need: tutorials, how-to guides, reference and explanation",
        definition: "Daniele Procida's systematic framework for technical documentation (diataxis.fr): each document serves one of four needs (learning, a task, information, understanding), and its kind determines its voice. In zed-kask it organizes the kask/docs sets and the voice of diataxis-diagram output.",
        constituents: &["tutorial", "how-to guide", "reference", "explanation"],
        authority: "operator ruling 2026-09-26; Procida, Diataxis (diataxis.fr)",
    },
    DerivedConcept {
        term: "sankey_diagram",
        aliases: &["sankey diagram", "sankey"],
        identity: "a flow diagram in which the width of each arrow is proportional to the quantity it carries",
        definition: "As established by Schmidt, 'The Sankey Diagram in Energy and Material Flow Management', Journal of Industrial Ecology 12(1):82-94 and 12(2):173-185 (2008): in the original engineering sense a simple Sankey conserves energy or mass at every node. In zed-kask, sankey-flow sets conservation per domain (mandatory, asserted, none).",
        constituents: &["flow", "quantity", "node"],
        authority: "operator ruling 2026-09-26; Schmidt, Journal of Industrial Ecology 12(1) (2008)",
    },
    DerivedConcept {
        term: "minimalism_documentation",
        aliases: &[
            "minimalism in documentation",
            "minimalist documentation",
            "minimalism documentation",
        ],
        identity: "documentation designed around the reader's real tasks, cutting material that does not support action",
        definition: "John M. Carroll's minimalist instruction (The Nurnberg Funnel, MIT Press, 1990): task-oriented, brief material that lets users act and recover from errors. In zed-kask it anchors doc-update's fewer-than-75-files condensation cap (working target 60).",
        constituents: &["task", "documentation"],
        authority: "operator rulings 2026-09-26 and 2026-09-30 (cap); Carroll, The Nurnberg Funnel (1990)",
    },
    DerivedConcept {
        term: "strangler_fig_pattern",
        aliases: &[
            "strangler fig pattern",
            "strangler fig",
            "strangler fig application",
        ],
        identity: "replace a legacy system incrementally by routing behavior through a new layer until the old one can be removed",
        definition: "Martin Fowler's StranglerFigApplication (martinfowler.com, 2004): new code grows around the old, taking over one capability at a time while both run. In zed-kask it is refactor-architecture's one-domain-per-commit service extraction.",
        constituents: &["legacy system", "incremental migration"],
        authority: "operator ruling 2026-09-26; Fowler, StranglerFigApplication, martinfowler.com (2004)",
    },
    DerivedConcept {
        term: "deep_module",
        aliases: &["deep module", "deep modules"],
        identity: "a module whose interface is small relative to the functionality it hides",
        definition: "John Ousterhout, A Philosophy of Software Design (2018), Ch. 4 'Modules Should Be Deep': deep modules are those whose interfaces are much simpler than their implementations; small modules tend to be shallow because the benefit they provide is negated by the cost of learning and using their interfaces. Ch. 7.1 names the shallow extreme — the pass-through method, which does little except invoke another method whose signature is similar or identical to the callee's, fixed by exposing the lower level directly or merging. In zed-kask it is the target of deep-module and refactor-architecture's deletion test.",
        constituents: &["interface", "implementation"],
        authority: "operator ruling 2026-09-26; Ousterhout, A Philosophy of Software Design (2018), Ch. 4 'Modules Should Be Deep' and Ch. 7.1 'Pass-through methods'",
    },
    DerivedConcept {
        term: "owasp_llm_top_10",
        aliases: &[
            "owasp llm top 10",
            "owasp llm top-10",
            "owasp top 10 for llm applications",
        ],
        identity: "the OWASP list of the ten most critical security risks in large language model applications",
        definition: "OWASP Top 10 for LLM Applications, 2025 edition (OWASP GenAI Security Project). In zed-kask it frames kask-seam-audit's security track.",
        constituents: &["security risk", "llm application"],
        authority: "operator ruling 2026-09-26; OWASP Top 10 for LLM Applications (2025)",
    },
    DerivedConcept {
        term: "hicks_law",
        aliases: &["hick's law", "hicks law", "hick-hyman law"],
        identity: "decision time grows with the logarithm of the number of equally likely choices",
        definition: "Hick, 'On the rate of gain of information', Quarterly Journal of Experimental Psychology 4(1):11-26 (1952). In zed-kask it grounds kask-seam-audit's five-primary-action budget per card or panel.",
        constituents: &["decision time", "choice"],
        authority: "operator ruling 2026-09-26; Hick, Quarterly Journal of Experimental Psychology 4(1) (1952)",
    },
    DerivedConcept {
        term: "plussing",
        aliases: &["plussing", "plus-ing"],
        identity: "improving an idea by building on it without judging it, adding to what works rather than rejecting what does not",
        definition: "Pixar's feedback practice described by Ed Catmull, Creativity, Inc. (2014): criticism is offered as an addition ('plus') to the idea, never as a veto. In zed-kask it is improv's default mode: build on agreeable seeds and omit the rest.",
        constituents: &["feedback", "idea"],
        authority: "operator ruling 2026-09-26; Catmull, Creativity, Inc. (2014)",
    },
    DerivedConcept {
        term: "yes_and",
        aliases: &["yes and", "yes, and", "yes-and", "improvisation", "improv"],
        identity: "accept the other player's offer as given and extend it",
        definition: "The offer-and-accept principle of improvisational theatre (Keith Johnstone, Impro: Improvisation and the Theatre, 1979): blocking an offer stops the scene, accepting and adding advances it. In zed-kask it grounds improv's Yes And and Yes But modes.",
        constituents: &["offer", "acceptance"],
        authority: "operator ruling 2026-09-26; Johnstone, Impro: Improvisation and the Theatre (1979)",
    },
    DerivedConcept {
        term: "expectations_gap",
        aliases: &[],
        identity: "price-implied expectations minus fundamentals-demonstrated capability, per leg (growth, margin, duration)",
        definition: "The Mauboussin-Rappaport Expectations Investing sense: start from the known stock price, reverse-solve the expectations it implies, and hold them against what the fundamentals have demonstrated — Higgins SGR anchoring the growth leg. Not the audit sense (Liggio 1974), the macro sense (Treasury GDP-neutral level), or the strategy and EU-politics senses (Wikidata Q5034469, Q5162851). The nameable violation: calling a raw multiple an expectation without the reverse-solve.",
        constituents: &["implied expectation", "demonstrated capability"],
        authority: "operator ruling 2026-09-19; Rappaport & Mauboussin, Expectations Investing: Reading Stock Prices for Better Returns, Harvard Business School Press (2001)",
    },
    DerivedConcept {
        term: "superforecasting",
        aliases: &["superforecaster", "good judgment project method"],
        identity: "triage -> Fermi-decompose -> outside view (base rate) -> inside view -> Bayesian updating on evidence -> synthesis -> calibrated probability, scored by Brier over resolved forecasts",
        definition: "The forecasting method of Tetlock & Gardner, Superforecasting: The Art and Science of Prediction (2015), distilled from the Good Judgment Project: triage questions into the Goldilocks zone, break them into tractable sub-questions, anchor on a reference-class base rate before case detail, update incrementally on evidence, synthesize perspectives, and state granular probabilities judged by calibration across many resolved questions, never by one outcome.",
        constituents: &[
            "base rate",
            "fermi decomposition",
            "bayesian updating",
            "brier score",
        ],
        authority: "operator ruling 2026-09-25; Tetlock & Gardner, Superforecasting: The Art and Science of Prediction, Crown (2015)",
    },
    DerivedConcept {
        term: "explanation_quality_marker",
        aliases: &["explanation quality markers", "eqm", "eqms"],
        identity: "a theory-guided reasoning pattern scored 0/1/2 in a forecast rationale; composites flag poor forecasts more reliably than they identify excellent ones",
        definition: "The rationale-quality instrument of Karvetski, Huang, Kucinskas et al., Measuring Judgment Quality in Natural-Language Explanations: Evidence from Forecasting Tournaments, Forecasting Research Institute (2026): 60 markers (good habits and warning signs) scored by an LLM, aggregated to forecast- and forecaster-level composites, validated against realized accuracy.",
        constituents: &["forecast rationale", "composite score", "brier score"],
        authority: "operator ruling 2026-09-25; Karvetski, Huang, Kucinskas et al., Measuring Judgment Quality in Natural-Language Explanations: Evidence from Forecasting Tournaments, Forecasting Research Institute (2026)",
    },
    DerivedConcept {
        term: "metacognition",
        aliases: &[
            "metacognitive",
            "knowing what you know",
            "dunning kruger effect",
        ],
        identity: "judging the reach of one's own knowledge; the skill needed to perform is the skill needed to judge the performance, so self-assessment must be checked against external feedback",
        definition: "David Dunning's account of self-knowledge and expertise (Kruger & Dunning 1999; Dunning 2011, 2019): people with poor expertise carry a double curse, lacking both the competence and the knowledge needed to recognize its absence, and cannot see where the geography of their ignorance begins; the same deficit hides superior competence in others (the Cassandra quandary). The corrective is feedback from outside the self-judgment.",
        constituents: &["self-assessment", "expertise", "external feedback"],
        authority: "operator ruling 2026-09-25; Kruger & Dunning, Unskilled and Unaware of It, Journal of Personality and Social Psychology 77(6) (1999); Dunning, The Dunning-Kruger Effect: On Being Ignorant of One's Own Ignorance, Advances in Experimental Social Psychology 44 (2011)",
    },
    DerivedConcept {
        term: "scenario_planning",
        aliases: &["scenario plan", "scenario analysis method"],
        identity: "focal question -> driving forces -> two critical uncertainties as axes -> divergent narratives -> implications and early-warning indicators, with the project itself assessed for learning and performance",
        definition: "Peter Schwartz's method for thinking about the long view (1991): frame a decision-relevant focal question, map driving forces, choose two independent critical uncertainties, write divergent plausible futures, and derive strategies with leading indicators; Thomas Chermack's performance-based framework (2011) adds the assessment of whether the scenario project changed learning and decisions.",
        constituents: &[
            "focal question",
            "driving forces",
            "critical uncertainty",
            "early-warning indicator",
        ],
        authority: "operator ruling 2026-09-25; Schwartz, The Art of the Long View, Doubleday (1991); Chermack, Scenario Planning in Organizations, Berrett-Koehler (2011)",
    },
    DerivedConcept {
        term: "maia_event_based_scenario_template",
        aliases: &["maia scenario template", "event-based scenario template"],
        identity: "type -> subject -> events (binomial yes/no questions with deadlines) -> path dependence -> probability with basis (technical feasibility or scaling/distribution), over fixed time horizons",
        definition: "The Merchant Adventures (MAIA) event-based scenario process: construct sets of key future events as yes/no questions over fixed time horizons in different contexts, each event carrying a name, a dated yes/no question, path dependence, a probability estimate, and a basis in terms of technical feasibility or scaling/distribution issues. Four scenario types (company update, company analysis, emerging economic potential, economic potential analysis) crossed with three time horizons (tactical 12-18 months, strategic 3-5 years, long-term 7-10 years); the events can function as a forward-looking basis for thinking about portfolio risks. Source: MAIA, 'Time Horizons, Expected Events and the Scenario Process' (zk-reference/maia-corpus); the post itself builds on Tetlock's Superforecasting and Schwartz's call for imagination and analytical realism.",
        constituents: &["scenario_planning", "superforecasting"],
        authority: "operator ruling 2026-10-06 (scenario-server redesign PR-05, authorized by the implementation-plan grant); MAIA, Time Horizons, Expected Events and the Scenario Process, zk-reference/maia-corpus",
    },
    DerivedConcept {
        term: "falsifiability",
        aliases: &[
            "falsifiable",
            "strong inference",
            "multiple working hypotheses",
            "eliminative inference",
        ],
        identity: "a claim is admissible only if some observation could contradict it; rival hypotheses are eliminated by discriminating tests, and survivors are corroborated, never confirmed",
        definition: "Popper's demarcation criterion (The Logic of Scientific Discovery, 1959) joined to Chamberlin's method of multiple working hypotheses (1890) and Platt's strong inference (1964): hold several falsifiable hypotheses at once, design tests whose outcomes rule some out, and eliminate on contradiction. Pearl's do-operator (Causality, 2009) supplies the minimal counterfactual for causal hypotheses.",
        constituents: &[
            "falsifier",
            "discriminating test",
            "counterfactual",
            "corroboration",
        ],
        authority: "operator ruling 2026-09-25; Popper, The Logic of Scientific Discovery (1959); Platt, Strong Inference, Science 146 (1964); Chamberlin, The Method of Multiple Working Hypotheses, Science 15 (1890); Pearl, Causality (2009)",
    },
    DerivedConcept {
        term: "finer_criteria",
        aliases: &["finer"],
        identity: "a research question is judged Feasible, Interesting, Novel, Ethical and Relevant before study design",
        definition: "The five-criterion screen for a good research question from Hulley, Cummings et al., Designing Clinical Research (1988; 4th ed. 2013). Each criterion is a judgment, not a measurement.",
        constituents: &["feasible", "interesting", "novel", "ethical", "relevant"],
        authority: "operator ruling 2026-09-25; Hulley, Cummings, Browner, Grady & Newman, Designing Clinical Research (1988; 4th ed. 2013)",
    },
    DerivedConcept {
        term: "pico",
        aliases: &["pico framework", "picot"],
        identity: "a clinical question structured as Population, Intervention, Comparison, Outcome",
        definition: "The well-built clinical question of Richardson, Wilson, Nishikawa & Hayward (1995): name the population, the intervention or exposure, the comparison, and the outcome, so the question is answerable and testable.",
        constituents: &["population", "intervention", "comparison", "outcome"],
        authority: "operator ruling 2026-09-25; Richardson, Wilson, Nishikawa & Hayward, The well-built clinical question, ACP Journal Club 123(3) (1995)",
    },
    DerivedConcept {
        term: "experimental_design",
        aliases: &[
            "experimental design",
            "study design",
            "design of experiments",
        ],
        identity: "a preregistered plan that maps rival hypotheses and material auxiliary failures to discriminating observations, then specifies how those observations will be obtained with valid measurement, identification, adequate information, controlled error and reproducible execution",
        definition: "The design-of-experiments sense joining Platt's strong inference to NIST/SEMATECH statistical engineering, JCGM measurement discipline and the Shadish–Cook–Campbell validity questions: generate materially different candidate designs, reject those that cannot distinguish rivals or localize important measurement/model failures, and select among valid survivors by information, cost, risk and reversibility. It is not form filling and not the execution itself.",
        constituents: &[
            "falsifiability",
            "measurement",
            "identification",
            "information",
            "reproducibility",
        ],
        authority: "operator ruling 2026-10-18 (experimental-design capability); Platt, Strong Inference, Science 146 (1964), doi:10.1126/science.146.3642.347; NIST/SEMATECH Engineering Statistics Handbook, Process Improvement; JCGM 100:2008, doi:10.59161/JCGM100-2008E",
    },
    DerivedConcept {
        term: "hypothesis_testing",
        aliases: &["hypothesis testing", "scientific hypothesis testing"],
        identity: "a human-gated inquiry lifecycle that frames falsifiable rivals, designs a valid opportunity for refutation, preregisters the design, obtains measurements through a bound executor, interprets outcomes without binary-test fallacies, audits the findings and submits the conclusion to human review",
        definition: "The functional lifecycle sense used by zed-kask: hypothesis-framer owns formulation, falsifiability owns admissibility and rivals, experimental-design owns the study design, domain executors measure, the experimentation registry seals the lab record, grounding-verify audits factual claims, and the operator owns review. It is broader than a null-hypothesis significance test and narrower than autonomous science.",
        constituents: &[
            "falsifiability",
            "experimental design",
            "preregistration",
            "measurement",
            "human review",
        ],
        authority: "operator ruling 2026-10-18 (rename co-scientist to the functional hypothesis-testing lifecycle); Popper, The Logic of Scientific Discovery (1959); Platt, Strong Inference (1964); Nosek et al., The preregistration revolution, doi:10.1073/pnas.1708274114",
    },
    DerivedConcept {
        term: "measurement_uncertainty",
        aliases: &["measurement uncertainty", "uncertainty of measurement"],
        identity: "a non-negative parameter associated with a measurement result that characterizes the dispersion of quantity values reasonably attributable to the measurand",
        definition: "The VIM/GUM metrological sense: uncertainty qualifies a measurement result and propagates contributions from the measurement model, calibration, reference data and repeatability. It is not ignorance in general and not a substitute for identifying the measurand or validating instrument scope.",
        constituents: &["measurement", "measurand", "dispersion", "calibration"],
        authority: "operator ruling 2026-10-18 (experimental-design capability); JCGM 100:2008, Evaluation of measurement data — Guide to the expression of uncertainty in measurement, doi:10.59161/JCGM100-2008E; JCGM 200:2012 VIM",
    },
    DerivedConcept {
        term: "randomized_experiment",
        aliases: &[
            "randomized experiment",
            "randomised experiment",
            "random assignment",
        ],
        identity: "an experiment in which experimental units are assigned to treatments by a chance mechanism specified independently of their potential outcomes",
        definition: "The design-of-experiments sense: random assignment protects the treatment contrast from systematic association with unit characteristics under the stated assignment mechanism. It does not by itself validate measurement, guarantee external validity, repair attrition or justify an underpowered analysis.",
        constituents: &[
            "experiment",
            "experimental unit",
            "treatment",
            "random assignment",
        ],
        authority: "operator ruling 2026-10-18 (experimental-design capability); Fisher, The Design of Experiments (1935); NIST/SEMATECH Engineering Statistics Handbook, Completely Randomized Designs",
    },
    DerivedConcept {
        term: "equivalence_testing",
        aliases: &[
            "equivalence testing",
            "equivalence test",
            "practical equivalence",
        ],
        identity: "a prespecified bounded-claim test whose evidence criterion requires an adequately precise interval to fall within a practically meaningful equivalence region",
        definition: "The bounded-inference sense: equivalence is evidence that the effect lies inside a margin defined before confirmatory data inspection, commonly assessed by two one-sided tests or the corresponding confidence interval. Failure to reject a point null is not equivalence, and a post-hoc margin cannot convert imprecision into support.",
        constituents: &[
            "equivalence margin",
            "confidence interval",
            "precision",
            "practical significance",
        ],
        authority: "operator ruling 2026-10-18 (experimental-design capability); Lakens, Scheel & Isager, Equivalence Testing for Psychological Research: A Tutorial (2018), doi:10.1177/2515245918770963",
    },
    DerivedConcept {
        term: "preregistration",
        aliases: &[
            "preregistration",
            "pre-registration",
            "preregistered analysis",
        ],
        identity: "a time-stamped, immutable specification of confirmatory hypotheses, outcomes, analysis, exclusions, multiplicity and stopping decisions recorded before inspecting the confirmatory data",
        definition: "The confirmatory-research sense: preregistration separates planned tests from exploratory adaptations and preserves every later deviation visibly. In zed-kask the scientific DesignSpec is complete at experiment_propose; the later experiment_seal fingerprints and attests the already-declared contract rather than authorizing design after baseline inspection.",
        constituents: &["hypothesis", "analysis plan", "stopping rule", "provenance"],
        authority: "operator ruling 2026-10-18 (experimental-design capability); Nosek et al., The preregistration revolution, PNAS 115(11):2600–2606 (2018), doi:10.1073/pnas.1708274114",
    },
    DerivedConcept {
        term: "rubric",
        aliases: &["rubrics", "evaluation rubric", "target rubrics"],
        identity: "a named, typed pass/fail criterion declared before measurement and verified by a deterministic check",
        definition: "The Agentic Scientist's machine-checkable experiment criterion (Lange, Grillotti, Yamada, Charakorn & Ha, The Agentic Scientist: Orchestrating Coding Agents for Automated Scientific (Co-)Discovery, arXiv:2610.18933, 2026): each declared experiment carries a rubric bundle — at least one metric rubric and at least one implementation rubric, with stable snake_case ids and concrete measurable descriptions — and each rubric's verification is a typed check (source_contains / not_contains / regex patterns, or a recorded harness report), assessed as passed / failed / not_assessed. In zed-kask it grounds the experimentation server's typed rubric block on experiment_propose and shares its verification vocabulary with the recorded-run evaluator kinds — one grammar from declaration to recorded run.",
        constituents: &[
            "metric rubric",
            "implementation rubric",
            "verification",
            "assessment",
        ],
        authority: "operator ruling 2026-10-15 (agentic-scientist-v3 integration, decision D3); Lange, Grillotti, Yamada, Charakorn & Ha, The Agentic Scientist, arXiv:2610.18933 (2026)",
    },
    DerivedConcept {
        term: "ablation",
        aliases: &["ablation study", "ablated variant"],
        identity: "a variant derived from a parent that removes or disables exactly one component, evaluated under the same sealed contract to isolate that component's contribution",
        definition: "The controlled-removal study from The Agentic Scientist's variants pipeline (Lange, Grillotti, Yamada, Charakorn & Ha, arXiv:2610.18933, 2026): the winning program is compared against baselines and ablated variants — each derived from the parent and differing in exactly the ablated component — under the same sealed evaluator, with cross-task generalization checks. In zed-kask it grounds the experimentation server's variant role: an ablation-role variant requires a parent_variant_id and is evaluated under the experiment's sealed contract.",
        constituents: &["baseline", "ablated component", "sealed evaluation"],
        authority: "operator ruling 2026-10-15 (agentic-scientist-v3 integration, decision D3); Lange, Grillotti, Yamada, Charakorn & Ha, The Agentic Scientist, arXiv:2610.18933 (2026)",
    },
    DerivedConcept {
        term: "multi_criteria_decision_analysis",
        aliases: &[
            "mcda",
            "multiple criteria decision analysis",
            "swing weighting",
        ],
        identity: "alternatives scored on weighted criteria, aggregated by a value model, with sensitivity analysis on the weights",
        definition: "Decision analysis over several criteria (Belton & Stewart, Multiple Criteria Decision Analysis, 2002): structure criteria, elicit weights (swing weighting per Keeney & Raiffa, Decisions with Multiple Objectives, 1976), score alternatives, aggregate with a weighted-sum value model, and test how the ranking depends on the weights.",
        constituents: &["criterion", "weight", "value model", "sensitivity analysis"],
        authority: "operator ruling 2026-09-25; Belton & Stewart, Multiple Criteria Decision Analysis: An Integrated Approach (2002); Keeney & Raiffa, Decisions with Multiple Objectives (1976)",
    },
    DerivedConcept {
        term: "scientific_debugging",
        aliases: &["root cause analysis", "systematic debugging", "debugging"],
        identity: "make the failure reproducible, hypothesize causes, predict and run one-variable experiments, and conclude only from observed results",
        definition: "Debugging as the scientific method (Zeller, Why Programs Fail, 2009): observe the failure, form hypotheses, derive predictions, test by experiment, and refine until the cause is diagnosed; with Agans's rules (Debugging: The 9 Indispensable Rules, 2002) — make it fail, quit thinking and look, change one thing at a time.",
        constituents: &[
            "reproduction",
            "hypothesis",
            "experiment",
            "regression test",
        ],
        authority: "operator ruling 2026-09-25; Zeller, Why Programs Fail: A Guide to Systematic Debugging (2009); Agans, Debugging: The 9 Indispensable Rules (2002)",
    },
    DerivedConcept {
        term: "gorilla_game",
        aliases: &["gorilla", "gorilla assessment"],
        identity: "in a technology market, the company owning the de facto standard captures outsized, durable returns",
        definition: "The technology-investing model of Moore, Johnson & Kippola, The Gorilla Game (1998): in hypergrowth markets a proprietary, standard-setting architecture with high switching costs makes one company the gorilla, with chimps and monkeys taking the rest.",
        constituents: &["de facto standard", "switching cost", "hypergrowth"],
        authority: "operator ruling 2026-09-25; Moore, Johnson & Kippola, The Gorilla Game (1998)",
    },
    DerivedConcept {
        term: "wardley_map",
        aliases: &["wardley mapping", "wardley maps"],
        identity: "a value chain of components placed on an evolution axis from genesis through custom and product to commodity",
        definition: "Simon Wardley's situational-awareness map (Wardley Maps, 2016): anchor on a user need, lay out the dependent value chain, and position each component by evolution (genesis, custom-built, product, commodity) to see movement and choose strategy.",
        constituents: &["user need", "value chain", "evolution axis"],
        authority: "operator ruling 2026-09-25; Wardley, Wardley Maps (2016)",
    },
    DerivedConcept {
        term: "hidden_champion",
        aliases: &["hidden champions"],
        identity: "a little-known firm that leads its narrow global market niche through focus, depth and closeness to customers",
        definition: "Hermann Simon's category (Hidden Champions of the 21st Century, 2009): a company that is number one to three in its world market, below broad public visibility, holding a narrowly defined niche through customer closeness, deep value chains and globalization of the niche.",
        constituents: &["market niche", "global market share", "customer closeness"],
        authority: "operator ruling 2026-09-25; Simon, Hidden Champions of the 21st Century (2009)",
    },
    DerivedConcept {
        term: "investment_thesis",
        aliases: &["three pillar thesis", "maia thesis structure"],
        identity: "business franchise + management quality + valuation (expectations gap) combine into the thesis on a company",
        definition: "The Merchant Adventures (MAIA) thesis structure: (1) Business/Franchise — what the company does to make money and why it is profitable and growing (how it takes one dollar and turns it into two); (2) Management — why the people running the company are the right ones to keep it growing and making money; (3) Valuation — the expectations the current price discounts, and the gap versus what the business is capable of achieving with quality management or the management it has. The thesis carries its own confirm-or-refute factors (Popper falsifiability).",
        constituents: &[
            "business franchise",
            "management quality",
            "expectations gap",
        ],
        authority: "operator ruling 2026-09-27; Merchant Adventures (John Brooks), Technological Capability and Human Judgment (Substack 138628465)",
    },
    DerivedConcept {
        term: "maia_listening",
        aliases: &["maia listening", "maia v3 listening", "maia method"],
        identity: "an earnings call read through a stance block and seven sections, keeping only claims linked to a strategic path, each cited verbatim from the transcript",
        definition: "The MAIA method's earnings-call listening template (v3): seven sections (margin trajectory, working capital power, moat evidence, capital allocation, expectations gap, guidance versus expectations, management consistency), a horizon model centered on the 12-36 month seam between tactical checkpoints and strategic goals, and a retrieve-cite-verify discipline so every evidence quote is a substring of the source.",
        constituents: &["strategic checkpoint", "horizon model", "verbatim evidence"],
        authority: "operator ruling 2026-09-25 (the MAIA method is the reference model); the MAIA v3 listening template retired 2026-10-18 with the company-research skills — the retrieve-cite-verify discipline it carried survives in grounding-verify",
    },
    DerivedConcept {
        term: "fagan_inspection",
        aliases: &["code inspection", "code review", "formal inspection"],
        identity: "planning -> overview -> preparation -> inspection meeting (defect detection) -> rework -> follow-up, with defects collected, not fixed, during inspection",
        definition: "The formal software inspection of Fagan, Design and code inspections to reduce errors in program development, IBM Systems Journal 15(3) (1976): a structured, role-based examination that separates defect detection from correction and verifies rework in follow-up; complexity reduction per Ousterhout, A Philosophy of Software Design (2018).",
        constituents: &["defect detection", "rework", "follow-up"],
        authority: "operator ruling 2026-09-25; Fagan, Design and code inspections to reduce errors in program development, IBM Systems Journal 15(3) (1976); Ousterhout, A Philosophy of Software Design (2018)",
    },
    DerivedConcept {
        term: "reflective_prompt_evolution",
        aliases: &[
            "gepa",
            "genetic pareto",
            "pareto frontier",
            "non-dominated sort",
        ],
        identity: "evolve text artifacts by reflecting on execution trajectories, mutating and recombining candidates, and keeping the Pareto frontier of non-dominated variants",
        definition: "GEPA (Agrawal et al., GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning, arXiv:2507.19457, 2025): natural-language reflection on trajectories proposes prompt mutations, and selection keeps a Pareto frontier; non-dominated sorting and crowding distance follow NSGA-II (Deb, Pratap, Agarwal & Meyarivan, IEEE Transactions on Evolutionary Computation 6(2), 2002).",
        constituents: &["trajectory", "reflection", "mutation", "pareto frontier"],
        authority: "operator ruling 2026-09-25; Agrawal et al., GEPA, arXiv:2507.19457 (2025); Deb, Pratap, Agarwal & Meyarivan, A fast and elitist multiobjective genetic algorithm: NSGA-II, IEEE TEC 6(2) (2002)",
    },
    DerivedConcept {
        term: "transcript_linked_media",
        aliases: &[
            "reduct",
            "reduct video",
            "transcript-based video editing",
            "transcript reel",
        ],
        identity: "a transcript bundled with its media, word-aligned, so editing or selecting text edits the media and correcting the transcript never moves its timings",
        definition: "The Reduct.video model: a recording and its word-level transcript are one linked artifact. Users correct the transcript, highlight and tag passages, and compose reels by selecting transcript ranges, and the video is cut from those ranges; the same operations are exposed through the Reduct API and user interface.",
        constituents: &[
            "word-aligned transcript",
            "highlight",
            "reel",
            "transcript correction",
        ],
        authority: "operator ruling 2026-09-25; Reduct.video product and API (reduct.video)",
    },
    DerivedConcept {
        term: "entropy",
        aliases: &[
            "shannon entropy",
            "information entropy",
            "conditional entropy",
        ],
        identity: "H = -sum p log p, conditional on a knowledge state",
        definition: "An observer's uncertainty over the distribution consistent with what is known. Jaynes's maximum-entropy reading of Boltzmann: the same formal quantity serves statistical mechanics (multiplicity of microstates) and information theory (an observer's uncertainty), and it is always conditional on a knowledge state. Task entropy is the multiplicity of answers consistent with the specification plus everything the system has compiled.",
        constituents: &["probability distribution", "knowledge state"],
        authority: "operator ruling 2026-09-27; Shannon (1948), A Mathematical Theory of Communication; Jaynes (1957), Information Theory and Statistical Mechanics",
    },
    DerivedConcept {
        term: "deterministic_computation",
        aliases: &["deterministic compute"],
        identity: "execution with multiplicity 1: same input, one output",
        definition: "Computation whose output is a function of its input — the program is an executable proof that the answer is pinned. Enforces zero conditional entropy relative to the spec, which is why deterministic machines serve as trust anchors. Spec errors remain upstream epistemic risk no paradigm removes.",
        constituents: &["program", "input", "specification"],
        authority: "operator ruling 2026-09-27; Turing (1936), On Computable Numbers, with an Application to the Entscheidungsproblem",
    },
    DerivedConcept {
        term: "probabilistic_computation",
        aliases: &["probabilistic compute", "learned computation"],
        identity: "execution with multiplicity > 1: same input, distribution over outputs",
        definition: "Computation whose output is a sample from a learned approximation of a posterior — the model's knowledge limit, not the world's and not the user's. Indexical: varies with the training distribution. Model confidence is not task probability; calibration against resolved outcomes re-ties the machine's distribution to the world's.",
        constituents: &[
            "learned model",
            "training distribution",
            "posterior approximation",
        ],
        authority: "operator ruling 2026-09-27; Gershman & Goodman (2014), Amortized Inference in Learning; Hu, Jain & Elmoznino (2024), Amortizing intractable inference in large language models, arXiv:2310.04363",
    },
    DerivedConcept {
        term: "verification_oracle",
        aliases: &["oracle", "deterministic checker"],
        identity: "a named deterministic procedure that settles a candidate answer at bounded cost",
        definition: "The collapse path for a probabilistic proposal: verifying a candidate is routinely orders of magnitude cheaper than generating it by search (a P/NP-shaped asymmetry). Every D label names its oracle; a D label with no real oracle is checkably wrong. The set of cheap oracles is the verification-cost frontier, and it moves as tooling improves.",
        constituents: &[
            "candidate answer",
            "cost bound",
            "verification-cost frontier",
        ],
        authority: "operator ruling 2026-09-27; Turing (1939), Systems of Logic Based on Ordinals",
    },
    DerivedConcept {
        term: "calibrated_forecast",
        aliases: &["calibrated prediction", "calibrated forecasting"],
        identity: "a probability assignment scored against resolved outcomes",
        definition: "The collapse path when no characterizable posterior exists and no cheap oracle can settle the question: judgment quantified as probability and re-tied to the world by external ground truth — resolved outcomes, Brier scoring. The discipline that keeps probabilistic computation honest at the knowledge frontier.",
        constituents: &["probability assignment", "resolved outcome", "brier score"],
        authority: "operator ruling 2026-09-27; Tetlock & Gardner (2015), Superforecasting; Brier (1950), Verification of forecasts expressed in terms of probability",
    },
    DerivedConcept {
        term: "bayesian_inference",
        aliases: &["bayes rule", "bayesian updating", "posterior update"],
        identity: "posterior odds = prior odds x likelihood ratio",
        definition: "Updating a prior probability into a posterior as evidence arrives, per Bayes's theorem. The explicit-probabilistic collapse path: the posterior is the best characterizable distribution, and each observation re-weights it. The reference discipline for evidence-weighted belief revision.",
        constituents: &["prior probability", "likelihood", "posterior probability"],
        authority: "operator ruling 2026-09-27; Bayes (1763), An Essay towards solving a Problem in the Doctrine of Chances; Laplace (1812), Theorie analytique des probabilites",
    },
    DerivedConcept {
        term: "finite_game",
        aliases: &["carse finite game"],
        identity: "play to end the game: fixed rules, known players, a horizon at which it ends and a winner is declared",
        definition: "Carse's finite game: played for the purpose of winning, with fixed rules, identifiable players, and an agreed horizon at which it ends. Its questions are resolvable — they terminate on an oracle or a resolved outcome, not a maintained position.",
        constituents: &["rules", "players", "horizon", "winning"],
        authority: "operator ruling 2026-09-27; Carse (1986), Finite and Infinite Games",
    },
    DerivedConcept {
        term: "repeated_game",
        aliases: &["iterated game", "repeated interaction"],
        identity: "a stage game played over multiple rounds, where history conditions future play",
        definition: "The repeated (iterated) game of game theory: the same stage game played over rounds, each round's outcome conditioning the next. Reputation, reciprocity and cooperation emerge from repetition — the folk-theorem ground. Its questions are about maintained conditions across rounds, not one-shot resolution.",
        constituents: &["stage game", "history", "strategy", "reputation"],
        authority: "operator ruling 2026-09-27; Fudenberg & Tirole (1991), Game Theory",
    },
    DerivedConcept {
        term: "syntax",
        aliases: &["syntactic", "form", "formal form"],
        identity: "the form side of representation: an alphabet plus formation rules that determine which configurations are well-formed, independent of interpretation",
        definition: "Representation as shape and protocol: the rules over an alphabet that decide which configurations are well-formed, and the shape-preserving transformations between them. Autonomous from meaning in study (Chomsky: syntax can be investigated with the methods of formal logic and mathematics) while serving as the calculus in which formal semantics is stated (Tarski). Syntactic questions are mechanically decidable relative to their grammar - the pole where validation is cheapest.",
        constituents: &["alphabet", "formation rule", "well-formedness"],
        authority: "operator ruling 2026-09-28; Chomsky (1957), Syntactic Structures; Tarski (1944), The Semantic Conception of Truth",
    },
    DerivedConcept {
        term: "semantics",
        aliases: &["meaning", "semantic content"],
        identity: "the meaning side of representation: what expressions denote and the use they have, beyond their form",
        definition: "Representation as meaning: the relation between expressions and what they are about - Tarski's semantic conception (truth and denotation, exact for formalized languages) and Wittgenstein's meaning-as-use (natural-language meaning lives in the practice that uses it). Semantic questions are not settled by form-checking in general; they are grounded against a world, a registry, or resolved outcomes - the pole where verification is costliest and human judgment is load-bearing.",
        constituents: &["denotation", "truth conditions", "use"],
        authority: "operator ruling 2026-09-28; Tarski (1944), The Semantic Conception of Truth; Wittgenstein (1953), Philosophical Investigations",
    },
    DerivedConcept {
        term: "determinism",
        aliases: &["deterministic", "deterministic process"],
        identity: "same cause, same effect: state evolution with multiplicity 1 - the successor state is a function of state and input, with no distribution over outcomes",
        definition: "The property of a process whose course is pinned: given the same state and input, the same successor follows, with no multiplicity of outcomes. The general form of deterministic_computation: a guarantee about the process, not about correctness - a deterministic machine can be deterministically wrong, and spec errors remain upstream epistemic risk that no determinism removes.",
        constituents: &["state", "transition function", "multiplicity 1"],
        authority: "operator ruling 2026-09-28; Turing (1936), On Computable Numbers, with an Application to the Entscheidungsproblem",
    },
    DerivedConcept {
        term: "computation",
        aliases: &["computing", "effective computation"],
        identity: "the effective derivation of outputs from inputs by finite mechanical steps - what a symbol system does when it carries out a procedure",
        definition: "Turing's effective calculability made mechanical: the step-derivation of outputs from inputs by finite operations, instantiated equally by hardware, by formal systems, and by the programmed symbol systems that are the real object of study (Newell and Simon: the machine, not just the hardware). The neutral substrate both deterministic and probabilistic processes run on - the P-axis describes the guarantee, not the substrate: execution with multiplicity 1 is deterministic, execution whose output is a distribution is probabilistic.",
        constituents: &[
            "effective procedure",
            "finite steps",
            "input-output relation",
        ],
        authority: "operator ruling 2026-09-28; Turing (1936), On Computable Numbers; Newell and Simon (1976), Computer Science as Empirical Inquiry: Symbols and Search",
    },
    DerivedConcept {
        term: "template",
        aliases: &["scaffold", "prompt template"],
        identity: "a parameterized form: fixed scaffold content with substitution slots, instantiated deterministically by binding its variables to a context",
        definition: "A deterministic scaffold for probabilistic output: fixed content with slots whose binding is a pure function of a context map - render with multiplicity 1 - consumed by a model whose continuation is probabilistic. The registry instance is the project's Jinja2 template contract: a typed input contract, a deterministic render, and a prompt whose value lies in what the model does with it. Templates pin the form so generation can spend itself on content.",
        constituents: &["fixed scaffold", "substitution slot", "context binding"],
        authority: "operator ruling 2026-09-28; Jinja2 templating semantics (Pallets Projects); registry instance kask/registry/templates/",
    },
    DerivedConcept {
        term: "procedural_knowledge_ontology",
        aliases: &["PKO", "Procedural Knowledge Ontology"],
        identity: "procedures and their executions: a procedure is a plan for how something is done; executing it instantiates the plan's steps and can be assessed against it",
        definition: "The Procedural Knowledge Ontology (PKO): Carriero, Scrocca, Baroni, Azzini and Celino, 'Procedural Knowledge Ontology (PKO)', ESWC 2025 (doi:10.1007/978-3-031-94578-6_19; arXiv:2503.20634; https://w3id.org/pko) — explicit modeling of procedures, their executions and related resources, reusing and extending P-Plan, PROV-O and Dublin Core. Bridged in kask/crates/hkask-bridge-ontology/src/pko.rs, which loads the full v2.0.0 ontology from sources/.",
        constituents: &["procedure", "step", "step execution"],
        authority: "operator ruling 2026-09-28 (reference-set integration directive); Carriero et al., 'Procedural Knowledge Ontology (PKO)', ESWC 2025, doi:10.1007/978-3-031-94578-6_19",
    },
    DerivedConcept {
        term: "golem_narrative_ontology",
        aliases: &[
            "GOLEM",
            "GOLEM ontology",
            "Golem Ontology for Narrative and Fiction",
        ],
        identity: "narrative and fiction as interconnected systems — events, characters, social relationships, settings, narrative inference — modeled independently of any single domain",
        definition: "The GOLEM Ontology for Narrative and Fiction: Pianzola, Pannach, Cheng, Yang and Scotti (GOLEM Lab), ontology v1.1 (https://w3id.org/golem/ontology, doi:10.5281/zenodo.14911396, CC BY 4.0), an extension of CIDOC-CRM and LRMoo; the paper is Pianzola, Cheng, Pannach, Yang and Scotti, 'The GOLEM Ontology for Narrative and Fiction', Humanities 14(10):193 (MDPI, 2025). Bridged in kask/crates/hkask-bridge-ontology/src/golem.rs.",
        constituents: &["narrative event", "character", "setting"],
        authority: "operator ruling 2026-09-28 (reference-set integration directive); Pianzola et al., GOLEM Ontology v1.1, doi:10.5281/zenodo.14911396; Pianzola et al., Humanities 14(10):193 (2025)",
    },
    DerivedConcept {
        term: "media_creation_ontology",
        aliases: &[
            "MovieLabs OMC",
            "OMC",
            "Ontology for Media Creation",
            "MovieLabs Ontology for Media Creation",
        ],
        identity: "media production workflows — creative works, assets, and the capture to post to distribution pipeline — as named entities and relations for interoperable production software",
        definition: "The MovieLabs Ontology for Media Creation (OMC), MovieLabs (https://movielabs.com/ontology-for-media-creation/; official RDF artifact github.com/MovieLabs/OMC, omc.ttl v2.8, namespace https://movielabs.com/omc/rdf/schema/v2.8#): a published multi-part specification of the concepts and relations of media creation workflows. Bridged in kask/crates/hkask-bridge-ontology/src/omc.rs.",
        constituents: &["creative work", "asset", "production workflow"],
        authority: "operator ruling 2026-09-28 (reference-set integration directive); MovieLabs, Ontology for Media Creation, https://movielabs.com/ontology-for-media-creation/",
    },
    DerivedConcept {
        term: "scientific_evidence_ontology",
        aliases: &[
            "SEPIO",
            "Scientific Evidence and Provenance Information Ontology",
        ],
        identity: "scientific claims, the evidence lines that support or dispute them, and the methods, tools and agents that produced the evidence",
        definition: "The Scientific Evidence and Provenance Information Ontology (SEPIO): Brush, Shefchek and Haendel, 'SEPIO: A Semantic Model for the Integration and Analysis of Scientific Evidence', ICBO 2016 (https://ceur-ws.org/Vol-1747/IT605_ICBO2016.pdf), Monarch Initiative, OBO namespace purl.obolibrary.org/obo/SEPIO_ (OWL release 2023-06-13; the project's current linkML model at https://w3id.org/sepio-model is the open migration choice noted in the bridge). Bridged in kask/crates/hkask-bridge-ontology/src/sepio.rs.",
        constituents: &["assertion", "evidence line", "provenance"],
        authority: "operator ruling 2026-09-28 (reference-set integration directive); Brush, Shefchek and Haendel, ICBO 2016; Monarch Initiative SEPIO-ontology (OWL release 2023-06-13)",
    },
    DerivedConcept {
        term: "bayesian_network",
        aliases: &["bayes network", "bayesian net", "belief network"],
        identity: "a DAG of random variables, each with a conditional probability table over its parents; the joint distribution is the product of the conditionals; queries are answered by marginalization over the joint",
        definition: "Pearl's network formalism (Probabilistic Reasoning in Intelligent Systems, 1988): structure as a directed acyclic graph, quantification as conditional probability tables, and inference as marginalization — per-node marginals from the joint without enumerating it wholesale. The nameable violation: a 'marginal' computed by averaging a conditional over one parent while ignoring the others, or a joint assembled from conditionals whose CPT arity is not 2^parents for binary events.",
        constituents: &[
            "bayesian_inference",
            "conditional probability",
            "directed acyclic graph",
        ],
        authority: "operator ruling 2026-10-02; Pearl, Probabilistic Reasoning in Intelligent Systems, Morgan Kaufmann (1988)",
    },
    DerivedConcept {
        term: "discounted_cash_flow",
        aliases: &["dcf", "discounted cashflow"],
        identity: "intrinsic value = present value of projected free cash flows + terminal value, discounted at the cost of capital over a staged horizon",
        definition: "The Williams (1938) origin made operational by Damodaran (Investment Valuation, 2012): value is the sum of discounted expected free cash flows over an explicit stage plus a discounted terminal value; the discount rate is the cost of capital (WACC for the enterprise). The nameable violation: a 'DCF' that omits the terminal value, discounts at a rate unrelated to the capital's cost, or treats a price as an input rather than an output.",
        constituents: &[
            "free cash flow",
            "terminal value",
            "cost of capital",
            "present value",
        ],
        authority: "operator ruling 2026-10-02; Damodaran, Investment Valuation, Wiley (2012); Williams, The Theory of Investment Value, Harvard University Press (1938)",
    },
    DerivedConcept {
        term: "arbitrage",
        aliases: &["arb", "riskless profit"],
        identity: "a price divergence between equivalent claims permitting riskless profit; divergences within the transaction-cost band are not actionable",
        definition: "The law of one price enforced by arbitrage (Hull, Options, Futures and Other Derivatives): equivalent claims must trade at the same price once financing and carrying costs are accounted; a divergence beyond the transaction-cost band — the sum of spreads, fees and slippage on both legs — is the arbitrage signal, and a divergence within the band is coherent pricing. The nameable violation: calling an in-band divergence an arbitrage opportunity, or measuring the gap without the cost band.",
        constituents: &["law of one price", "transaction cost", "price divergence"],
        authority: "operator ruling 2026-10-02; Hull, Options, Futures and Other Derivatives; the law of one price / no-arbitrage principle",
    },
    DerivedConcept {
        term: "preventive_maintenance",
        aliases: &["preventative maintenance", "pm maintenance"],
        identity: "maintenance carried out to mitigate degradation and reduce the probability of failure",
        definition: "IEC 60050-192:2015 IEV 192-06-05, verbatim: maintenance carried out to mitigate degradation and reduce the probability of failure. Its siblings by the same vocabulary: condition-based maintenance (IEV 192-06-07, assessment of physical condition) and scheduled maintenance (IEV 192-06-12, a specified time schedule) — the calendar cadence backed by a measured condition. Note 2 to entry: preventive maintenance can sometimes be performed without disabling the item (on-line maintenance, IEV 192-06-30) — the maintenance pass over a running machine is exactly that.",
        constituents: &["degradation", "probability of failure"],
        authority: "operator ruling 2026-10-10; IEC 60050-192:2015, IEV 192-06-05 (Electropedia, publication 2024-03-08)",
    },
    DerivedConcept {
        term: "condition_based_maintenance",
        aliases: &["cbm", "conditional maintenance"],
        identity: "preventive maintenance based on the assessment of physical condition",
        definition: "IEC 60050-192:2015 IEV 192-06-07, verbatim: preventive maintenance based on the assessment of physical condition. Note 1 to entry: the condition assessment can be by operator observation, conducted according to a schedule, or by condition monitoring (IEV 192-06-28) of system parameters. The trigger is a measured state, not the calendar — a threshold crossed, a deviation observed. The staleness warning that fires when the last successful weekly update exceeds seven days is condition-based maintenance in exactly this sense: the schedule (IEV 192-06-12) is backed by a measured condition.",
        constituents: &["preventive maintenance", "physical condition assessment"],
        authority: "operator ruling 2026-10-10; IEC 60050-192:2015, IEV 192-06-07 (Electropedia, publication 2024-03-08)",
    },
    DerivedConcept {
        term: "cache",
        aliases: &["caching"],
        identity: "a hardware or software component that stores data so that future requests for that data can be served faster",
        definition: "The stored data might be the result of an earlier computation or a copy of data stored elsewhere. A cache hit serves the request from the cache; a miss recomputes or reads the slower backing store — the more requests served from the cache, the faster the system performs. The defining maintenance property follows from the identity: a cache is deletable exactly when re-derivation (recomputation or refetch) is available and cheaper than storage — the recoverability leg of probe-before-delete. Hardlink-shared caches (uv) free fewer blocks than their du size; a cache's apparent size is an upper bound on reclaimable space.",
        constituents: &["stored data", "future requests"],
        authority: "operator ruling 2026-10-10; Wikipedia, Cache (computing) (2026-10-10 revision) — the registry's Wikidata-citation practice (cf. pdca_cycle Q820214)",
    },
    DerivedConcept {
        term: "system_health_monitoring",
        aliases: &["health monitoring", "shm"],
        identity: "continuous or periodic acquisition of data about a system's state, compared against expected conditions, to surface degradation or deviation before failure",
        definition: "An integrated technical framework that continuously or periodically acquires data about the state of a subject — a machine, a structure, a service — for early fault diagnosis (IEEE Technology Navigator, Health Monitoring System; Tsalapati & Koubarakis, SHM ontology, ISWC 2023). The sense leg of a cybernetic feedback loop (Wiener 1948; Ashby 1956; this registry, operator ruling 2026-09-18): measurement compared against a set point, the deviation driving action whose effect enters the next measurement. A health check that measures but never compares (no thresholds, no warnings) is monitoring in name only — a silent leg is a broken loop.",
        constituents: &[
            "state acquisition",
            "expected condition",
            "deviation signal",
        ],
        authority: "operator ruling 2026-10-10; IEEE Technology Navigator (Health Monitoring System); Tsalapati & Koubarakis, SHM: A Light-weight, Mid-level Ontology for Reliable System Health Monitoring (ISWC 2023)",
    },
    DerivedConcept {
        term: "fine_tuning",
        aliases: &["fine tuning", "finetuning", "fine-tune"],
        identity: "continued training of a pre-trained model on task-specific data, adapting its weights — fully or through parameter-efficient adapters such as LoRA — without changing the architecture",
        definition: "The machine-learning sense: a model whose parameters were learned by pre-training on a large corpus is trained further on a smaller, task-specific dataset, so its behaviour adapts to the task while the architecture stays fixed. In zed-kask it names the ML concept as it appears in research content &mdash; the tree performs no fine-tuning (the training pipeline was removed 2026-10-17, out of scope for the co-researcher mandate). The research-area anchor is CSO 3.5's fine-tuning learning topic, with its neural, memory-efficient and parameter-efficient sub-topics; ISO/IEC 22989:2022 — the AI concepts vocabulary — defines training, machine learning and retraining but not fine-tuning, so no ISO definition is cited.",
        constituents: &["machine learning", "model"],
        authority: "operator ruling 2026-10-13; CSO 3.5, cso:fine-tuning_learning (vendored pin)",
    },
    DerivedConcept {
        term: "skill",
        aliases: &["agent skill", "skill artifact"],
        identity: "persistent natural-language behavioral source specifying reusable task behavior, managed as an independently identified software unit, activated by a skills-compatible agent host",
        definition: "The Skillware sense (Fan & Lan, arXiv:2607.18970v3, vendored pin): a Skill Artifact is 'an addressable artifact whose primary behavioral source specifies reusable task behavior for activation and execution by a skills-compatible agent' (3.2), and a Skillware Unit is 'the separately addressable identity that acquires, activates, versions, maintains, and removes the carrier' (3.2). Three necessary conditions operationalize category membership: behavioral primacy, independent software identity, and an Agent Host execution relationship (abstract). In zed-kask a skill is the SKILL.md + templates unit loaded by agent_skills — the system's capability unit.",
        constituents: &["task", "agent"],
        authority: "operator ruling 2026-10-13 (open-loops proposal adopted); Fan & Lan, Skillware: A Software Ontology and Engineering Lifecycle for Persistent Behavioral Artifacts, arXiv:2607.18970v3 (vendored pin, SOURCES.lock line 251)",
    },
    DerivedConcept {
        term: "training",
        aliases: &["model training"],
        identity: "process to determine or improve the parameters of a machine learning model by using training data",
        definition: "The machine-learning sense (ISO/IEC 22989:2022, 3.3.15, cite-only — ISO bytes are copyrighted, never vendored): 'process to determine or to improve the parameters of a machine learning model, based on a machine learning algorithm, by using training data' (the source text carries inline cross-references to 3.1.23, 3.2.10 and 3.3.16, elided here). The nameable violation is applying it to non-ML senses (workforce training). In zed-kask it names the ML concept as it appears in research content — the tree performs no model training (the training pipeline was removed 2026-10-17, out of scope for the co-researcher mandate). No cso:training topic exists in CSO 3.5 (grepped), so the derived ruling is the honest path, never a fabrication over a published anchor.",
        constituents: &["machine learning", "model"],
        authority: "operator ruling 2026-10-13 (open-loops proposal adopted); ISO/IEC 22989:2022, 3.3.15",
    },
    DerivedConcept {
        term: "narrative",
        aliases: &[],
        identity: "a structured account of events, characters, social relationships and settings whose components influence one another, modeled domain-independently of any single work",
        definition: "The GOLEM sense (Pianzola et al., GOLEM Ontology v1.1, vendored pin): 'Narrative phenomena can be viewed as interconnected systems in which various components influence one another', with the key elements of narrative structure — events, characters, social relationships, and settings — interrelating (golem_v1-1.ttl ontology description); GOLEM models narratives and fiction independently of their specific domains, as an extension of CIDOC-CRM and LRMoo. The bare term matches no gc: topic under exact normalization, so the derived ruling is the honest path.",
        constituents: &["character", "work"],
        authority: "operator ruling 2026-10-13 (open-loops proposal adopted); Pianzola et al., GOLEM Ontology for Narrative and Fiction v1.1, doi:10.5281/zenodo.14911396 (vendored pin)",
    },
    DerivedConcept {
        term: "fitts_law",
        aliases: &["fitts's law", "fittss law", "fitts law"],
        identity: "movement time to acquire a target is a logarithmic function of the ratio of distance to target width: MT = a + b log2(2D / W)",
        definition: "Paul M. Fitts, 'The information capacity of the human motor system in controlling the amplitude of movement', Journal of Experimental Psychology 47(6):381-391 (1954): small or distant interactive targets increase acquisition time and error rate. In zed-kask it grounds kask-seam-audit's minimum target-width floor (28-32px mouse floor) in the measured-layout loop.",
        constituents: &["target width", "distance", "movement time"],
        authority: "operator ruling 2026-10-14 (Cohort 1 skill-corpus audit); Fitts, Journal of Experimental Psychology 47(6):381-391 (1954)",
    },
    DerivedConcept {
        term: "requisite_variety",
        aliases: &[
            "law of requisite variety",
            "ashby's law",
            "ashbys law",
            "ashby requisite variety",
        ],
        identity: "only variety can destroy variety: a regulator's variety of responses must be at least as large as the variety of disturbances it must regulate (V_R >= V_D - V_O)",
        definition: "W. Ross Ashby, An Introduction to Cybernetics (1956), Ch. 11 'Requisite Variety': for a regulator to hold outcomes within an acceptable target set against a set of disturbances, its repertoire of distinct responses must span the disturbance classes. In zed-kask it grounds pragmatic-cybernetics and self-improvement's variety check across PDCA iterations.",
        constituents: &["cybernetic_feedback_loop", "disturbance", "regulator"],
        authority: "operator ruling 2026-10-14 (Cohort 1 skill-corpus audit); Ashby, An Introduction to Cybernetics (1956), Ch. 11",
    },
    DerivedConcept {
        term: "spin_glass",
        aliases: &[
            "spin glasses",
            "frustrated landscape",
            "replica symmetry breaking",
        ],
        identity: "a disordered system with competing (frustrated) interactions whose rugged energy landscape has exponentially many metastable valleys separated by energy barriers, breaking ergodicity",
        definition: "Giorgio Parisi, 'Infinite Number of Order Parameters for Spin-Glasses', Physical Review Letters 43(23):1754 (1979), doi:10.1103/physrevlett.43.1754 (Nobel Prize in Physics 2021), over the Sherrington-Kirkpatrick model (1975): competing constraints prevent simultaneous satisfaction (frustration), trapping the system in local metastable valleys that store history and cannot be exited by local hill-climbing without crossing an energy barrier. In zed-kask it grounds gradient-hunter's substrate ontology for non-ergodic code/system deserts.",
        constituents: &["frustration", "metastable state", "energy barrier"],
        authority: "operator ruling 2026-10-14 (Cohort 1 skill-corpus audit); Parisi, Physical Review Letters 43(23):1754 (1979), doi:10.1103/physrevlett.43.1754",
    },
    DerivedConcept {
        term: "allosteric_regulation",
        aliases: &[
            "allostery",
            "allosteric coupling",
            "allosteric population shift",
            "mwc model",
        ],
        identity: "effector binding at a distal site shifts the conformational ensemble population at the active site without direct steric contact",
        definition: "Monod, Wyman & Changeux, 'On the nature of allosteric transitions: a plausible model', Journal of Molecular Biology 12(1):88-118 (1965): regulation at a distance through a conformational equilibrium shift across coupled states. In zed-kask it grounds gradient-hunter's allosteric population shift shape and broken-allosteric-coupling reason class when an upstream context or task fails to shift an orchestrator's downstream strategy ensemble.",
        constituents: &["distal site", "conformational ensemble", "coupling"],
        authority: "operator ruling 2026-10-14 (Cohort 1 skill-corpus audit); Monod, Wyman & Changeux, Journal of Molecular Biology 12(1):88-118 (1965)",
    },
    DerivedConcept {
        term: "regression_discontinuity_design",
        aliases: &["regression discontinuity", "rdd"],
        identity: "causal identification of a treatment effect from a discontinuous jump in outcome at a known assignment threshold along a continuous running variable",
        definition: "Thistlethwaite & Campbell, 'Regression-discontinuity analysis: An alternative to the ex post facto experiment', Journal of Educational Psychology 51(6):309-317 (1960): observations just below and just above a deterministic cutoff are comparable, so a step change at the threshold identifies the local regime effect. In zed-kask it grounds gradient-hunter's regression-discontinuity boundary shape.",
        constituents: &["running variable", "threshold", "treatment effect"],
        authority: "operator ruling 2026-10-14 (Cohort 1 skill-corpus audit); Thistlethwaite & Campbell, Journal of Educational Psychology 51(6):309-317 (1960)",
    },
    DerivedConcept {
        term: "kanban",
        aliases: &[
            "kanban method",
            "kanban board",
            "kanban system",
            "pull system",
        ],
        identity: "a visual pull system that makes work visible across workflow stages and regulates flow by bounding work in progress",
        definition: "Taiichi Ohno, Toyota Production System: Beyond Large-Scale Production (1988; Japanese ed. 1978), and David J. Anderson, Kanban: Successful Evolutionary Change for Your Technology Business (2010): work items move across explicit workflow states only when downstream capacity is available (pull, not push), with explicit column policies and WIP limits governing flow. In zed-kask it grounds hkask-mcp-kata-kanban and the kanban-task-management skill.",
        constituents: &["workflow state", "pull signal", "work_in_progress_limit"],
        authority: "operator ruling 2026-10-14 (Cohort 2 skill-corpus audit); Ohno, Toyota Production System (1988); Anderson, Kanban: Successful Evolutionary Change for Your Technology Business (2010)",
    },
    DerivedConcept {
        term: "work_in_progress_limit",
        aliases: &["wip limit", "wip limits", "work-in-progress limit"],
        identity: "an explicit upper bound on the number of work items concurrently in a workflow state, turning capacity saturation into a visible pull signal",
        definition: "David J. Anderson, Kanban (2010), grounded in Little's Law (John D. C. Little, 'A Proof for the Queuing Formula: L = lambda W', Operations Research 9(3):383-387, 1961): bounding concurrent work in progress (L) bounds average lead time (W = L / lambda) and surfaces bottlenecks immediately instead of buffering queues. In zed-kask it grounds KanbanColumn::wip_limit and kanban-task-management's WIP-limit gate before moving tasks into InProgress or Review.",
        constituents: &["kanban", "concurrency bound", "lead time"],
        authority: "operator ruling 2026-10-14 (Cohort 2 skill-corpus audit); Anderson, Kanban (2010); Little, Operations Research 9(3):383-387 (1961)",
    },
    DerivedConcept {
        term: "invest_criteria",
        aliases: &["invest", "invest backlog item", "invest task decomposition"],
        identity: "six quality criteria for a well-formed work item: Independent, Negotiable, Valuable, Estimable, Small, and Testable",
        definition: "Bill Wake, 'INVEST in Good Stories, and SMART Tasks' (xp123.com, August 17, 2003): a work item or user story is ready for execution when it is Independent (minimal order coupling), Negotiable (captures essence, not over-constrained contract details), Valuable (delivers observable outcome), Estimable (understood well enough to size), Small (completable within one iteration), and Testable (carries falsifiable acceptance criteria). In zed-kask it grounds kanban-task-management's decomposition rubric.",
        constituents: &["independence", "value", "testability"],
        authority: "operator ruling 2026-10-14 (Cohort 2 skill-corpus audit); Wake, 'INVEST in Good Stories, and SMART Tasks' (2003)",
    },
    DerivedConcept {
        term: "fermi_decomposition",
        aliases: &[
            "fermi estimation",
            "fermi problem",
            "fermi breakdown",
            "fermi sub-questions",
        ],
        identity: "decomposing an unknown quantity or probability into a structured combination of smaller, more checkable sub-estimates so errors partially cancel",
        definition: "Enrico Fermi's order-of-magnitude decomposition method, operationalized for probabilistic forecasting by Tetlock & Gardner, Superforecasting: The Art and Science of Prediction (2015), Ch. 5: break a complex target question into tractable constituent sub-questions (knowable vs unknowable drivers), estimate each component explicitly, and combine them mathematically. In zed-kask it grounds superforecasting's Stage 2 (stage_2_fermi.j2), scenario_calibrate's sub_questions, and kanban_goal_create's observable criteria decomposition.",
        constituents: &["sub-question", "component estimate", "superforecasting"],
        authority: "operator ruling 2026-10-14 (Cohort 2 skill-corpus audit); Tetlock & Gardner, Superforecasting (2015); Fermi estimation (Wikidata Q1141521)",
    },
    DerivedConcept {
        term: "reference_class_forecasting",
        aliases: &["reference class", "outside view", "base rate anchoring"],
        identity: "anchoring a prediction on the empirical distribution (base rate) of outcomes in a relevant reference class of past cases before adjusting for case-specific inside-view particulars",
        definition: "Kahneman & Tversky, 'Intuitive prediction: Biases and corrective procedures', TIMS Studies in Management Science 12:313-327 (1979), and Flyvbjerg, 'From Nobel Prize to Project Management: Getting Risks Right', Project Management Journal 37(3):5-15 (2006); operationalized in Tetlock & Gardner, Superforecasting (2015), Ch. 3: select an empirical reference class of comparable historical instances, establish its statistical base rate first (the outside view), and only then adjust toward case-specific causal evidence (the inside view). In zed-kask it grounds superforecasting's Stage 3 (stage_3_outside_view.j2) and scenario_calibrate's reference_class / base_rate fields.",
        constituents: &["base rate", "outside view", "inside view"],
        authority: "operator ruling 2026-10-14 (Cohort 2 skill-corpus audit); Kahneman & Tversky (1979); Flyvbjerg, Project Management Journal 37(3):5-15 (2006); Tetlock & Gardner, Superforecasting (2015)",
    },
    DerivedConcept {
        term: "critical_uncertainties",
        aliases: &["critical uncertainty", "scenario axes"],
        identity: "the two independent driving forces rated highest in both importance for the focal decision and uncertainty of outcome, forming the orthogonal axes of a 2x2 scenario matrix",
        definition: "Peter Schwartz, The Art of the Long View: Planning for the Future in an Uncertain World (1991; Wilk review doi:10.1016/0024-6301(91)90051-o): after mapping STEEP driving forces on importance and uncertainty, select the two most impactful and uncertain forces that are causally independent of each other to span four divergent quadrant futures. In zed-kask it grounds scenario-planning's Phase 2b (driving-forces.j2 and axes-and-narratives.j2).",
        constituents: &["scenario_planning", "driving force", "uncertainty"],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Schwartz, The Art of the Long View (1991), doi:10.1016/0024-6301(91)90051-o",
    },
    DerivedConcept {
        term: "early_warning_indicators",
        aliases: &[
            "early indicators",
            "leading indicators",
            "scenario tripwires",
        ],
        identity: "observable, measurable signals tracked after scenario construction that reveal which scenario trajectory is beginning to unfold and trigger contingent strategies",
        definition: "Peter Schwartz, The Art of the Long View (1991), Step 8 'Selection of Leading Indicators and Signposts', and Thomas J. Chermack, Scenario Planning in Organizations (2011; doi:10.1177/1534484304270637): concrete, observable tripwires tied to specific scenario narratives that monitor which future is materializing in time to activate contingent hedges. In zed-kask it grounds scenario-planning's implications-indicators.j2 and scenario_assess's has_early_warning_indicators metric.",
        constituents: &[
            "scenario_planning",
            "observable signal",
            "contingent strategy",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Schwartz, The Art of the Long View (1991); Chermack, Scenario Planning in Organizations (2011), doi:10.1177/1534484304270637",
    },
    DerivedConcept {
        term: "deletion_test",
        aliases: &["ousterhout deletion test", "module deletion test"],
        identity: "mentally deleting a module or abstraction to test whether its complexity vanishes — proving it was a shallow pass-through — or reappears across callers — proving it hides real complexity and earns its keep",
        definition: "John Ousterhout, A Philosophy of Software Design (2018), Ch. 4 'Modules Should Be Deep' and Ch. 7.1 'Pass-through methods': evaluate an abstraction by asking what happens if it is removed; if callers absorb no meaningful logic because the module merely forwarded parameters, it is shallow overhead to eliminate. In zed-kask it grounds deep-module, refactor-architecture, essentialist G1, and upstream-rebase's decide.j2.",
        constituents: &["deep_module", "interface", "implementation complexity"],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Ousterhout, A Philosophy of Software Design (2018), Ch. 4 & Ch. 7",
    },
    DerivedConcept {
        term: "architecture_decision_record",
        aliases: &[
            "adr",
            "adrs",
            "architectural decision record",
            "architectural decision records",
        ],
        identity: "a short, versioned document capturing one architecturally significant decision, the context and forces behind it, the decision itself, and its consequences",
        definition: "Michael Nygard, 'Documenting Architecture Decisions' (Cognitect Blog, November 15, 2011): record each architecturally significant decision in source control with Title, Status, Context, Decision, and Consequences so future maintainers understand the trade-offs rather than re-litigating or accidentally reversing them. In zed-kask it grounds refactor-architecture's known_adrs and adr_proposals.",
        constituents: &["context", "decision", "consequences"],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Nygard, 'Documenting Architecture Decisions', Cognitect (2011)",
    },
    DerivedConcept {
        term: "entity_relationship_diagram",
        aliases: &[
            "erd",
            "entity-relationship model",
            "er diagram",
            "entity relationship model",
        ],
        identity: "a structural data model and diagram notation representing entities, their typed attributes, and cardinality-constrained relationships",
        definition: "Peter Pin-Shan Chen, 'The entity-relationship model—toward a unified view of data', ACM Transactions on Database Systems 1(1):9-36 (1976), doi:10.1145/320434.320440: models a domain's schema as entity sets, attribute types/keys, and relationships with explicit cardinality ratios (1:1, 1:N, M:N). In zed-kask it grounds diataxis-diagram's erd extraction and Crow's Foot Mermaid erDiagram rendering.",
        constituents: &["entity", "attribute", "relationship", "cardinality"],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Chen, ACM Transactions on Database Systems 1(1):9-36 (1976), doi:10.1145/320434.320440",
    },
    DerivedConcept {
        term: "state_diagram",
        aliases: &[
            "statechart",
            "statecharts",
            "state machine diagram",
            "harel statechart",
        ],
        identity: "a directed graph of discrete states, guarded transitions, and hierarchical composite or orthogonal states modeling reactive system behavior",
        definition: "David Harel, 'Statecharts: a visual formalism for complex systems', Science of Computer Programming 8(3):231-274 (1987), doi:10.1016/0167-6423(87)90035-9: extends finite-state transition diagrams with hierarchical depth (composite states), concurrency (fork/join orthogonal regions), and guarded event-triggered transitions. In zed-kask it grounds diataxis-diagram's stateDiagram-v2 extraction and rendering.",
        constituents: &["state", "transition", "guard condition", "composite state"],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Harel, Science of Computer Programming 8(3):231-274 (1987), doi:10.1016/0167-6423(87)90035-9",
    },
    DerivedConcept {
        term: "topological_sort",
        aliases: &[
            "topological ordering",
            "topological order",
            "kahn's algorithm",
        ],
        identity: "a linear ordering of the vertices of a directed acyclic graph such that for every directed edge u -> v, vertex u precedes v in the ordering",
        definition: "Arthur B. Kahn, 'Topological sorting of large networks', Communications of the ACM 5(11):558-562 (1962), doi:10.1145/368996.369025: schedules a network of precedence-constrained tasks into a valid linear execution sequence; a complete ordering of all |V| vertices exists if and only if the directed graph contains no cycles. In zed-kask it grounds upstream-rebase's functional-unit re-application order (map.j2 and execute.j2) and scenario_quantify's event-tree evaluation order.",
        constituents: &[
            "directed_acyclic_graph",
            "precedence constraint",
            "linear ordering",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 3 skill-corpus audit); Kahn, Communications of the ACM 5(11):558-562 (1962), doi:10.1145/368996.369025",
    },
    DerivedConcept {
        term: "adjacent_possible",
        aliases: &["the adjacent possible", "kauffman adjacent possible"],
        identity: "the set of novel states, products, or economic niches reachable in a single combinatorial step from the current configuration of a system",
        definition: "Stuart A. Kauffman, Investigations (Oxford University Press, 2000): biological and economic systems expand non-ergodically into the 'adjacent possible' — the frontier of configurations one reaction or combination away from what currently exists; each realized innovation reconfigures the frontier and makes new niches possible. In zed-kask it is a research-content concept; the company-research Lens 5 and economic-trajectory consumers were removed 2026-10-18 with the companies research skills.",
        constituents: &[
            "combinatorial innovation",
            "non-ergodicity",
            "economic niche",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Kauffman, Investigations, Oxford University Press (2000)",
    },
    DerivedConcept {
        term: "earnings_quality",
        aliases: &[
            "accruals quality",
            "accruals anomaly",
            "forensic accounting",
        ],
        identity: "the degree to which reported accounting earnings reflect sustainable cash-backed economic performance rather than non-repeating accruals or aggressive recognition",
        definition: "Richard G. Sloan, 'Do Stock Prices Fully Reflect Information in Accruals and Cash Flows about Future Earnings?', The Accounting Review 71(3):289-315 (1996); Discussion in Review of Accounting Studies (1999), doi:10.1023/a:1009682117424; Dechow, Ge & Schrand, Journal of Accounting and Economics 50(2-3):344-401 (2010): earnings backed by operating cash flows persist at a higher rate than earnings driven by working-capital accruals. In zed-kask it is a research-content concept; the company-research-flash forensic stages it grounded were removed 2026-10-18 with the companies research skills.",
        constituents: &["operating cash flow", "accruals", "revenue recognition"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Sloan, The Accounting Review 71(3):289-315 (1996); doi:10.1023/a:1009682117424",
    },
    DerivedConcept {
        term: "conservation_of_energy",
        aliases: &[
            "first law of thermodynamics",
            "mass-energy conservation",
            "flow conservation",
        ],
        identity: "in a closed or steady-state node without storage, total inflow equals total outflow",
        definition: "The first law of thermodynamics and continuity principle applied to flow networks by Mario Schmidt, 'The Sankey Diagram in Energy and Material Flow Management', Journal of Industrial Ecology 12(1):82-94 and 12(2):173-185 (2008), doi:10.1111/j.1530-9290.2008.00004.x: for conserved quantities (energy, mass, and balanced accounting allocations), the sum of inputs into any internal node equals the sum of outputs leaving it. In zed-kask it grounds sankey-flow's mandatory conservation mode and per-node lisp_eval balance check.",
        constituents: &["sankey_diagram", "inflow", "outflow"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Schmidt, Journal of Industrial Ecology 12(1):82-94 (2008), doi:10.1111/j.1530-9290.2008.00004.x",
    },
    DerivedConcept {
        term: "open_information_extraction",
        aliases: &["openie", "open ie"],
        identity: "domain-independent extraction of relational tuples (arg1, relation, arg2) from text where the relation phrase is surface text from the sentence rather than a pre-specified ontology IRI",
        definition: "Banko, Cafarella, Soderland, Broadhead & Etzioni, 'Open Information Extraction from the Web', IJCAI (2007), and Soderland et al., 'Adapting Open Information Extraction to Domain-Specific Relations', AI Magazine 31(3) (2010), doi:10.1609/aimag.v31i3.2305: extracts binary relational tuples whose predicates are short surface verb phrases from the source text, distinct from closed-vocabulary RDF triples. In zed-kask it grounds structured-extraction's extract-relations.j2.",
        constituents: &["subject", "surface predicate", "object", "trigger span"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Banko et al., IJCAI (2007); Soderland et al., AI Magazine 31(3) (2010), doi:10.1609/aimag.v31i3.2305",
    },
    DerivedConcept {
        term: "coreference_resolution",
        aliases: &["coreference", "anaphora resolution"],
        identity: "partitioning referring expressions — pronouns, definite noun phrases, proper names — into equivalence clusters that denote the same discourse entity",
        definition: "Sameer Pradhan et al., 'CoNLL-2012 Shared Task: Modeling Multilingual Unrestricted Coreference in OntoNotes', EMNLP-CoNLL (2012), OpenAlex W2155069789: resolving anaphoric mentions (such as 'she', 'it', 'the company') to their canonical antecedent entity before extracting relational tuples. In zed-kask it grounds structured-extraction's coreference step in extract-relations.j2.",
        constituents: &["referring expression", "antecedent", "entity cluster"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Pradhan et al., CoNLL-2012 Shared Task (2012), OpenAlex W2155069789",
    },
    DerivedConcept {
        term: "value_chain",
        aliases: &["wardley value chain", "porter value chain"],
        identity: "the dependency-ordered chain of activities and components required to fulfill an anchored user need, ordered by visibility from user-facing capabilities at the top to foundational infrastructure at the bottom",
        definition: "Michael E. Porter, Competitive Advantage: Creating and Sustaining Superior Performance (Free Press, 1985), and Simon Wardley, Wardley Maps: Topographical intelligence in business (2016): the vertical axis of a Wardley map, anchoring at the top on the user and user need and tracing the directed graph of dependent components downward by decreasing visibility to the user. In zed-kask it grounds wardley-mapper's map construction.",
        constituents: &["wardley_map", "user need", "component dependency"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Porter, Competitive Advantage (1985); Wardley, Wardley Maps (2016)",
    },
    DerivedConcept {
        term: "commoditization",
        aliases: &["commoditize", "evolutionary drift"],
        identity: "the climatic rightward evolution of components from Genesis and Custom-Built through Product/Rental to standardized, ubiquitous Commodity/Utility",
        definition: "Simon Wardley, Wardley Maps: Topographical intelligence in business (2016), Ch. 3 'Evolution': competition and supply-and-demand forces drive every component rightward along the evolution axis from novel Genesis through bespoke Custom and off-the-shelf Product to standardized Commodity/Utility, turning former differentiators into cost-of-doing-business utilities and enabling higher-order component creation. In zed-kask it grounds wardley-mapper's should_commoditize flag and movement synthesis.",
        constituents: &["wardley_map", "evolution axis", "utility"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Wardley, Wardley Maps (2016), Ch. 3",
    },
    DerivedConcept {
        term: "style_transfer",
        aliases: &["text style transfer", "arbitrary style transfer"],
        identity: "rewriting a source text to match the syntactic, lexical, and rhetorical signature of a target exemplar voice while preserving its propositional and technical content",
        definition: "Emily Reif et al., 'A Recipe for Arbitrary Text Style Transfer with Large Language Models', ACL (2022), doi:10.18653/v1/2022.acl-short.94: retrieval-augmented in-context exemplar prompting that transforms surface voice, clause structure, and cadence while holding underlying factual and technical tokens invariant. In zed-kask it grounds writing-style and corpus_compose / corpus_rewrite.",
        constituents: &[
            "exemplar retrieval",
            "voice signature",
            "content preservation",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Reif et al., ACL (2022), doi:10.18653/v1/2022.acl-short.94",
    },
    DerivedConcept {
        term: "stylometry",
        aliases: &["computational stylometry", "burrows delta"],
        identity: "quantitative measurement of linguistic and distributional features of texts to characterize authorial or stylistic distance",
        definition: "John F. Burrows, ''Delta': a Measure of Stylistic Difference and a Guide to Likely Authorship', Literary and Linguistic Computing 17(3):267-287 (2002), doi:10.1093/llc/17.3.267, and Efstathios Stamatatos, 'A survey of modern authorship attribution methods', JASIST 60(3):538-556 (2009), doi:10.1002/asi.21001: measuring stylistic proximity between a candidate text and a reference author corpus. In zed-kask it grounds writing-style's centroid-distance validation disclosure.",
        constituents: &["author corpus", "stylistic distance", "cosine_distance"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Burrows, LLC 17(3):267-287 (2002), doi:10.1093/llc/17.3.267; Stamatatos, JASIST 60(3):538-556 (2009), doi:10.1002/asi.21001",
    },
    DerivedConcept {
        term: "cosine_distance",
        aliases: &["cosine similarity", "angular distance"],
        identity: "1 - (u . v) / (||u|| ||v||), measuring directional divergence between two non-zero vectors in an inner-product embedding space independent of vector magnitude",
        definition: "Gerard Salton, A. Wong & C. S. Yang, 'A Vector Space Model for Automatic Indexing', Communications of the ACM 18(11):613-620 (1975): the complement of the cosine of the angle between two embedding vectors, ranging from 0.0 (collinear) to 2.0 (opposite). In zed-kask it grounds hkask-mcp-corpus's centroid_distance_max validation gate in corpus_compose and corpus_rewrite and corpus_dedup_chunks / corpus_consolidate_chunks.",
        constituents: &["inner product", "embedding vector", "centroid"],
        authority: "operator ruling 2026-10-14 (Cohort 4 skill-corpus audit); Salton, Wong & Yang, Communications of the ACM 18(11):613-620 (1975)",
    },
    DerivedConcept {
        term: "cash_conversion_cycle",
        aliases: &["ccc", "working capital cycle"],
        identity: "DSO + DIO - DPO, measuring the time in days between cash outlay for working-capital inputs and cash collection from sales",
        definition: "Verlyn D. Richards & Eugene J. Laughlin, 'A Cash Conversion Cycle Approach to Liquidity Analysis', Financial Management 9(1):32-38 (1980): combines days sales outstanding (receivables), days inventory outstanding (inventory), and days payables outstanding (payables) into the net duration capital is tied up in operating working capital. In zed-kask it is a research-content concept.",
        constituents: &[
            "days sales outstanding",
            "days inventory outstanding",
            "days payables outstanding",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Richards & Laughlin, Financial Management 9(1):32-38 (1980)",
    },
    DerivedConcept {
        term: "audit_evidence",
        aliases: &[
            "isa 500",
            "vouching and tracing",
            "vouching",
            "tracing",
            "evidence reliability hierarchy",
        ],
        identity: "information used in arriving at audit conclusions, evaluated by sufficiency and appropriateness across the reliability hierarchy and directional testing (vouching claims to sources and tracing sources to the report)",
        definition: "IAASB, International Standard on Auditing (ISA) 500: Audit Evidence (2009): assertions are tested in two directions — vouching (from report claim to supporting source record for existence and accuracy) and tracing (from source records to the report for completeness) — graded by source reliability (audited statements > interim filings > transcripts > press releases > web) with inquiry alone never constituting corroboration. In zed-kask it grounds grounding-verify.",
        constituents: &["assertion", "vouching", "tracing", "provenance"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); IAASB, International Standard on Auditing ISA 500: Audit Evidence (2009)",
    },
    DerivedConcept {
        term: "materiality",
        aliases: &["isa 320", "audit materiality"],
        identity: "misstatements or omissions are material if they could reasonably be expected to influence the decisions of users taken on the basis of the report",
        definition: "IAASB, International Standard on Auditing (ISA) 320: Materiality in Planning and Performing an Audit (2009): verification effort is scoped to load-bearing claims whose falsity or omission would alter the report's conclusions or the user's decision, rather than expending budget on incidental phrasing. In zed-kask it grounds grounding-verify.",
        constituents: &[
            "audit_evidence",
            "load-bearing assertion",
            "decision relevance",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); IAASB, International Standard on Auditing ISA 320 (2009)",
    },
    DerivedConcept {
        term: "vertical_slice",
        aliases: &["vertical slicing", "tracer bullet"],
        identity: "a work increment that cuts through all architectural layers required to deliver one observable, end-to-end testable behavior",
        definition: "Andrew Hunt & David Thomas, The Pragmatic Programmer (1999), and Bill Wake, 'INVEST in Good Stories, and SMART Tasks' (2003): decomposing work into narrow end-to-end feature paths across types, ports, services, and surfaces rather than horizontal single-layer batches, so each slice leaves the system working and verifiable. In zed-kask it grounds task-breakdown and tdd.",
        constituents: &[
            "invest_criteria",
            "end-to-end behavior",
            "verification seam",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Hunt & Thomas, The Pragmatic Programmer (1999); Wake (2003)",
    },
    DerivedConcept {
        term: "compensation_masking",
        aliases: &["compensatory aggregation", "non-compensatory threshold"],
        identity: "the failure mode of linear weighted-sum aggregation in which high scores on secondary criteria mask a severe deficiency on a critical criterion, mitigated by per-criterion floor or veto thresholds",
        definition: "Ralph L. Keeney & Howard Raiffa, Decisions with Multiple Objectives (1976), and Valerie Belton & Theodor J. Stewart, Multiple Criteria Decision Analysis: An Integrated Approach (2002): in additive multi-criteria scoring, trade-offs between weights allow a catastrophic failure on one dimension to hide behind a passing weighted average unless a non-compensatory per-criterion veto threshold is enforced. In zed-kask it grounds mcda and task-breakdown's quality-gate veto (max score <= 0.30).",
        constituents: &[
            "multi_criteria_decision_analysis",
            "weighted sum",
            "veto threshold",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Keeney & Raiffa (1976); Belton & Stewart, Multiple Criteria Decision Analysis (2002)",
    },
    DerivedConcept {
        term: "good_regulator_theorem",
        aliases: &["good regulator", "conant-ashby theorem"],
        identity: "every good regulator of a system must be a model of that system",
        definition: "Roger C. Conant & W. Ross Ashby, 'Every good regulator of a system must be a model of that system', International Journal of Systems Science 1(2):89-97 (1970), doi:10.1080/00207727008920220: any regulator that achieves optimal regulation of a system's outcomes must contain a homomorphic representation of the system being regulated. In zed-kask it grounds pragmatic-cybernetics, task-breakdown's context_summary, code-review's change_model, and bug-hunt's crate_model.",
        constituents: &[
            "cybernetic_feedback_loop",
            "system model",
            "requisite_variety",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Conant & Ashby, International Journal of Systems Science 1(2):89-97 (1970), doi:10.1080/00207727008920220",
    },
    DerivedConcept {
        term: "counterfactual",
        aliases: &["structural counterfactual", "actual causation"],
        identity: "evaluating what outcome would obtain in a structural causal model if a hypothesized cause were surgically intervened upon while holding background exogenous variables and confounders fixed",
        definition: "Judea Pearl, Causality (Cambridge University Press, 2009), and Joseph Y. Halpern & Judea Pearl, 'Causes and Explanations: A Structural-Model Approach. Part I: Causes', British Journal for the Philosophy of Science 56(4):843-887 (2005), doi:10.1093/bjps/axi147: a minimal-world query testing whether effect Y would still occur under do(not X) with all confounders held constant. In zed-kask it grounds falsifiability's falsifiability-counterfactual.j2 and superforecasting's Stage 3 inside view.",
        constituents: &["falsifiability", "do_calculus", "confounder"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Pearl, Causality (2009); Halpern & Pearl, BJPS 56(4):843-887 (2005), doi:10.1093/bjps/axi147",
    },
    DerivedConcept {
        term: "do_calculus",
        aliases: &["do-operator", "causal intervention"],
        identity: "the formal interventional operator do(X = x) that replaces the causal mechanism determining X with a constant assignment while leaving all other mechanisms intact",
        definition: "Judea Pearl, 'Causal diagrams for empirical research', Biometrika 82(4):669-688 (1995), and Causality (2009): distinguishes causal intervention P(Y | do(X = x)) — surgical graph mutilation removing incoming edges into X — from observational conditioning P(Y | X = x). In zed-kask it grounds falsifiability's counterfactual construction.",
        constituents: &["counterfactual", "directed_acyclic_graph", "intervention"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Pearl, Biometrika 82(4):669-688 (1995); Pearl, Causality (2009)",
    },
    DerivedConcept {
        term: "cognitive_load",
        aliases: &["intrinsic cognitive load", "extraneous cognitive load"],
        identity: "the amount of working-memory information a developer or reader must hold simultaneously to understand or safely modify a module or interface",
        definition: "John Sweller, 'Cognitive load during problem solving: Effects on learning', Cognitive Science 12(2):257-285 (1988), and John Ousterhout, A Philosophy of Software Design (2018), Ch. 2: system complexity manifests as the mental burden placed on readers by shallow abstractions, scattered state, and obscure invariants. In zed-kask it grounds code-review's Form axis, deep-module, and adhd-mode.",
        constituents: &["deep_module", "working memory", "interface simplicity"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Sweller, Cognitive Science 12(2):257-285 (1988); Ousterhout, A Philosophy of Software Design (2018)",
    },
    DerivedConcept {
        term: "change_amplification",
        aliases: &["shotgun surgery"],
        identity: "the symptom of software complexity in which a seemingly simple logical change requires modifications across many separate files, methods, or call sites",
        definition: "John Ousterhout, A Philosophy of Software Design (2018), Ch. 2.1 'Symptoms of Complexity': when knowledge or policy is duplicated across shallow modules instead of encapsulated behind a deep interface, every change fans out across multiple locations. In zed-kask it grounds code-review, deep-module, and refactor-architecture.",
        constituents: &["deep_module", "locality", "coupling"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Ousterhout, A Philosophy of Software Design (2018), Ch. 2.1",
    },
    DerivedConcept {
        term: "exploratory_testing",
        aliases: &[
            "session-based test management",
            "testing charter",
            "htsm",
            "heuristic test strategy model",
        ],
        identity: "simultaneous learning, test design, and test execution guided by time-boxed charters and heuristic test strategy models rather than pre-scripted test procedures",
        definition: "Cem Kaner, Testing Computer Software (1988), James Bach, 'Exploratory Testing Explained' (2003) and Heuristic Test Strategy Model (2006), and Elisabeth Hendrickson, Explore It!: Reduce Risk and Increase Confidence with Exploratory Testing (Pragmatic Bookshelf, 2013): structured, charter-driven investigation of software quality threats ('Explore [target] with [resources] to discover [information]'). In zed-kask it grounds bug-hunt.",
        constituents: &["testing charter", "heuristic", "test_oracle"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Kaner (1988); Bach & Bolton, HTSM (2006); Hendrickson, Explore It! (2013)",
    },
    DerivedConcept {
        term: "test_oracle",
        aliases: &["oracle problem", "weinberg oracle"],
        identity: "a mechanism — specified contract, metamodel, consistency invariant, or human quality criterion — for determining whether a system's observed behavior on a test or probe is acceptable",
        definition: "Elaine J. Weyuker, 'On Testing Non-Testable Programs', The Computer Journal 25(4):465-470 (1982), Jerry Weinberg, Quality Software Management (1992), and Earl T. Barr et al., 'The Oracle Problem in Software Testing: A Survey', IEEE Transactions on Software Engineering 41(5):507-525 (2015), doi:10.1109/tse.2014.2372785: distinguishes specified contract violations from heuristic pattern matches and unverified observations. In zed-kask it grounds bug-hunt's bug-hunt-oracle.j2 and tdd.",
        constituents: &["verification_oracle", "contract", "quality criterion"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Weyuker (1982); Weinberg (1992); Barr et al., IEEE TSE 41(5):507-525 (2015), doi:10.1109/tse.2014.2372785",
    },
    DerivedConcept {
        term: "mutation_testing",
        aliases: &["mutation analysis", "surviving mutant"],
        identity: "evaluating test-suite fault-detection adequacy by injecting small syntactic faults (mutants) into program source and measuring whether the test suite fails (kills the mutant) or passes (leaving a surviving mutant)",
        definition: "Richard A. DeMillo, Richard J. Lipton & Frederick G. Sayward, 'Hints on Test Data Selection: Help for the Practicing Programmer', IEEE Computer 11(4):34-41 (1978), and Yue Jia & Mark Harman, 'An Analysis and Survey of the Development of Mutation Testing', IEEE Transactions on Software Engineering 37(5):649-678 (2011), doi:10.1109/tse.2010.62: surviving mutants pinpoint concrete code paths where the test suite is blind. In zed-kask it grounds bug-hunt's mutation-guided chartering and verification-compression's fault-detection preservation.",
        constituents: &["mutant", "test_oracle", "fault detection"],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); DeMillo, Lipton & Sayward (1978); Jia & Harman, IEEE TSE 37(5):649-678 (2011), doi:10.1109/tse.2010.62",
    },
    DerivedConcept {
        term: "bug_taxonomy",
        aliases: &["beizer bug taxonomy", "beizer taxonomy", "defect taxonomy"],
        identity: "a structured classification of software defects across requirements, structural/control-flow, data, coding, interface, integration, timing/concurrency, and configuration categories",
        definition: "Boris Beizer, Software Testing Techniques, 2nd ed. (Van Nostrand Reinhold, 1990): categorizes software bugs by their structural and lifecycle origin so test strategy and exploratory probes target the failure modes characteristic of the system's architecture. In zed-kask it grounds bug-hunt's charter focus and finding classification.",
        constituents: &[
            "exploratory_testing",
            "defect category",
            "pattern signature",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 5 skill-corpus audit); Beizer, Software Testing Techniques, 2nd ed. (1990)",
    },
    DerivedConcept {
        term: "curry_howard_correspondence",
        aliases: &[
            "curry-howard isomorphism",
            "curry howard",
            "propositions as types",
            "formulae-as-types",
        ],
        identity: "the structural isomorphism between formal logic and computational type systems in which propositions correspond to types, proofs correspond to programs, and proof normalization corresponds to evaluation",
        definition: "William A. Howard, 'The Formulae-as-Types Notion of Construction', in To H.B. Curry: Essays on Combinatory Logic, Lambda Calculus and Formalism (Academic Press, 1980), pp. 479-490, and Philip Wadler, 'Propositions as Types', Communications of the ACM 58(12):75-84 (2015): a proposition is inhabited as a type if and only if it has a constructive proof term. In zed-kask it grounds lean-prover's Prop/Type discipline and verification-compression's proof obligations.",
        constituents: &["proposition", "type system", "proof term"],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Howard, 'The Formulae-as-Types Notion of Construction' (1980)",
    },
    DerivedConcept {
        term: "test_suite_reduction",
        aliases: &["test suite minimization", "regression test selection"],
        identity: "selecting a minimal representative subset of verification steps or test cases that preserves the full coverage of specified requirements, falsifiers, and fault-detection obligations of the original suite",
        definition: "Mary Jean Harrold, Rajiv Gupta & Mary Lou Soffa, 'A Methodology for Controlling the Size of a Test Suite', ACM Transactions on Software Engineering and Methodology 2(3):270-285 (1993), doi:10.1145/152388.152391, and Milos Gligoric et al., 'Practical Regression Test Selection with Dynamic File Dependencies', ISSTA (2015), doi:10.1145/2568225.2568271: reduces verification runtime by eliminating redundant steps only when requirement coverage and fault-detection capability are provably preserved. In zed-kask it grounds verification-compression.",
        constituents: &[
            "regression_testing",
            "requirement coverage",
            "fault detection",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Harrold, Gupta & Soffa, ACM TOSEM 2(3):270-285 (1993), doi:10.1145/152388.152391; Gligoric et al. (2015), doi:10.1145/2568225.2568271",
    },
    DerivedConcept {
        term: "portability",
        aliases: &["vendor portability", "switching cost", "lock-in resistance"],
        identity: "the degree of effectiveness and efficiency with which a system, model weight, or component can be transferred from one hardware, software, or provider environment to another without vendor lock-in",
        definition: "ISO/IEC 25010:2023 (Systems and software engineering — Systems and software Quality Requirements and Evaluation (SQuaRE) — Product quality model, cite-only): product quality characteristic encompassing adaptability, installability, and replaceability across environments. In zed-kask it grounds Principle P1 (Open-Source Model Portability) and provider-audit's inbound and retirement evaluations.",
        constituents: &["adaptability", "replaceability", "open weights"],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); ISO/IEC 25010:2023 (cite-only)",
    },
    DerivedConcept {
        term: "socratic_method",
        aliases: &[
            "socratic questioning",
            "socratic interrogation",
            "dialectical questioning",
        ],
        identity: "a disciplined questioning procedure that systematically probes recall, mechanism, rationale, edge cases, and synthesis to expose hidden assumptions and contradictions",
        definition: "Richard Paul & Linda Elder, The Art of Socratic Questioning (Foundation for Critical Thinking, 2006): structured inquiry that tests understanding rather than surface fluency by escalating from factual recall through causal mechanism and design rationale to adversarial boundary cases and cross-concept synthesis. In zed-kask it grounds grill-me and prompt-enhance's high-effort critique escalation loop.",
        constituents: &["assumption probing", "counterexample", "synthesis"],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Paul & Elder, The Art of Socratic Questioning (2006)",
    },
    DerivedConcept {
        term: "premortem",
        aliases: &["project premortem", "prospective hindsight"],
        identity: "a prospective hindsight risk-analysis procedure that assumes before execution that a design or project has failed and generates concrete causal explanations for that failure",
        definition: "Gary Klein, 'Performing a Project Premortem', Harvard Business Review 85(9):18-19 (2007), grounded in Deborah J. Mitchell, J. Edward Russo & Nancy Pennington, 'Back to the Future: Temporal Perspective in the Explanation of Events', Journal of Behavioral Decision Making 2(1):25-38 (1989): imagining that a failure has already occurred increases the identification of specific failure modes by ~30% compared to standard risk brainstorming. In zed-kask it grounds program-manager's design-gate.j2.",
        constituents: &["prospective hindsight", "failure mode", "mitigation"],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Klein, Harvard Business Review 85(9):18-19 (2007); Mitchell, Russo & Pennington (1989)",
    },
    DerivedConcept {
        term: "is_ought_problem",
        aliases: &[
            "is-ought gap",
            "hume's guillotine",
            "humes guillotine",
            "is ought distinction",
        ],
        identity: "the epistemic and logical boundary separating descriptive statements of what is the case (IS) from normative or prescriptive statements of what ought to be the case (OUGHT)",
        definition: "David Hume, A Treatise of Human Nature (1739), Book III, Part I, Section I: a normative prescription ('ought') cannot be deductively derived from purely descriptive factual premises ('is') without an explicit bridging goal, value, or specification contract. In zed-kask it grounds pragmatic-semantics and code-review's IS/OUGHT adjudication.",
        constituents: &[
            "descriptive assertion",
            "normative prescription",
            "specification",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Hume, A Treatise of Human Nature (1739), Book III, Part I, Section I",
    },
    DerivedConcept {
        term: "optimality_theory",
        aliases: &[
            "ot ranking",
            "strict dominance hierarchy",
            "violable constraints",
        ],
        identity: "a constraint-satisfaction framework in which candidates are evaluated against a strict lexicographic dominance hierarchy of violable constraints (C1 >> C2 >> ... >> Cn)",
        definition: "Alan Prince & Paul Smolensky, Optimality Theory: Constraint Interaction in Generative Grammar (1993 Technical Report; Blackwell, 2004), doi:10.1002/9780470759400: constraints are ranked in a strict dominance hierarchy where any single violation of a higher-ranked constraint is fatal compared to arbitrarily many violations of lower-ranked constraints. In zed-kask it grounds pragmatic-semantics' conflict resolution (Hard >> Strong >> Weak).",
        constituents: &[
            "constraint hierarchy",
            "lexicographic dominance",
            "optimal candidate",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Prince & Smolensky, Optimality Theory (1993/2004), doi:10.1002/9780470759400",
    },
    DerivedConcept {
        term: "epistemic_modality",
        aliases: &["epistemic certainty", "evidential modality"],
        identity: "qualification of a statement according to the speaker's or agent's degree of certainty, evidentiary basis, and commitment to the truth of the proposition",
        definition: "F. R. Palmer, Mood and Modality (Cambridge University Press, 1986; 2nd ed. 2001): distinguishes epistemic modality (the status of the proposition with respect to knowledge and evidence — declarative fact, calibrated probability, subjunctive possibility) from deontic modality (obligation and permission — hard, strong, and weak constraints). In zed-kask it grounds pragmatic-semantics' certainty-level classification.",
        constituents: &[
            "declarative fact",
            "probabilistic estimate",
            "subjunctive hypothesis",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 6 skill-corpus audit); Palmer, Mood and Modality, Cambridge University Press (1986/2001)",
    },
    DerivedConcept {
        term: "rank_reversal",
        aliases: &["rank reversals", "ahp rank reversal", "weight flip"],
        identity: "a change in the relative ordering of decision alternatives induced by perturbing criterion weights or by adding or removing an alternative under a multi-criteria aggregation rule",
        definition: "Valerie Belton & Tony Gear, 'On a short-coming of Saaty's method of analytic hierarchies', Omega 11(3):228-230 (1983), and Evangelos Triantaphyllou, 'Two new cases of rank reversals when the AHP and some of its additive variants are used that do not occur with the multiplicative AHP', Journal of Multi-Criteria Decision Analysis 10(1):11-25 (2001), doi:10.1002/mcda.284: measures sensitivity of a multi-criteria ranking by identifying the smallest relative one-at-a-time weight perturbation (min_flip) that changes the top-ranked alternative. In zed-kask it grounds mcda's sensitivity-analysis stage.",
        constituents: &[
            "multi_criteria_decision_analysis",
            "sensitivity analysis",
            "weight perturbation",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); Belton & Gear, Omega 11(3):228-230 (1983); Triantaphyllou, J. MCDA 10(1):11-25 (2001), doi:10.1002/mcda.284",
    },
    DerivedConcept {
        term: "catastrophic_forgetting",
        aliases: &[
            "catastrophic interference",
            "knowledge forgetting",
            "intruder dimensions",
        ],
        identity: "the degradation of a pre-trained model's previously acquired capabilities and knowledge when its weights are updated on a new task distribution",
        definition: "Michael McCloskey & Neal J. Cohen, 'Catastrophic Interference in Connectionist Networks: The Sequential Learning Problem', Psychology of Learning and Motivation 24:109-165 (1989), Robert M. French, 'Catastrophic forgetting in connectionist networks', Trends in Cognitive Sciences 3(4):128-135 (1999), and Dan Biderman et al., 'LoRA Learns Less and Forgets Less', TMLR (2024), arXiv:2405.09673: fine-tuning updates can overwrite pre-trained representations unless constrained by low-rank adapters or knowledge-preserving subspace initialization (such as CorDA-KP). In zed-kask it is a research-content concept only; the lora-training G5 gate it formerly grounded was removed 2026-10-17 with the training pipeline.",
        constituents: &["fine_tuning", "knowledge preservation", "low-rank subspace"],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); McCloskey & Cohen (1989); French (1999); Biderman et al., arXiv:2405.09673 (2024)",
    },
    DerivedConcept {
        term: "dependent_type",
        aliases: &[
            "dependent types",
            "dependent type theory",
            "pi type",
            "sigma type",
        ],
        identity: "a type family indexed by terms (B : A -> Sort u), generalized by dependent product types (Pi) and dependent sum types (Sigma)",
        definition: "Nicolaas Govert de Bruijn, 'The mathematical language AUTOMATH, its usage, and some of its extensions', Lecture Notes in Mathematics 125 (1970), doi:10.1007/bfb0060623, Per Martin-Lof, Intuitionistic Type Theory (Bibliopolis, 1984), and Mario Carneiro, 'The Type Theory of Lean' (CMU Master's Thesis, 2019): types may depend on values, allowing universal quantifiers (forall x : A, P x) to be represented as dependent function types and constructive witness-carrying existentials as Sigma types in Type (distinct from proof-irrelevant Exists in Prop). In zed-kask it grounds lean-prover.",
        constituents: &[
            "curry_howard_correspondence",
            "universal quantification",
            "constructive witness",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); de Bruijn, LNM 125 (1970), doi:10.1007/bfb0060623; Martin-Lof (1984); Carneiro (2019)",
    },
    DerivedConcept {
        term: "proof_irrelevance",
        aliases: &["propositional extensionality", "subsingleton elimination"],
        identity: "the type-theoretic principle that for any proposition P : Prop and any two proofs h1 h2 : P, h1 and h2 are definitionally equal (h1 == h2)",
        definition: "Mario Carneiro, 'The Type Theory of Lean' (2019), and Leonardo de Moura & Sebastian Ullrich, 'The Lean 4 Theorem Prover and Programming Language', CADE-28 (2021), doi:10.1007/978-3-030-79876-5_37: any two inhabitants of a proposition in Prop are definitionally equal when they exist, ensuring proofs carry no runtime data and cannot be pattern-matched to extract witnesses into Type (restricted large elimination), while never permitting uninhabited Props or sorryAx holes. In zed-kask it grounds lean-prover's Prop/Type boundary discipline.",
        constituents: &[
            "curry_howard_correspondence",
            "dependent_type",
            "type_erasure",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); Carneiro, 'The Type Theory of Lean' (2019); de Moura & Ullrich, CADE-28 (2021), doi:10.1007/978-3-030-79876-5_37",
    },
    DerivedConcept {
        term: "type_erasure",
        aliases: &["proof erasure", "runtime erasure"],
        identity: "the compilation pass that removes type arguments and Prop proof terms before code generation because runtime execution cannot branch on proof structure",
        definition: "Edwin Brady, Conor McBride & James McKinna, 'Inductive Families Need Not Store Their Indices', TYPES (2003), Mario Carneiro, 'The Type Theory of Lean' (2019), and Leonardo de Moura & Sebastian Ullrich, CADE-28 (2021), doi:10.1007/978-3-030-79876-5_37: because propositions in Prop are proof-irrelevant and large elimination into Type is restricted to subsingletons, proof terms erase completely at compile time with zero runtime overhead. In zed-kask it grounds lean-prover's erasure verification.",
        constituents: &["proof_irrelevance", "compilation", "zero runtime overhead"],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); Brady, McBride & McKinna (2003); Carneiro (2019); de Moura & Ullrich (2021), doi:10.1007/978-3-030-79876-5_37",
    },
    DerivedConcept {
        term: "delta_debugging",
        aliases: &["ddmin", "input minimization", "test case minimization"],
        identity: "an automated binary-search minimization algorithm (ddmin) that partitions a failure-inducing input or change set against a deterministic oracle until a 1-minimal failure-inducing subset is isolated",
        definition: "Andreas Zeller & Ralf Hildebrandt, 'Simplifying and isolating failure-inducing input', IEEE Transactions on Software Engineering 28(2):183-200 (2002), doi:10.1109/32.988498, and Zeller, Why Programs Fail (2009): systematically narrows a failing test case or configuration to the smallest configuration where removing any single element causes the failure to disappear. In zed-kask it grounds diagnose's minimal reproduction loop.",
        constituents: &[
            "scientific_debugging",
            "minimal reproduction",
            "test_oracle",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); Zeller & Hildebrandt, IEEE TSE 28(2):183-200 (2002), doi:10.1109/32.988498",
    },
    DerivedConcept {
        term: "git_bisect",
        aliases: &["bisection debugging", "commit bisection"],
        identity: "binary search over a directed acyclic commit graph using a deterministic test oracle to isolate the exact first commit that introduced a regression in O(log N) steps",
        definition: "Andreas Zeller, 'Yesterday, my program worked. Today, it does not. Why?', ESEC/FSE (1999), doi:10.1145/318774.318946, and Linus Torvalds & Junio C. Hamano, git-bisect(1) manual: halves the commit range between a known-good ancestor and a known-bad descendant at each step by executing a deterministic reproducer script (git bisect run). In zed-kask it grounds diagnose's bisection feedback-loop strategy.",
        constituents: &["delta_debugging", "directed_acyclic_graph", "test_oracle"],
        authority: "operator ruling 2026-10-14 (Cohort 7 skill-corpus audit); Zeller, ESEC/FSE (1999), doi:10.1145/318774.318946",
    },
    DerivedConcept {
        term: "scope_creep",
        aliases: &["feature creep", "unrequested scope"],
        identity: "uncontrolled expansion of a task's changes beyond its stated functional requirements — unrequested features, speculative configurability, or adjacent-code refactoring",
        definition: "Project Management Institute, A Guide to the Project Management Body of Knowledge (PMBOK Guide), and Andrej Karpathy's LLM coding principles (reified in coding-guidelines): adding features, abstractions, formatting changes, or adjacent refactors not requested by the user increases diff surface and defect risk without serving the functional goal. In zed-kask it grounds coding-guidelines' Surgical Changes and Simplicity First principles.",
        constituents: &["specification", "surgical change", "yagni"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); PMBOK Guide; Karpathy LLM coding principles",
    },
    DerivedConcept {
        term: "yagni",
        aliases: &["you aren't gonna need it", "you arent gonna need it"],
        identity: "do not add functionality, abstractions, or configurability on speculation about future needs; implement only what the immediate requirement demands",
        definition: "Kent Beck, Extreme Programming Explained: Embrace Change (Addison-Wesley, 1999), and Ronald E. Jeffries, 'You're NOT gonna need it!' (1998): speculative generality charges immediate implementation, testing, and cognitive-load costs for hypothetical future requirements that rarely materialize as imagined. In zed-kask it grounds coding-guidelines' Simplicity First principle and essentialist.",
        constituents: &["deletion_test", "cognitive_load", "scope_creep"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Beck, Extreme Programming Explained (1999); Jeffries (1998)",
    },
    DerivedConcept {
        term: "cognitive_dissonance",
        aliases: &["dissonance resolution", "festinger cognitive dissonance"],
        identity: "the epistemic tension arising when a cognitive system or memory store holds mutually contradictory assertions simultaneously, resolved by reducing importance, adding consonant evidence, or removing the dissonant element",
        definition: "Leon Festinger, A Theory of Cognitive Dissonance (Stanford University Press, 1957), doi:10.1515/9781503620766: contradictory cognitions produce pressure toward resolution through three canonical strategies — reducing the weight/confidence of dissonant elements, adding reconciling consonant cognitions, or removing (forgetting) the falsified dissonant cognition. In zed-kask it grounds therapy's contradiction-resolution taxonomy and memory_resolve_contradiction.",
        constituents: &["contradiction", "confidence", "reconciliation"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Festinger, A Theory of Cognitive Dissonance (1957), doi:10.1515/9781503620766",
    },
    DerivedConcept {
        term: "reification",
        aliases: &["knowledge reification", "lesson reification"],
        identity: "converting accumulated episodic experience and tacit patterns into durable, executable artifacts — skills, prompt templates, or project rules — that proactively govern future action",
        definition: "Etienne Wenger, Communities of Practice: Learning, Meaning, and Identity (Cambridge University Press, 1998), and Ikujiro Nonaka & Hirotaka Takeuchi, The Knowledge-Creating Company (Oxford University Press, 1995): the process of giving concrete, persistent form to abstract experience so past lessons guide future behavior directly without depending on episodic recall. In zed-kask it grounds therapy's learning-loop closure (experience -> memory -> therapy -> skill/template/rule).",
        constituents: &["skill", "episodic memory", "proactive guidance"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Wenger, Communities of Practice (1998); Nonaka & Takeuchi (1995)",
    },
    DerivedConcept {
        term: "homeostasis",
        aliases: &["homeostatic regulation", "ultrastability"],
        identity: "the capacity of a self-regulating system to maintain its essential variables within acceptable bounds despite external disturbances",
        definition: "Walter B. Cannon, 'Organization for Physiological Homeostasis', Physiological Reviews 9(3):399-431 (1929), doi:10.1152/physrev.1929.9.3.399, and W. Ross Ashby, Design for a Brain (1952): closed-loop negative feedback and requisite regulator variety hold critical system variables within their viable operating band against environmental disturbances. In zed-kask it grounds pragmatic-cybernetics and hkask-regulation.",
        constituents: &[
            "cybernetic_feedback_loop",
            "requisite_variety",
            "viable_system_model",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Cannon, Physiological Reviews 9(3):399-431 (1929), doi:10.1152/physrev.1929.9.3.399; Ashby, Design for a Brain (1952)",
    },
    DerivedConcept {
        term: "typestate",
        aliases: &["typestate pattern", "compile-time state machine"],
        identity: "a static type-system discipline in which the valid operations on an object depend on its current execution state encoded in its type, making invalid state transitions compile-time errors",
        definition: "Robert E. Strom & Shaula Yemini, 'Typestate: A programming language concept for enhancing software reliability', IEEE Transactions on Software Engineering 12(1):157-171 (1986), doi:10.1109/tse.1986.6312929: refines ordinary types with state-dependent operation availability; in Rust, affine move semantics and PhantomData type parameters consume the pre-transition value and return the post-transition type so use-before-init and invalid transitions fail to compile. In zed-kask it grounds idiomatic-rust's Principle 1 (Make invalid states unrepresentable).",
        constituents: &["dependent_type", "move semantics", "state_diagram"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Strom & Yemini, IEEE TSE 12(1):157-171 (1986), doi:10.1109/tse.1986.6312929",
    },
    DerivedConcept {
        term: "zero_cost_abstraction",
        aliases: &[
            "zero-cost abstractions",
            "zero overhead principle",
            "zero-overhead abstraction",
        ],
        identity: "what you do not use, you do not pay for, and what you do use compiles down to machine code no less efficient than hand-written lower-level code",
        definition: "Bjarne Stroustrup, The Design and Evolution of C++ (Addison-Wesley, 1994), §4.5 'Zero-overhead rule', and Steve Klabnik & Carol Nichols, The Rust Programming Language, Ch. 13: generic abstractions, iterators, newtypes, and ownership checks are resolved at compile time via monomorphization and inlining without hidden runtime boxing or dispatch overhead. In zed-kask it grounds idiomatic-rust's Principle 4.",
        constituents: &["monomorphization", "inlining", "compilation"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Stroustrup, The Design and Evolution of C++ (1994), §4.5; Klabnik & Nichols, The Rust Programming Language",
    },
    DerivedConcept {
        term: "algebraic_data_type",
        aliases: &[
            "algebraic data types",
            "adt",
            "sum and product types",
            "discriminated union",
        ],
        identity: "a composite type formed by product types (structs/tuples) and discriminated sum types (tagged enums with exhaustive pattern matching)",
        definition: "Rod M. Burstall, David B. MacQueen & Donald T. Sannella, 'HOPE: An experimental applicative language', LISP Conference (1980), Benjamin C. Pierce, Types and Programming Languages (MIT Press, 2002), and Ralf Jung et al., 'RustBelt: securing the foundations of the Rust programming language', PACMPL 2(POPL) (2017), doi:10.1145/3158154: combines products (conjunction of fields) and sums (disjunction of variants checked exhaustively by match) to model domain invariants exactly. In zed-kask it grounds idiomatic-rust's type-design methodology.",
        constituents: &["product type", "sum type", "exhaustive pattern matching"],
        authority: "operator ruling 2026-10-14 (Cohort 8 skill-corpus audit); Burstall, MacQueen & Sannella (1980); Pierce (2002); Jung et al., PACMPL 2(POPL) (2017), doi:10.1145/3158154",
    },
    DerivedConcept {
        term: "null_hypothesis",
        aliases: &["null hypotheses", "h0", "nil hypothesis"],
        identity: "the baseline statistical proposition (H0) — asserting zero treatment effect, no association, or an effect outside a prespecified equivalence/non-inferiority margin delta — tested for refutation before the research hypothesis (H1) is supported",
        definition: "Ronald A. Fisher, The Design of Experiments (Oliver & Boyd, 1935), and Jerzy Neyman & Egon S. Pearson, 'On the Problem of the Most Efficient Tests of Statistical Hypotheses', Philosophical Transactions of the Royal Society A 231:289-337 (1933): the default proposition against which experimental evidence is evaluated; for superiority/difference designs H0 states zero difference, whereas for non-inferiority or equivalence designs H0 states that the difference lies outside the acceptable margin delta. In zed-kask it grounds hypothesis-framer's hypothesis-operationalize.j2.",
        constituents: &[
            "falsifiability",
            "research hypothesis",
            "non_inferiority_margin",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Fisher, The Design of Experiments (1935); Neyman & Pearson, Phil. Trans. R. Soc. A 231:289-337 (1933)",
    },
    DerivedConcept {
        term: "non_inferiority_margin",
        aliases: &[
            "equivalence margin",
            "delta margin",
            "consort non-inferiority margin",
        ],
        identity: "the prespecified clinically or operationally acceptable boundary (delta > 0) by which a new intervention may be worse than an active control without losing acceptability",
        definition: "Gilda Piaggio et al., 'Reporting of Noninferiority and Equivalence Randomized Trials: Extension of the CONSORT 2010 Statement', JAMA 308(24):2594-2604 (2012), doi:10.1001/jama.2012.87802: in non-inferiority trials (H0: Delta <= -delta vs H1: Delta > -delta) and equivalence trials (H0: |Delta| >= delta vs H1: |Delta| < delta), the margin delta must be justified a priori; without a defined delta, the hypothesis is untestable. In zed-kask it grounds hypothesis-framer's testability gate.",
        constituents: &["null_hypothesis", "pico", "effect size"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Piaggio et al., JAMA 308(24):2594-2604 (2012), doi:10.1001/jama.2012.87802",
    },
    DerivedConcept {
        term: "working_memory",
        aliases: &["working memory capacity", "short-term working memory"],
        identity: "the capacity-bounded cognitive system that temporarily holds and manipulates task-relevant information during reasoning, comprehension, and action execution",
        definition: "Alan Baddeley, 'Working memory: looking back and looking forward', Nature Reviews Neuroscience 4(10):829-839 (2003), doi:10.1038/nrn1201, and George A. Miller, 'The magical number seven, plus or minus two: Some limits on our capacity for processing information', Psychological Review 63(2):81-97 (1956), doi:10.1037/h0043158: because working-memory slots decay across turns and saturate above ~5-7 chunks, interfaces and prose must restate current state explicitly and bound ungrouped lists. In zed-kask it grounds adhd-mode's 5-item list cap and per-turn state restatement.",
        constituents: &["cognitive_load", "chunking", "executive_function"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Baddeley, Nature Reviews Neuroscience 4(10):829-839 (2003), doi:10.1038/nrn1201; Miller, Psychological Review 63(2):81-97 (1956), doi:10.1037/h0043158",
    },
    DerivedConcept {
        term: "executive_function",
        aliases: &[
            "executive functions",
            "behavioral inhibition",
            "task initiation",
        ],
        identity: "the self-regulatory cognitive processes — task initiation, behavioral inhibition, working-memory maintenance, and goal-directed action sequencing — governing purposeful behavior",
        definition: "Russell A. Barkley, 'Behavioral inhibition, sustained attention, and executive functions: Constructing a unifying theory of ADHD', Psychological Bulletin 121(1):65-94 (1997), doi:10.1037/0033-2909.121.1.65: models ADHD as a deficit in behavioral inhibition and four executive functions (working memory, self-regulation of affect/motivation, internalization of speech, reconstitution). In zed-kask it grounds adhd-mode's structural rules: lead with the next action, number steps, suppress tangents, and end with one concrete next action.",
        constituents: &["working_memory", "task initiation", "tangent suppression"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Barkley, Psychological Bulletin 121(1):65-94 (1997), doi:10.1037/0033-2909.121.1.65",
    },
    DerivedConcept {
        term: "test_driven_development",
        aliases: &["tdd", "red-green-refactor", "red before green"],
        identity: "a software development discipline that progresses in narrow vertical slices at confirmed public seams by writing a failing test first (Red), implementing the minimal production code to pass it (Green), and verifying against an independent oracle",
        definition: "Kent Beck, Test-Driven Development: By Example (Addison-Wesley, 2002): write a single automated test for a desired behavior at a public boundary, watch it fail for the expected reason (Red), write the minimal production code to make it pass (Green), and refactor while keeping tests green. In zed-kask it grounds the tdd skill and testing-protocol.",
        constituents: &["vertical_slice", "test_oracle", "regression_testing"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Beck, Test-Driven Development: By Example (2002)",
    },
    DerivedConcept {
        term: "test_double",
        aliases: &["test doubles", "boundary fake", "mock object"],
        identity: "a stand-in object (dummy, stub, spy, mock, or fake) substituted only at external system boundaries via trait injection while keeping internal collaborators real",
        definition: "Gerard Meszaros, xUnit Test Patterns: Refactoring Test Code (Addison-Wesley, 2007): classifies stand-in objects used in automated tests (Dummy, Stub, Spy, Mock, Fake). In zed-kask, test doubles are restricted to external system boundaries (HTTP providers, clock, randomness) injected via ports (InferencePort, ToolPort), never internal domain modules or capability-stripped server constructors. In zed-kask it grounds tdd's Mocking rules.",
        constituents: &["test_harness", "test_driven_development", "system boundary"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Meszaros, xUnit Test Patterns (2007)",
    },
    DerivedConcept {
        term: "agent_to_agent_protocol",
        aliases: &["a2a protocol", "a2a", "agent2agent", "agent2agent protocol"],
        identity: "an inter-agent delegation and communication protocol in which agents advertise typed capabilities and skills via an AgentCard and exchange scoped Message turns that produce durable Task and Artifact records",
        definition: "Agent2Agent (A2A) Protocol Specification: defines five core protocol objects — AgentCard (JSON capability advertisement with input/output modes, skills, and auth), Task (unit of delegated work with lifecycle state), Message (communication turn), Artifact (durable task output), and Part (typed content unit). In zed-kask it is retained as the published-protocol reference anchor.",
        constituents: &["agent card", "task", "artifact"],
        authority: "operator ruling 2026-10-14 (Cohort 9 skill-corpus audit); Agent2Agent (A2A) Protocol Specification (agent2agent.info)",
    },
    DerivedConcept {
        term: "bloom_taxonomy",
        aliases: &[
            "bloom's taxonomy",
            "blooms taxonomy",
            "krathwohl taxonomy",
            "taxonomy of educational objectives",
        ],
        identity: "a hierarchical classification of cognitive complexity progressing from factual recall and comprehension through application and analysis to evaluation and synthesis",
        definition: "Benjamin S. Bloom, Max D. Engelhart, Edward J. Furst, Walker H. Hill & David R. Krathwohl, Taxonomy of Educational Objectives: The Classification of Educational Goals (Longmans, 1956), and David R. Krathwohl, 'A Revision of Bloom's Taxonomy: An Overview', Theory Into Practice 41(4):212-218 (2002), doi:10.1207/s15430421tip4104_2: structures cognitive operations along an ordered hierarchy of complexity. In zed-kask it grounds grill-me's 5-level interrogation ladder (Recall -> Mechanism -> Rationale -> Edge Cases -> Synthesis) and corpus_build_prompts's type_distribution.",
        constituents: &["socratic_method", "cognitive complexity", "synthesis"],
        authority: "operator ruling 2026-10-14 (Cohort 10 skill-corpus audit); Bloom et al. (1956); Krathwohl, Theory Into Practice 41(4):212-218 (2002), doi:10.1207/s15430421tip4104_2",
    },
    DerivedConcept {
        term: "zone_of_proximal_development",
        aliases: &["zpd", "vygotsky zpd", "instructional scaffolding"],
        identity: "the cognitive band between what a learner can solve independently and what they cannot solve even with guidance, targeted by adaptive difficulty adjustment",
        definition: "Lev S. Vygotsky, Mind in Society: The Development of Higher Psychological Processes (Harvard University Press, 1978), and David Wood, Jerome S. Bruner & Gail Ross, 'The role of tutoring in problem solving', Journal of Child Psychology and Psychiatry 17(2):89-100 (1976): adaptive instruction probes at the frontier of current competence, escalating difficulty when mastery is high and holding or reprobing when mastery is partial. In zed-kask it grounds grill-me's Feedback gate escalation thresholds (solid_ratio >= 0.80 -> escalate; 0.40..0.79 -> hold; < 0.40 -> reprobe; retire after 3 Gap attempts).",
        constituents: &["bloom_taxonomy", "socratic_method", "adaptive escalation"],
        authority: "operator ruling 2026-10-14 (Cohort 10 skill-corpus audit); Vygotsky, Mind in Society (1978); Wood, Bruner & Ross (1976)",
    },
    DerivedConcept {
        term: "retrieval_augmented_generation",
        aliases: &["rag", "retrieve-cite-verify"],
        identity: "conditioning language-model generation on non-parametric passages retrieved from an indexed document corpus so claims are anchored to explicit source passages and verifiable by substring checks",
        definition: "Patrick Lewis et al., 'Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks', NeurIPS 33:9459-9474 (2020), arXiv:2005.11401: combines parametric language-model memory with non-parametric retrieved text passages so generated assertions cite retrieved source chunks rather than relying on ungrounded parametric recall. In zed-kask it grounds listening's apply-template-rag.j2 and corpus_query.",
        constituents: &["maia_listening", "source passage", "substring verification"],
        authority: "operator ruling 2026-10-14 (Cohort 10 skill-corpus audit); Lewis et al., NeurIPS (2020), arXiv:2005.11401",
    },
    DerivedConcept {
        term: "semiotics",
        aliases: &[
            "syntax and semantics",
            "syntax-semantics distinction",
            "syntactics and semantics",
        ],
        identity: "the formal division of sign systems into syntactics (relations among formal expressions independent of interpretation), semantics (relations between expressions and their designated meanings or truth conditions), and pragmatics (relations between expressions and their users in context)",
        definition: "Charles W. Morris, Foundations of the Theory of Signs (University of Chicago Press, 1938), and Rudolf Carnap, Introduction to Semantics (Harvard University Press, 1942): distinguishes syntactic operations (governed solely by formal formation and transformation rules over symbols) from semantic operations (governed by designation, denotation, and truth conditions). In zed-kask it grounds region-routing's syntactic-vs-semantic representation axis crossed with Turing's deterministic-vs-probabilistic process axis.",
        constituents: &[
            "deterministic_computation",
            "probabilistic_computation",
            "pragmatic_semantics",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 10 skill-corpus audit); Morris, Foundations of the Theory of Signs (1938); Carnap, Introduction to Semantics (1942)",
    },
    DerivedConcept {
        term: "occam_razor",
        aliases: &[
            "occam's razor",
            "occams razor",
            "minimum description length",
            "mdl principle",
        ],
        identity: "entities, abstractions, and interface elements must not be multiplied beyond necessity; among candidate designs that satisfy the same behavioral contract, the one with the shortest description and smallest surface area is preferred",
        definition: "Ray J. Solomonoff, 'A formal theory of inductive inference. Part I', Information and Control 7(1):1-22 (1964), doi:10.1016/s0019-9958(64)90223-2, and Jorma Rissanen, 'Modeling by shortest data description', Automatica 14(5):465-471 (1978), doi:10.1016/0005-1098(78)90005-5: formalizes the principle of parsimony (Occam's razor) as choosing the hypothesis or design that minimizes the combined length of the model and the deviations from it. In zed-kask it grounds essentialist's 'always take away, never add' 3-gate eliminative interrogation.",
        constituents: &["deletion_test", "deep_module", "parsimony"],
        authority: "operator ruling 2026-10-14 (Cohort 11 skill-corpus audit); Solomonoff, Information and Control 7(1):1-22 (1964), doi:10.1016/s0019-9958(64)90223-2; Rissanen, Automatica 14(5):465-471 (1978), doi:10.1016/0005-1098(78)90005-5",
    },
    DerivedConcept {
        term: "edit_decision_list",
        aliases: &["edl", "reel edl", "non-linear edit decision list"],
        identity: "an ordered, deterministic sequence of timecode or word-range selection operations (Keep clips) derived from a source recording or timing-aligned transcript that specifies how media segments are cut and concatenated into a rendered reel without mutating the source media",
        definition: "Floraine Berthouzoz, Wilmot Li & Maneesh Agrawala, 'Tools for placing cuts and transitions in interview video', ACM Transactions on Graphics 31(4):67 (2012), doi:10.1145/2185520.2185563, SMPTE 258M / CMX 3600 EDL specification, and MovieLabs OMC v2.8: represents a non-destructive edit as a list of source word/time ranges whose union-merged Keep operations drive lossless stream-copy rendering. In zed-kask it grounds educt_edl_from_highlights, educt_render_edl, and the transcript-reel skill.",
        constituents: &[
            "transcript_linked_media",
            "word-range selection",
            "non-destructive editing",
        ],
        authority: "operator ruling 2026-10-14 (Cohort 11 skill-corpus audit); Berthouzoz, Li & Agrawala, ACM TOG 31(4):67 (2012), doi:10.1145/2185520.2185563; SMPTE 258M",
    },
    DerivedConcept {
        term: "frame_budget",
        aliases: &["frame rate", "vsync budget", "frame time budget"],
        identity: "the maximum wall-clock duration available on the UI foreground and renderer threads to process input, layout, paint, and submit one display frame without dropping a VSync deadline (8.33 ms at 120 FPS; 16.67 ms at 60 FPS)",
        definition: "GPUI Renderer Architecture (crates/gpui) and Display VSync Refresh Standards: the reciprocal of the display refresh rate (1 / 120 Hz = 8.33 ms; 1 / 60 Hz = 16.67 ms). Foreground tasks or layout/paint passes that exceed the frame budget starve input handling and cause visible frame drops or hitches even when aggregate throughput is high. In zed-kask it grounds gpui-bench's responsiveness target.",
        constituents: &["latency", "throughput", "responsiveness"],
        authority: "operator ruling 2026-10-14 (Cohort 12 skill-corpus audit); GPUI Renderer Architecture (crates/gpui); Display VSync Refresh Standards (120 Hz / 60 Hz)",
    },
    DerivedConcept {
        term: "instructions_per_cycle",
        aliases: &["ipc", "retired instructions per cycle", "cpu ipc"],
        identity: "retired CPU instructions divided by elapsed processor clock cycles (IPC = Instructions / Cycles), reported alongside total instruction count and wall-clock time",
        definition: "John L. Hennessy & David A. Patterson, Computer Architecture: A Quantitative Approach, 6th ed. (Morgan Kaufmann, 2017), and Andy Georges, Dries Buytaert & Lieven Eeckhout, 'Statistically rigorous Java performance evaluation', OOPSLA (2007), doi:10.1145/1297027.1297033: distinguishes algorithmic work reductions (fewer retired instructions) from microarchitectural stall changes (cache misses, branch mispredictions) and CPU frequency scaling. In zed-kask it grounds gpui-bench's hardware-counter reporting rules.",
        constituents: &["retired instructions", "clock cycles", "wall-clock time"],
        authority: "operator ruling 2026-10-14 (Cohort 12 skill-corpus audit); Hennessy & Patterson, Computer Architecture (2017); Georges, Buytaert & Eeckhout, OOPSLA (2007), doi:10.1145/1297027.1297033",
    },
    DerivedConcept {
        term: "equity_duration",
        aliases: &["cash flow duration", "macaulay equity duration"],
        identity: "sum(t * PV(FCF_t)) / sum(PV(FCF_t)), the present-value-weighted average maturity in years of an equity's projected free cash flows and terminal value",
        definition: "Patricia M. Dechow, Richard G. Sloan & Mark T. Soliman, 'Implied Equity Duration: A New Measure of Equity Risk', Review of Accounting Studies 9(2-3):197-228 (2004), doi:10.1023/b:rast.0000028186.44328.3f, extending Frederick R. Macaulay, Some Theoretical Problems Suggested by the Movements of Interest Rates, Bond Yields and Stock Prices in the United States Since 1856 (NBER, 1938): measures the cash-flow horizon and discount-rate sensitivity of an equity by weighting each future free cash flow (plus terminal value at the horizon year) by its time to receipt divided by total enterprise value. In zed-kask it is a research-content concept.",
        constituents: &["discounted_cash_flow", "free_cash_flow"],
        authority: "operator ruling 2026-10-14 (Cohort 13 skill-corpus audit); Dechow, Sloan & Soliman, Review of Accounting Studies 9(2-3):197-228 (2004), doi:10.1023/b:rast.0000028186.44328.3f; Macaulay (1938)",
    },
    DerivedConcept {
        term: "log_odds",
        aliases: &[
            "logit",
            "log-odds",
            "log odds",
            "log-odds space",
            "log-odds pooling",
        ],
        identity: "ln(p / (1 - p)), the canonical link function mapping open probabilities p in (0, 1) bijectively onto the unconstrained real line (-inf, +inf) with inverse sigmoid sigma(x) = 1 / (1 + e^-x)",
        definition: "Joseph Berkson, 'Application of the Logistic Function to Bio-Assay', Journal of the American Statistical Association 39(227):357-365 (1944), doi:10.2307/2280041, and Ville A. Satopaa et al., 'Combining Multiple Probability Predictions Using a Simple Logit Model', International Journal of Forecasting 30(2):344-356 (2014), doi:10.1016/j.ijforecast.2013.09.009: transforms bounded probabilities into additive log-odds units so linear tenor interpolation, extremity-adjusted forecast aggregation, and Bayesian log-odds evidence pooling never violate the [0, 1] boundary. In zed-kask it grounds superforecasting's extremized log-odds pooling and memory_update's Bayesian confidence combination.",
        constituents: &["calibrated_forecast", "bayesian_inference"],
        authority: "operator ruling 2026-10-14 (Cohort 13 skill-corpus audit); Berkson, JASA 39(227):357-365 (1944), doi:10.2307/2280041; Satopaa et al., Int. J. Forecasting 30(2):344-356 (2014), doi:10.1016/j.ijforecast.2013.09.009",
    },
    DerivedConcept {
        term: "run_length_encoding",
        aliases: &["run-length encoding", "rle", "run length encoding"],
        identity: "a run of n consecutive identical symbols is emitted as one symbol plus the count n",
        definition: "Lossless data compression in which a run of repeated values is stored as a single value and a count. The count preserves the exact repetition magnitude — unlike lossy deduplication, which discards it. In zed-kask it grounds the compaction pre-shrink's pass 1 (3+ identical adjacent lines collapse to one line plus an exact count marker). A compression citation, not a context-management claim: the 2026-10-18 real-thread census measured this pass at 0.0% median, so it stays as a cheap lossless guard, not a load-bearing pass.",
        constituents: &["data compression", "repetition"],
        authority: "operator ruling 2026-10-18 (compaction-pipeline census session); ISO/IEC 10918-1 (JPEG, CCITT T.81, 1992) run-length entropy coding; Salomon, Data Compression: The Complete Reference (4th ed., 2007)",
    },
    DerivedConcept {
        term: "truncation",
        aliases: &[
            "bounded-display truncation",
            "head-tail truncation",
            "windowing",
            "head-tail windowing",
        ],
        identity: "a text longer than a bound is replaced by its head and tail plus a marker naming the removed middle",
        definition: "Replacing a value that exceeds a display or transport bound with a bounded prefix and suffix, plus an in-band marker stating what was removed. The head/tail (bounded-display) form is distinguished from mid-content ellipsis: it cannot corrupt structured content, because the retained head and tail are contiguous and the cut lands on the caller's own boundary. In zed-kask it grounds the compaction pre-shrink's per-result windowing and the last-resort segment elision.",
        constituents: &["bound", "display", "provenance"],
        authority: "operator ruling 2026-10-18 (compaction-pipeline census session); POSIX head(1)/tail(1) bounded-display idiom (IEEE Std 1003.1)",
    },
    DerivedConcept {
        term: "extractive_summarization",
        aliases: &[
            "extractive summarization",
            "extractive compression",
            "extractive-before-abstractive staging",
        ],
        identity: "select or delete spans of the source text, emitting no new prose",
        definition: "Summarization that operates by selecting (or deleting) spans of the source, as opposed to abstractive summarization, which generates new text. The staging principle — cheap deterministic extractive selection before expensive abstractive compression — grounds the compaction pipeline's split between the deterministic pre-shrink (extractive: windowing, collapse) and the LLM summary pass (abstractive).",
        constituents: &["summary", "selection"],
        authority: "operator ruling 2026-10-18 (compaction-pipeline census session); Luhn, 'The Automatic Creation of Literature Abstracts', IBM Journal of Research and Development 2(2):159-165 (1958), doi:10.1147/rd.22.0159",
    },
    DerivedConcept {
        term: "context_window",
        aliases: &["context window", "input capacity", "context length"],
        identity: "the maximum number of tokens a model can attend to in one request",
        definition: "The bounded token budget a language model can process in a single request — input plus, where the provider shares the limit, the reserved output. Exceeding it is a hard provider rejection, not a degraded answer. In zed-kask it grounds the compaction budget calibration: the compaction model's input capacity, translated to a per-thread byte budget at the calibrated bytes-per-token ratio.",
        constituents: &["token", "capacity"],
        authority: "operator ruling 2026-10-18 (compaction-pipeline census session); Vaswani et al., 'Attention Is All You Need', NeurIPS 2017, arXiv:1706.03762 (self-attention over a fixed-length input window)",
    },
    DerivedConcept {
        term: "context_compaction",
        aliases: &["context compaction", "compaction", "thread compaction"],
        identity: "replace a conversation's history with a summary so the thread continues inside the model's context window",
        definition: "Replacing a thread's accumulated history with a model-written summary so subsequent requests fit the context window. In zed-kask the pipeline is a composition: deterministic pre-shrink on the request copy, budget calibration, planning (two-half / balanced segments / last-resort elision), concurrent segment summarization, and chronological merge — the stored history is never modified. The zed-kask-specific pipeline is specified in kask/docs/architecture/compaction-pipeline-spec.md.",
        constituents: &["context window", "extractive summarization", "map-reduce"],
        authority: "operator ruling 2026-10-18 (compaction-pipeline census session); zed-kask compaction-pipeline-spec.md (ratified 2026-09-29, D8)",
    },
    DerivedConcept {
        term: "security_vulnerability",
        aliases: &[
            "security vulnerabilities",
            "software vulnerability",
            "software vulnerabilities",
            "vulnerability",
        ],
        identity: "a weakness in a system that an attacker can exploit to violate its security properties",
        definition: "The security sense: a defect or weakness in software or a system that can be exploited to compromise confidentiality, integrity, or availability. The published anchor is CSO 3.5's security vulnerabilities topic (cso:security_vulnerabilities, the super-topic of cso:software_vulnerabilities) — a real CSO concept, but the resolver's exact match does not reach the singular form, so the derived entry records the singular-to-plural mapping rather than adding global singular/plural normalization (which would change resolution for every ontology caller). The nameable violation is applying the term to a non-security weakness (a design smell, an aesthetic defect).",
        constituents: &["software", "security"],
        authority: "operator ruling 2026-10-18; CSO 3.5, cso:security_vulnerabilities (vendored pin, sources/cso/CSO.3.5.owl)",
    },
    DerivedConcept {
        term: "software_modularity",
        aliases: &[
            "software modularity",
            "modularity",
            "modular design",
            "module decomposition",
        ],
        identity: "the degree to which a software system is composed of discrete, independently comprehensible and replaceable units with well-defined boundaries",
        definition: "The software-design sense: a system's decomposition into discrete units (modules, components) whose boundaries are well-defined, so each unit can be understood, changed, and replaced independently. CSO 3.5 publishes no software-modularity topic (grepped: only modular_exponentiation, modular_multiplication, modular_robotics, submodular, triple_modular_redundancy — none is the software-design sense), so this is a project-authored composition over published CSO constituents: separation of concerns (cso:separation_of_concerns), software design (cso:software_design), and software component (cso:software_component). The composition is OURS, recorded here with its provenance — never a private definition worn as a published anchor. The nameable violation is treating it as a synonym for code size or file count.",
        constituents: &[
            "separation of concerns",
            "software design",
            "software component",
        ],
        authority: "operator ruling 2026-10-18 (B.5b: author the derived entry); composed over CSO 3.5 cso:separation_of_concerns, cso:software_design, cso:software_component (vendored pin, sources/cso/CSO.3.5.owl) — no cso modularity topic exists",
    },
    DerivedConcept {
        term: "embedding",
        aliases: &[
            "vector embedding",
            "embeddings",
            "vector embeddings",
            "dense vector representation",
            "text embedding",
        ],
        identity: "a learned map from a discrete object (word, sentence, document, image) to a fixed-width real-valued vector such that semantic similarity between objects corresponds to geometric proximity between their vectors",
        definition: "Tomas Mikolov, Kai Chen, Greg Corrado & Jeffrey Dean, 'Efficient Estimation of Word Representations in Vector Space', arXiv:1301.3781 (2013): a dense, low-dimensional continuous vector representation learned so that objects used in similar contexts map to nearby points, making semantic similarity computable as a geometric distance (cosine similarity or Euclidean). In zed-kask it grounds the embeddings store and every semantic-recall path (curator_semantic_search, corpus_query, embed_for_semantic_recall). The nameable violation is treating an embedding as interpretable per-dimension or as a stable identifier: the coordinates are not features, and they change wholesale when the model or its width changes, so vectors from different models are not comparable and must never be mixed in one KNN index.",
        constituents: &["vector space model", "neural network", "cosine similarity"],
        authority: "operator ruling 2026-10-19 (card 75d2211b: resolve the coarse embedding/ANN vocabulary); Mikolov, Chen, Corrado & Dean, arXiv:1301.3781 (2013)",
    },
    DerivedConcept {
        term: "approximate_nearest_neighbor_search",
        aliases: &[
            "approximate nearest neighbor search",
            "approximate nearest neighbour search",
            "ann search",
            "ann",
            "vector search",
            "similarity search",
        ],
        identity: "retrieval of the stored vectors closest to a query vector under a distance metric, trading a bounded recall shortfall against exact search for sublinear query time",
        definition: "Yu. A. Malkov & D. A. Yashunin, 'Efficient and robust approximate nearest neighbor search using Hierarchical Navigable Small World graphs', arXiv:1603.09320 (2016, rev. 2018), and Jianyang Gao & Cheng Long, 'RaBitQ: Quantizing High-Dimensional Vectors with a Theoretical Error Bound for Approximate Nearest Neighbor Search', SIGMOD 2024, arXiv:2405.12497: graph-based indexes (HNSW) navigate a multi-layer proximity graph for logarithmic-complexity search, and randomized quantization (RaBitQ) compresses vectors to D-bit codes with a sharp error bound; both accept recall below 1.0 in exchange for speed. In zed-kask it grounds the vec0 KNN retrieval path behind corpus_query and curator_semantic_search. The nameable violation is reporting an approximate result as exact: an ANN hit set is a bounded-recall estimate, so a missing neighbor is not evidence of absence and recall must be measured, never assumed.",
        constituents: &[
            "nearest neighbor search",
            "embedding",
            "data compression",
            "metric space",
        ],
        authority: "operator ruling 2026-10-19 (card 75d2211b: resolve the coarse embedding/ANN vocabulary); Malkov & Yashunin, arXiv:1603.09320 (2016/2018); Gao & Long, SIGMOD 2024, arXiv:2405.12497",
    },
    DerivedConcept {
        term: "tokenization",
        aliases: &["tokenizer", "tokenize", "tokenisation"],
        identity: "the task of segmenting text into words, subwords, or tokens — the discrete units downstream language processing consumes",
        definition: "Jurafsky & Martin, Speech and Language Processing (3rd ed. draft, August 19, 2026), chapter 2 'Words and Tokens' (web.stanford.edu/~jurafsky/slp3/2.pdf): word and subword tokenization, including byte-pair encoding, segment text into the discrete units (words, subwords, tokens) that language models and NLP tools consume. In zed-kask it grounds the corpus pipeline's approximate token targets (converted to whitespace-word budgets, never tokenizer counts) and inference token accounting (billed_input_tokens vs total_input_tokens). The nameable violation is treating a token as a word: token and word counts are different quantities over the same text, and tokenizer-approximate budgets are never exact counts.",
        constituents: &[
            "word tokenization",
            "subword tokenization",
            "byte-pair encoding",
        ],
        authority: "operator ruling 2026-10-23 (card ffbf5147: resolve the tokenization/tokenizer vocabulary); Jurafsky & Martin, Speech and Language Processing (3rd ed. draft, August 19, 2026), ch. 2, web.stanford.edu/~jurafsky/slp3/2.pdf",
    },
    DerivedConcept {
        term: "security_review",
        aliases: &["secure code review", "security code review"],
        identity: "manual examination of source code to identify security vulnerabilities that automated tools often miss, with defect detection separated from correction",
        definition: "The OWASP Code Review Guide v2 (Conklin & Robinson, OWASP Foundation) and the OWASP Secure Code Review Cheat Sheet (cheatsheetseries.owasp.org): the process of manually examining source code to identify security vulnerabilities — analyzing application logic, data flow, and implementation details where human expertise and contextual judgment are required, complementing (never replacing) automated testing. In zed-kask it grounds the security-audit skill's review method and code-review's inline security pass. The nameable violation is presenting a security review as a comprehensive audit or certification: a review is evidence-bounded (checked / not_checked / residual_risk), and a clean pass never proves absence of vulnerabilities.",
        constituents: &["code review", "security vulnerability"],
        authority: "operator ruling 2026-10-24 (card 8f69c30d: resolve the coarse security-review vocabulary under the operator's resolution grant); OWASP Code Review Guide v2 (Conklin & Robinson, OWASP Foundation); OWASP Secure Code Review Cheat Sheet, cheatsheetseries.owasp.org",
    },
    DerivedConcept {
        term: "defense_in_depth",
        aliases: &["defence in depth", "layered defense", "layered security"],
        identity: "an information security strategy integrating people, technology, and operations capabilities to establish variable barriers across multiple layers, so the failure of one control does not leave the asset unprotected",
        definition: "NIST CSRC glossary 'defense-in-depth' (from CNSSI 4009 via NIST SP 800-30 Rev. 1; also NIST SP 800-39), csrc.nist.gov/glossary/term/defense_in_depth: 'Information security strategy integrating people, technology, and operations capabilities to establish variable barriers across multiple layers and missions of the organization.' In zed-kask it grounds the security-audit skill's defense-layer catalog. The nameable violation is counting a layer whose machinery is inert: a layer is live only with a live enforcement point, a de-advertised layer is recorded as a recorded absence, and a re-advertised layer names its new live enforcement point in the same change.",
        constituents: &["information security"],
        authority: "operator ruling 2026-10-24 (card 8f69c30d: resolve the coarse security-review vocabulary under the operator's resolution grant); NIST CSRC glossary 'defense-in-depth' (CNSSI 4009 via NIST SP 800-30 Rev. 1; NIST SP 800-39), csrc.nist.gov/glossary/term/defense_in_depth",
    },
];
/// Normalization matches the onto_anchor tool's: lowercase alphanumeric
/// characters only, separators and case folded away.
pub fn resolve_derived(term: &str) -> Option<&'static DerivedConcept> {
    let normalized: String = term
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    DERIVED_CONCEPTS.iter().find(|concept| {
        normalize(concept.term) == normalized
            || concept.aliases.iter().any(|a| normalize(a) == normalized)
    })
}

fn normalize(term: &str) -> String {
    term.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: [P5] The contested term of the 2026-09-10 session resolves
    /// as a derived concept with its identity and authority — never a void.
    #[test]
    fn net_margin_resolves_with_identity_and_authority() {
        let concept = resolve_derived("net margin").expect("net margin is defined");
        assert_eq!(concept.term, "net_margin");
        assert_eq!(concept.identity, "net income / revenue");
        assert!(
            concept.definition.contains("post-interest"),
            "the definition carries the load-bearing semantics: {}",
            concept.definition
        );
        assert!(concept.authority.contains("2026-09-10"));
    }

    /// expect: [P5] Aliases and separator variants resolve to the same
    /// concept.
    #[test]
    fn aliases_and_separators_resolve() {
        for term in [
            "net_margin",
            "Net Margin",
            "net profit margin",
            "npm",
            "roe",
            "return on equity",
            "sustainable-growth-rate",
            "sgr",
            "inference resilience boundary",
            "intervention receipt",
            "observed recovery",
            "brier score",
            "brier_score",
            "forecast calibration",
            "test harness",
            "property-based testing",
            "cybernetic feedback loop",
            "bounded model checking",
            "godel machine",
            "risk premium",
            "risk premia",
            "equity risk premium",
            "expectations gap",
            "expectations_gap",
            "pdca",
            "plan-do-check-act",
            "deming cycle",
            "improvement kata",
            "coaching kata",
            "gemba walk",
            "genchi gembutsu",
            "goodhart's law",
            "security vulnerability",
            "security vulnerabilities",
            "software vulnerability",
            "software modularity",
            "modular design",
        ] {
            assert!(
                resolve_derived(term).is_some(),
                "{term} must resolve — nothing is undefined"
            );
        }
    }

    #[test]
    fn risk_premium_resolves_with_identity_and_authority() {
        let concept = resolve_derived("risk premium").expect("risk premium is defined");
        assert_eq!(concept.term, "risk_premium");
        assert_eq!(
            resolve_derived("equity risk premium").map(|c| c.term),
            Some("risk_premium")
        );
        assert!(concept.definition.contains("Q523022"));
        assert!(concept.authority.contains("2026-09-19"));
    }

    #[test]
    fn expectations_gap_resolves_with_identity_and_authority() {
        let concept = resolve_derived("expectations gap").expect("expectations gap is defined");
        assert_eq!(concept.term, "expectations_gap");
        assert!(concept.definition.contains("reverse-solve"));
        assert!(concept.authority.contains("2026-09-19"));
        // The audit homonym (Liggio 1974) is deliberately not aliased: the
        // singular "expectation gap" stays unresolved.
        assert!(resolve_derived("expectation gap").is_none());
    }

    #[test]
    fn investment_thesis_resolves_with_identity_and_authority() {
        let concept = resolve_derived("investment thesis").expect("investment thesis is defined");
        assert_eq!(concept.term, "investment_thesis");
        assert!(
            concept
                .definition
                .contains("expectations the current price discounts")
        );
        assert!(concept.authority.contains("2026-09-27"));
        assert!(concept.authority.contains("Merchant Adventures"));
    }

    /// expect: [P5] The Lean/Kata vocabulary the skills use resolves to
    /// published anchors (LEI lexicon, Wikidata) with the 2026-09-24 ruling,
    /// and the PDCA identity names all four steps.
    #[test]
    fn lean_kata_terms_resolve_with_published_authority() {
        let pdca = resolve_derived("PDCA cycle").expect("PDCA is defined");
        assert_eq!(pdca.term, "pdca_cycle");
        for step in ["plan", "do", "check", "act"] {
            assert!(pdca.identity.contains(step), "PDCA identity names {step}");
        }
        assert!(pdca.authority.contains("Q820214"));
        for (term, marker) in [
            ("improvement kata", "Q7830807"),
            ("coaching kata", "Q7830807"),
            ("gemba walk", "Lean Enterprise Institute"),
            ("goodhart's law", "Q2575082"),
        ] {
            let concept = resolve_derived(term).expect("lean term is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.contains("2026-09-24"),
                "{term} cites the ruling"
            );
        }
    }

    /// expect: [P5] The maintenance vocabulary the system-maintenance skill
    /// uses resolves to published anchors (IEC 60050-192 Electropedia) with
    /// the 2026-10-10 ruling, and the CBM identity names its parent.
    #[test]
    fn maintenance_terms_resolve_with_published_authority() {
        let cbm = resolve_derived("condition-based maintenance").expect("CBM is defined");
        assert_eq!(cbm.term, "condition_based_maintenance");
        assert!(
            cbm.identity.contains("preventive maintenance"),
            "CBM identity names its parent"
        );
        for (term, marker) in [
            ("preventive maintenance", "IEV 192-06-05"),
            ("condition-based maintenance", "IEV 192-06-07"),
            ("cache", "Cache (computing)"),
            ("system health monitoring", "ISWC 2023"),
        ] {
            let concept = resolve_derived(term).expect("maintenance term is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.contains("2026-10-10"),
                "{term} cites the ruling"
            );
        }
    }

    /// expect: skill reference models resolve to the operator-named source,
    /// not the coarse 5W1H core anchor.
    #[test]
    fn forecasting_skill_reference_models_resolve_with_authority() {
        for (term, marker) in [
            ("superforecasting", "Tetlock & Gardner"),
            ("EQM", "Karvetski"),
            ("Explanation Quality Markers", "Karvetski"),
            ("metacognition", "Dunning"),
            ("scenario planning", "Schwartz"),
            ("scenario planning", "Chermack"),
            ("falsifiability", "Popper"),
            ("strong inference", "Platt"),
            ("FINER criteria", "Hulley"),
            ("PICO", "Richardson"),
            ("multi-criteria decision analysis", "Belton & Stewart"),
            ("MCDA", "Keeney & Raiffa"),
            ("root cause analysis", "Zeller"),
            ("gorilla game", "Moore"),
            ("Wardley map", "Wardley"),
            ("hidden champions", "Simon"),
            ("MAIA listening", "MAIA"),
            ("Fagan inspection", "Fagan"),
            ("code review", "Ousterhout"),
            ("GEPA", "Agrawal"),
            ("Pareto frontier", "Deb"),
            ("Reduct", "Reduct.video"),
            ("Viable System Model", "Beer"),
            ("algedonic signal", "Beer"),
            ("definition of done", "Scrum Guide"),
            ("requirements engineering", "29148"),
            ("prompt engineering", "Liu et al."),
            ("Diataxis", "Procida"),
            ("Sankey diagram", "Schmidt"),
            ("minimalism in documentation", "Carroll"),
            ("strangler fig pattern", "Fowler"),
            ("deep module", "Ousterhout"),
            ("OWASP LLM Top 10", "OWASP"),
            ("Hick's law", "Hick"),
            ("plussing", "Catmull"),
            ("yes and", "Johnstone"),
            ("improvisation", "Johnstone"),
            ("Bayesian inference", "Bayes"),
            ("finite game", "Carse"),
            ("repeated game", "Fudenberg"),
        ] {
            let concept = resolve_derived(term).expect("reference model is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                // "operator ruling" — singular or plural (the minimalism cap
                // entry cites two rulings); the intent is the ruling citation,
                // not the singular grammatical form.
                concept.authority.starts_with("operator ruling"),
                "{term} cites the ruling"
            );
        }
    }

    /// P8.4 (entropy-matched computation): the five routing rulings resolve
    /// with their published groundings.
    #[test]
    fn p84_routing_terms_resolve_with_published_authority() {
        for (term, marker) in [
            ("entropy", "Shannon (1948)"),
            ("entropy", "Jaynes (1957)"),
            ("conditional entropy", "Jaynes"),
            ("deterministic computation", "Turing (1936)"),
            ("probabilistic computation", "Gershman & Goodman (2014)"),
            ("probabilistic computation", "arXiv:2310.04363"),
            ("verification oracle", "Turing (1939)"),
            ("oracle", "Turing (1939)"),
            ("calibrated forecast", "Tetlock & Gardner (2015)"),
            ("calibrated forecast", "Brier (1950)"),
        ] {
            let concept = resolve_derived(term).expect("P8.4 ruling is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-"),
                "{term} cites the ruling"
            );
        }
    }

    /// expect: [P5] The mcp-tool-review anchor rulings (2026-10-02) resolve
    /// with their published authorities — the composition group's fidelity
    /// checks and the coherence check cite these terms.
    #[test]
    fn mcp_tool_review_anchor_rulings_resolve_with_authority() {
        for (term, marker) in [
            ("Bayesian network", "Pearl"),
            ("Bayes network", "Pearl"),
            ("discounted cash flow", "Damodaran"),
            ("DCF", "Williams"),
            ("arbitrage", "law of one price"),
        ] {
            let concept = resolve_derived(term).expect("mcp-tool-review anchor is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-"),
                "{term} cites the ruling"
            );
        }
    }

    /// expect: [P8.3, B.5] The 2026-10-18 ontology-item-3 rulings resolve
    /// with their published anchors and never fall back to the coarse 5W1H
    /// core: the security-vulnerability singular maps to CSO 3.5's published
    /// plural topic, and software modularity is the project-authored
    /// composition over published CSO constituents (no CSO modularity topic
    /// exists — the composition is ours, recorded with provenance).
    #[test]
    fn ontology_item_3_rulings_resolve_with_published_authority() {
        for (term, expected_term, marker) in [
            (
                "security vulnerability",
                "security_vulnerability",
                "security_vulnerabilities",
            ),
            (
                "software vulnerability",
                "security_vulnerability",
                "CSO 3.5",
            ),
            (
                "software modularity",
                "software_modularity",
                "cso:separation_of_concerns",
            ),
            (
                "modularity",
                "software_modularity",
                "cso:software_component",
            ),
        ] {
            let concept =
                resolve_derived(term).expect("the B.5 ontology ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-18"),
                "{term} cites the 2026-10-18 ruling"
            );
        }
        // The constituents of the modularity composition each resolve
        // through the ladder to their published CSO anchors — a derived
        // entry never fabricates a constituent anchor.
        for (constituent, concept) in [
            ("separation of concerns", "cso:separation_of_concerns"),
            ("software design", "cso:software_design"),
            ("software component", "cso:software_component"),
        ] {
            let resolved = crate::term_resolution::resolve_term(constituent);
            assert_eq!(resolved.concept, concept, "{constituent}: {resolved:?}");
        }
    }

    /// expect: [P5] The 2026-10-19 embedding/ANN ruling resolves both terms
    /// as derived concepts citing their verified primary authorities, and
    /// every constituent resolves above the coarse core anchor (a derived
    /// entry never fabricates a constituent anchor).
    #[test]
    fn embedding_and_ann_rulings_resolve_with_published_authority() {
        for (term, expected_term, marker) in [
            ("embedding", "embedding", "arXiv:1301.3781"),
            ("vector embedding", "embedding", "Mikolov"),
            ("embeddings", "embedding", "2013"),
            (
                "approximate nearest neighbor search",
                "approximate_nearest_neighbor_search",
                "arXiv:1603.09320",
            ),
            (
                "ANN search",
                "approximate_nearest_neighbor_search",
                "arXiv:2405.12497",
            ),
            (
                "vector search",
                "approximate_nearest_neighbor_search",
                "SIGMOD 2024",
            ),
        ] {
            let concept = resolve_derived(term).expect("the 2026-10-19 ruling must resolve");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.contains("2026-10-19"),
                "{term} cites the 2026-10-19 ruling"
            );
        }
        // The load-bearing semantics are nameable in each definition: the
        // embedding's model-comparability violation and the ANN's
        // approximate-is-not-exact violation.
        assert!(
            resolve_derived("embedding")
                .expect("embedding")
                .definition
                .contains("never be mixed in one KNN index"),
            "the embedding definition names the cross-model mixing violation"
        );
        assert!(
            resolve_derived("approximate nearest neighbor search")
                .expect("ann")
                .definition
                .contains("bounded-recall"),
            "the ANN definition names the approximate-is-not-exact violation"
        );
        // Constituents resolve through the ladder themselves — above the
        // coarse core anchor. (`index` is deliberately NOT a constituent:
        // the only remaining `index` sense is SUMO's financial benchmark,
        // the FIBO data-structure sense having left the ladder.)
        for (constituent, expected_concept) in [
            ("vector space model", "cso:vector_space_model"),
            ("neural network", "cso:neural_network"),
            ("cosine similarity", "cosine_distance"),
            ("nearest neighbor search", "cso:nearest_neighbor_search"),
            ("data compression", "cso:data_compression"),
            ("metric space", "cso:metric_space"),
            ("embedding", "embedding"),
        ] {
            let resolved = crate::term_resolution::resolve_term(constituent);
            assert_eq!(
                resolved.concept, expected_concept,
                "{constituent}: {resolved:?}"
            );
            assert_ne!(
                resolved.tier, "core",
                "{constituent} must resolve above the coarse core anchor"
            );
        }
    }

    #[test]
    fn experimental_design_rulings_resolve_with_published_authority() {
        for (term, marker) in [
            ("experimental design", "NIST/SEMATECH"),
            ("hypothesis testing", "Nosek"),
            ("measurement uncertainty", "JCGM 100:2008"),
            ("randomized experiment", "Fisher"),
            ("equivalence testing", "Lakens"),
            ("preregistration", "Nosek"),
        ] {
            let concept = resolve_derived(term).expect("experimental-design ruling must resolve");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-18"),
                "{term} cites the 2026-10-18 ruling"
            );
        }
    }

    /// expect: [P5] Every derived entry cites an authority — an entry with
    /// no authority citation is a fabricated definition, the exact disease
    /// this registry replaces.
    #[test]
    fn every_entry_cites_authority() {
        for concept in DERIVED_CONCEPTS {
            assert!(
                !concept.authority.is_empty(),
                "{} must cite its authority",
                concept.term
            );
        }
    }

    /// expect: [P5] The Cohort 1 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_1_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("Fitts's law", "fitts_law", "Fitts"),
            ("fittss_law", "fitts_law", "1954"),
            ("requisite variety", "requisite_variety", "Ashby"),
            ("law of requisite variety", "requisite_variety", "1956"),
            ("spin glass", "spin_glass", "Parisi"),
            (
                "frustrated landscape",
                "spin_glass",
                "10.1103/physrevlett.43.1754",
            ),
            ("allosteric regulation", "allosteric_regulation", "Monod"),
            (
                "allosteric population shift",
                "allosteric_regulation",
                "1965",
            ),
            (
                "regression discontinuity design",
                "regression_discontinuity_design",
                "Thistlethwaite",
            ),
            ("RDD", "regression_discontinuity_design", "1960"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 1 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The compaction-pipeline census rulings (2026-10-18)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn compaction_pipeline_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("run-length encoding", "run_length_encoding", "Salomon"),
            ("RLE", "run_length_encoding", "10918"),
            ("truncation", "truncation", "1003.1"),
            ("head-tail windowing", "truncation", "2026-10-18"),
            (
                "extractive summarization",
                "extractive_summarization",
                "Luhn",
            ),
            (
                "extractive-before-abstractive staging",
                "extractive_summarization",
                "1958",
            ),
            ("context window", "context_window", "1706.03762"),
            (
                "context compaction",
                "context_compaction",
                "compaction-pipeline-spec",
            ),
            ("thread compaction", "context_compaction", "2026-10-18"),
        ] {
            let concept = resolve_derived(term)
                .expect("compaction-pipeline ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
        }
    }

    /// expect: [P5] The Cohort 2 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_2_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("kanban", "kanban", "Ohno"),
            ("kanban method", "kanban", "Anderson"),
            ("work in progress limit", "work_in_progress_limit", "Little"),
            ("WIP limit", "work_in_progress_limit", "Anderson"),
            ("INVEST criteria", "invest_criteria", "Wake"),
            ("invest", "invest_criteria", "2003"),
            ("Fermi decomposition", "fermi_decomposition", "Tetlock"),
            ("Fermi estimation", "fermi_decomposition", "Q1141521"),
            (
                "reference class forecasting",
                "reference_class_forecasting",
                "Kahneman",
            ),
            ("outside view", "reference_class_forecasting", "Flyvbjerg"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 2 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 3 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_3_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            (
                "critical uncertainties",
                "critical_uncertainties",
                "Schwartz",
            ),
            ("scenario axes", "critical_uncertainties", "1991"),
            (
                "early warning indicators",
                "early_warning_indicators",
                "Schwartz",
            ),
            ("scenario tripwires", "early_warning_indicators", "Chermack"),
            ("deletion test", "deletion_test", "Ousterhout"),
            ("module deletion test", "deletion_test", "2018"),
            (
                "architecture decision record",
                "architecture_decision_record",
                "Nygard",
            ),
            ("ADR", "architecture_decision_record", "2011"),
            (
                "entity relationship diagram",
                "entity_relationship_diagram",
                "Chen",
            ),
            (
                "ERD",
                "entity_relationship_diagram",
                "10.1145/320434.320440",
            ),
            ("state diagram", "state_diagram", "Harel"),
            (
                "statechart",
                "state_diagram",
                "10.1016/0167-6423(87)90035-9",
            ),
            ("topological sort", "topological_sort", "Kahn"),
            (
                "topological ordering",
                "topological_sort",
                "10.1145/368996.369025",
            ),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 3 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 4 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_4_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("adjacent possible", "adjacent_possible", "Kauffman"),
            ("earnings quality", "earnings_quality", "Sloan"),
            (
                "accruals quality",
                "earnings_quality",
                "10.1023/a:1009682117424",
            ),
            (
                "conservation of energy",
                "conservation_of_energy",
                "Schmidt",
            ),
            (
                "flow conservation",
                "conservation_of_energy",
                "10.1111/j.1530-9290.2008.00004.x",
            ),
            (
                "open information extraction",
                "open_information_extraction",
                "Banko",
            ),
            (
                "OpenIE",
                "open_information_extraction",
                "10.1609/aimag.v31i3.2305",
            ),
            (
                "coreference resolution",
                "coreference_resolution",
                "Pradhan",
            ),
            (
                "anaphora resolution",
                "coreference_resolution",
                "CoNLL-2012",
            ),
            ("value chain", "value_chain", "Porter"),
            ("commoditization", "commoditization", "Wardley"),
            ("style transfer", "style_transfer", "Reif"),
            ("stylometry", "stylometry", "Burrows"),
            ("Burrows Delta", "stylometry", "10.1093/llc/17.3.267"),
            ("cosine distance", "cosine_distance", "Salton"),
            ("cosine similarity", "cosine_distance", "1975"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 4 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 5 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_5_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("cash conversion cycle", "cash_conversion_cycle", "Richards"),
            ("CCC", "cash_conversion_cycle", "1980"),
            ("audit evidence", "audit_evidence", "ISA 500"),
            ("vouching and tracing", "audit_evidence", "IAASB"),
            ("materiality", "materiality", "ISA 320"),
            ("vertical slice", "vertical_slice", "Hunt & Thomas"),
            ("compensation masking", "compensation_masking", "Keeney"),
            (
                "good regulator theorem",
                "good_regulator_theorem",
                "Conant & Ashby",
            ),
            (
                "good regulator",
                "good_regulator_theorem",
                "10.1080/00207727008920220",
            ),
            ("counterfactual", "counterfactual", "Halpern & Pearl"),
            ("do calculus", "do_calculus", "Pearl"),
            ("do-operator", "do_calculus", "1995"),
            ("cognitive load", "cognitive_load", "Sweller"),
            ("change amplification", "change_amplification", "Ousterhout"),
            ("exploratory testing", "exploratory_testing", "Hendrickson"),
            ("HTSM", "exploratory_testing", "Bach"),
            ("test oracle", "test_oracle", "Barr"),
            ("oracle problem", "test_oracle", "10.1109/tse.2014.2372785"),
            ("mutation testing", "mutation_testing", "Jia & Harman"),
            (
                "surviving mutant",
                "mutation_testing",
                "10.1109/tse.2010.62",
            ),
            ("bug taxonomy", "bug_taxonomy", "Beizer"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 5 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 6 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_6_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            (
                "Curry-Howard correspondence",
                "curry_howard_correspondence",
                "Howard",
            ),
            (
                "propositions as types",
                "curry_howard_correspondence",
                "1980",
            ),
            ("test suite reduction", "test_suite_reduction", "Harrold"),
            (
                "regression test selection",
                "test_suite_reduction",
                "10.1145/152388.152391",
            ),
            ("portability", "portability", "25010"),
            ("Socratic method", "socratic_method", "Paul & Elder"),
            ("Socratic questioning", "socratic_method", "2006"),
            ("premortem", "premortem", "Klein"),
            ("prospective hindsight", "premortem", "Mitchell"),
            ("is-ought problem", "is_ought_problem", "Hume"),
            ("Hume's guillotine", "is_ought_problem", "1739"),
            (
                "Optimality Theory",
                "optimality_theory",
                "Prince & Smolensky",
            ),
            ("OT ranking", "optimality_theory", "10.1002/9780470759400"),
            ("epistemic modality", "epistemic_modality", "Palmer"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 6 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 7 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_7_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("rank reversal", "rank_reversal", "Triantaphyllou"),
            ("AHP rank reversal", "rank_reversal", "10.1002/mcda.284"),
            (
                "catastrophic forgetting",
                "catastrophic_forgetting",
                "McCloskey & Cohen",
            ),
            (
                "catastrophic interference",
                "catastrophic_forgetting",
                "2405.09673",
            ),
            ("dependent type", "dependent_type", "de Bruijn"),
            ("dependent types", "dependent_type", "10.1007/bfb0060623"),
            ("proof irrelevance", "proof_irrelevance", "Carneiro"),
            (
                "type erasure",
                "type_erasure",
                "10.1007/978-3-030-79876-5_37",
            ),
            ("proof erasure", "type_erasure", "Carneiro"),
            ("vouching", "audit_evidence", "ISA 500"),
            ("tracing", "audit_evidence", "ISA 500"),
            ("delta debugging", "delta_debugging", "Zeller & Hildebrandt"),
            ("ddmin", "delta_debugging", "10.1109/32.988498"),
            ("git bisect", "git_bisect", "10.1145/318774.318946"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 7 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 8 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_8_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("scope creep", "scope_creep", "PMBOK"),
            ("feature creep", "scope_creep", "Karpathy"),
            ("YAGNI", "yagni", "Beck"),
            ("you aren't gonna need it", "yagni", "Jeffries"),
            ("cognitive dissonance", "cognitive_dissonance", "Festinger"),
            (
                "dissonance resolution",
                "cognitive_dissonance",
                "10.1515/9781503620766",
            ),
            ("reification", "reification", "Wenger"),
            ("homeostasis", "homeostasis", "Cannon"),
            (
                "homeostatic regulation",
                "homeostasis",
                "10.1152/physrev.1929.9.3.399",
            ),
            ("negative feedback", "cybernetic_feedback_loop", "Wiener"),
            ("typestate", "typestate", "Strom & Yemini"),
            ("typestate pattern", "typestate", "10.1109/tse.1986.6312929"),
            (
                "zero-cost abstraction",
                "zero_cost_abstraction",
                "Stroustrup",
            ),
            ("algebraic data type", "algebraic_data_type", "Jung"),
            (
                "algebraic data types",
                "algebraic_data_type",
                "10.1145/3158154",
            ),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 8 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-"),
                "{term} cites the ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 9 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_9_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("null hypothesis", "null_hypothesis", "Fisher"),
            ("H0", "null_hypothesis", "Neyman & Pearson"),
            (
                "non-inferiority margin",
                "non_inferiority_margin",
                "Piaggio",
            ),
            (
                "equivalence margin",
                "non_inferiority_margin",
                "10.1001/jama.2012.87802",
            ),
            ("working memory", "working_memory", "Baddeley"),
            (
                "working memory capacity",
                "working_memory",
                "10.1037/h0043158",
            ),
            ("executive function", "executive_function", "Barkley"),
            (
                "executive functions",
                "executive_function",
                "10.1037/0033-2909.121.1.65",
            ),
            ("test-driven development", "test_driven_development", "Beck"),
            ("TDD", "test_driven_development", "2002"),
            ("test double", "test_double", "Meszaros"),
            ("boundary fake", "test_double", "2007"),
            (
                "Agent2Agent protocol",
                "agent_to_agent_protocol",
                "agent2agent.info",
            ),
            ("A2A protocol", "agent_to_agent_protocol", "A2A"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 9 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 10 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_10_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("Bloom's taxonomy", "bloom_taxonomy", "Krathwohl"),
            (
                "taxonomy of educational objectives",
                "bloom_taxonomy",
                "10.1207/s15430421tip4104_2",
            ),
            (
                "zone of proximal development",
                "zone_of_proximal_development",
                "Vygotsky",
            ),
            ("ZPD", "zone_of_proximal_development", "1978"),
            (
                "retrieval-augmented generation",
                "retrieval_augmented_generation",
                "Lewis",
            ),
            ("RAG", "retrieval_augmented_generation", "2005.11401"),
            ("semiotics", "semiotics", "Morris"),
            ("syntax and semantics", "semiotics", "Carnap"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 10 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 11 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_11_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("Occam's razor", "occam_razor", "Solomonoff"),
            (
                "minimum description length",
                "occam_razor",
                "10.1016/0005-1098(78)90005-5",
            ),
            ("edit decision list", "edit_decision_list", "Berthouzoz"),
            ("EDL", "edit_decision_list", "10.1145/2185520.2185563"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 11 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 12 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_12_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            ("frame budget", "frame_budget", "VSync"),
            ("frame rate", "frame_budget", "120 Hz"),
            (
                "instructions per cycle",
                "instructions_per_cycle",
                "Hennessy & Patterson",
            ),
            ("IPC", "instructions_per_cycle", "10.1145/1297027.1297033"),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 12 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    /// expect: [P5] The Cohort 13 skill-corpus audit rulings (2026-10-14)
    /// resolve to their published authorities and never fall back to the
    /// coarse 5W1H core anchor.
    #[test]
    fn cohort_13_audit_rulings_resolve_with_authority() {
        for (term, expected_term, marker) in [
            (
                "equity duration",
                "equity_duration",
                "Dechow, Sloan & Soliman",
            ),
            (
                "Macaulay equity duration",
                "equity_duration",
                "10.1023/b:rast.0000028186.44328.3f",
            ),
            ("cash flow duration", "equity_duration", "Macaulay"),
            ("log-odds", "log_odds", "Berkson"),
            ("logit", "log_odds", "10.2307/2280041"),
            (
                "log-odds pooling",
                "log_odds",
                "10.1016/j.ijforecast.2013.09.009",
            ),
        ] {
            let concept =
                resolve_derived(term).expect("Cohort 13 audit ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-14"),
                "{term} cites the 2026-10-14 ruling"
            );
        }
    }

    #[test]
    fn tokenization_ruling_resolves_with_authority() {
        for (term, expected_term, marker) in [
            ("tokenization", "tokenization", "Jurafsky"),
            ("tokenizer", "tokenization", "ffbf5147"),
            ("tokenisation", "tokenization", "2026-10-23"),
        ] {
            let concept =
                resolve_derived(term).expect("tokenization ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-23"),
                "{term} cites the 2026-10-23 ruling"
            );
        }
    }

    /// expect: [P5] The 2026-10-24 security-review rulings resolve both
    /// coarse terms as derived concepts citing their published authorities,
    /// and every constituent resolves through the ladder — a derived entry
    /// never fabricates a constituent anchor.
    #[test]
    fn security_review_rulings_resolve_with_published_authority() {
        for (term, expected_term, marker) in [
            (
                "security review",
                "security_review",
                "OWASP Code Review Guide",
            ),
            ("secure code review", "security_review", "Cheat Sheet"),
            ("defense in depth", "defense_in_depth", "CNSSI 4009"),
            ("defence in depth", "defense_in_depth", "NIST SP 800-39"),
        ] {
            let concept = resolve_derived(term)
                .expect("the security-review ruling must resolve in derived.rs");
            assert_eq!(
                concept.term, expected_term,
                "{term} resolves to {expected_term}"
            );
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.starts_with("operator ruling 2026-10-24"),
                "{term} cites the 2026-10-24 ruling"
            );
        }
        // The constituents each resolve through the ladder — never fabricated.
        assert!(
            resolve_derived("code review").is_some(),
            "code review resolves to fagan_inspection"
        );
        assert!(
            resolve_derived("security vulnerability").is_some(),
            "security vulnerability resolves to the 2026-10-18 ruling"
        );
        let resolved = crate::term_resolution::resolve_term("information security");
        assert_eq!(
            resolved.concept, "cso:information_security",
            "information security resolves to the CSO domain supplement"
        );
    }
}
