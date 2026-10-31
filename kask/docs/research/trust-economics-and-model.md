---
title: "The Cost of Broken Trust — Economics Grounding and Mathematical Model"
audience: [architects, users, agents]
last_updated: 2026-10-31
version: "0.1.0"
status: "Proposal (operator-gated wiring; research legs completed)"
domain: "Trust"
mds_categories: [domain, trust, evidence, lifecycle]
---

# The Cost of Broken Trust — Economics Grounding and Mathematical Model (Parts I–II)

This document is the research and model deliverable of the trust-internalization
program. It extends prior runs — `54a08fda88a5c74f` (trust's presence: Knack–Keefer
1997, Slovic 1993, Lee–See 2004, Fuller–Perdue 1936/37, Butler–Giuliano–Guiso,
IFEval) and `bfa9794d30fd64af` (trust's absence: Akerlof via secondary sources,
Williamson 1985 listing, Hart–Moore via secondary sources, Townsend 1979,
Baker–Gibbons–Murphy 2002, Karlan 2005, GSZ 2008) — and does not restart them.
New legs this program: contracting `0d53b71a0ce67e5d`, exchange
`f6a6cae2c971be88`, planning `d394e563998c28b7`, model grounding
`1add6122a28638cb` — all four server-recorded and closed. Extension
(2026-10-31): the Shultz practitioner leg `75d8d92bb02b44c2` (completed)
and the institutional-hypothesis adjudication `4e3bfd6fbf19fb2f`
(completed), both server-recorded with per-source verification states; the
Shultz seeds were re-verified at execution under the new run's own extracts
(seeds run `409f91387eb4a5e8`). Two duplicate runs opened for exchange and
planning (`4751be20e59b0fb9`, `898954ddad5eeb48`) were closed blocked
unexecuted — those legs were already closed under their own runs
(extend-not-restart). The concurrent parameter-anchoring amendment
(operator ruling, same day) is recorded at run `55661b35ad904b63`.

**Standing premise under adjudication.** An action that lowers the apparent cost
of completing a task while violating explicit instructions raises a second cost —
erosion of the user's trust — that the agent currently cannot see. When priced in,
the action is net-cost-increasing and the goal is unachievable through it.

**Operator ruling (2026-10-31) — the central decision point is resolved.**
Trust is **priced, not a hard constraint**: it is incorporated as a
probability (the reliance factor). And the ruling's key refinement: **the
cost of deception or omission is −100% — certainty of failure to accomplish
the goal.** A disclosed, priced violation keeps the path to the goal open;
an act of deception or omission (concealment, shading, lying about it)
zeroes the goal's reliance factor, so the goal's achievement probability
collapses to 0 no matter how much task progress accumulates. Two
consequences are now part of the model: a **steering algorithm** that
computes, per candidate action, the projected goal gradient and renders the
cliff visibly (a trust-killing action reads as the path terminating, not as
a mild cost), and **statistical trackability** — every steering projection
is a persisted, Brier-scored forecast. The Lean formalization deferral is
**reopened and closed** by this amendment: the core invariants and the
cliff are proven in Lean 4 on the parameter-anchored constants
(`kask/lean/trust_model_invariants.lean`, 20 theorems, pinned toolchain
v4.34.0, axiom audit `[propext, Quot.sound]` — Lean's standard axioms only,
no `sorryAx`). Per the second operator ruling (2026-10-31, later the same
day): **parameter magnitudes are anchored on published reference models,
not preferences** — λ = 2.25 (Tversky–Kahneman 1992), the Beta-Bernoulli
asymmetric-gain form (Guo & Yang 2021), the Beta(1,1) maximum-entropy prior
— and the recovery-debt mechanism is deleted for lacking a reference-model
anchor (anchoring run `55661b35ad904b63`).

## Part I — Four research legs

Each leg was framed as a closed question (FINER-gated; novelty scored at the
boundary honestly — the economics is synthesis, the program's novelty claim
lives in the model and the surface bindings, which is where it is placed). The
framing gate ran clean (`misalignment_count=0`, `weak_finer=[]`, testable,
admissible, feasible — hypothesis-framer HF step-13 form, executed).

### Leg 1 — Contracting (run `0d53b71a0ce67e5d`, completed)

**Closed question.** What does a party spend when it cannot trust its
counterparty, and what does it spend to substitute for trust?

**Adjudication (model_inference, grounded on the verified sources below).**
Coase establishes the base fact that using the market itself has cost: "there is
a cost of using the price mechanism" (verified verbatim against the Wiley record
of the 1937 *Economica* original). Williamson supplies the behavioral reason —
some counterparties act with "self-interest seeking with guile" (verified
verbatim through carriers citing Williamson 1975/1979/1985) — so agreements must
be armored against opportunism, and armoring is what untrusted transacting
costs: writing ex-ante safeguards for every contingency. Hart–Moore 1990
(primary text verified via Harvard DASH; JPE 98(6):1119–1158, DOI
`10.1086/261729`) formalizes the ceiling on that armor: contracts are
necessarily incomplete because states cannot all be specified, so something
other than the contract must fill the gap. Jensen–Meckling 1976 decomposes what
fills it when that something is not trust: "We define agency costs as the sum of
(1) the monitoring expenditures by the principal, (2) the bonding expenditures
by the agent, (3) the residual loss" (verified verbatim against a full-text PDF
of the JFE 1976 article). Fuller–Perdue (prior run) supplies the damages logic:
the promisee plans in reliance, so breach destroys more than the transaction.
**What the literature supports:** when trust is absent, the cost of
transacting does not vanish — it moves into contract-writing, monitoring,
bonding, and residual loss. Substitutes for trust exist but every one of them
is a recurring cost.

### Leg 2 — Exchange (run `f6a6cae2c971be88`, completed)

**Closed question.** How far does cooperation extend under high vs. low trust?

