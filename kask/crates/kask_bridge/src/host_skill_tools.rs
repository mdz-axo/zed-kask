//! The editor's skill tools (`skill`, `render_template`, `lisp_eval`) exposed
//! to delegated agents as `host/*` tools on the existing IPC tool path.
//!
//! This wraps the governed `ToolPort` rather than adding a dispatch route: the
//! IPC `ToolDefinition`/`ToolInvoke` handlers, their allowlist checks, and the
//! swarm tool loop are reused unchanged. Each tool calls the same function its
//! in-editor tool calls, so a local swarm agent gets the Curator's skill access.

use std::sync::Arc;

use hkask_tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};

pub(crate) const SERVER: &str = "host";
/// Read-only skill access: catalog content and pure computation, which every
/// in-editor thread already has. Granted without a `delegated_tools` entry;
/// the caller's card allowlist still applies.
pub(crate) const SKILL_TOOLS: [&str; 3] = ["host/skill", "host/render_template", "host/lisp_eval"];

pub(crate) type SkillRequest = (String, tokio::sync::oneshot::Sender<Result<String, String>>);

pub(crate) struct HostSkillToolPort {
    pub(crate) inner: Arc<dyn ToolPort>,
    /// Skill activation needs the GPUI-held catalog; see `start`'s drainer.
    pub(crate) skill_tx: tokio::sync::mpsc::UnboundedSender<SkillRequest>,
}

impl HostSkillToolPort {
    async fn activate(&self, args: serde_json::Value) -> Result<serde_json::Value, String> {
        let input: agent::SkillToolInput =
            serde_json::from_value(args).map_err(|e| format!("invalid skill input: {e}"))?;
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.skill_tx
            .send((input.name, reply))
            .map_err(|_| "skill catalog task is not running".to_string())?;
        let rendered = rx
            .await
            .map_err(|_| "skill catalog task dropped the request".to_string())??;
        Ok(serde_json::Value::String(rendered))
    }
}

fn run_pure(tool: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    match tool {
        "render_template" => {
            let input = serde_json::from_value(args)
                .map_err(|e| format!("invalid render_template input: {e}"))?;
            agent::render_registry_template(&input).map(serde_json::Value::String)
        }
        "lisp_eval" => agent::evaluate_lisp(
            serde_json::from_value(args).map_err(|e| format!("invalid lisp_eval input: {e}"))?,
        ),
        other => Err(format!("unknown host tool '{other}'")),
    }
}

fn info<T: agent::AgentTool>(name: &str) -> ToolInfo {
    ToolInfo {
        name: name.to_string(),
        description: T::description().to_string(),
        input_schema: T::input_schema().to_value(),
    }
}

impl ToolPort for HostSkillToolPort {
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        agent: hkask_types::WebID,
    ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
        if server != SERVER {
            return self.inner.invoke(server, tool, args, agent);
        }
        Box::pin(async move {
            let result = if tool == "skill" {
                self.activate(args).await
            } else {
                run_pure(tool, args)
            };
            result.map_err(ToolPortError::InvocationFailed)
        })
    }

    fn get_tool_info<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
    ) -> ToolFuture<'a, Option<ToolInfo>> {
        if server != SERVER {
            return self.inner.get_tool_info(server, tool);
        }
        let info = match tool {
            "skill" => Some(info::<agent::SkillTool>(tool)),
            "render_template" => Some(info::<agent::RenderTemplateTool>(tool)),
            "lisp_eval" => Some(info::<agent::LispEvalTool>(tool)),
            _ => None,
        };
        Box::pin(async move { info })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Refusing;
    impl ToolPort for Refusing {
        fn invoke<'a>(
            &'a self,
            server: &'a str,
            _: &'a str,
            _: serde_json::Value,
            _: hkask_types::WebID,
        ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            Box::pin(async move { Ok(serde_json::json!({ "inner": server })) })
        }
        fn get_tool_info<'a>(&'a self, _: &'a str, _: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

    fn port() -> (
        HostSkillToolPort,
        tokio::sync::mpsc::UnboundedReceiver<SkillRequest>,
    ) {
        let (skill_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            HostSkillToolPort {
                inner: Arc::new(Refusing),
                skill_tx,
            },
            rx,
        )
    }

    fn webid() -> hkask_types::WebID {
        hkask_types::WebID::from_persona(b"test")
    }

    #[tokio::test]
    async fn host_tools_advertise_the_editor_tool_schemas() {
        let (port, _rx) = port();
        for tool in ["skill", "render_template", "lisp_eval"] {
            let info = port.get_tool_info(SERVER, tool).await.expect("host tool");
            assert!(info.input_schema.is_object(), "{tool}");
            assert!(!info.description.is_empty(), "{tool}");
        }
        assert!(port.get_tool_info(SERVER, "terminal").await.is_none());
    }

    #[tokio::test]
    async fn lisp_eval_runs_the_editor_evaluator() {
        let (port, _rx) = port();
        let value = port
            .invoke(
                SERVER,
                "lisp_eval",
                serde_json::json!({"form": "(max 2 7)"}),
                webid(),
            )
            .await
            .expect("evaluated");
        assert_eq!(value, serde_json::json!(7));
    }

    #[tokio::test]
    async fn skill_activation_goes_through_the_catalog_channel() {
        let (port, mut rx) = port();
        let drainer = tokio::spawn(async move {
            let (name, reply) = rx.recv().await.expect("request");
            reply
                .send(Ok(format!("<skill_content name=\"{name}\">")))
                .ok();
        });
        let value = port
            .invoke(SERVER, "skill", serde_json::json!({"name": "tdd"}), webid())
            .await
            .expect("activated");
        assert_eq!(value, serde_json::json!("<skill_content name=\"tdd\">"));
        drainer.await.expect("drainer");
    }

    #[tokio::test]
    async fn other_servers_pass_through_to_the_governed_port() {
        let (port, _rx) = port();
        let value = port
            .invoke("research", "search", serde_json::Value::Null, webid())
            .await
            .expect("delegated");
        assert_eq!(value, serde_json::json!({"inner": "research"}));
    }
}
