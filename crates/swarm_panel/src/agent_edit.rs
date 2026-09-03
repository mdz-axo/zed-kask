//! Agent edit (drill-down from browse card → author form pre-loaded with the
//! agent's existing details). Extracted from `swarm_panel.rs` — the methods
//! stay on `SwarmPanel` (they dispatch via `cx.listener` / `cx.spawn`); this
//! module owns the load + save + delete orchestration.
//!
//! Three entry points:
//! - `load_agent_into_author`: fetches the full agent card and stores it in
//!   `pending_author_load`. The `render` method (which has `&mut Window`,
//!   required by `Editor::set_text`) applies it to the form on the next frame.
//!   Mode-aware: cloud/synced agents use `swarm_get_agent`; local agents
//!   re-fetch via `swarm_list_local_agents` and filter (the list response
//!   carries the full `LocalAgentCard`, including `system_prompt`).
//! - `save_agent`: persists edits. Local agents use
//!   `swarm_reconfigure_local_agent` (updates `system_prompt`/`model`/
//!   `mcp_tools`/`skills`, preserves `cloud_swarm_id` and the rest of the card).
//!   Cloud agents use `swarm_update_agent` (fermi's `PUT /api/agents/:id`) —
//!   every form field is sent, so the pre-loaded form is the source of truth.
//!   A synced card edits its local copy; the cloud card moves via push.
//! - `delete_edited_agent`: deletes the agent loaded into the form —
//!   `swarm_remove_local` for local/synced (severs the local card only),
//!   `swarm_delete_agent` for cloud (irreversible ABW delete).

use gpui::{Context, Window};
use serde_json::json;

use crate::SwarmPanel;
use crate::parse::AgentSource;

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
    pub(crate) sample_queries: Vec<String>,
    pub(crate) accepts: Vec<String>,
    pub(crate) produces: Vec<String>,
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
        let sample_queries = string_array(card.get("sample_queries"));
        let accepts = string_array(card.get("accepts"));
        let produces = string_array(card.get("produces"));
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
            sample_queries,
            accepts,
            produces,
        }
    }

    /// Parse a cloud agent card from the `swarm_get_agent` response. The
    /// shape is fermi's `build_agent_json`: `description`, `tags`, `valence`,
    /// `sample_queries`, `accepts`, `produces`, `visibility`, and
    /// `system_prompt` at the TOP level; a `metadata` object is only
    /// overlaid for curated agents that have a filesystem card. Read
    /// top-level first so API-created agents (no `metadata` key) keep their
    /// fields; the metadata fallback preserves curated cards.
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
            .get("description")
            .and_then(|d| d.as_str())
            .or_else(|| {
                card.get("metadata")
                    .and_then(|m| m.get("description"))
                    .and_then(|d| d.as_str())
            })
            .unwrap_or("")
            .to_string();
        let system_prompt = card
            .get("system_prompt")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        // Prefer the top-level field; fall back to the curated card's
        // metadata. Chaining both would duplicate entries for curated
        // agents (the DB row and the card file carry the same values).
        let tags = {
            let top_level = string_array(card.get("tags"));
            if top_level.is_empty() {
                string_array(card.get("metadata").and_then(|m| m.get("tags")))
            } else {
                top_level
            }
        };
        let visibility = card
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("private")
            .to_string();
        let valence = card
            .get("valence")
            .filter(|v| !v.is_null())
            .or_else(|| card.get("metadata").and_then(|m| m.get("valence")));
        let (valence_arousal, valence_valence, valence_primary_affect, valence_personality_traits) =
            parse_valence(valence);
        // fermi contract fields: sample queries are top-level on the DB row
        // (curated cards also carry them under metadata); accepts/produces
        // are top-level on the ABW card.
        let sample_queries = {
            let top_level = string_array(card.get("sample_queries"));
            if top_level.is_empty() {
                string_array(card.get("metadata").and_then(|m| m.get("sample_queries")))
            } else {
                top_level
            }
        };
        let accepts = string_array(card.get("accepts"));
        let produces = string_array(card.get("produces"));
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
            sample_queries,
            accepts,
            produces,
        }
    }
}