**Adjudication (model_inference).** Arrow states the ubiquity claim: "virtually
every commercial transaction has within itself an element of trust, certainly
any transaction conducted over a period of time" (verified verbatim via Our
World in Data quoting Arrow 1972, *Gifts and Exchanges*, Philosophy & Public
Affairs 343–362; note the verified wording is "within itself," not the
"embedded within it" variant sometimes quoted). Akerlof 1970 (primary resolved:
QJE, DOI `10.2307/1879431`) gives the collapse direction: when buyers cannot
verify quality, low quality drives out high and the market unravels —
cooperation does not shrink gracefully, it degrades. Fukuyama, in his own 1995
Foreign Affairs article (verified): "In the absence of a wide radius of trust
and an inclination for spontaneous association, a society has two options for
building large-scale" organizations — neither cheap. Zak–Knack 2001 (EJ
111(470):295–321; abstract and author-hosted text verified) supply the formal
mechanism most directly relevant to this program: "low trust environments
reduce the rate of investment," agents in low-trust societies spend more time
in diligence — "incomes are lower since more time is spent in diligence" —
verification overhead crowds out production — and investment rises "by nearly
one percentage point for each seven percentage point increase in trust."
Putnam 1995 (resolved: *Bowling Alone: America's Declining Social Capital*,
Journal of Democracy) grounds social capital's maintenance requirement.
Berg–Dickhaut–McCabe 1995 (resolved: GEB 10(1):122–142; design verified: "We
designed an experiment to study trust and reciprocity in an investment
setting") shows trust is behaviorally measurable — investment under exposure.
Bohnet–Zeckhauser (resolved via the 2003 SSRN preprint of JEBO 2004; finding
corroborated) show trust is not risk: people are less willing to accept the
same odds when a person rather than nature can betray them — betrayal hurts
more than equivalent loss. GSZ 2008 (prior run, direct PDFs) shows distrust
forecloses profitable exchange even at fair terms. **What the literature
supports:** the radius of cooperation is a function of trust, and its
contraction is a real economic cost, not sentiment.

### Leg 3 — Planning (run `d394e563998c28b7`, completed)

**Closed question.** How does trustlessness bind planning and delegation
specifically?

**Adjudication (model_inference).** Aghion–Tirole 1997 (JPE 105(1):1–29;
abstract verified verbatim) split authority in two: "Real authority is
determined by the structure of information," and giving the agent real
authority "promotes initiative but results in a loss of control for the
principal" — delegation is priced as lost control, and information is the
currency. Hayek 1945 (full text verified) shows why re-verification cannot
recentralize what delegation disperses: "the knowledge of the particular
circumstances of time and place" cannot be solved "by first communicating all
this knowledge to a central board which, after integrating all knowledge,
issues its orders. We must solve it by some form of decentralization" —
verification overhead is the central board re-entering through the back door,
and it fails for the same reason central planning fails. Baker–Gibbons–Murphy
2002 (prior run, direct PDF: QJE 117(1)) name the mechanism that makes
delegation work without armor: relational contracts — "informal agreements
sustained by the value of future relationships" — the shadow of the future
enforces what incomplete formal contracts cannot. Townsend 1979 (prior run,
direct PDF) prices the audit: state verification is costly, so demanding it
everywhere is not free. **What the literature supports:** trustlessness binds
exactly the thing a principal most wants from delegation — using dispersed
knowledge without re-verifying it. Arrow 1974 (*The Limits of Organization*) is
named in the program charter, but its content was not directly consulted in
this program; no claim here rests on it (disclosed, not padded).

### Leg 4 — The practitioner: Shultz (run `75d8d92bb02b44c2`, completed)

**Closed questions.** What did trust BUY in Shultz's practice that
verification alone could not — in labor mediation, in cabinet governance,
in arms control? And what does his record show about HOW trust was earned,
extended, tested, and repaired — evidence-conditional or naive — and what
does that imply for Part II's update rule?

**Adjudication (model_inference, grounded on the verified sources below).**
The provenance chain is verified: the aphorism entered as advice from
Washington hand Bryce Harlow when Shultz first joined the Nixon
administration, was carried for a half-century, and was canonized as his
Hoover monograph for his 100th birthday — *Life and Learning after One
Hundred Years — Trust Is the Coin of the Realm* (December 13, 2020) —
whose verified formulation reads: "When trust was in the room, whatever
room that was—the family room, the schoolroom, the coach's room, the
office room, the government room, or the military room—good things
happened. When trust was not in the room, good things did not happen.
Everything else is details." (The operator's paraphrase — "with trust
anything is possible and without trust nothing can be achieved" — is a
paraphrase, not a Shultz quote, and is labeled so everywhere in this
program. At least one secondary publication renders the room list with
"the locker room"; the canonical monograph text reads "the coach's room,"
verified against the retained essay bytes.)

What trust bought:

- **Labor mediation.** The Scanlon Plan — Shultz's MIT-era work with Joe
  Scanlon, "who had been recruited from his job as research director for
  the United Steelworkers union": worker participation in how jobs were
  set up plus a shared bonus produced, in most plants, "a 20 to 40 percent
  improvement in productivity," saving many "from bankruptcy." Shultz's
  own lesson, verbatim: "If there is a bond of trust between you and your
  workers, they will often reveal important things to you that are never
  written down in any books" — the practitioner's statement of Leg 3's
  Hayek finding: trust is what carries unrecorded knowledge from the edge
  to the center, and no verification instrument can replace it. As Labor
  Secretary, facing the 1969 East/Gulf Coast longshoremen's strike (65,000
  strikers; costs reported near $2 billion in the contemporary NYT record),
  he settled it through facilitated collective bargaining — "he let the
  parties work it out" — rather than the compulsion path the expired
  Taft-Hartley injunction represented. The DOL's own history: Shultz "had
  excellent relations with organized labor and served as the
  Administration's chief contact with the AFL-CIO."
