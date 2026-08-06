//! The `ScenariosWidget` GPUI view — renders the scenario pipeline overview,
//! calibration summary, event matrix, event tree list, and recent forecasts
//! inline in agent markdown. Read-only.
//!
//! This replaces the deleted standalone `ScenariosView` from `kask_panel`.
//! Data comes from the parsed `ScenariosBlockBody` instead of from `ToolInvoker`
//! MCP tool fetches — the agent (curator) calls `scenario_status` and emits
//! the result as a ```scenarios fenced block.

use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, IntoElement, ParentElement, Render, Styled,
    Window, div,
};
use gpui_util::ResultExt as _;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize, prelude::*};

use crate::block::{
    EventNode, FIBO_BRIER_SCORE, FIBO_FORECAST_ID, FIBO_SCENARIO_PROBABILITY, ScenariosBlockBody,
};

/// Server that hosts the scenario pipeline tools. Used as the fallback dispatch
/// target when a block carries no dispatchable provenance (T2 hardcoded path).
const DEFAULT_SERVER: &str = "hkask-mcp-scenarios";

/// Visible hint surfaced when the process-global `ToolInvoker` has not been
/// wired (e.g. before the post-login deferred task runs). Per the repo `.rules`
/// startup-failure-signal trap, this is a visible state, not a silent no-op.
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";

/// The scenarios widget view. Renders inline in agent markdown (via the D18
/// seam composed by `hkask-viz-core`).
pub struct ScenariosWidget {
    body: ScenariosBlockBody,
    focus_handle: FocusHandle,
    /// The tool name currently being dispatched, if a rung click is in flight.
    dispatch_in_flight: Option<String>,
    /// Visible error/hint surfaced when dispatch cannot proceed (missing
    /// invoker, provenance mismatch). Never silently dropped per repo `.rules`.
    dispatch_error: Option<String>,
    /// Text returned by the most recent successful dispatch, if any. Kept
    /// minimal — the agent conversation is the durable record.
    dispatch_result: Option<String>,
    /// Composed revision request surfaced as a copyable draft when the
    /// conversation injector is absent (no active conversation). Lets the user
    /// still use the "I disagree" body even when it can't be injected. Cleared
    /// when a successful inject fires (repo `.rules`: visible, not a silent
    /// no-op).
    disagree_draft: Option<String>,
}

impl ScenariosWidget {
    /// Create a new scenarios widget for the parsed block body.
    pub fn new(body: ScenariosBlockBody, cx: &mut Context<Self>) -> Self {
        hkask_tool_invoker::record_render(
            body.provenance.tool.clone(),
            body.provenance.span_id.clone(),
        );
        tracing::info!(
            target: "reg.widget.render",
            tool = body.provenance.tool.as_deref().unwrap_or(""),
            span_id = body.provenance.span_id.as_deref().unwrap_or(""),
            "REG",
        );
        Self {
            body,
            focus_handle: cx.focus_handle(),
            dispatch_in_flight: None,
            dispatch_error: None,
            dispatch_result: None,
            disagree_draft: None,
        }
    }

