# RAG Synthesis: Dunning, Tetlock & Cognitive Models for Memory System Design

**Source**: John Brooks corpus (32,897 chunks, `kask/corpus/memory/john-brooks.db`)
**Method**: Semantic query against the corpus DB for Dunning's models, Tetlock's superforecasting, cognitive dissonance, expertise development, and memory/recall processes. Retrieved chunks are cited by `entity_ref` (the corpus chunk ID). All quotes are verbatim from the corpus.

---

## Retrieved Sources (by theme)

### 1. Dunning — "The Trouble of Not Knowing What You Do Not Know"

**Source**: `john-brooks:138299529_the-trouble-of-not-knowing-what-you_txt:0-17`
**Author**: David Dunning, from *Reason, Bias, and Inquiry* (Oxford University Press)
**Retrieved via**: query "Dunning-Kruger effect metacognition self-assessment calibration overconfidence incompetence skill" (top scores 0.68–0.69)

This is a **primary Dunning source** — a full chapter by Dunning himself, chunked across 18 passages. Key excerpts:

#### The double curse (chunk :5)
> "people with poor expertise lack what they need to be able to recognize their shortcomings. It is not that they fail to recognize their deficits; instead, they are simply not in a position to recognize those deficits and should not be expected to do so... they suffer a **double curse**: Not only are they incompetent but they are also too incompetent to recognize just how deep their incompetence runs."

#### The two-task structure: cognitive + metacognitive (chunk :1)
> "In psychology, coming up with a battle plan in the first place can be called the **cognitive task**... The second task, one of assessing the battle plan's soundness, is the **meta-cognitive task**... one central component is evaluating the worth of one's reasoning. Is the reasoning accurate, or does it contain errors?"

#### Overclaiming and false knowledge (chunk :3)
> "over 90% of respondents reported familiarity with at least one of these items. However, this presents a problem: These items are ones we have simply invented among ourselves in our office. These concepts do not exist."

#### Hypocognition — lacking a representation (chunk :11)
> "**Hypocognition** is lacking a linguistic or cognitive representation for some object, emotion, category, or idea (Wu & Dunning, 2018)."

#### The advisor's paradox (chunk :16)
> "people with the most need for advice are no more likely to seek it out than those who do not need it... We have termed this problem the **Cassandra quandary**... This inability to evaluate the expertise of others is more severe among poor performers, who cannot accurately identify which individuals are best to approach for advice."

#### Corralling the unknown (chunk :12)
> "not all of the unknown lies in the realm of the unknown unknown but can be brought into the realm of the known unknown or even the known—if people would simply pay more attention to what they might be ignorant of... They act as though they have complete information even when information outside their knowledge could change their decision."

#### Experts attend to missing information (chunk :13)
> "people who are expert are better at attending to information that is missing (Sanbonmatsu et al., 1992). Apparently, ridding one's self of hypocognition is useful in aiding people to recognize and weight omissions in information, thus aiding their choices. **Blatantly pointing out to people that there is information they miss and asking them to list what that information is also prompts them to be less overconfident** in their decisions (Feduzi & Runde, 2014; Walters et al., 2017)."

#### The veil of false belief (chunk :14)
> "35% to 40% of what each side endorsed as true was actually false... conservatives and liberals lived in different factual worlds—and this divergence occurred even though every respondent on every question had the option of saying 'I don't know'"

#### Confidence and action (chunk :17)
> "real knowledge involves not only awareness of a fact but also **the confidence to act on that fact**. Much of this confidence depends on how people evaluate their expertise, but this chapter has pointed out all the ways in which that evaluation can go awry."

### 2. Dunning — Observability of Errors and the Observer

**Source**: `john-brooks:137434530_observability-of-errors-and-the-observer_txt:0`
**Retrieved via**: top hit (score 0.77) for the Dunning-Kruger query

> "We have to develop both expertise and also the meta-cognitive faculties to see how we are actually performing and see our errors. Dunning's advice ends up being simple, but has the benefit of being anchored in rigorous structured and replicated research... Meta-cognitive tools to correct for the Dunning-Kruger effect can be as simple as **getting feedback from friends or associates with expertise** in an area, or **being careful of assuming you understand a situation**, which is new or unexpected or rare."