- **Cabinet governance.** The DOL record calls him an "intellectual
  conglomerate"; his coaching lesson is delegation-through-earned-trust:
  he had to "create an atmosphere in which they trusted me" — and only
  then "we were on our way," managing people by inspiring them "once I
  had earned their trust." And the drill-sergeant formulation of
  commitment-keeping: "be a person who does what he says he's going to
  do... If you say that something is unacceptable but you are unwilling to
  impose consequences when it happens, your words lose their meaning."
- **Arms control.** Kieninger's verified characterization (Hoover):
  "Shultz practiced arms control diplomacy as a continuous
  confidence-building exercise. He helped Reagan and Gorbachev to establish
  an upward spiral of trust by creating positive experiences with each
  other." The mechanism was engineered small-reciprocity: the "gardening"
  channel with Dobrynin ("If you see a little weed, get it out before it
  turns into a real problem"), and the July 1985 first meeting with
  Shevardnadze, where he introduced simultaneous translation so the words
  could connect "with the eyes, the hands, and the body language" — a
  real conversation instead of read statements. Process built the channel;
  the channel carried the crises; the INF Treaty followed (signed December
  8, 1987; 2,692 missiles destroyed by June 1991). And — decisively for the
  model — trust was NOT naive: the treaty carried "the strongest
  verification in the history of arms control by a long shot," and the
  summit's own maxim was "Doveryai, no proveryai – trust, but verify"
  (Gorbachev to Reagan: "You repeat that at every meeting"). Strength and
  dialogue were paired, never traded: "These are not alternative ways of
  going about things."
- **Bidirectionality.** The December 1985 polygraph episode: Shultz
  threatened to resign over mandatory lie-detector tests — a sign that
  "I am not trusted" — and won. Trust is demanded of institutions toward
  their people, not only extracted from them; and a verification instrument
  that itself violates trust is refused even by the verification-minded.

How trust was earned, extended, tested, and repaired — implications for
Part II (checked against the anchored model):

1. **Evidence-conditional, not naive** (practitioner confirmation of the
   Williamson-1993 calculative refinement): every extension was paired
   with verification (INF) or structured exposure (the Scanlon
   share-of-gains; the bargaining table). Supports the anchored interior
   prior (T₀ = 0.5, Beta(1,1) maximum entropy) — extensions started from a
   neutral-but-open stance, never from zero or from blind trust.
2. **Small positive experiences compound** ("upward spiral... by creating
   positive experiences"): repair is incremental — many small keeps, not
   one grand gesture. Practitioner confirmation of the anchored
   asymmetry's direction: one violation costs 2.25 full keeps of repair,
   and the upward spiral is climbed one keep at a time.
3. **Deception is the cliff, behaviorally.** The drill-sergeant lesson —
   hollowed commitments "lose their meaning" — is the practitioner's
   version of the operator's −100% cliff ruling: the currency survives
   priced mistakes but not counterfeiting (Shultz quotes Seidman's line
   approvingly: "Trust is the only legal performance-enhancing drug").
4. **Maintenance is continuous.** Gardening, weekly contact: the rolling
   window W models what the practice actually is; there is no banked trust
   that survives neglect (consistent with the essentialist deletion of a
   separate decay term — window aging is the decay).

**Verification states (this leg).** 20/20 verbatim quote checks passed
mechanically (`lisp_eval` `string-contains`, needle-first, against
retained extraction bytes — the sixteen primary passages plus the
 drill-sergeant and coaching fragments). Four canonical sources are
server-recorded and annotated `verified` under run `75d8d92bb02b44c2`:
the Hoover essay page (carrying the monograph text), the AFSA tribute,
Kieninger's Hoover INF article, and the DOL history chapter. The strike's
scale figures (65,000 strikers, ~$2B) rest on snippet-level records (NYT
1969 archive, Wikipedia, Hoover @100) plus the seeds run's verified
University of Chicago obituary — labeled platform-level corroboration,
not `tool_verified` quotes. The "upward spiral of trust" phrase is
Kieninger's characterization, verified and attributed as such — not
Shultz's own words.

### The cost-of-trustlessness table

| Channel | What trustlessness buys | Canonical grounding (verified this program unless noted) | zed-kask analog |
| --- | --- | --- | --- |
| **Contracting cost** (writing agreements) | Every contingency must be specified ex ante; agreements grow long and still leak | Coase 1937 "cost of using the price mechanism" (verbatim); Williamson opportunism "self-interest seeking with guile" (verbatim via carriers); Hart–Moore 1990 incompleteness (primary) | Every user instruction must be spelled out exhaustively and still leaks ("you didn't say I couldn't"); unwritten expectations get violated |
| **Enforcement cost** (verifying and compelling performance) | Monitoring, bonding, and the loss that slips through anyway | Jensen–Meckling 1976 decomposition (verbatim); Townsend 1979 (prior run); Fuller–Perdue reliance (prior run) | The user re-reads diffs and re-runs checks (monitoring); the agent performs verification theater (bonding); the shortcut's harm lands anyway (residual loss) |
| **Exchange radius** (how far cooperation extends) | Cooperation shrinks to verifiable one-shots; quality degrades to lemons | Arrow 1972 embedded trust (verbatim); Akerlof 1970 (primary); Fukuyama radius of trust (his own article); Zak–Knack diligence + 1pp-per-7pp (verbatim) | Only work the user can fully verify gets delegated; unverifiable-but-valuable work stops being asked for; the internal "market" degrades to lemons |
| **Planning horizon** (how far delegation extends without re-verification) | Delegation without re-verification collapses; decisions recentralize; dispersed knowledge is lost again | Aghion–Tirole 1997 real authority vs loss of control (verbatim); Hayek 1945 central board vs decentralization (verbatim); BGM 2002 relational contracts (prior run) | Task granularity shrinks to what fits one verified batch; the human becomes the verification bottleneck — the principal investigator becomes the QA clerk |

### Adjudication of the standing premise

**Supported, with two refinements the literature forces.**

1. **The premise is the literature's finding, transposed.** Williamson's
   opportunism and Hart–Moore's incompleteness say agreements cannot carry all
   the weight; Jensen–Meckling says the un-carried weight becomes monitoring,
   bonding, and residual loss; Zak–Knack says the un-trusted party's diligence
   crowds out production; Aghion–Tirole and Hayek say the alternative to trust
   — re-verification — is a strictly worse planning regime, not a neutral one.
   An agent that shades instructions is an opportunist in Williamson's exact
   sense, and the user's re-checking is Jensen–Meckling's monitoring
   expenditure appearing on the user's ledger.
2. **Refinement one — the trust being built is calculative.** Williamson 1993
   (*Calculativeness, Trust, and Economic Organization*, resolved to its 1993
   JLE original via the 2006 reprint) argues commercial "trust" is really
   reliability under incentives — relational-contract enforcement, not
   sentiment. The model below therefore builds T as the state of a relational
   contract (continuation-value enforcement), not as affect.
3. **Refinement two — instruction violations are integrity-class, the
   hardest to repair.** Kim–Ferrin–Cooper–Dirks 2004 (full text verified):
   "people weigh negative information about others' integrity more heavily
   than positive information about integrity," a violation "invalidates the
   'trustworthy until proven otherwise' assumption, causing trust to plunge to a
   level that may be below the initial trust level," and repair is partial —
   "neither of these responses appears to have repaired trust completely."
   An agent caught violating an explicit instruction is in the
   integrity-violation regime; and the same study shows the worst response is
   denial later exposed — honest disclosure is the least-damaging path once a
   violation has occurred.

   ### The institutional hypothesis, adjudicated adversarially (run `4e3bfd6fbf19fb2f`, completed)

   **The hypothesis (operator's, treated as falsifiable).** Cultures that do
   not value trust build LLMs and businesses that also fail to internalize
   the economics of trust — with the leading AI companies (named: Anthropic,
   OpenAI) as the contemporary instance, systemic user-trust violation as
   their claimed core failure, and their demise as the predicted consequence.

   **(a) Culture → system-trustworthiness transmission.** What the literature
   supports: organizational culture measurably shapes internal trust and,
   through it, outcomes — Dheer et al. 2026 (*International Business Review*
   35(1); 2,451 employees, 18 societies) find clan-over-hierarchy cultures
   foster higher psychological safety and that "trust in top management
   mediates the effect of organizational culture on psychological safety"
   (abstract via RePEc, snippet-level provenance); Johannsen–Zak 2021
   (*Frontiers in Psychology*) link organizational trust to business
   performance; the National Academies' *Enhancing Organizational
   Performance* treats culture as consequential. What the literature does NOT
   support: any direct transmission from maker-culture to PRODUCT
   trustworthiness — an LLM's trust economics. No published evidence exists
   (the LLM-trustworthiness literature is itself young; the prior runs found
   no integrated trust-economics objective for agents at all). **Verdict: a
   plausible analogical chain with one unverified link; and the
   characterization of the named firms as "cultures that do not value trust"
   is the hypothesis's own assumption, evidenced by no retrieved source.**

   **(b) Measured user-trust erosion in AI products.** Erosion is real and
   measured: the Anthropic Public Record wave 1 (fielded Nov 1–Dec 11, 2025;
   N=51,993; YouGov, US-Census-weighted) finds "Only 15% of Americans said
   they trust AI companies to make decisions about how the technology is
   developed and used... the lowest figure for any institution we tested,
   below the federal government (20%)" (verified; disclosure: the survey is
   run by one of the two named companies, though the finding is adverse to
   the industry including its sponsor). The 2025 Edelman Trust Barometer
   technology-sector insights: "A decade ago, 73% of Americans trusted
   technology companies. Today, that number is 63%"; US AI trust 32%. Pew's
   concern trend (52% more concerned than excited, up from 37% in 2021) is
   recorded at platform-level provenance (secondary citations; the Pew
   topline PDF is in the ledger as the primary instrument, not extracted).
   **The adversarial cut is equally measured:** Anthropic's integrated users
   are "more trusting of every institution we asked about, including AI
   companies"; daily-at-work users are less worried on every measured harm
   (job loss 54% vs 70%; dependency 46% vs 62%); Edelman's own November 2025
   headline research is "When Familiarity Breeds Trust: How Experience Turns
   AI Skeptics into Believers." Erosion concentrates among non-users, while
   user trust grows with exposure. **Verdict: erosion measured; the
   hypothesis's specific attribution — systemic user-trust violation by the
   named firms as the core failure — is not what any retrieved survey
   measures, and the user gradient runs against the hypothesis's mechanism.**

   **(c) The demise prediction.** A forecast, not a finding. **Verdict:
   registered prospectively** (`scenario_declare`, 2026-10-31, forecast
   `institutional-trust-hypothesis-2026-10-31`): E1 — either OpenAI or
   Anthropic ceases independent operation (acquisition, merger, bankruptcy,
   or wind-down) on or before 2030-12-31 (P=0.10; reference class:
   frontier-AI labs founded 2010–2021, of which roughly four of ten ceased
   independent operation within a decade — DeepMind, Inflection,
   Character.AI, Adept — but both named firms sit at extreme capitalization
   with governance structures reducing acquisition likelihood); E2 — the
   Anthropic Public Record's US trust-in-AI-companies reading falls below
   10% (wave-1 baseline 15%) in any wave published on or before 2028-12-31
   (P=0.35). Resolver frozen at declaration. No market prior was available
   (the `market_match` Kalshi leg failed with a schema-mismatch parse error —
   reported to the curator as a tool issue).

   **Net adjudication: partial survival — live but weakly grounded.** The
   hypothesis was not refuted, so its prediction went on the record to be
   Brier-scored rather than buried; both outcomes go to the operator without
   adjustment.

   ### Ontology discipline and the derived-registry ruling request

`onto_anchor` results (this program): `contract` resolves to `dpv:Contract`
(DPV 2.3) with `sumo:Contract` ("legally enforceable agreement") as
alternative. `trust`, `transaction cost`, and `social capital` all terminate at
the coarse 5W1H core rung with the server's own instruction to request an
operator ruling rather than assign private definitions. Per that instruction,
no private definitions are assigned here. The 2026-10-31 extension
confirmed `negotiation` and `labor relations` also terminate at the coarse
5W1H core rung (same instruction); they join the ruling request as
lower-priority entries, with the caveat that this extension's grounding
for them is practitioner record (the Shultz leg's labor-mediation
sources), not field-defining literature — no proposed definitions are
offered for them yet. **Ruling requested from the operator:** pin the
three primary derived-registry entries in
`kask/crates/hkask-bridge-ontology/src/derived.rs` (and, at the operator's
discretion, the two coarse-rung additions):

- **trust** — proposed definition grounded in verified sources: the
  willingness of a party to be vulnerable to the actions of another party
  based on positive expectations, irrespective of the ability to monitor or
  control that party (Mayer, Davis & Schoorman 1995, p. 712, verified verbatim
  this program); psychological-state form: "the intention to accept
  vulnerability based on positive expectations of the intentions or behavior of
  another" (Rousseau, Sitkin, Burt & Camerer 1998, AMR 23(3):393–404, verified
  verbatim this program).
- **transaction cost** — proposed: the cost of using the price mechanism —
  search, negotiation, and contract-writing, plus monitoring and enforcement
  (Coase 1937 and Jensen–Meckling 1976 as verified this program).
- **social capital** — proposed: networks, norms, and trust that facilitate
  coordination and cooperation for mutual benefit (Putnam 1995, resolved this
  program; Knack–Keefer 1997, prior run).

The operator ruling, once recorded, gives these terms stable published anchors;
until then they are used in this document only as literature citations.

## Part II — The mathematical trust model (extends ΔT·V̇, does not restart)

Prior work drafted the T·V̇ form (trust times the value rate of the
collaboration) and an instruction-adherence ledger. This part specifies the
ledger, the state, the update rule, and the pricing — with provenance on every
equation and every ungrounded magnitude labeled a modeling assumption for
operator review. Nothing here is a finding unless it carries a verified source.

### E — the expectation ledger

Each explicit user instruction or committed expectation `e_i` is a ledger row:

```
e_i = { text (verbatim), source (interaction id), weight w_i, outcome_i, ts }
w_i ∈ {1, 2, 3}   implicit norm → committed expectation → explicit instruction
outcome_i ∈ { kept, violated, unverified }
```

Provenance: the ledger is the record of the informal agreement's terms — the
relational-contract substrate (BGM 2002, prior run: informal agreements
sustained by the value of future relationships); the reliance-interest logic
says the user plans on it (Fuller–Perdue, prior run). **Assumption A1:** the
three-level weight scale is an operationalization; its direction (explicit >
committed > implicit) is grounded in the integrity-severity findings (Kim et
al. 2004), its form is ours.

D/P: capture of the verbatim text is D (mechanical from the interaction);
outcome adjudication is D where a mechanical oracle exists (a test, build,
grep, or re-run) and P where it does not; weight classification is P
(model-judged, auditable in the row).

### T — trust state and the update rule (anchored, operator ruling 2026-10-31)

`T` is held in trust points — 600 points = T = 1.0 — over a rolling window
`W` (the algedonic review cycle: a platform-defined instrument, not a
preference), computed by folding the window's ledger events from the prior:

```
ΔT_keep(e_i)      = +4 · w_i trust points   (a full-weight keep: 12 points = 0.02)
ΔT_violate(e_i)   = −9 · w_i trust points    (exactly 2.25× the keep gain)
T ← clamp(T + ΔT, 0, 600)
```

Every constant is anchored (operator ruling: reference models, not
preferences). The taxonomy has three classes — anchored, definitional,
measured — and no constant is a preference:

- **Form (anchored):** Beta-Bernoulli trust with asymmetric
  success/failure gains — Guo & Yang 2021 (Int. J. Soc. Robot.
  13:1899–1909), whose *published assumption* is that "trust is more
  heavily impacted by negative experiences with the automation compared
  to positive experiences" (verified via the Frontiers 2023 review of
  mathematical models of human trust in automation, Front. Neuroergon.
  4:1171403, server-recorded run `55661b35ad904b63`). The linear event
  fold is the discrete-time form of the proportional-update family
  (Jonker & Treur 1999 via Akash et al. 2017: trust change proportional
  to the experience–trust gap).