    fn render_scaffolding(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let prompt = scaffolding_for_state(&self.body);
        let rung_tool = prompt.tool_hint.clone();

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex().gap_2().child(
                    div()
                        .id("scenarios-scaffolding-next")
                        .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                            this.dispatch_rung(&rung_tool.clone(), cx);
                        }))
                        .child(
                            Label::new(format!("Next: {}", prompt.stage))
                                .size(LabelSize::Small)
                                .color(Color::Accent),
                        )
                        .child(
                            Label::new(format!("→ /{}", prompt.tool_hint))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        ),
                ),
            )
            .child(
                Label::new(prompt.prompt)
                    .size(LabelSize::Small)
                    .color(Color::Default),
            )
            .into_any_element()
    }

    fn render_pipeline_stages(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let stages: Vec<AnyElement> = PIPELINE_STAGES
            .iter()
            .map(|(stage, tool, desc)| {
                let rung_tool = (*tool).to_string();
                v_flex()
                    .gap_0p5()
                    .id((*tool).to_string())
                    .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                        this.dispatch_rung(&rung_tool.clone(), cx);
                    }))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Label::new(*stage)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(format!("/{tool}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        Label::new(*desc)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new("Scenario Pipeline (Schwartz → Tetlock → Chermack)")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(stages)
            .into_any_element()
    }

    fn render_pipeline_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let pipeline = &self.body.pipeline;

        let brier_text = pipeline
            .overall_brier
            .map(|b| format!("{b:.3}"))
            .unwrap_or_else(|| "—".to_string());

        let tiles = vec![
            ("Forecasts", format!("{}", pipeline.forecast_count)),
            ("Resolved", format!("{}", pipeline.resolved_count)),
            ("Pending", format!("{}", pipeline.pending_count)),
            ("Brier", brier_text),
        ];

        let tile_elements: Vec<AnyElement> = tiles
            .into_iter()
            .map(|(label, value)| {
                v_flex()
                    .p_2()
                    .gap_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(label)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(value)
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    .into_any_element()
            })
            .collect();

        h_flex()
            .gap_2()
            .flex_wrap()
            .children(tile_elements)
            .into_any_element()
    }

    fn render_calibration(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(cal) = &self.body.calibration else {
            return div().into_any_element();
        };

        let brier_text = cal
            .overall_brier
            .map(|b| format!("{b:.3}"))
            .unwrap_or_else(|| "—".to_string());
        let overconf_text = cal
            .overconfidence_score
            .map(|o| format!("{o:+.3}"))
            .unwrap_or_else(|| "—".to_string());
        let interp = cal.interpretation.as_deref().unwrap_or("unknown");

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!("Calibration ({FIBO_BRIER_SCORE})"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Label::new(format!("Brier: {brier_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("Overconfidence: {overconf_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!(
                            "{}/{} resolved",
                            cal.resolved_forecasts, cal.total_forecasts
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    ),
            )
            .child(
                Label::new(interp)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .into_any_element()
    }

    fn render_event_matrix(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(tree) = &self.body.event_tree else {
            return div().into_any_element();
        };
        if tree.nodes.is_empty() {
            return div().into_any_element();
        }

        let nodes: Vec<&EventNode> = tree.nodes.iter().collect();

        let dots: Vec<AnyElement> = nodes
            .iter()
            .map(|node| {
                let prob = node
                    .probability
                    .or(node.marginal_probability)
                    .unwrap_or(0.5);
                let uncertainty = (prob - 0.5).abs();
                let color = if uncertainty > 0.3 {
                    Color::Warning
                } else if prob > 0.5 {
                    Color::Created
                } else {
                    Color::Muted
                };
                let prob_pct = format!("{:.0}%", prob * 100.0);
                let unc_pct = format!("{:.0}%", uncertainty * 100.0);

                h_flex()
                    .gap_2()
                    .child(
                        Label::new(node.name.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("P={prob_pct}"))
                            .size(LabelSize::XSmall)
                            .color(color),
                    )
                    .child(
                        Label::new(format!("unc={unc_pct}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!(
                    "Event Matrix ({FIBO_SCENARIO_PROBABILITY}) — {} events",
                    tree.nodes.len()
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(
                Label::new("Events sorted by probability. High-uncertainty events (near 50%) are worth calibrating.")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .children(dots)
            .into_any_element()
    }

    fn render_event_tree_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(tree) = &self.body.event_tree else {
            return div().into_any_element();
        };

        let joint_text = tree
            .joint_probability
            .map(|j| format!("{j:.4}"))
            .unwrap_or_else(|| "—".to_string());

        let nodes: Vec<AnyElement> = tree
            .nodes
            .iter()
            .map(|node| {
                let prob = node
                    .probability
                    .or(node.marginal_probability)
                    .unwrap_or(0.0);
                let prob_pct = format!("{:.0}%", prob * 100.0);
                let tier = node
                    .certainty_tier
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let brier = node
                    .brier_score
                    .map(|b| format!("Brier={b:.3}"))
                    .unwrap_or_default();
                let parents = if node.parent_ids.is_empty() {
                    "root".to_string()
                } else {
                    format!("← {}", node.parent_ids.join(", "))
                };

                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Label::new(node.name.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(prob_pct)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            )
                            .child(Label::new(tier).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                Label::new(parents)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .when(!brier.is_empty(), |this| {
                        this.child(
                            Label::new(brier)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .when_some(node.question.as_ref(), |this, q| {
                        this.child(Label::new(q).size(LabelSize::XSmall).color(Color::Muted))
                    })
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Event Tree: {}", tree.subject))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("Joint P={joint_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .child(
                        Label::new(format!("{} events", tree.event_count))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .children(nodes)
            .into_any_element()
    }

    fn render_recent_forecasts(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let recent = &self.body.recent_forecasts;
        if recent.is_empty() {
            return div().into_any_element();
        }

        let rows: Vec<AnyElement> = recent
            .iter()
            .map(|f| {
                let prob_pct = format!("{:.0}%", f.probability * 100.0);
                let outcome_text = f
                    .outcome
                    .map(|o| if o { "✓ occurred" } else { "✗ didn't" })
                    .unwrap_or("pending");
                let color = if f.outcome.is_some() {
                    Color::Muted
                } else {
                    Color::Accent
                };

                h_flex()
                    .gap_2()
                    .child(
                        Label::new(f.event_name.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(Label::new(prob_pct).size(LabelSize::XSmall).color(color))
                    .child(
                        Label::new(outcome_text)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!("Recent Forecasts ({FIBO_FORECAST_ID})"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }

    /// Render a visible dispatch status row: pending spinner label, error/hint,
    /// or the truncated result of the last successful dispatch. Emits nothing
    /// when idle so the widget stays compact.
    fn render_dispatch_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(tool) = &self.dispatch_in_flight {
            return Some(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Dispatching /{tool} …"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.dispatch_error {
            let border_color = cx.theme().colors().border;
            return Some(
                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(error.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                    .into_any_element(),
            );
        }
        if let Some(result) = &self.dispatch_result {
            // Unwrap the MCP `{"content": …}` envelope (repo `.rules` single
            // seam) so the user sees the payload, not the wrapper.
            let payload = hkask_types::tool_response::parse_tool_response(result)
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .unwrap_or_else(|| result.clone());
            // Truncate long tool output so the conversation remains the durable record.
            let preview = if payload.chars().count() > 120 {
                let truncated: String = payload.chars().take(120).collect();
                format!("{truncated}…")
            } else {
                payload
            };
            return Some(
                Label::new(format!("Dispatched: {preview}"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .into_any_element(),
            );
        }
        None
    }

    /// Shared `on_click` body for the scaffolding `Next:` label and each
    /// `PIPELINE_STAGES` rung. Resolves the dispatch plan from the block's
    /// provenance and the rung's tool, then routes through the governed
    /// `shared_tool_invoker()` (OCAP/gas-budgeted in production via
    /// `McpRuntime`).
    ///
    /// Surfaced states (never silent per repo `.rules`):
    /// - `INVOKER_NOT_WIRED_MSG` when `shared_tool_invoker()` returns `None`.
    pub(crate) fn dispatch_rung(&mut self, rung_tool: &str, cx: &mut Context<Self>) {
        let (server, tool, args) = build_dispatch_args(
            &self.body.provenance,
            rung_tool,
            DEFAULT_SERVER,
            serde_json::json!({}),
        );

        let invoker = match shared_tool_invoker() {
            None => {
                self.dispatch_error = Some(INVOKER_NOT_WIRED_MSG.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
            Some(invoker) => invoker,
        };

        self.dispatch_error = None;
        self.dispatch_in_flight = Some(tool.clone());
        let task = invoker.invoke_tool(&server, &tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                this.dispatch_in_flight = None;
                match outcome {
                    Ok(text) => {
                        this.dispatch_result = Some(text);
                        this.dispatch_error = None;
                    }
                    Err(error) => {
                        this.dispatch_error = Some(error);
                        this.dispatch_result = None;
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Compose the provenance-scoped "I disagree" body. References the
    /// scenario subject (from the event tree) and the provenance tool so the
    /// agent can correlate the revision request to the exact `scenario_status`
    /// result the widget rendered. Falls back to a generic "this scenario"
    /// framing when the event tree or its subject is absent (grill-me edge
    /// case c).
    fn compose_disagree_body(&self) -> String {
        let subject = self
            .body
            .event_tree
            .as_ref()
            .map(|tree| tree.subject.clone())
            .filter(|subject| !subject.is_empty())
            .unwrap_or_else(|| "this scenario".to_string());
        let tool = self
            .body
            .provenance
            .tool
            .as_deref()
            .unwrap_or("scenario_status");
        // Reference the ontology anchor when available so the agent can
        // correlate the revision to the ontology-anchored artifact.
        let anchor_clause = self
            .body
            .ontology
            .as_deref()
            .filter(|a| !a.is_empty())
            .map(|a| format!(" [{a}]"))
            .unwrap_or_default();
        format!(
            "Re: the scenario assessment for {subject} (via {tool}){anchor_clause}.\n\
             I believe this scenario assessment is incorrect. Please re-check the framing, events, and probabilities.\n\n\
             My concern: "
        )
    }

    /// The "I disagree" affordance handler (C). Composes the provenance-scoped
    /// revision request and injects it back into the active conversation via
    /// the kask `shared_injector()` (D21 widget→agent seam). When no
    /// conversation is active, surfaces the composed body as a copyable draft
    /// instead of a silent no-op (repo `.rules`). Never auto-sends when the
    /// injector is absent — the production injector only pre-fills the
    /// composer; the user reviews and submits.
    fn on_disagree_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.compose_disagree_body();
        tracing::info!(target: "reg.widget.disagree", "REG");
        if let Some(injector) = hkask_conversation_injector::shared_injector() {
            // The production injector pre-fills the editor synchronously and
            // returns a `Task::ready(Ok(()))`; the returned `Result` is always
            // `Ok`. Await in a detached task so a hypothetical async impl's
            // error path is surfaced (not silently dropped — repo `.rules`),
            // and so `clippy::let_underscore_future` is not triggered.
            let draft = body.clone();
            let task = injector.inject(body, window, cx);
            cx.spawn(async move |this, cx| {
                if let Err(error) = task.await {
                    tracing::warn!(
                        target: "reg.widget.disagree",
                        error = %error,
                        "conversation inject failed; surfacing draft"
                    );
                    this.update(cx, |this, cx| {
                        this.disagree_draft = Some(draft);
                        cx.notify();
                    })
                    .log_err();
                }
            })
            .detach();
            self.disagree_draft = None;
        } else {
            // No active conversation: surface the composed body as a draft so
            // the user can still copy it into chat (visible, not a silent
            // no-op — repo `.rules`).
            self.disagree_draft = Some(body);
        }
        cx.notify();
    }
}

impl Focusable for ScenariosWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScenariosWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new("Scenario Planning")
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    // C: "I disagree" affordance — composes a provenance-scoped
                    // revision request back into the active conversation (D21).
                    .child(
                        div()
                            .id("scenarios-disagree")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.on_disagree_click(window, cx);
                            }))
                            .child(
                                Label::new("I disagree")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            ),
                    ),
            )
            // Fallback draft (no active conversation): surface the composed
            // body so the user can copy it into chat — visible, not a silent
            // no-op (repo `.rules`).
            .when_some(self.disagree_draft.clone(), |this, draft| {
                this.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(border_color)
                        .child(
                            Label::new(draft)
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        ),
                )
            })
            .child(self.render_scaffolding(cx))
            .when_some(self.render_dispatch_status(cx), |this, status| {
                this.child(status)
            })
            .child(self.render_pipeline_stages(cx))
            .child(self.render_pipeline_overview(cx))
            .child(self.render_calibration(cx))
            .child(self.render_event_matrix(cx))
            .child(self.render_event_tree_list(cx))
            .child(self.render_recent_forecasts(cx))
    }
}

// ── Instruction scaffolding ───────────────────────────────────────────────

struct ScaffoldingPrompt {
    stage: &'static str,
    prompt: String,
    tool_hint: String,
}

fn scaffolding_for_state(body: &ScenariosBlockBody) -> ScaffoldingPrompt {
    let p = &body.pipeline;

    if p.forecast_count == 0 {
        return ScaffoldingPrompt {
            stage: "Frame",
            prompt: "No forecasts recorded. Run scenario_frame to start a 7-turn coaching conversation that scopes your decision. Then scenario_brainstorm to generate candidate events.".to_string(),
            tool_hint: "scenario_frame".to_string(),
        };
    }

    if p.resolved_count == 0 {
        return ScaffoldingPrompt {
            stage: "Calibrate",
            prompt: format!(
                "You have {} pending forecast(s). Run scenario_quantify to resolve probabilities, then scenario_calibrate to apply Fermi decomposition and base-rate shrinkage.",
                p.pending_count
            ),
            tool_hint: "scenario_quantify".to_string(),
        };
    }

    let brier_text = p
        .overall_brier
        .map(|b| format!("Overall Brier: {b:.3}"))
        .unwrap_or_else(|| "No Brier score yet".to_string());

    if let Some(ref cal) = body.calibration {
        let interp = cal.interpretation.as_deref().unwrap_or("unknown");
        return ScaffoldingPrompt {
            stage: "Learn",
            prompt: format!(
                "{brier_text} — {interp}. {}/{} forecasts resolved. Run scenario_assess to evaluate the project across Chermack's five phases, or scenario_cross_validate to compare independent estimates.",
                cal.resolved_forecasts, cal.total_forecasts
            ),
            tool_hint: "scenario_assess".to_string(),
        };
    }

    ScaffoldingPrompt {
        stage: "Score",
        prompt: format!(
            "{brier_text}. {}/{} resolved. Run scenario_score to compute Brier scores, then scenario_calibration to build your calibration curve.",
            p.resolved_count, p.forecast_count
        ),
        tool_hint: "scenario_score".to_string(),
    }
}

/// Pipeline stage guidance — the full Schwartz/Tetlock/Chermack flow.
const PIPELINE_STAGES: &[(&str, &str, &str)] = &[
    (
        "1. Frame",
        "scenario_frame",
        "7-turn coaching conversation: what decision hangs on this?",
    ),
    (
        "2. Brainstorm",
        "scenario_brainstorm",
        "4-round protocol: diverge → ground → link → prune",
    ),
    (
        "3. Build",
        "scenario_build",
        "Extract events from research into a dependency tree",
    ),
    (
        "4. Quantify",
        "scenario_quantify",
        "Resolve marginal + joint probabilities, sensitivity ranking",
    ),
    (
        "5. Calibrate",
        "scenario_calibrate",
        "Fermi decomposition + outside/inside view + bias correction",
    ),
    (
        "6. Score",
        "scenario_score",
        "Brier scoring against known outcomes",
    ),
    (
        "7. Assess",
        "scenario_assess",
        "Chermack 5-phase project assessment",
    ),
];

// ── Pure dispatch-planning logic (T2/T4) ──────────────────────────────────
//
// Kept free of GPUI executor / global state so the dispatch decision is
// unit-testable directly (the repo `.rules` racy-global trap: never unit-test by
// mutating `set_tool_invoker`). The `on_click` handler in `dispatch_rung`
// composes this pure function with `shared_tool_invoker()`.

/// Build the dispatch tuple for a pipeline-rung click.
///
/// Rungs *advance* to a different pipeline tool than the one that produced
/// the block (a `scenario_status` block's "Frame" rung dispatches
/// `scenario_frame`), so `provenance.tool` is NOT a dispatch guard — the
/// rung's own tool is authoritative. Provenance contributes only its `server`
/// (for alias dispatch); the args come from the rung, not the producer.
fn build_dispatch_args(
    provenance: &BlockProvenance,
    rung_tool: &str,
    default_server: &str,
    default_args: serde_json::Value,
) -> (String, String, serde_json::Value) {
    let server = provenance
        .server
        .clone()
        .unwrap_or_else(|| default_server.to_string());
    (server, rung_tool.to_string(), default_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{CalibrationSummary, PipelineOverview, parse_scenarios_body};

    #[test]
    fn scaffolding_empty_pipeline_suggests_frame() {
        let body = ScenariosBlockBody::default();
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Frame");
        assert_eq!(prompt.tool_hint, "scenario_frame");
    }

    #[test]
    fn scaffolding_pending_forecasts_suggests_quantify() {
        let body = ScenariosBlockBody {
            viz: Some("scenarios".into()),
            pipeline: PipelineOverview {
                forecast_count: 3,
                resolved_count: 0,
                pending_count: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Calibrate");
        assert_eq!(prompt.tool_hint, "scenario_quantify");
    }

    #[test]
    fn scaffolding_resolved_with_calibration_suggests_assess() {
        let body = ScenariosBlockBody {
            viz: Some("scenarios".into()),
            pipeline: PipelineOverview {
                forecast_count: 5,
                resolved_count: 3,
                pending_count: 2,
                overall_brier: Some(0.12),
                ..Default::default()
            },
            calibration: Some(CalibrationSummary {
                total_forecasts: 5,
                resolved_forecasts: 3,
                overall_brier: Some(0.12),
                overconfidence_score: Some(0.05),
                interpretation: Some("good".to_string()),
            }),
            ..Default::default()
        };
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Learn");
        assert_eq!(prompt.tool_hint, "scenario_assess");
    }

    // ── build_dispatch_args pure-logic tests (required minimum) ────────────

    fn provenance_with(
        tool: Option<&str>,
        server: Option<&str>,
        args: serde_json::Value,
    ) -> BlockProvenance {
        BlockProvenance {
            tool: tool.map(str::to_string),
            server: server.map(str::to_string),
            args,
            span_id: None,
        }
    }

    #[test]
    fn dispatch_args_empty_provenance_falls_back_to_hardcoded() {
        // T2 fallback: a block with no provenance dispatches the rung's tool
        // against the default server with the hardcoded default args.
        let provenance = BlockProvenance::default();
        assert!(!provenance.is_dispatchable());

        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_frame_rung_yields_scenario_frame_default() {
        // Pins the T2 acceptance shape: the Frame rung dispatches the
        // `scenario_frame` tool against the scenarios server with empty args.
        let provenance = BlockProvenance::default();
        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_dispatchable_uses_provenance_server_and_rung_args() {
        // Provenance is now server-only: provenance.tool is NOT a dispatch
        // guard (rungs advance to a different tool than the producer), and
        // provenance.args is ignored for rungs — the rung's own tool and the
        // default args are authoritative.
        let provenance = provenance_with(
            Some("scenario_quantify"),
            Some("hkask-mcp-scenarios"),
            serde_json::json!({"event_id": "e1", "subject": "AAPL"}),
        );
        assert!(provenance.is_dispatchable());

        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_quantify",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_quantify");
        // provenance.args is NOT merged into the rung dispatch — rung args win.
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_dispatchable_match_uses_provenance_server_not_default() {
        // Provenance.server is used for alias dispatch; the default server is
        // only the fallback when provenance.server is absent.
        let provenance = provenance_with(
            Some("scenario_quantify"),
            Some("hkask-mcp-scenarios-staging"),
            serde_json::json!({}),
        );
        let (server, tool, _args) = build_dispatch_args(
            &provenance,
            "scenario_quantify",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios-staging");
        assert_eq!(tool, "scenario_quantify");
    }

    #[test]
    fn dispatch_args_status_block_rung_dispatches_not_mismatch() {
        // D1 regression: a `scenario_status`-produced block (provenance.tool
        // = "scenario_status", server = "hkask-mcp-scenarios") + a
        // `scenario_frame` rung must DISPATCH, not surface a mismatch error.
        // Rungs advance to a different tool than the producer; provenance.tool
        // is NOT a guard. Provenance contributes only its server.
        let provenance = provenance_with(
            Some("scenario_status"),
            Some("hkask-mcp-scenarios"),
            serde_json::json!({}),
        );
        assert!(provenance.is_dispatchable());

        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_non_dispatchable_with_tool_some_falls_back() {
        // tool is Some but server is None → provenance.server is None →
        // fallback to default server. The rung's own tool + args are still
        // authoritative.
        let provenance = provenance_with(Some("scenario_status"), None, serde_json::json!({}));
        assert!(!provenance.is_dispatchable());

        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
        );
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    // ── GPUI integration tests via the governed `shared_tool_invoker()` ──────
    //
    // These mutate the process-global `ToolInvoker`, which is racy across
    // parallel tests (the repo `.rules` racy-global trap). `GLOBAL_TEST_LOCK`
    // serializes the two tests below within this crate's test binary so they
    // never observe each other's invoker.
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Records `(server, tool, args)` for every `invoke_tool` call and returns
    /// canned JSON. Implements `Send + Sync` so it can be stored behind the
    /// `Arc<dyn ToolInvoker>` process global.
    #[derive(Default)]
    struct MockToolInvoker {
        calls: std::sync::Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl hkask_tool_invoker::ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> gpui::Task<Result<String, String>> {
            self.calls
                .lock()
                .expect("MockToolInvoker calls mutex poisoned")
                .push((server.to_string(), tool.to_string(), args));
            gpui::Task::ready(Ok("{}".to_string()))
        }
    }

    /// RAII guard that restores the global invoker to `None` on drop so a test
    /// failure cannot leak a mock into sibling tests.
    struct InvokerGuard;
    impl Drop for InvokerGuard {
        fn drop(&mut self) {
            hkask_tool_invoker::set_tool_invoker(None);
        }
    }

    #[gpui::test]
    async fn dispatch_rung_routes_frame_rung_through_invoker(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        // Empty body → empty (non-dispatchable) provenance → T2 hardcoded fallback.
        let widget =
            cx.update(|cx| cx.new(|cx| ScenariosWidget::new(ScenariosBlockBody::default(), cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let calls = mock.calls.lock().expect("calls poisoned").clone();
        assert_eq!(calls.len(), 1, "exactly one dispatch");
        assert_eq!(calls[0].0, "hkask-mcp-scenarios");
        assert_eq!(calls[0].1, "scenario_frame");
        assert_eq!(calls[0].2, serde_json::json!({}));

        // The widget cleared in-flight and recorded the canned result.
        let (in_flight, has_result) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_in_flight.clone(),
                    widget.dispatch_result.is_some(),
                )
            })
        });
        assert!(in_flight.is_none(), "in-flight cleared after completion");
        assert!(has_result, "successful dispatch stored a result");
    }

    #[gpui::test]
    async fn dispatch_rung_surfaces_missing_invoker_as_visible_error(
        cx: &mut gpui::TestAppContext,
    ) {
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        hkask_tool_invoker::set_tool_invoker(None);

        let widget =
            cx.update(|cx| cx.new(|cx| ScenariosWidget::new(ScenariosBlockBody::default(), cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert_eq!(error.as_deref(), Some(INVOKER_NOT_WIRED_MSG));
        assert!(in_flight.is_none());
    }

    #[gpui::test]
    async fn dispatch_rung_routes_rung_for_server_produced_block(cx: &mut gpui::TestAppContext) {
        // D1 GPUI regression: a `scenario_status`-produced block + a
        // `scenario_frame` rung click must dispatch through the invoker
        // against the provenance server with the rung's tool and empty args —
        // NOT surface a provenance-mismatch error and dispatch nothing.
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let body = ScenariosBlockBody {
            provenance: BlockProvenance {
                tool: Some("scenario_status".into()),
                server: Some("hkask-mcp-scenarios".into()),
                args: serde_json::json!({}),
                span_id: None,
            },
            ..Default::default()
        };
        let widget = cx.update(|cx| cx.new(|cx| ScenariosWidget::new(body, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert!(
            error.is_none(),
            "no dispatch error for rung on produced block"
        );
        assert!(in_flight.is_none(), "in-flight cleared after completion");
        let calls = mock.calls.lock().expect("calls poisoned").clone();
        assert_eq!(calls.len(), 1, "exactly one dispatch for the rung");
        assert_eq!(calls[0].0, "hkask-mcp-scenarios");
        assert_eq!(calls[0].1, "scenario_frame");
        assert_eq!(calls[0].2, serde_json::json!({}));
    }

    // ── "I disagree" compose-back affordance (C, D21) ───────────────────────
    //
    // Mirrors the portfolio widget's disagree tests. These mutate the
    // process-global `ConversationInjector` (a separate global from
    // `TOOL_INVOKER`), so they take `GLOBAL_TEST_LOCK` too and use an RAII
    // `ConversationInjectorGuard` to reset the global on drop.

    /// Records the body of every `inject` call. `Send + Sync` for the
    /// `Arc<dyn ConversationInjector>` global.
    #[derive(Default)]
    struct MockConversationInjector {
        bodies: std::sync::Mutex<Vec<String>>,
    }

    impl hkask_conversation_injector::ConversationInjector for MockConversationInjector {
        fn inject(
            &self,
            body: String,
            _window: &mut gpui::Window,
            _cx: &mut gpui::App,
        ) -> gpui::Task<Result<(), String>> {
            self.bodies
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(body);
            gpui::Task::ready(Ok(()))
        }
    }

    /// RAII guard that restores the conversation-injector global to `None` on
    /// drop so a test failure cannot leak a mock into sibling tests.
    struct ConversationInjectorGuard;
    impl Drop for ConversationInjectorGuard {
        fn drop(&mut self) {
            hkask_conversation_injector::set_active_injector(None);
        }
    }

    /// Trivial root view for `add_window_view` so the test can obtain a `Window`
    /// for `on_disagree_click` without rendering `ScenariosWidget` (which would
    /// need a theme global this leaf crate's tests don't initialise). Renders a
    /// bare `div()`.
    struct DummyView;
    impl Render for DummyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// Body with an event-tree subject and dispatchable provenance, so the
    /// disagree body references both the subject and the provenance tool.
    fn body_with_subject_and_provenance() -> ScenariosBlockBody {
        ScenariosBlockBody {
            viz: Some("scenarios".into()),
            pipeline: PipelineOverview::default(),
            calibration: None,
            event_tree: Some(crate::block::EventTreeSummary {
                subject: "AAPL earnings".into(),
                event_count: 2,
                joint_probability: Some(0.12),
                root_ids: Vec::new(),
                nodes: Vec::new(),
            }),
            recent_forecasts: Vec::new(),
            provenance: BlockProvenance {
                tool: Some("scenario_status".into()),
                server: Some("hkask-mcp-scenarios".into()),
                args: serde_json::json!({}),
                span_id: None,
            },
            ontology: None,
        }
    }

    #[gpui::test]
    async fn disagree_routes_through_injector(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = ConversationInjectorGuard;
        let mock = std::sync::Arc::new(MockConversationInjector::default());
        hkask_conversation_injector::set_active_injector(Some(mock.clone()));

        let body = body_with_subject_and_provenance();
        // Use a throwaway window root so we get a `Window` for `on_disagree_click`
        // without rendering `ScenariosWidget` (no theme global in these tests).
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| ScenariosWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(bodies.len(), 1, "exactly one inject");
        assert!(bodies[0].contains("Re:"), "body references the revision");
        assert!(
            bodies[0].contains("AAPL earnings"),
            "body references the event-tree subject"
        );
        assert!(
            bodies[0].contains("scenario_status"),
            "body references the provenance tool"
        );

        // A successful inject clears the fallback draft.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        assert!(draft.is_none(), "draft cleared after a successful inject");
    }

    #[gpui::test]
    async fn disagree_surfaces_draft_when_no_injector(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = ConversationInjectorGuard;
        hkask_conversation_injector::set_active_injector(None);

        let body = body_with_subject_and_provenance();
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| ScenariosWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        // No injector: the composed body is surfaced as a copyable draft
        // (visible, not a silent no-op — repo `.rules`), and no panic.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        let draft = draft.expect("draft surfaced when no injector is active");
        assert!(draft.contains("Re:"), "draft carries the revision prefix");
        assert!(draft.contains("AAPL earnings"), "draft carries the subject");
    }

    #[gpui::test]
    async fn disagree_body_falls_back_when_subject_absent(cx: &mut gpui::TestAppContext) {
        // grill-me edge case (c): absent event tree → generic "this scenario"
        // framing. `compose_disagree_body` is pure, so no window is needed.
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = ConversationInjectorGuard;

        let empty = ScenariosBlockBody::default();
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| ScenariosWidget::new(empty, cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("this scenario"),
            "absent subject falls back to the generic framing"
        );
        assert!(
            body.contains("scenario_status"),
            "absent provenance tool falls back to the default tool"
        );
    }

    #[test]
    fn block_body_parses_ontology_field() {
        // The server emits `"ontology": "pko:Procedure"` or `"dcterms:Dataset"`.
        // The widget must parse it (additive `#[serde(default)]`).
        let json = r##"{"viz":"scenarios","ontology":"pko:Procedure"}"##;
        let body = parse_scenarios_body(json).expect("parses");
        assert_eq!(body.ontology.as_deref(), Some("pko:Procedure"));
    }

    #[test]
    fn block_body_parses_without_ontology_field() {
        // Older blocks without the field still parse (defaults to None).
        let json = r##"{"viz":"scenarios"}"##;
        let body = parse_scenarios_body(json).expect("parses");
        assert!(body.ontology.is_none());
    }

    #[gpui::test]
    async fn disagree_body_includes_ontology_when_present(cx: &mut gpui::TestAppContext) {
        // When the block carries an ontology tag, the compose-back body
        // references it so the agent can correlate the revision.
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = ConversationInjectorGuard;

        let mut body = body_with_subject_and_provenance();
        body.ontology = Some("pko:Procedure".to_string());
        let widget = cx.update(|cx| cx.new(|cx| ScenariosWidget::new(body, cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("[pko:Procedure]"),
            "compose-back body must reference the ontology concept: {body}"
        );
    }
}
