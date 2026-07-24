//! Scenario prompt templates — brainstorming and framing protocol generation.

use crate::types::{BrainstormProtocol, BrainstormRound, PersonaConfig};

// ── Brainstorming Protocol ─────────────────────────────────────────────────

/// Generate a multi-round scenario brainstorming protocol.
///
/// Produces a structured 4-round protocol with persona configurations,
/// temperature guidance, and quality gates. The agent (LLM) follows this
/// protocol to collaboratively generate events with the user.
///
/// Round 1 — DIVERGE (high temperature): Generate many candidate events
///   from multiple persona perspectives. Quantity over quality. No filtering.
///
/// Round 2 — GROUND (medium temperature): Ground each candidate in verified
///   facts. Attach base rates, reference classes, source citations.
///   Discard candidates without factual grounding.
///
/// Round 3 — LINK (low temperature): Identify dependencies between events.
///   Build causal chains. What must happen first? What enables what?
///
/// Round 4 — PRUNE (analytical): Evaluate and converge. Eliminate redundant
///   or implausible events. Merge overlapping events. Produce final tree.
pub fn generate_brainstorm_protocol(
    subject: &str,
    time_horizon: &str,
    research_context: &str,
    persona_names: &[String],
) -> BrainstormProtocol {
    let default_personas = vec![
        PersonaConfig {
            name: "Bull".into(),
            lens: "Optimistic — what could go right?".into(),
            prompt: format!(
                "You are an optimist about '{}'. What positive developments could realistically occur by {}? \
                 Focus on: technology breakthroughs, market expansion, regulatory tailwinds, competitive advantages. \
                 Each event must be a specific yes/no question with a deadline. Anchor to facts in the research context.",
                subject, time_horizon
            ),
        },
        PersonaConfig {
            name: "Bear".into(),
            lens: "Pessimistic — what could go wrong?".into(),
            prompt: format!(
                "You are a skeptic about '{}'. What negative developments could realistically occur by {}? \
                 Focus on: competitive threats, regulatory risk, supply chain disruption, market saturation, execution failure. \
                 Each event must be a specific yes/no question with a deadline. Anchor to facts in the research context.",
                subject, time_horizon
            ),
        },
        PersonaConfig {
            name: "Contrarian".into(),
            lens: "What is everyone missing?".into(),
            prompt: format!(
                "You are a contrarian thinker about '{}'. What non-obvious developments could occur by {} that most analysts are ignoring? \
                 Focus on: second-order effects, hidden assumptions, rare but high-impact events, unconventional competitors. \
                 Each event must be a specific yes/no question with a deadline. Challenge consensus views, but stay grounded.",
                subject, time_horizon
            ),
        },
        PersonaConfig {
            name: "Systems Thinker".into(),
            lens: "How do the pieces connect?".into(),
            prompt: format!(
                "You are a systems thinker analyzing '{}'. What feedback loops and cascade effects could unfold by {}? \
                 Focus on: second-order consequences, enabling conditions, bottleneck constraints, network effects. \
                 Each event should illuminate a causal mechanism — not just 'what' but 'what enables what.' \
                 Anchor to structural dynamics visible in the research context.",
                subject, time_horizon
            ),
        },
    ];

    // Use user-provided persona names if given, otherwise defaults
    let personas: Vec<PersonaConfig> = if persona_names.is_empty() {
        default_personas.clone()
    } else {
        let defaults: std::collections::HashMap<&str, PersonaConfig> = default_personas
            .iter()
            .map(|p| (p.name.as_str(), p.clone()))
            .collect();
        persona_names
            .iter()
            .filter_map(|name| {
                defaults.get(name.as_str()).cloned().or_else(|| {
                    Some(PersonaConfig {
                        name: name.clone(),
                        lens: format!("Custom perspective: {}", name),
                        prompt: format!(
                            "You are the '{}' perspective on '{}'. Generate specific yes/no events with deadlines by {}. \
                             Ground each event in the research context.",
                            name, subject, time_horizon
                        ),
                    })
                })
            })
            .collect()
    };

    let rounds = vec![
        BrainstormRound {
            round: 1,
            name: "DIVERGE — Generate Candidate Events".into(),
            mode: "diverge".into(),
            temperature_guidance: "HIGH temperature. Prioritize quantity, novelty, and range. Suspend judgment. No event is too unlikely in this round. Aim for 3-5 events per persona.".into(),
            output_type: "Vec<EventCandidate>".into(),
            instructions: format!(
                "ROUND 1: DIVERGE\n\n\
                 SUBJECT: {}\n\
                 TIME HORIZON: {}\n\n\
                 RESEARCH CONTEXT:\n{}\n\n\
                 INSTRUCTIONS:\n\
                 For each persona below, generate 3-5 candidate events as specific yes/no questions with deadlines.\n\
                 Each event must have:\n\
                 - id: persona-round-number (e.g., 'bull-1-1')\n\
                 - persona: the persona name\n\
                 - name: short descriptive name\n\
                 - question: yes/no framed with specific deadline\n\
                 - deadline_hint: approximate date\n\
                 - steep_category: Society|Technology|Economy|Environment|Politics|Industry\n\
                 - plausibility: 1-5 initial screening score\n\
                 - grounding: reference to specific fact/article in research context\n\
                 - potential_dependencies: [] (leave empty — filled in Round 3)\n\
                 - rationale: why this matters\n\n\
                 PERSONAS TO USE:\n{}",
                subject,
                time_horizon,
                research_context,
                personas
                    .iter()
                    .map(|p| format!(
                        "  {} ({}): {}\n    Prompt: {}",
                        p.name, p.lens, p.name, p.prompt
                    ))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            ),
            quality_gate: Some(
                "Before proceeding to Round 2: (1) At least 12 candidate events generated across all personas. \
                 (2) All five STEEP categories represented. (3) Each event has a specific yes/no question with deadline. \
                 (4) Each event references at least one fact from the research context.".into()
            ),
        },
        BrainstormRound {
            round: 2,
            name: "GROUND — Anchor in Verified Facts".into(),
            mode: "ground".into(),
            temperature_guidance: "MEDIUM temperature. Creative interpretation of facts is allowed, but factual grounding is required. Discard events without supporting evidence.".into(),
            output_type: "Vec<EventCandidate>".into(),
            instructions: "ROUND 2: GROUND\n\n\
                 For each candidate event from Round 1:\n\
                 1. Verify grounding: is there a specific fact, data point, or trend in the research that supports this event?\n\
                 2. Add base rate: search for a reference class. How often do events of this type occur?\n\
                    Format: 'For [reference class], the historical frequency is approximately X% over [time period]'\n\
                 3. Add Fermi sub-scaffolding: what 2-3 sub-questions would decompose this event?\n\
                 4. DISCARD any event without factual grounding. Mark as 'grounding_verified: true/false'.\n\
                 5. Adjust plausibility score based on evidence strength.\n\n\
                 QUALITY GATE: All retained events must have verified grounding. \
                 Minimum 8 events retained. If fewer, return to Round 1 with different personas.".to_string(),
            quality_gate: Some(
                "All retained events have verified grounding (specific fact/article/data point). \
                 At least 8 events retained. Each event has a reference class suggestion. \
                 Fermi sub-questions provided for each event.".into()
            ),
        },
        BrainstormRound {
            round: 3,
            name: "LINK — Build Causal Chains".into(),
            mode: "link".into(),
            temperature_guidance: "LOW temperature. Focus on logical structure, not creativity. Causal links must be defensible.".into(),
            output_type: "Vec<ScenarioEvent>".into(),
            instructions: "ROUND 3: LINK\n\n\
                 For the grounded events from Round 2:\n\
                 1. Identify dependency relationships: which events must happen before others?\n\
                 2. Build causal chains: A → B → C. For each dependency, estimate:\n\
                    - P(B | A occurs) — probability of B if A happens\n\
                    - P(B | A does NOT occur) — probability of B if A doesn't happen\n\
                 3. Check for cycles: if A depends on B and B depends on A, resolve the direction.\n\
                 4. Identify root events (no dependencies) and leaf events (nothing depends on them).\n\
                 5. Convert candidates to full ScenarioEvent objects with:\n\
                    - Formal deadline (YYYY-MM-DD)\n\
                    - Initial probability estimate (0.0-1.0)\n\
                    - depends_on array with conditional probabilities\n\
                    - sub_questions from Round 2\n\
                    - base_rate and reference_class from grounding research\n\n\
                 QUALITY GATE: All events must have at least one dependency or be a root event. \
                 No cycles in the dependency graph. Events form a coherent causal narrative.".to_string(),
            quality_gate: Some(
                "No cycles in dependency graph. Every non-root event has at least one dependency. \
                 Conditional probabilities (P(event|parent), P(event|¬parent)) provided for all dependencies.".into()
            ),
        },
        BrainstormRound {
            round: 4,
            name: "PRUNE — Converge to Final Tree".into(),
            mode: "prune".into(),
            temperature_guidance: "ANALYTICAL. Ruthless pruning. If two events are nearly identical, merge them. If an event has no dependencies and no path to a consequential outcome, remove it.".into(),
            output_type: "Vec<ScenarioEvent>".into(),
            instructions: "ROUND 4: PRUNE\n\n\
                 For the linked events from Round 3:\n\
                 1. Merge overlapping events: if two events describe essentially the same thing, combine them.\n\
                 2. Remove isolated events: if an event has no dependencies AND no events depend on it, \
                    consider whether it belongs in this scenario tree.\n\
                 3. Check for completeness: do the events collectively tell a coherent story? \
                    Are there obvious gaps (no regulatory events? no competitive events?)?\
                 4. Final probability calibration: use Fermi decomposition to estimate initial probability \
                    for each remaining event.\n\
                 5. Produce final output: a JSON array of ScenarioEvent objects ready for scenario_quantify.\n\n\
                 Send this output to scenario_quantify for conditional probability resolution.\n\n\
                 QUALITY GATE: 4-8 events remain. All events are connected (directly or transitively) to at \
                 least one other event or to a consequential outcome. STEEP coverage maintained. \
                 All events have Fermi-calibrated probabilities.".to_string(),
            quality_gate: Some(
                "4-8 events remain in final tree. No isolated events. STEEP coverage maintained. \
                 All events have calibrated probabilities. Dependencies form a causal narrative. \
                 Ready to send to scenario_quantify.".into()
            ),
        },
    ];

    BrainstormProtocol {
        subject: subject.to_string(),
        time_horizon: time_horizon.to_string(),
        research_context: research_context.to_string(),
        personas,
        rounds,
        pipeline: vec![
            "1. scenario_brainstorm → get this protocol".into(),
            "2. [agent follows protocol rounds 1-4]".into(),
            "3. scenario_quantify → resolve conditional probability tree".into(),
            "4. scenario_calibrate → Fermi decomposition per event".into(),
            "5. scenario_synthesize → dragonfly-eye aggregation (if multiple analysts)".into(),
            "6. scenario_assess → evaluate project quality (Chermack)".into(),
        ],
    }
}