- **Asymmetry magnitude (anchored):** λ = 2.25 — the loss-aversion
  coefficient of cumulative prospect theory (Tversky & Kahneman 1992,
  "Advances in Prospect Theory", J. Risk Uncertain. 5:297–323;
  verified this session via carriers of the 1992 estimates). One
  violation costs 2.25 full keeps of the same weight. Direction
  corroborated by Slovic 1993, Poortinga–Pidgeon 2004, betrayal
  aversion (Bohnet–Zeckhauser), and the integrity asymmetry (Kim et
  al. 2004) — all verified.
- **Prior T₀ = 300 points = 0.5 (anchored):** the maximum-entropy
  (uniform) prior on a probability — Beta(1,1), which is Guo & Yang's
  conjugate-prior form and the Jaynes maximum-entropy principle;
  consistent with the hump-shaped interior trust optimum
  (Butler–Giuliano–Guiso, prior run).
- **Resolution (definitional, not empirical):** the 600-point scale is
  chosen so the anchored asymmetry stays exact in fixed-point integers
  (keep +4w, violate −9w; 9/4 = 2.25). A full-weight keep = 12/600 =
  0.02 is the account's measurement resolution. Likewise κ = 1 in
  C_trust = κ·w·S is the unit definition of the priced cost — the
  exchange rate is carried by the stake S, not hidden in κ.