> "Dunning's admonition to **know yourself** may sound like a standard recommendation of a psychologist — but we aren't looking for therapeutic value in his advice. **We want to improve our analytical accuracy in making forecasts.**"

### 3. Tetlock — Superforecasting (Brier scoring, calibration, feedback)

**Source**: `john-brooks:Superforecasting_tetlock_txt:*`
**Retrieved via**: queries on confidence calibration, Brier scoring, and expertise development

#### Brier scoring defined (chunk :71)
> "The math behind this system was developed by Glenn W. Brier in 1950, hence results are called Brier scores. In effect, **Brier scores measure the distance between what you forecast and what actually happened**. So Brier scores are like golf scores: lower is better. Perfection is 0. A hedged fifty-fifty call, or random guessing in the aggregate, will produce a Brier score of 0.5. A forecast that is wrong to the greatest possible extent—saying there is a 100% chance that something will happen and it doesn't—scores a disastrous 2.0."

#### Reasonable vs. correct (chunk :92-93)
> "the question is not 'Was the IC's judgment correct?' It is 'Was the IC's judgment reasonable?'... a pro may correctly see that there is a high probability of winning the hand, bet big, get unlucky, and lose, but that doesn't mean her bet was unwise. Good poker players, investors, and executives all understand this. **If they don't, they can't remain good at what they do—because they will draw false lessons from experience, making their judgment worse over time.**"

#### The IC's hubris — no red teams (chunk :94)
> "the IC fell prey to hubris. As a result, it wasn't merely wrong. **It was wrong when it said it couldn't be wrong.** Postmortems even revealed that the IC had never seriously explored the idea that it could be wrong. 'There were no red teams to attack the prevailing views, no analyses from devil's advocates, no papers that provided competing possibilities.'"

#### Learning from experience requires scorable forecasts (chunk :273-274)
> "effective learning from experience can't happen without **clear feedback**, and you can't have clear feedback unless your forecasts are **unambiguous and scorable**... Vague expectations about indefinite futures are not helpful. **Fuzzy thinking can never be proven wrong.** And only when we are proven wrong so clearly that we can no longer deny it to ourselves will we adjust our mental models."

#### Tacit knowledge and informed practice (chunk :195)
> "We need 'tacit knowledge,' the sort we only get from bruising experience... learning to forecast requires trying to forecast... **But not all practice improves skill. It needs to be informed practice.** You need to know which mistakes to look out for—and which best practices really are best."

### 4. Cognitive Dissonance — Universal Principles of Design

**Source**: `john-brooks:Universal_Principles_of_Design__Lidwell_txt:39-40`
**Retrieved via**: query on cognitive dissonance and contradiction resolution

#### The three resolution strategies (chunk :39)
> "People alleviate cognitive dissonance in one of three ways: by **reducing the importance of dissonant cognitions**, **adding consonant cognitions**, or **removing or changing dissonant cognitions**."

#### The point of minimum justification (chunk :40)
> "A small incentive is usually required to get a person to consider an unpleasant thought or engage in an unpleasant activity. Any incentive beyond this small incentive reduces, not increases, the probability of changing attitudes and beliefs—this critical point is known as the **point of minimum justification**."

#### Design implication (chunk :40)
> "Use consonant and dissonant information when attempting to change beliefs. **Engage people to invest their time, attention, and participation to create dissonant cognitions, and then provide simple and immediate mechanisms to alleviate the dissonance.**"

### 5. Memory Limits and Chunking

**Source**: `john-brooks:Universal_Principles_of_Design__Lidwell_txt:35`
**Retrieved via**: query on memory consolidation and recall

> "Large strings of numbers are difficult to recall. **Chunking** large strings of numbers into multiple, smaller strings can help... The seminal work on short-term memory limits is 'The Magical Number Seven, Plus or Minus Two' by George Miller (1956)... his original estimate for short-term memory capacity was 7 ± 2 chunks."

> Cites Baddeley, *Human Memory: Theory and Practice* (1997) and Cowan, "The Magical Number Four in Short-Term Memory" (2001) — the update to Miller's 7±2.

### 6. The Dilution Effect — irrelevant information weakens judgment

**Source**: `john-brooks:Superforecasting_tetlock_txt:178`
**Retrieved via**: query on cognitive dissonance (semantic proximity)

