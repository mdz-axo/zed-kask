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
//! financial-analysis vocabulary — resolved to a void because FIBO
//! publishes no ratio terms, and a pre-interest operating formula wore
//! the label through three operator corrections).

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
    /// the build (same discipline as the FIBO fixture).
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
        aliases: &["feedback loop"],
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
        term: "maia_listening",
        aliases: &["maia listening", "maia v3 listening", "maia method"],
        identity: "an earnings call read through a stance block and seven sections, keeping only claims linked to a strategic path, each cited verbatim from the transcript",
        definition: "The MAIA method's earnings-call listening template (v3): seven sections (margin trajectory, working capital power, moat evidence, capital allocation, expectations gap, guidance versus expectations, management consistency), a horizon model centered on the 12-36 month seam between tactical checkpoints and strategic goals, and a retrieve-cite-verify discipline so every evidence quote is a substring of the source.",
        constituents: &["strategic checkpoint", "horizon model", "verbatim evidence"],
        authority: "operator ruling 2026-09-25 (the MAIA method is the reference model); MAIA v3 listening template, kask/registry/templates/listening/apply-template.j2",
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
];

/// Resolve a term (or alias) against the derived registry.
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
        ] {
            let concept = resolve_derived(term).expect("reference model is defined");
            assert!(
                concept.authority.contains(marker),
                "{term}: {}",
                concept.authority
            );
            assert!(
                concept.authority.contains("2026-09-25"),
                "{term} cites the ruling"
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
}