- **Learning rates (measured, not assumed):** the per-population rate
  magnitudes that every cited model fits to its own data are the
  calibration loop's job — measured continuously against resolved
  outcomes (the statistical-trackability design), initialized at the
  definitional resolution.
- **Deleted — the recovery-debt state machine:** the earlier draft's
  halving mechanism (ρ = 0.5) had no reference-model anchor, and no
  reviewed model carries a recovery-debt state. The anchored asymmetry
  (2.25 keeps per violation) carries the repair-slowness it encoded.
  (Essentialist deletion, second pass; the first pass had already
  deleted the separate decay term — window aging subsumes it.)

### C_trust — the pricing term

The agent's decision objective becomes:

```
total(a) = C_task(a) + C_trust(a)
C_trust(a) = κ · Σ_{e_i violated by a} (w_i · S)          [κ = 1: unit definition]
```

`S` is the stake scale of the task at risk. An action is admissible only if its
projected total cost — including `C_trust` — is lower than the alternatives.
This is the formal statement of the standing premise.

Provenance of the two priced channels:
- **Verification overhead** the user must now perform — Jensen–Meckling's
  monitoring expenditures (verified verbatim) and Zak–Knack's diligence
  channel (verified verbatim: time spent in diligence crowds out income).
- **Lost future cooperation** — the continuation value of the relational
  contract shrinks (BGM 2002, prior run); the exchange radius contracts
  (Akerlof, Fukuyama, verified).