> "irrelevant information of this sort does sway us... those who got the irrelevant information lost confidence. Why? With nothing to go on but evidence that fits their stereotype... the signal feels strong and clear. But add irrelevant information and we can't help but see Robert or David more as a person than a stereotype, which weakens the fit. Psychologists call this the **dilution effect**."

---

## Synthesis: Implications for Memory System Design

### A. Confidence calibration (the Brier loop)

**Dunning's double curse** (chunk :5) establishes the foundational constraint: a model that writes its own confidence scores will miscalibrate, because the act of evaluating confidence requires the metacognitive skill the model lacks. **Tetlock's Brier scoring** (chunk :71) provides the solution: confidence is a forecast, and forecasts must be scored against outcomes. The key Tetlock insight (chunk :273): "effective learning from experience can't happen without clear feedback, and you can't have clear feedback unless your forecasts are unambiguous and scorable."

**Implication for zed-kask**: The existing `Confidence` struct (`hkask-types/src/visibility.rs:141-145`) stores confidence but it is set once (default 1.0) and only changes via Bayesian combination during consolidation or decay. There is no outcome feedback loop. The Brier infrastructure exists in `hkask-scenarios-widget` but is not wired to memory. The design must:
1. Treat each recalled memory's confidence as a forecast ("this memory is relevant/true with probability p")
2. Wire outcome observation (did the action informed by this memory succeed?) to Brier scoring
3. Use Brier scores to update confidence via the existing `combine_confidences` Bayesian function (`bayesian.rs:86-96`)

### B. Cognitive dissonance and the therapy process

**Festinger's three resolution strategies** (chunk :39, via Lidwell) map directly to the therapy process:
1. **Reduce the importance** of dissonant cognitions → lower confidence on the contradicted memory
2. **Add consonant cognitions** → insert a new semantic memory that reconciles the contradiction (Q3 reflection)
3. **Remove or change dissonant cognitions** → expire or update the contradicted memory

**Tetlock's "no red teams" failure** (chunk :94) is the cautionary tale: the IC "was wrong when it said it couldn't be wrong" because it "never seriously explored the idea that it could be wrong." The therapy process must be the **red team** — it actively searches for contradictions the system would otherwise ignore.

**Dunning's hypocognition** (chunk :11) adds a deeper layer: the system may lack a cognitive representation for the contradiction itself. The therapy process must name contradictions explicitly — "blatantly pointing out to people that there is information they miss... prompts them to be less overconfident" (chunk :13).