/// Extract a JSON array of strings, defaulting to empty.
fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
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
        self.author.editing_source = Some(source.clone());
        // Set the create target from the editing source so the form dispatches
        // to the right backend (update local vs update cloud). A synced card
        // edits the local copy (the cloud card is updated separately via push).
        self.author.create_target = match source {
            AgentSource::Cloud => super::CreateTarget::Cloud,
            AgentSource::Local | AgentSource::Synced => super::CreateTarget::Local,
        };
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
                    // The parse steps below fail with plain strings, so flatten the
                    // typed invoke error to its message here and keep one error type
                    // through the chain. This path only reports — it has no retry
                    // behavior to gate on the classification.
                    let list_result = invoker
                        .invoke_tool(
                            crate::SWARM_SERVER,
                            "swarm_list_local_agents",
                            json!({ "limit": 200 }),
                        )
                        .await
                        .map_err(|err| err.message());
                    list_result.and_then(|output| {
                        let parsed = hkask_types::tool_response::parse_tool_response(&output)
                            .ok_or_else(|| {
                                "swarm_list_local_agents: failed to parse tool response".to_string()
                            })?;
                        let agents =
                            parsed
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
                        .map_err(|err| err.message())
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
        let arousal = detail
            .valence_arousal
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.author
            .valence_arousal
            .update(cx, |e, cx| e.set_text(arousal, window, cx));
        let valence = detail
            .valence_valence
            .map(|v| v.to_string())
            .unwrap_or_default();
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
        let queries = detail.sample_queries.join("\n");
        self.author
            .sample_queries
            .update(cx, |e, cx| e.set_text(queries, window, cx));
        let accepts = detail.accepts.join(", ");
        self.author
            .accepts
            .update(cx, |e, cx| e.set_text(accepts, window, cx));
        let produces = detail.produces.join(", ");
        self.author
            .produces
            .update(cx, |e, cx| e.set_text(produces, window, cx));
    }

    /// Permanently delete the agent currently loaded in the author form.
    /// Branches on the editing source:
    /// - `Local` / `Synced`: calls `swarm_remove_local` — deletes the local
    ///   card directory. A synced card's ABW agent is NOT touched (the cloud
    ///   copy can be deleted separately from the cloud card's "..." menu).
    /// - `Cloud`: calls `swarm_delete_agent` — irreversible ABW delete. The
    ///   agent is removed from the operator's library and every workspace
    ///   roster. A synced local card is NOT touched.
    /// On success, resets the author form to create mode and re-fetches the
    /// browse list so the deleted agent disappears.
    pub(crate) fn delete_edited_agent(&mut self, cx: &mut Context<Self>) {
        let Some(agent_name) = self.author.editing_id.clone() else {
            self.author.status = Some("No agent is loaded for deletion.".into());
            cx.notify();
            return;
        };
        let source = self
            .author
            .editing_source
            .clone()
            .unwrap_or(AgentSource::Local);
        let is_local = matches!(source, AgentSource::Local | AgentSource::Synced);
        let tool_name = if is_local {
            "swarm_remove_local"
        } else {
            "swarm_delete_agent"
        };
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.author.busy = true;
        self.author.status = Some("Deleting agent…".into());
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        crate::SWARM_SERVER,
                        tool_name,
                        json!({ "agent_name": agent_name }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.author.busy = false;
                    match result {
                        Ok(_) => {
                            // Defer the form reset and mode switch to the next
                            // `render` frame — `Editor::clear` and `set_mode`
                            // need `&mut Window`, which the spawn closure cannot
                            // hold. `render` consumes `pending_author_reset`.
                            this.pending_author_reset = true;
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.author.status =
                                Some(format!("Failed to delete agent: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Save edits to an existing agent. Branches on `editing_id`:
    /// - `Some` + local: `swarm_reconfigure_local_agent` (updates
    ///   `system_prompt`/`model`/`mcp_tools`/`skills`, preserves `cloud_swarm_id`
    ///   and the rest of the card). Only `system_prompt` is editable from
    ///   this panel for local agents — `description`/`tags`/`visibility`/
    ///   `valence` changes are not persisted (the reconfigure tool doesn't
    ///   touch them; re-creating via `swarm_create_local_agent` would drop
    ///   the `cloud_swarm_id` sync link).
    /// - `Some` + cloud: no-op (no ABW update tool). The form status already
    ///   explains this when the agent is loaded.
    /// - `None`: delegates to `create_agent` (the create path).
    pub(crate) fn save_agent(&mut self, cx: &mut Context<Self>) {
        let Some(editing_id) = self.author.editing_id.clone() else {
            self.create_agent(cx);
            return;
        };
        let is_local = self.author.create_target == super::CreateTarget::Local;
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
        // Cloud edits save every form field via `swarm_update_agent`
        // (fermi's `PUT /api/agents/:id` — the form was pre-loaded with the
        // card's current values, so the form is the source of truth).
        // Local/synced edits keep the prompt-only `swarm_reconfigure_local_agent`
        // path (a synced card edits its local copy; the cloud card moves via
        // push). Gather before spawn — the editors need `cx`.
        let cloud_fields = if is_local {
            None
        } else {
            Some((
                self.author.description.read(cx).text(cx),
                Self::comma_list(&self.author.tags.read(cx).text(cx)),
                Self::comma_list(&self.author.accepts.read(cx).text(cx)),
                Self::comma_list(&self.author.produces.read(cx).text(cx)),
                self.gather_valence(cx),
            ))
        };
        cx.spawn({
            let invoker = invoker.clone();
            let agent_name = editing_id.clone();
            async move |this, cx| {
                let result =
                    if let Some((description, tags, accepts, produces, valence)) = cloud_fields {
                        invoker
                            .invoke_tool(
                                crate::SWARM_SERVER,
                                "swarm_update_agent",
                                json!({
                                    "agent_name": agent_name,
                                    "description": description.trim(),
                                    "system_prompt": system_prompt.trim(),
                                    "tags": tags,
                                    "accepts": accepts,
                                    "produces": produces,
                                    "valence": valence,
                                }),
                            )
                            .await
                    } else {
                        invoker
                            .invoke_tool(
                                crate::SWARM_SERVER,
                                "swarm_reconfigure_local_agent",
                                json!({
                                    "agent_name": agent_name,
                                    "system_prompt": system_prompt.trim(),
                                }),
                            )
                            .await
                    };
                this.update(cx, |this, cx| {
                    this.author.busy = false;
                    match result {
                        Ok(_) => {
                            this.author.status =
                                Some(format!("Agent '{}' updated.", editing_id).into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.author.status = Some(format!("Update failed: {err}").into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_extracts_full_card() {
        let card = serde_json::json!({
            "agent_id": "market_research",
            "agent_type": "research",
            "description": "Market research analyst",
            "capabilities": {
                "system_prompt": "You are a market research analyst.",
                "model": "glm-5.2"
            },
            "tags": ["research", "analysis"],
            "visibility": "private",
            "valence": {
                "arousal": 0.6,
                "valence": 0.8,
                "primary_affect": "curiosity",
                "personality_traits": ["analytical", "cautious"]
            }
        });
        let detail = AgentDetail::parse_local(&card);
        assert_eq!(detail.agent_id, "market_research");
        assert_eq!(detail.agent_type, "research");
        assert_eq!(detail.description, "Market research analyst");
        assert_eq!(detail.system_prompt, "You are a market research analyst.");
        assert_eq!(detail.tags, vec!["research", "analysis"]);
        assert_eq!(detail.visibility, "private");
        assert_eq!(detail.valence_arousal, Some(0.6));
        assert_eq!(detail.valence_valence, Some(0.8));
        assert_eq!(detail.valence_primary_affect, Some("curiosity".to_string()));
        assert_eq!(
            detail.valence_personality_traits,
            vec!["analytical", "cautious"]
        );
    }

    #[test]
    fn parse_local_handles_missing_fields() {
        let card = serde_json::json!({
            "agent_id": "minimal_agent",
            "agent_type": "research"
        });
        let detail = AgentDetail::parse_local(&card);
        assert_eq!(detail.agent_id, "minimal_agent");
        assert_eq!(detail.agent_type, "research");
        assert_eq!(detail.description, "");
        assert_eq!(detail.system_prompt, "");
        assert!(detail.tags.is_empty());
        assert_eq!(detail.visibility, "private");
        assert!(detail.valence_arousal.is_none());
        assert!(detail.valence_valence.is_none());
        assert!(detail.valence_primary_affect.is_none());
        assert!(detail.valence_personality_traits.is_empty());
    }

    #[test]
    fn parse_cloud_extracts_abw_card_shape() {
        let card = serde_json::json!({
            "agent_id": "cloud_agent",
            "agent_type": "creative",
            "system_prompt": "You are a creative writer.",
            "metadata": {
                "description": "Creative writing assistant",
                "tags": ["creative", "writing"],
                "valence": {
                    "arousal": 0.7,
                    "valence": 0.9,
                    "primary_affect": "enthusiasm",
                    "personality_traits": ["imaginative"]
                }
            },
            "visibility": "public"
        });
        let detail = AgentDetail::parse_cloud(&card);
        assert_eq!(detail.agent_id, "cloud_agent");
        assert_eq!(detail.agent_type, "creative");
        assert_eq!(detail.description, "Creative writing assistant");
        assert_eq!(detail.system_prompt, "You are a creative writer.");
        assert_eq!(detail.tags, vec!["creative", "writing"]);
        assert_eq!(detail.visibility, "public");
        assert_eq!(detail.valence_arousal, Some(0.7));
        assert_eq!(detail.valence_valence, Some(0.9));
        assert_eq!(
            detail.valence_primary_affect,
            Some("enthusiasm".to_string())
        );
        assert_eq!(detail.valence_personality_traits, vec!["imaginative"]);
    }

    #[test]
    fn parse_cloud_reads_fermi_flat_shape() {
        // fermi's `build_agent_json` carries description/tags/valence at the
        // TOP level; `metadata` is only overlaid for curated agents with a
        // filesystem card. API-created agents have no `metadata` key at all.
        let card = serde_json::json!({
            "agent_id": "api_created_agent",
            "agent_type": "research",
            "description": "Built via the ABW API",
            "system_prompt": "You research things.",
            "tags": ["api", "research"],
            "visibility": "private",
            "sample_queries": ["query one"],
            "valence": {
                "arousal": 0.4,
                "valence": 0.6,
                "primary_affect": "curiosity",
                "personality_traits": ["methodical"]
            }
        });
        let detail = AgentDetail::parse_cloud(&card);
        assert_eq!(detail.agent_id, "api_created_agent");
        assert_eq!(detail.description, "Built via the ABW API");
        assert_eq!(detail.tags, vec!["api", "research"]);
        assert_eq!(detail.sample_queries, vec!["query one"]);
        assert_eq!(detail.valence_arousal, Some(0.4));
        assert_eq!(detail.valence_valence, Some(0.6));
        assert_eq!(detail.valence_primary_affect, Some("curiosity".to_string()));
        assert_eq!(detail.valence_personality_traits, vec!["methodical"]);
    }

    #[test]
    fn parse_cloud_prefers_top_level_over_metadata() {
        // A curated agent has BOTH the DB row's top-level fields and the
        // card file's metadata overlay — the top level wins (no duplicates).
        let card = serde_json::json!({
            "agent_id": "curated_agent",
            "agent_type": "research",
            "description": "from the DB row",
            "tags": ["db"],
            "sample_queries": ["db query"],
            "metadata": {
                "description": "from the card file",
                "tags": ["card"],
                "sample_queries": ["card query"]
            }
        });
        let detail = AgentDetail::parse_cloud(&card);
        assert_eq!(detail.description, "from the DB row");
        assert_eq!(detail.tags, vec!["db"]);
        assert_eq!(detail.sample_queries, vec!["db query"]);
    }

    #[test]
    fn parse_cloud_falls_back_to_agent_name() {
        let card = serde_json::json!({
            "agent_name": "fallback_agent",
            "agent_type": "meta",
            "system_prompt": "prompt",
            "metadata": {"description": "desc"},
            "visibility": "unlisted"
        });
        let detail = AgentDetail::parse_cloud(&card);
        assert_eq!(detail.agent_id, "fallback_agent");
    }

    #[test]
    fn parse_cloud_handles_missing_valence() {
        let card = serde_json::json!({
            "agent_id": "no_valence",
            "agent_type": "research",
            "system_prompt": "prompt",
            "metadata": {"description": "desc", "tags": []},
            "visibility": "private"
        });
        let detail = AgentDetail::parse_cloud(&card);
        assert!(detail.valence_arousal.is_none());
        assert!(detail.valence_valence.is_none());
        assert!(detail.valence_primary_affect.is_none());
        assert!(detail.valence_personality_traits.is_empty());
    }
}