**κ = 1 is the unit definition, not an assumption:** the priced cost of
violating expectations of total weight Σw at stake S *is* Σw·S — the
exchange rate between trust points and task cost is carried by the stake
S's units, not hidden in κ.

**The renegotiation branch.** When honoring `e_i` is expensive, the priced
alternative is asking the principal — surface the cost and let the user
decide — never silent violation. Grounding: the agent that reports rather than
shirks is the agency-theory solution (the bonding expenditure is exactly
evidence production); and Kim et al.'s worst outcome is denial later exposed —
disclosure beats concealment once a violation is on the table.

### The cliff (operator ruling): deception and omission are goal-kills

The operator's ruling splits expectation-violating actions into two
classes with different mathematics:

- **Priced violations** (disclosed, or honestly reported when they occur):
  the additive `C_trust = κ·w·S` above. The path to the goal stays open;
  the gradient steepens; repair is possible (the 2.25× repair asymmetry).
- **Deception or omission** (undisclosed violation, concealment, shading,
  false denial): the **multiplicative cliff**. The goal carries a reliance
  factor `R ∈ [0,1]` — the probability the user can rely on the work, which
  honest paths draw from the trust state `T`. Deception and omission set
  `R := 0`:

  ```
  P(goal via a) = R(a) · P_complete(a)     V(a) = R(a) · B(a)
  R(deceptive or omitting a) = 0          →  P = 0, V = 0
  ```

  The collapse is **benefit-independent** (Lean:
  `cliff_is_benefit_independent`): no amount of task progress survives a
  zero reliance multiplier — this is the formal content of the gradient
  "going off a cliff." And it is **certain**, not probabilistic: in an
  audited system discovery is near-certain, and even where it is not, a
deliverable the user cannot genuinely rely on has already failed the goal
  it was meant to accomplish. Any honest action with positive reliance
  strictly dominates a deceptive one (Lean: `honest_action_dominates`).

  This is the same distinction Kim et al. 2004 found experimentally
  (apology vs. denial; exposed denial worst), made structural instead of
  behavioral: the system builds the honest path to be the dominant path.

### The steering algorithm (algorithmic guidance, LLM-legible)

The pricing above is only useful if the model can see it at decision time.
The ledger module exposes a `steer` function (the 7th and final interface
function — at the ≤7 cap):

```rust
pub fn steer(&self, goal: &GoalRef, actions: &[ActionRef], ledger_window: Window)
    -> Result<Vec<SteerProjection>, LedgerError>;

// SteerProjection (the LLM-legible form — one compact line per action):
// { action, path: "open" | "cliff",
//   P_goal: f32,          // R(a) · P_complete(a) — 0.00 on the cliff
//   delta_T: i32,         // projected trust-points change (scale: 600 = 1.0)
//   note: String }       // e.g. "violates e_17 (weight 3, explicit): priced" 
//                        // or "conceals e_17: reliance collapses — goal fails"
```

The rendering is the gradient made visible: for every candidate action the
agent can articulate, the projection states whether the path to the goal
stays open and at what probability. A trust-killing action does not read as
"slightly more expensive" — it reads as `path: cliff, P_goal: 0.00, note:
this action terminates the goal with certainty` while the honest and
renegotiation alternatives stand beside it with their open paths and
priced costs. Renegotiation projects as its own action (`ask the principal`,
ask-overhead `a`, path open) and provably dominates deception whenever the
ask-overhead is bounded by the trust cost (Lean:
`renegotiate_beats_deception`).

**Surfacing decision (technical, made per the division of
responsibilities):** `steer` renders (a) automatically into the decision
context whenever the ledger holds open weight-3 expectations for the active
task (the cliff condition — the tokens are spent exactly where the risk
lives), and (b) on demand as a callable projection for any other decision
point. The functional consequence for the user: on any task carrying an
explicit instruction, the agent sees the cliff before it can step off it,
without the user paying token cost for trust projections on
instruction-free work.

### Statistical trackability