**Implication for zed-kask**: The therapy process (designed in `q3-q5-reflection-writable-memory.md`) must:
1. Scan for EAV contradictions (same entity+attribute, divergent values/confidence)
2. Classify each contradiction using Festinger's three strategies
3. Propose a resolution per strategy (lower confidence / add reconciling memory / expire)
4. Require operator approval for modifications (the external check Dunning's dual-burden requires)

### C. Expertise development and recall quality

**Dunning's "make everybody better performers"** principle (verified via Wikipedia, Dunning & Helzer 2014): the best way to improve self-accuracy is to improve the underlying capability. Applied to memory: don't try to fix confidence directly — fix recall quality and let confidence track that.

**Tetlock's informed practice** (chunk :195): "not all practice improves skill. It needs to be informed practice. You need to know which mistakes to look out for." The memory system's recall path must surface *why* a memory was recalled (relevance score, confidence, connectedness) so the model can evaluate whether the recall was useful — this is the feedback that makes practice informed.

**Dunning on experts attending to missing information** (chunk :13): "people who are expert are better at attending to information that is missing." The recall path should signal what was *not* recalled — the known unknowns. This is the hypocognition guard: if the system has no memory matching a query, that absence should be explicit, not silent.

**Implication for zed-kask**: The recall ranking function (`recall_score = relevance × decayed_confidence × connectedness`) should be extended with:
1. **Absence signaling**: when recall returns zero results, the context injector already logs this (`context_injector.rs:283-285`), but the model should also be told "no relevant memory found" — not just silence
2. **Recall metadata**: the model should see *why* each memory was ranked (relevance score, confidence, connectedness) so it can evaluate recall quality
3. **Connectedness as expertise signal**: a memory referenced by many others is "more expert" in Dunning's sense — it has been tested against more contexts

### D. The dilution effect and context injection

**Tetlock's dilution effect** (chunk :178): irrelevant information weakens judgment by diluting the signal. The current context injector (`context_injector.rs:216-300`) injects all recalled snippets above the confidence threshold — there is no mechanism to filter out irrelevant-but-similar memories.

**Implication for zed-kask**: The recall ranking function should penalize memories that are similar to the query but irrelevant to the task. This is what connectedness provides: a memory with high embedding similarity but low connectedness (not referenced by other memories) is likely a dilution candidate. The ranking `relevance × confidence × connectedness` naturally down-weights these.

### E. Structured reflection and evidence-grounding

**Dunning's "know yourself"** (chunk :0, observability source): "We want to improve our analytical accuracy in making forecasts." The advice is to use "meta-cognitive tools to correct for the Dunning-Kruger effect" — specifically, "getting feedback from friends or associates with expertise" and "being careful of assuming you understand a situation, which is new or unexpected or rare."

**Tetlock on fuzzy thinking** (chunk :274): "Fuzzy thinking can never be proven wrong. And only when we are proven wrong so clearly that we can no longer deny it to ourselves will we adjust our mental models."

**Implication for zed-kask**: The Q3 reflection pass must force evidence citation (each insight must cite specific h_mem IDs). This is the structured-reflection constraint that prevents self-serving narratives — the Dunning debiasing mechanism. The therapy process must also cite the specific contradictory h_mem IDs it proposes to resolve.

### F. The Cassandra quandary and curator authority

**Dunning's Cassandra quandary** (chunk :16-17): "people with the most need for advice are no more likely to seek it out" and "this inability to evaluate the expertise of others is more severe among poor performers, who cannot accurately identify which individuals are best to approach for advice."

**Implication for zed-kask**: This validates the curator-only write authority design (Q5). User threads are "poor performers" in Dunning's sense — they cannot accurately evaluate which memories are worth writing. The curator, with its regulation loop and Brier scoring, is the "expert" that can evaluate. Restricting write access to the curator follows Dunning's principle: only the agent with calibrated feedback should write to memory.

---

## Summary: Corpus-Verified Insights → Design Decisions

| Corpus insight | Source (entity_ref) | Design decision | Where in the design |
|---|---|---|---|
| Double curse: can't evaluate own competence | `138299529:5` | Confidence must be calibrated by external outcomes, not self-assessment | Q5 confidence floor (0.5) + Brier loop |
| Brier scoring: distance between forecast and outcome | `Superforecasting_tetlock:71` | Wire Brier scoring into memory confidence calibration | Q5 Brier loop |
| Clear feedback requires scorable forecasts | `Superforecasting_tetlock:273` | Treat each memory's confidence as a scorable forecast | Q5 Brier loop |
| Three dissonance resolution strategies | `Universal_Principles_of_Design:39` | Therapy process classifies contradictions by strategy | Therapy process design |
| No red teams → wrong when said it couldn't be wrong | `Superforecasting_tetlock:94` | Therapy process is the red team — actively searches for contradictions | Therapy process design |
| Hypocognition: lacking a representation | `138299529:11` | Therapy must name contradictions explicitly | Therapy process design |
| Experts attend to missing information | `138299529:13` | Recall should signal absence (known unknowns) | Recall ranking + absence signaling |
| Blatantly pointing out missing info reduces overconfidence | `138299529:13` | Context injector should tell the model when recall is empty | `context_injector.rs:283-285` (already logs, should also inject) |
| Dilution effect: irrelevant info weakens judgment | `Superforecasting_tetlock:178` | Connectedness down-weights similar-but-isolated memories | Recall ranking function |
| Informed practice needs feedback on mistakes | `Superforecasting_tetlock:195` | Recall metadata (why ranked) lets model evaluate recall quality | Recall ranking + metadata |
| Cassandra quandary: poor performers can't identify experts | `138299529:16-17` | Curator-only write authority (user threads can't evaluate) | Q5 permission boundary |
| Confidence to act depends on evaluating expertise | `138299529:17` | Confidence is the action-enabling signal — must be calibrated | Q5 Brier loop |
| Fuzzy thinking can never be proven wrong | `Superforecasting_tetlock:274` | Reflection must force evidence citation (no fuzzy insights) | Q3 structured reflection |
| Know yourself → improve analytical accuracy | `137434530:0` | The whole memory system is a metacognitive tool for the curator | System-level design |
