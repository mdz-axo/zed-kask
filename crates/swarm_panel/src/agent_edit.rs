//! Agent edit (drill-down from browse card → author form pre-loaded with the
//! agent's existing details). Extracted from `swarm_panel.rs` — the methods
//! stay on `SwarmPanel` (they dispatch via `cx.listener` / `cx.spawn`); this
//! module owns the load + save orchestration.
//!
//! Two entry points:
//! - `load_agent_into_author`: fetches the full agent card and stores it in
//!   `pending_author_load`. The `render` method (which has `&mut Window`,
//!   required by `Editor::set_text`) applies it to the form on the next frame.
//!   Mode-aware: cloud/synced agents use `swarm_get_agent`; local agents
//!   re-fetch via `swarm_list_local_agents` and filter (the list response
//!   carries the full `LocalAgentCard`, including `system_prompt`).
//! - `save_agent`: persists edits. Local agents use
//!   `swarm_reconfigure_local_agent` (updates `system_prompt`/`model`/
//!   `mcp_tools`/`skills`, preserves `cloud_id` and the rest of the card).
//!   Cloud agents have no update tool — the form renders read-only with a
//!   note pointing the operator to "Clone to Local" to edit.

use gpui::{Context, Window};
use serde_json::json;

use crate::parse::AgentSource;
use crate::SwarmPanel;

/// The fields extracted from an agent card that the author form can populate.
/// Source: `swarm_get_agent` (cloud) or `swarm_list_local_agents` (local).
/// Stored on `SwarmPanel::pending_author_load` and applied in `render`
/// (which has `&mut Window`, required by `Editor::set_text` — the spawn
/// closure does not).
pub(crate) struct AgentDetail {
    pub(crate) agent_id: String,
    pub(crate) agent_type: String,
    pub(crate) description: String,
    pub(crate) system_prompt: String,
    pub(crate) tags: Vec<String>,
    pub(crate) visibility: String,
    pub(crate) valence_arousal: Option<f64>,
    pub(crate) valence_valence: Option<f64>,
    pub(crate) valence_primary_affect: Option<String>,
    pub(crate) valence_personality_traits: Vec<String>,
}