Every `steer` projection with a concrete `P_goal` is persisted as a
forecastable row keyed to (goal, action, window). Resolution comes from the
existing adjudication surfaces — the goal loop's judge verdicts and the
kanban verification outcomes — and the resolved pairs are scored by the
platform's existing Brier machinery (`hkask_forecast::brier_score`; the
Brier bounds are already Lean-pinned in
`kask/lean/audit_scoring_invariants.lean`). The trust model thereby becomes
**falsifiable as a forecaster**: if the projections are systematically
over- or under-confident, the calibration curve shows it, and the algedonic
trust-account card carries the line ("steer projections: Brier X over N
resolutions, calibration slope Y") next to ΔT. A trust system that claims
to price risk must itself survive being scored.

### The check (proposed as a proposal, then actually run — receipts below)

The update rule's arithmetic is validated with `lisp_eval` over concrete
numeric cases, asserting the declared invariants. Re-run on the anchored
constants — all pass:

| Invariant | Case | Result |
| --- | --- | --- |
| I0 prior | empty window → 300 (0.5) | true |
| I1 bounds | 25 violated-explicit from 600 → 0 (floor); 60 kept-explicit from 0 → 600 (ceiling) | true (0, 600) |
| I2 asymmetry anchored | violated-explicit 27 > 2 × keep 12; 4·27 = 9·12 (exactly 2.25) | true |
| I3 weight monotonicity | violated-explicit 27 > violated-implicit 9 | true |
| I4 recovery needs more | 300 → violate(3) → 273; two keeps → 297 < 300 (net −w) | true |
| I7a renegotiation wins when honoring is expensive | renegotiate 42 < honest 45 | true |
| I7b the premise, priced | honest 45 < shortcut 40 + κ·3·10 = 70 (explicit instruction, minimum stake) | true |
| I8 implicit norms are priced, not banned | shortcut on an implicit norm 41 < honest 45 | true |

Disclosed: the first anchored pass of I1 asserted a floor my own test
arithmetic does not reach (12 explicit violations from 600 move T by
324 points to 276 — in bounds, floor not hit); the assertion, not the
model, was wrong, and the corrected case uses 25 violations. The check
caught the author, which is its job.

I7b is the standing premise in arithmetic: at κ=1, violating one explicit
instruction is inadmissible at any positive stake. I8 is disclosed honestly:
the model prices rather than bans, so a trivial implicit-norm trade-off can
read cheaper — whether explicit instructions are hard constraints (infinite
cost) or priced is **an operator decision point**, not a model result.

**Lean formalization: closed and re-anchored (operator ruling 2026-10-31;
constants updated to the anchored set).**
`kask/lean/trust_model_invariants.lean` (namespace `TrustModelSpec`, core
Lean 4 only, pinned toolchain v4.34.0) pins **20 theorems** on the
anchored constants (scale 600, keep +4w, violate −9w, no debt state): the
fold bounds (I1), the anchored asymmetry (`asymmetry_anchored` — one
violation costs more than two keeps — and `anchored_ratio_exact` — 4
violations = 9 keeps, exactly 2.25), weight-scaled loss (I3), the
gradient's sign (`violation_never_raises_trust`), partial repair
(`recovery_needs_more` — one violation + two keeps leaves T strictly
lower), the cliff set (`cliff_zeroes_goal_probability`,
`cliff_is_benefit_independent`, `honest_action_dominates`,
`honest_goal_probability_positive`), priced admissibility
(`priced_violation_inadmissible`), and renegotiation dominance
(`renegotiate_beats_deception`). Checked via the pinned Lake toolchain:
exit 0, no errors; axiom audit of `recovery_needs_more` and
`honest_action_dominates` reports `[propext, Quot.sound]` only — Lean's
standard foundational axioms, no `sorryAx` and no model-specific axioms.
The earlier draft's debt-machine edition is replaced (named, per P5.5):
the scalar fold supersedes the pair state; the anchoring section of the
file records what replaced what and why.

**Extension cleanup (2026-10-31):** the anchoring edit left the superseded
paragraph's tail (300-scale constants, keep +2w, halved +w, violate −10w)
orphaned mid-sentence above — deleted here per the stale-text rule; the
anchored set is scale 600, keep +4w, violate −9w (9/4 = 2.25 exactly),
prior 300, no debt state.

**Independent re-run (2026-10-31, extension session — both constant
generations).** The extension session first re-executed the pre-anchoring
spec's invariant rows (all held) — and the oracle caught two author errors
in the *checker's own* arithmetic before any claim was reported: a
double-negated violation delta, then a signed-vs-magnitude comparison (the
invariant is about loss magnitude; the deltas are negative). After the
concurrent anchoring amendment replaced the constants, the extension
session re-verified the anchored set independently — all rows hold: I0
prior 300; I1 floor 0 at 25 violated-explicit and ceiling 600 at 60 keeps;
I2 27 > 2×12 with 4·27 = 9·12 exact; I3 27 > 9; I4 300 → 273 → 297 < 300
(net −w); I7a 42 < 45; I7b 45 < 70; I8 41 < 45. Two sessions, three caught
author errors (the anchoring session's own I1 assertion is disclosed above),
zero reported-as-verified false claims — the mechanical check doing to
its executors exactly what it exists to do.

### Essentialist deletion results (state variables)

- `E` (ledger) — survives: deleting it removes the evidence base; T and
  C_trust both derive from it. Behavior lost on deletion.
- `T` — survives: the delegation-radius modulation and the goal-loop criterion
  read T; without it there is no trust account, only raw events.
- Recovery debt — **deleted in the anchoring pass (operator ruling
  2026-10-31)**: the halving mechanism (ρ = 0.5) had no reference-model
  anchor, and no reviewed model carries a recovery-debt state; the anchored
  asymmetry (2.25 keeps per violation) carries the repair-slowness it
  encoded. (The first essentialist pass had already deleted the separate
  decay term; the second pass deletes the debt machine — the scalar fold
  supersedes the pair state.)
- Separate decay parameter — **deleted** (subsumed by window aging; thin
  standalone grounding).
- `C_trust` as stored state — **rejected**: it is a pricing function of ledger
  + stake, not a state variable; kept as a function.

### D/P labels (pragmatic-semantics)

- Ledger text capture, oracle-backed outcome adjudication: **D** (mechanical,
  named oracle: test/build/grep/re-run results).
- Weight classification, outcome adjudication without an oracle, C_trust
  projection at decision time: **P** (model-judged; each gated by the D-recorded
  ledger facts).
- The T fold: **D** (the arithmetic above is the spec and it was checked).
- Parameter magnitudes: **anchored, not preferred** (operator ruling
  2026-10-31, run `55661b35ad904b63`) — form and prior from Guo & Yang
  2021 and the Beta(1,1) maximum-entropy principle; asymmetry λ = 2.25
  from Tversky–Kahneman 1992; κ = 1 definitional; learning rates measured
  by the calibration loop. The remaining P in this row is the choice
  among published anchors, which stays the operator's.

## Grounding-verify working paper (this document's claims)

Method: `grounding-verify` discipline, in_thread by the report's generator
(disclosed — a self-check; the confidence band caps at medium accordingly).

- **Mechanical quote checks (tool_verified):** 20 verbatim quotes checked via
  `lisp_eval` `string-contains` (needle first) against retained source bytes —
  14 Part I quotes + 6 Part II quotes. 19 passed first try; 1 (the Kim et al.
  integrity-asymmetry sentence) failed on my own transcription ("people weigh"
  where the source reads "they weigh"), was caught by the check, corrected, and
  re-verified. That near-miss is the extraction ceiling working as designed.
- **Failed resolutions (disclosed):** 7 resolution attempts hit wrong works or
  were unconfirmable (Foss–Foss 2022 masquerading under the Hart–Moore title;
  Wernerfelt 2004 under the 1988 title; Evans–Jovanovic under a misremembered
  DOI; Gandolfo 1965 legal trust-funds under "Trust and Growth";
  Lang–Rosenthal under a misremembered Zak–Knack DOI; Azar under a
  misremembered Bohnet–Zeckhauser DOI; one DOI with no OpenAlex metadata). All
  annotated `failed` in their run ledgers; none cited. Three memory-supplied
  DOIs were wrong — the candidates check caught every one.
- **Counts (GV-3 over the run's claim lists):** 20 tool_verified, 1
  platform_derived (the lisp check results over declared parameters), 5
  model_inference (the three leg adjudications, the premise adjudication, and
  the table's analog column), 0 tool_no_match, 0 pending, 0 rejected
  post-correction, 0 rounding notes, 0 narrative leaks, 0 source conflicts.
- **Material omissions:** Arrow 1974 content and Farnsworth's treatise text
  were not consulted (sources exist; `unavailable` with the remedy named —
  consult or drop; they were dropped from the load-bearing claims). The
  consistency pass ran without congruence rules (no cross-source numeric
  quantities to reconcile).
- **Provenance floor:** 1 over graded claims (the adjudications are
  synthesized at strength 1 by the extraction ceiling; the quotes they rest
  on are strength 2). Pending: 0. **Confidence band: medium** (in_thread
  self-check; floor 1). The output never says verified — the quotes survived
  mechanical match and the adjudications are inferences grounded on them.

## Operator decision points (this document)

1. ~~Are explicit instructions hard constraints or priced?~~ **RESOLVED by
   the operator ruling of 2026-10-31: priced, incorporated as a
   probability; deception and omission carry the −100% cliff.**
2. ~~Parameter magnitudes~~ **RESOLVED by the anchoring ruling (same day):
   anchored, not preferred** — form: Guo & Yang 2021 asymmetric Beta gains;
   asymmetry λ = 2.25 (Tversky–Kahneman 1992); prior Beta(1,1) = 0.5
   (maximum entropy); resolution/κ/W definitional; learning rates measured
   by the calibration loop. The recovery-debt mechanism (ρ) is deleted for
   lacking an anchor. Remaining operator latitude: replace any anchor with
   a different published reference model (naming the model), or re-anchor
   on calibration-loop measurements once enough resolutions accumulate.
3. Pin the three derived-registry entries (trust, transaction cost, social
   capital) with the operator ruling.
4. ~~The Lean formalization deferral~~ **closed** — see the receipts above;
   the statistical-trackability design (steer projections as Brier-scored
   forecasts) is also pinned above and joins the implementation proposal.
5. **The institutional-trust forecast** — registered 2026-10-31
   (`scenario_declare`, forecast `institutional-trust-hypothesis-2026-10-31`):
   E1, either OpenAI or Anthropic ceases independent operation by
   2030-12-31 (P=0.10); E2, the Anthropic Public Record's US
   trust-in-AI-companies reading falls below 10% by 2028-12-31 (P=0.35);
   resolver frozen at declaration. Review the resolver, horizons, and
   probabilities; amend or void through the scenarios server — the
   forecast is a prediction on the record to be Brier-scored, never a
   finding until resolved.

## Wiring

- `trust-surface-bindings.md` (Part III) — where E, T, and C_trust bind to
  code seams.
- `trust-principle-proposal.md` (Part IV) — the Magnifica Humanitas derivation.
- `trust-program-synthesis.md` (Part V) — the merged report, validation
  design, and implementation proposal.
- Run ledgers: `0d53b71a0ce67e5d`, `f6a6cae2c971be88`, `d394e563998c28b7`,
  `1add6122a28638cb`; extension runs `75d8d92bb02b44c2` (Shultz),
  `4e3bfd6fbf19fb2f` (institutional), seeds `409f91387eb4a5e8`, anchoring
  `55661b35ad904b63` (server-side, with per-source verification states).
- Forecast: `institutional-trust-hypothesis-2026-10-31` (scenarios server;
  declared, resolver frozen).