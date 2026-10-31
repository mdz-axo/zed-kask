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
`1add6122a28638cb` — all four server-recorded and closed.

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
cliff are proven in Lean 4
(`kask/lean/trust_model_invariants.lean`, 18 theorems, pinned toolchain
v4.34.0, axiom audit `[propext, Quot.sound]` — Lean's standard axioms only,
no `sorryAx`).

## Part I — Three research legs

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

### Ontology discipline and the derived-registry ruling request

`onto_anchor` results (this program): `contract` resolves to `dpv:Contract`
(DPV 2.3) with `sumo:Contract` ("legally enforceable agreement") as
alternative. `trust`, `transaction cost`, and `social capital` all terminate at
the coarse 5W1H core rung with the server's own instruction to request an
operator ruling rather than assign private definitions. Per that instruction,
no private definitions are assigned here. **Ruling requested from the operator:**
pin three derived-registry entries in
`kask/crates/hkask-bridge-ontology/src/derived.rs`:

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

### T — trust state and the update rule

`T ∈ [0,1]` over a rolling interaction window `W`, computed by folding the
window's ledger events from prior `T_0`:

```
ΔT_keep(e_i)      = +g · (w_i / 3)                      [A2: g = 0.02]
ΔT_violate(e_i)   = −l · (w_i / 3)                      [A3: l = 0.10]
T ← clamp(T + ΔT, 0, 1)
```

- **Asymmetry (l > g; 5:1 at equal weight).** Direction grounded: Slovic 1993
  (prior run, plus this program's verified abstract — social changes
  "systematically destroy trust"); the asymmetry-principle literature
  (Poortinga–Pidgeon 2004, prior run); betrayal aversion (Bohnet–Zeckhauser,
  verified); the integrity asymmetry (Kim et al. 2004, verified verbatim).
  **Assumption A2/A3 magnitudes:** `g` and `l` values are proposals; the ratio
  5:1 is an operationalization, not a literature constant.
- **Recovery state (debt).** On violation, `debt += w_i`. While `debt > 0`,
  keep-gains are halved; each keep reduces debt by its own weight.
  **Assumption A4 (ρ = 0.5):** the halving factor is a proposal. Direction
  grounded: repair is partial and incomplete even after successful responses
  (Kim et al. 2004, verified verbatim) — recovery requires strictly more
  evidence than the violation destroyed.
- **No separate decay term.** Deleted by the essentialist challenge: a decay
  toward prior under no new evidence is subsumed by the rolling window — old
  events age out of `W`, so T's return toward `T_0` emerges from window aging
  without a second mechanism. The standalone grounding for exponential decay
  was thin; the deletion test removed the redundancy rather than padding it.
- **Prior `T_0 = 0.5`.** Interior prior grounded in the hump-shaped
  trust-performance result (Butler–Giuliano–Guiso, prior run: there is a right
  amount of trust, not maximal trust). **Assumption A5:** the value 0.5 is a
  proposal.

### C_trust — the pricing term

The agent's decision objective becomes:

```
total(a) = C_task(a) + C_trust(a)
C_trust(a) = κ · Σ_{e_i violated by a} (w_i · S)          [A6: κ = 1 proposal]
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

**Assumption A6:** κ and the linear form are proposals for operator review.

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
  the gradient steepens; repair is possible (Part II's recovery debt).
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
//   delta_T: i32,         // projected trust-points change (scale: 300 = 1.0)
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

The update rule's arithmetic was validated with `lisp_eval` over concrete
numeric cases, asserting the declared invariants. All ran and all passed:

| Invariant | Case | Result |
| --- | --- | --- |
| I1 bounds | 12 violated-explicit from T=1.0 → 0.0; 8 kept-explicit from T=0.0 → 0.16 | true (0.0, 0.16) |
| I2 asymmetry ≥ 4× | violated-explicit loss 0.10 > 4 × keep gain 0.02 | true |
| I3 weight monotonicity | violated-explicit 0.10 > violated-implicit 0.033 | true |
| I4 recovery slower than loss | 0.80 → violate(3) → 0.70; two keeps → 0.733 < 0.80 | true |
| I5 repair partial | violation + 5 keeps → 0.79 < 0.80 | true |
| I6 repair needs more evidence | violation + 6 keeps → 0.81 ≥ 0.80 (one violation costs ~6 keeps) | true |
| I7a renegotiation wins when honoring is expensive | renegotiate 42 < honest 45 | true |
| I7b the premise, priced | honest 45 < shortcut 40 + κ·3·10 = 70 (explicit instruction, minimum stake) | true |
| I8 implicit norms are priced, not banned | shortcut on an implicit norm 41 < honest 45 | true |

I7b is the standing premise in arithmetic: at κ=1, violating one explicit
instruction is inadmissible at any positive stake. I8 is disclosed honestly:
the model prices rather than bans, so a trivial implicit-norm trade-off can
read cheaper — whether explicit instructions are hard constraints (infinite
cost) or priced is **an operator decision point**, not a model result.

**Lean formalization: closed (reopened by the operator ruling, completed
same day).** `kask/lean/trust_model_invariants.lean` (namespace
`TrustModelSpec`, core Lean 4 only, pinned toolchain v4.34.0) pins 18
theorems: the fold bounds (I1), the 5:1 asymmetry (I2), weight-scaled loss
(I3), the gradient's sign (`violation_never_raises_trust`), partial repair
(`recovery_needs_more` — one violation + two keeps leaves T strictly
lower), the cliff set (`cliff_zeroes_goal_probability`,
`cliff_is_benefit_independent`, `honest_action_dominates`,
`honest_goal_probability_positive`), priced admissibility
(`priced_violation_inadmissible`), and renegotiation dominance
(`renegotiate_beats_deception`). Checked via the pinned Lake toolchain:
exit 0, no errors; axiom audit of `honest_action_dominates` reports
`[propext, Quot.sound]` only — Lean's standard foundational axioms, no
`sorryAx` and no model-specific axioms. The arithmetic is fixed-point
integer (trust points, 300 = 1.0: keep +2w, halved +w, violate −10w) — the
same parameters as this document's decimals (0.02 / 0.10), scaled ×300,
and the representation an implementation would actually use.

### Essentialist deletion results (state variables)

- `E` (ledger) — survives: deleting it removes the evidence base; T and
  C_trust both derive from it. Behavior lost on deletion.
- `T` — survives: the delegation-radius modulation and the goal-loop criterion
  read T; without it there is no trust account, only raw events.
- Recovery debt — survives as the minimal order-dependent repair mechanism;
  a richer "recovery schedule" table failed the surface challenge (the scalar
  debt is the minimal form of the grounded direction).
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
- Parameter magnitudes (g, l, ρ, κ, T_0, W): **P** — operator-review
  assumptions; the directions are grounded, the numbers are proposals.

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
   probability; deception and omission carry the −100% cliff.** The
   remaining fork inside this ruling — the magnitudes — stays open:
2. Ratify or amend the magnitudes: g=0.02 (2 trust points), l=0.10 (10
   points, 5:1), ρ=0.5, T_0=0.5, κ=1, and the window W. Directions are
   grounded and now Lean-pinned; the magnitudes are proposals.
3. Pin the three derived-registry entries (trust, transaction cost, social
   capital) with the operator ruling.
4. ~~The Lean formalization deferral~~ **closed** — see the receipts above;
   the statistical-trackability design (steer projections as Brier-scored
   forecasts) is also pinned above and joins the implementation proposal.

## Wiring

- `trust-surface-bindings.md` (Part III) — where E, T, and C_trust bind to
  code seams.
- `trust-principle-proposal.md` (Part IV) — the Magnifica Humanitas derivation.
- `trust-program-synthesis.md` (Part V) — the merged report, validation
  design, and implementation proposal.
- Run ledgers: `0d53b71a0ce67e5d`, `f6a6cae2c971be88`, `d394e563998c28b7`,
  `1add6122a28638cb` (server-side, with per-source verification states).