impl AgentDetail {
    /// Parse a local agent card from the `swarm_list_local_agents` response.
    /// The card is the serialized `LocalAgentCard` struct (see
    /// `hkask-mcp-swarm/src/local_registry.rs`).
    fn parse_local(card: &serde_json::Value) -> Self {
        let agent_id = card
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let agent_type = card
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("research")
            .to_string();
        let description = card
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let system_prompt = card
            .get("capabilities")
            .and_then(|c| c.get("system_prompt"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let tags = card
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let visibility = card
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("private")
            .to_string();
        let valence = card.get("valence");
        let (valence_arousal, valence_valence, valence_primary_affect, valence_personality_traits) =
            parse_valence(valence);
        Self {
            agent_id,
            agent_type,
            description,
            system_prompt,
            tags,
            visibility,
            valence_arousal,
            valence_valence,
            valence_primary_affect,
            valence_personality_traits,
        }
    }

    /// Parse a cloud agent card from the `swarm_get_agent` response. The card
    /// shape mirrors `CreateAgentRequest`'s output (see
    /// `hkask-mcp-swarm/src/cloud_tools.rs::build_agent_card`):
    /// `agent_id`/`agent_type`/`system_prompt`/`visibility` top-level,
    /// `metadata.description`/`metadata.tags`/`metadata.valence`,
    /// `capabilities.model`/`capabilities.mcp_tools`/`capabilities.skills`.
    fn parse_cloud(card: &serde_json::Value) -> Self {
        let agent_id = card
            .get("agent_id")
            .or_else(|| card.get("agent_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let agent_type = card
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("research")
            .to_string();
        let description = card
            .get("metadata")
            .and_then(|m| m.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        let system_prompt = card
            .get("system_prompt")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let tags = card
            .get("metadata")
            .and_then(|m| m.get("tags"))
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let visibility = card
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("private")
            .to_string();
        let valence = card.get("metadata").and_then(|m| m.get("valence"));
        let (valence_arousal, valence_valence, valence_primary_affect, valence_personality_traits) =
            parse_valence(valence);
        Self {
            agent_id,
            agent_type,
            description,
            system_prompt,
            tags,
            visibility,
            valence_arousal,
            valence_valence,
            valence_primary_affect,
            valence_personality_traits,
        }
    }
}

/// Extract valence fields from a `metadata.valence` (cloud) or top-level
/// `valence` (local) object. Returns `(arousal, valence, primary_affect,
/// personality_traits)` with `None`/empty for absent fields.
fn parse_valence(
    valence: Option<&serde_json::Value>,
) -> (Option<f64>, Option<f64>, Option<String>, Vec<String>) {
    let v = match valence {
        Some(v) => v,
        None => return (None, None, None, Vec::new()),
    };
    let arousal = v.get("arousal").and_then(|a| a.as_f64());
    let valence = v.get("valence").and_then(|a| a.as_f64());
    let primary_affect = v
        .get("primary_affect")
        .and_then(|a| a.as_str())
        .map(String::from);
    let personality_traits = v
        .get("personality_traits")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    (arousal, valence, primary_affect, personality_traits)
}

impl SwarmPanel {
    /// Open the author panel with `agent`'s existing details loaded, so the
    /// operator can view the full settings and adjust them. Triggered by
    /// double-click on the card or the Edit affordance.
    ///
    /// Sets `editing_id` on the form so the submit path knows it's an edit,
    /// not a create. The name field is made read-only (renaming would change
    /// the agent id — a different operation). The actual form population is
    /// deferred to `render` (which has `&mut Window` for `Editor::set_text`);
    /// the spawn stores the result in `pending_author_load`.
    pub(crate) fn load_agent_into_author(
        &mut self,
        agent_id: String,
        source: AgentSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.author.editing_id = Some(agent_id.clone());
        self.author.status = Some("Loading agent details…".into());
        self.author.busy = false;
        self.author.name.update(cx, |e, _| e.set_read_only(true));
        self.pending_author_load = None;
        self.set_mode(crate::PanelMode::Author, window, cx);

        let is_local = source == AgentSource::Local;
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = if is_local {
                    let list_result = invoker
                        .invoke_tool(
                            crate::SWARM_SERVER,
                            "swarm_list_local_agents",
                            json!({ "limit": 200 }),
                        )
                        .await;
                    list_result.and_then(|output| {
                        let parsed = hkask_types::tool_response::parse_tool_response(&output)
                            .ok_or_else(|| {
                                "swarm_list_local_agents: failed to parse tool response"
                                    .to_string()
                            })?;
                        let agents = parsed
                            .get("agents")
                            .and_then(|a| a.as_array())
                            .ok_or_else(|| {
                                "swarm_list_local_agents: missing agents array".to_string()
                            })?;
                        let card = agents
                            .iter()
                            .find(|a| {
                                a.get("agent_id").and_then(|v| v.as_str())
                                    == Some(agent_id.as_str())
                            })
                            .cloned()
                            .ok_or_else(|| {
                                format!("agent '{}' not found in local registry", agent_id)
                            })?;
                        Ok(AgentDetail::parse_local(&card))
                    })
                } else {
                    invoker
                        .invoke_tool(
                            crate::SWARM_SERVER,
                            "swarm_get_agent",
                            json!({ "agent_name": agent_id }),
                        )
                        .await
                        .and_then(|output| {
                            let parsed = hkask_types::tool_response::parse_tool_response(&output)
                                .ok_or_else(|| {
                                    "swarm_get_agent: failed to parse tool response".to_string()
                                })?;
                            Ok(AgentDetail::parse_cloud(&parsed))
                        })
                };
                this.update(cx, |this, cx| {
                    this.author.status = None;
                    match result {
                        Ok(detail) => {
                            // Store for `render` to apply (it has `&mut Window`).
                            this.pending_author_load = Some(detail);
                            if !is_local {
                                this.author.status = Some(
                                    "Viewing ABW agent. Edits cannot be saved \
                                     (no ABW update tool). Clone to Local to edit."
                                        .into(),
                                );
                            }
                        }
                        Err(err) => {
                            this.author.editing_id = None;
                            this.author.status =
                                Some(format!("Failed to load agent: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Apply a pending agent load to the author form. Called from `render`
    /// (which has `&mut Window`, required by `Editor::set_text`). Clears
    /// `pending_author_load` after applying.
    pub(crate) fn apply_pending_author_load(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.pending_author_load.take() else {
            return;
        };
        self.author
            .name
            .update(cx, |e, cx| e.set_text(detail.agent_id, window, cx));
        self.author
            .description
            .update(cx, |e, cx| e.set_text(detail.description, window, cx));
        self.author
            .system_prompt
            .update(cx, |e, cx| e.set_text(detail.system_prompt, window, cx));
        self.author.agent_type = detail.agent_type;
        let tags_joined = detail.tags.join(", ");
        self.author
            .tags
            .update(cx, |e, cx| e.set_text(tags_joined, window, cx));
        self.author.visibility = detail.visibility;
        let arousal = detail.valence_arousal.map(|v| v.to_string()).unwrap_or_default();
        self.author
            .valence_arousal
            .update(cx, |e, cx| e.set_text(arousal, window, cx));
        let valence = detail.valence_valence.map(|v| v.to_string()).unwrap_or_default();
        self.author
            .valence_valence
            .update(cx, |e, cx| e.set_text(valence, window, cx));
        let affect = detail.valence_primary_affect.unwrap_or_default();
        self.author
            .valence_primary_affect
            .update(cx, |e, cx| e.set_text(affect, window, cx));
        let traits = detail.valence_personality_traits.join(", ");
        self.author
            .valence_personality_traits
            .update(cx, |e, cx| e.set_text(traits, window, cx));
    }

    /// Save edits to an existing agent. Branches on `editing_id`:
    /// - `Some` + local: `swarm_reconfigure_local_agent` (updates
    ///   `system_prompt`/`model`/`mcp_tools`/`skills`, preserves `cloud_id`
    ///   and the rest of the card). Only `system_prompt` is editable from
    ///   this panel for local agents — `description`/`tags`/`visibility`/
    ///   `valence` changes are not persisted (the reconfigure tool doesn't
    ///   touch them; re-creating via `swarm_create_local_agent` would drop
    ///   the `cloud_id` sync link).
    /// - `Some` + cloud: no-op (no ABW update tool). The form status already
    ///   explains this when the agent is loaded.
    /// - `None`: delegates to `create_agent` (the create path).
    pub(crate) fn save_agent(&mut self, cx: &mut Context<Self>) {
        let Some(editing_id) = self.author.editing_id.clone() else {
            self.create_agent(cx);
            return;
        };
        let is_local = Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local;
        if !is_local {
            self.author.status = Some(
                "ABW agents cannot be updated from this panel. Clone to Local \
                 to edit."
                    .into(),
            );
            cx.notify();
            return;
        }
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let system_prompt = self.author.system_prompt.read(cx).text(cx);
        if system_prompt.trim().is_empty() {
            self.author.status = Some("System prompt is required.".into());
            cx.notify();
            return;
        }
        self.author.busy = true;
        self.author.status = None;
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            let agent_name = editing_id.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        crate::SWARM_SERVER,
                        "swarm_reconfigure_local_agent",
                        json!({
                            "agent_name": agent_name,
                            "system_prompt": system_prompt.trim(),
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.author.busy = false;
                    match result {
                        Ok(_) => {
                            this.author.status =
                                Some(format!("Agent '{}' updated.", editing_id).into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.author.status =
                                Some(format!("Update failed: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