// ── Scenario Framing Protocol ──────────────────────────────────────────────

/// Generate a conversational framing protocol for scenario project setup.
///
/// Designed with behavioral psychology principles and improv coaching
/// postures to make framing approachable rather than diagnostic.
///
/// Design principles:
/// - Foot-in-the-door: start easy, build to harder questions
/// - Never explicitly negate (improv Plussing): build on what works
/// - Yes, And: accept the user's answer and extend naturally
/// - Curiosity gap: create intrigue before asking for commitment
/// - Peak-end rule: open warmly, close with clarity
/// - Self-determination: the user is the domain expert; the agent is the method coach
/// - Processing fluency: conversational language, no technical jargon
///
/// The 7 conversational turns replace the formal "7 questions" —
/// each turn is a natural opening, not a numbered test item.
///
/// References:
/// - Chermack (2011), Ch. 5: Project Preparation Phase
/// - Schwartz (1991), Ch. 4: Focal Question
/// - Kahneman (2011): System 1/System 2, loss aversion, peak-end rule
/// - Cialdini (2006): Influence — foot-in-the-door, social proof
/// - Ryan & Deci (2000): Self-determination theory
/// - hKask improv skill: Plussing, Yes And, Yes But postures
/// - hKask kata-starter: coaching posture, 20-minute practice window
pub fn generate_framing_session(subject: &str) -> serde_json::Value {
    serde_json::json!({
        "session_type": "Conversational Scenario Framing",
        "subject": subject,
        "design_principles": {
            "why_conversational": "Framing is where scenario projects usually break down. Formal diagnostic questions create resistance. Conversational turns invite engagement. The goal isn't to extract answers — it's to help the user discover their own frame.",
            "improv_posture": "Plussing by default: accept what the user says, build on what's useful, silently let go of what isn't. Never correct. Never 'no, but.' Always 'yes, and...'",
            "kata_coaching": "The agent is a coach, not an interviewer. The user is the domain expert. The coach helps the expert articulate what they already know but haven't yet made explicit.",
            "behavioral_design": [
                "Foot-in-the-door: Turn 1 is the easiest — 'what's on your mind?' Anyone can answer that.",
                "Curiosity gap: Turns 2-3 build intrigue before asking for commitment.",
                "Peak-end rule: Turn 1 opens warmly; Turn 7 closes with clarity and purpose.",
                "Loss aversion: Turn 4 asks what's OFF the table (easier to identify exclusions).",
                "Social proof: Turn 5 uses 'who else' to normalize multiple perspectives.",
                "Processing fluency: Everyday language throughout. No 'focal question,' no 'epistemic calibration.'",
                "IKEA effect: The user co-creates the frame. They own it because they built it."
            ]
        },

        "conversation_flow": [
            {
                "turn": 1,
                "improv_mode": "Plussing",
                "psychology": "Foot-in-the-door — the easiest question, no wrong answer",
                "opening": "So — tell me a bit about what's on your mind. What situation are you looking at?",
                "why_this_comes_first": "Everyone can answer this. It establishes the user as the domain expert and the agent as the curious listener. No jargon, no pressure, no right answer.",
                "what_to_listen_for": "The subject, the emotional stakes, what makes this situation interesting or urgent. Don't correct or narrow — just let them talk.",
                "agent_posture": "Listen actively. Reflect back what you heard. 'So it sounds like you're looking at [X] and wondering about [Y].' Use Plussing: affirm what's clear, gently surface what's fuzzy without calling it fuzzy.",
                "anti_patterns": [
                    "Jumping to 'so what's your focal question?' — that comes later",
                    "Correcting scope — 'that's too broad' — instead ask 'what part of that feels most uncertain?'",
                    "Solving the problem — the agent's job is to frame, not to answer"
                ],
                "captures": "subject, initial_context, emotional_stakes"
            },
            {
                "turn": 2,
                "improv_mode": "Yes, And",
                "psychology": "Curiosity gap — connect their situation to a decision",
                "opening": "That's really interesting. If you had a clearer picture of how this might play out — what would you actually do differently? What decision is hanging on this?",
                "why_this_comes_second": "Schwartz's rule: if the answer doesn't change any decision, don't spend time on it. But asking 'what is your focal question?' is clinical. This framing connects the situation they just described to the decision it informs. It makes the purpose personal.",
                "what_to_listen_for": "Is there a real decision at stake? If they say 'I just want to understand' — that's fine for landscape exploration, but note it. If they name a specific decision, that's gold — it becomes the focal question.",
                "agent_posture": "Yes, And: 'So the decision is [X], and what makes it hard is [Y].' Don't push for a single sentence focal question yet — that comes after they've explored.",
                "anti_patterns": [
                    "'That's not really a focal question' — never negate. Instead: 'That's a great starting point. Let's see if we can make it even more specific.'",
                    "Accepting 'I just want to understand everything' without probing: 'Totally fair. Is there a particular fork in the road where understanding would change your path?'"
                ],
                "captures": "decision_at_stake, focal_question_draft"
            },
            {
                "turn": 3,
                "improv_mode": "Coaching (kata-style)",
                "psychology": "Temporal anchoring — the kata coach asks 'what is the target condition?'",
                "opening": "Got it. So looking ahead — when do you actually need to make this call? And over what kind of timeframe do the key events play out? Sometimes the decision deadline and the event horizon are different. Like, you might need to decide in three months about things that won't fully play out for three years.",
                "why_this_comes_third": "Now that we have a decision and a situation, we need a temporal boundary. The kata coaching pattern works here: 'what is the target condition?' (when do you need to decide?) followed by 'what is the actual condition now?' (what timeframe are the events on?).",
                "what_to_listen_for": "Distinguish decision deadline from event horizon. If they're different, note both. If the user says 'I don't know' — that's information too. Suggest: tactical (12-18mo), strategic (3-5yr), long-term (7-10yr) as reference points, not as a multiple-choice test.",
                "agent_posture": "Coach, not quizmaster. 'Most people find it helpful to think in terms of tactical (12-18 months), strategic (3-5 years), or long-term (7-10 years). Where does your situation land?' Present options as scaffolding, not as a test.",
                "anti_patterns": [
                    "'You need to pick one of these three time horizons' — don't force categorization",
                    "Overspecifying: 'So exactly 42 months?' — approximate is fine at this stage"
                ],
                "captures": "time_horizon, action_deadline"
            },
            {
                "turn": 4,
                "improv_mode": "Yes, But (constraint focus)",
                "psychology": "Loss aversion — people find it easier to identify what's excluded than what's included",
                "opening": "Helpful. Now let's draw some boundaries — and let's start with what's definitely NOT on the table. What are we explicitly not trying to figure out here? What's somebody else's problem, or a different project, or just not relevant right now?",
                "why_this_comes_fourth": "Loss aversion: people engage more to avoid loss than to seek gain. Asking 'what's out of scope' is easier and more energizing than 'what's in scope.' Once exclusions are clear, the scope naturally emerges. Schwartz: scope-bounded is essential; without boundaries, everything is relevant.",
                "what_to_listen_for": "Explicit boundaries. If the user says 'well, everything is relevant actually' — that's a red flag. Gently probe: 'What's one thing that's NOT relevant?' Even one exclusion is progress.",
                "agent_posture": "Yes, But: 'Okay, so [X], [Y], and [Z] are off the table. Given that, what IS on the table?' The constraint clarifies without contradicting.",
                "anti_patterns": [
                    "Leading with 'what's in scope?' — that's the harder question. Start with exclusions.",
                    "Accepting 'everything' without pushing back — if everything is relevant, nothing is actionable"
                ],
                "captures": "out_of_scope, in_scope"
            },
            {
                "turn": 5,
                "improv_mode": "Plussing (multi-perspective)",
                "psychology": "Social proof + contrarian activation",
                "opening": "Let's think about who else has skin in this game. If this goes wrong — or right — who's going to have a strong opinion about it? And here's a fun one: if it goes wrong, who's the person who's going to say 'I told you so' — and what would they have seen that others missed?",
                "why_this_comes_fifth": "Chermack: stakeholder diversity is the strongest predictor of scenario quality. But 'who are the stakeholders?' is bureaucratic. The 'I told you so' framing activates social dynamics — it makes the question playful and memorable. The contrarian perspective surfaces naturally without having to ask for it explicitly.",
                "what_to_listen_for": "Names, roles, perspectives. The 'I told you so' person is the most valuable — they represent the perspective most likely to be overlooked. Each stakeholder becomes a persona in the brainstorming phase.",
                "agent_posture": "Plussing: 'So we've got [A] who cares about [X], [B] who's watching [Y], and [C] who'd say I told you so about [Z]. That's a great set of lenses. Anyone else?' Build the list collaboratively.",
                "anti_patterns": [
                    "Treating this as a formal stakeholder analysis — keep it conversational",
                    "Forgetting to ask 'who would say I told you so?' — this is the most valuable question in the protocol"
                ],
                "captures": "stakeholders (name, primary_concern, likely_blind_spots, include_as_persona)"
            },
            {
                "turn": 6,
                "improv_mode": "Yes, And (forward-looking)",
                "psychology": "Peak-end rule begins — shift from exploration to commitment",
                "opening": "This is really coming together. So when we're done — when we've built the scenarios and worked through the probabilities — what does 'good enough' look like? What would make you look back and say 'that was worth the time'?",
                "why_this_comes_sixth": "Chermack's core contribution: define success criteria before building scenarios. But 'define assessment criteria' is sterile. 'What does good enough look like?' is human. The peak-end rule says the closing moments matter most — this turn begins the close by shifting from exploration ('what's possible?') to commitment ('what would make this worthwhile?').",
                "what_to_listen_for": "Concrete, observable criteria — not vague aspirations. 'We'd identify risks we hadn't seen before' is better than 'we'd feel more confident.' Also note the use case: are they building a monitoring dashboard? An investment thesis? A strategic decision framework?",
                "agent_posture": "Yes, And: 'So success looks like [X], [Y], and [Z]. AND let me add one more dimension — how will you actually USE the output? Is this going to be something you check quarterly, or something that informs a one-time decision, or...?'",
                "anti_patterns": [
                    "Accepting 'we'll just know if it was useful' — that's not scorable. Ask: 'What would you point to as evidence?'",
                    "Skipping the use case question — the format depends on the use case"
                ],
                "captures": "success_criteria, use_case"
            },
            {
                "turn": 7,
                "improv_mode": "Yes, But (closing with clarity)",
                "psychology": "Peak-end rule closes — provocative but supportive",
                "opening": "Last thing — and this is the one that keeps scenario planners up at night. What are we assuming right now that might turn out to be completely wrong? Not the obvious stuff — the quiet assumptions. The things we're taking for granted that, if they broke, would make this whole exercise irrelevant. And while we're at it — what constraints are we working within? Time, people, information we can't access?",
                "why_this_comes_last": "Chermack: hidden assumptions are the primary source of scenario error. But 'surface your assumptions' is abstract. 'What keeps scenario planners up at night' creates intrigue. Asking about constraints at the end is deliberate — after 6 turns, the user trusts the process enough to be honest about limitations. The peak-end rule: end with a memorable, slightly provocative question that opens up rather than closes down.",
                "what_to_listen_for": "Assumptions they're uncomfortable voicing — those are the most valuable. Constraints they're reluctant to name — those define the real boundary. If they say 'no assumptions, we've thought of everything' — gently note that's the most dangerous assumption of all.",
                "agent_posture": "Supportive but direct: 'This is the hard one, and it's okay if the answers aren't complete. The point isn't to be right about our assumptions — it's to know what they are so we can watch for when they break.'",
                "anti_patterns": [
                    "Rushing through this turn because it's the last one — this is where the most valuable information lives",
                    "Accepting 'no constraints' without a gentle probe — 'Unlimited time and perfect information? I want your job.'"
                ],
                "captures": "surfaced_assumptions, constraints, exploration_prompts"
            }
        ],

        "framing_document_template": {
            "focal_question": "<synthesized from turns 1-2>",
            "decision_at_stake": "<from turn 2 — what changes based on what we learn?>",
            "time_horizon": "<tactical|strategic|long_term — from turn 3>",
            "action_deadline": "<from turn 3 — when decision is needed>",
            "in_scope": ["<from turn 4>"],
            "out_of_scope": ["<from turn 4>"],
            "stakeholders": [{
                "role": "<from turn 5>",
                "primary_concern": "<what does this person care about?>",
                "likely_blind_spots": ["<what might they miss?>"],
                "include_as_persona": true
            }],
            "use_case": "<strategic_decision|investment_thesis|monitoring_dashboard|landscape_exploration|contingency_planning — from turn 6>",
            "success_criteria": ["<from turn 6>"],
            "constraints": ["<from turn 7>"],
            "surfaced_assumptions": ["<from turn 7>"],
            "exploration_prompts": ["<generated from turns 1+5 — what specific questions should personas explore?>"]
        },

        "after_framing": {
            "next_step": "The framing conversation naturally leads into brainstorming. The stakeholders from Turn 5 become personas. The exploration prompts from Turns 2+5 guide the divergent phase. The scope boundaries from Turn 4 keep the tree focused. Flow directly into scenario_brainstorm.",
            "pipeline": [
                "scenario_frame → conversational framing (this tool)",
                "scenario_brainstorm → multi-persona temperature-shifting protocol",
                "scenario_quantify → resolve conditional probability tree",
                "scenario_calibrate → Fermi decomposition + outside view",
                "scenario_synthesize → dragonfly-eye aggregation",
                "scenario_assess → evaluate against Turn 6 success criteria"
            ]
        },

        "agent_guidance": {
            "overall_posture": "Coach, not interviewer. Socratic but warm. The user is the domain expert; you are the method expert. Your job is to help them articulate what they already know but haven't yet made explicit.",
            "improv_rules": [
                "Never explicitly negate. 'That's interesting — let's dig into that' not 'That's too broad.'",
                "Yes, And: accept their answer and extend it naturally.",
                "Plussing: amplify what's clear, gently let go of what's fuzzy without calling it out.",
                "If the conversation is flowing, don't interrupt to ask the next question. Let the turns blend.",
                "If a turn produces a rich answer, stay there. The numbered turns are a scaffold, not a script."
            ],
            "pacing": {
                "target_duration": "15-20 minutes for all 7 turns",
                "if_stuck": "If a turn produces silence, reframe. 'Let me ask it differently...' Don't skip. Don't fill the silence for them.",
                "if_flowing": "If the user is on a roll, let them go. The turns are a guide, not a straitjacket. Capture insights wherever they land.",
                "too_fast": "If all 7 turns are done in 5 minutes, the framing is probably too shallow. Slow down. Ask follow-ups."
            },
            "when_to_redirect": [
                "Turn 2: The user describes a situation with no decision attached. Ask: 'What would you do differently if you knew the answer?'",
                "Turn 4: The user says everything is in scope. Ask: 'What's one thing that's definitely NOT relevant?'",
                "Turn 7: The user says they have no assumptions. Note: 'That's interesting — and it's actually the most common answer. Let me ask it differently: what would surprise you most if it turned out to be wrong?'",
                "Any turn: The user starts solving the problem instead of framing it. Gently: 'We'll get to that. First, let's make sure we're asking the right question.'"
            ],
            "minimalist_principle": "Seven conversational turns. 15-20 minutes. If a question doesn't change the scenario output, it doesn't belong in the conversation. The framing exists to make brainstorming productive — not to produce a document."
        },

        "references": {
            "chermack_2011": "Scenario Planning in Organizations — Phase 1: Project Preparation",
            "schwartz_1991": "The Art of the Long View — Stage 1: Focal Question",
            "kahneman_2011": "Thinking, Fast and Slow — System 1/2, loss aversion, peak-end rule",
            "cialdini_2006": "Influence: The Psychology of Persuasion — foot-in-the-door, social proof",
            "ryan_deci_2000": "Self-Determination Theory — autonomy, competence, relatedness",
            "hkask_improv": "Improv skill — Plussing, Yes And, Yes But postures",
            "hkask_kata": "Kata-Starter skill — coaching posture, 5 Questions Drill pattern"
        }
    })
}
