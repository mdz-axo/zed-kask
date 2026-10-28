//! The editor's skill tools (`skill`, `render_template`, `lisp_eval`,
//! `onto_anchor`) exposed to delegated agents as `host/*` tools on the existing
//! IPC tool path.
//!
//! This wraps the governed `ToolPort` rather than adding a dispatch route: the
//! IPC `ToolDefinition`/`ToolInvoke` handlers, their allowlist checks, and the
//! swarm tool loop are reused unchanged. Each tool calls the same function its
//! in-editor tool calls, so a local swarm agent gets the Curator's skill access.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use hkask_tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};

pub(crate) const SERVER: &str = "host";
/// Read-only skill access: catalog content and pure computation, which every
/// in-editor thread already has. Granted without a `delegated_tools` entry;
/// the caller's card allowlist still applies.
pub(crate) const SKILL_TOOLS: [&str; 4] = [
    "host/skill",
    "host/render_template",
    "host/lisp_eval",
    "host/onto_anchor",
];

pub(crate) type SkillRequest = (String, tokio::sync::oneshot::Sender<Result<String, String>>);

pub(crate) struct HostSkillToolPort {
    pub(crate) inner: Arc<dyn ToolPort>,
    /// Skill activation needs the GPUI-held catalog; see `start`'s drainer.
    pub(crate) skill_tx: tokio::sync::mpsc::UnboundedSender<SkillRequest>,
    #[cfg(test)]
    pub(crate) lisp_probe: Option<Arc<LispProbe>>,
}

#[cfg(test)]
pub(crate) struct LispProbe {
    pub(crate) events: tokio::sync::mpsc::UnboundedSender<LispEvent>,
    // Only this gate is polled on the async runtime. The bounded blocking gate
    // below runs exclusively inside spawn_blocking, never on an async worker.
    pub(crate) before_spawn: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    pub(crate) worker_release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) enum LispEvent {
    GuardReady(Arc<AtomicBool>),
    Spawned,
    Cancelled,
    Entered(std::thread::ThreadId),
    Evaluated(Result<serde_json::Value, String>),
    Exited,
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

        "onto_anchor" => {
            let input: agent::OntoAnchorToolInput = serde_json::from_value(args)
                .map_err(|e| format!("invalid onto_anchor input: {e}"))?;
            serde_json::to_value(agent::resolve_onto_anchor(&input))
                .map_err(|e| format!("failed to serialize onto_anchor output: {e}"))
        }
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

/// A dropped request signals, but never joins, its blocking worker. Created
/// before the future is polled or the worker is queued; the flag is monotonic.
struct LispCancellation {
    flag: Arc<AtomicBool>,
    #[cfg(test)]
    probe: Option<Arc<LispProbe>>,
}

impl Drop for LispCancellation {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some(probe) = &self.probe
            && probe.events.send(LispEvent::Cancelled).is_err()
        {
            tracing::debug!("Lisp cancellation observer dropped");
        }
    }
}

fn run_lisp(
    args: serde_json::Value,
    #[cfg(test)] probe: Option<Arc<LispProbe>>,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + Send {
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancel_on_drop = LispCancellation {
        flag: cancellation.clone(),
        #[cfg(test)]
        probe: probe.clone(),
    };
    #[cfg(test)]
    if let Some(probe) = &probe {
        probe
            .events
            .send(LispEvent::GuardReady(cancellation.clone()))
            .expect("guard observer");
    }
    async move {
        let _cancel_on_drop = cancel_on_drop;
        #[cfg(test)]
        if let Some(probe) = &probe
            && let Some(release) = probe.before_spawn.lock().await.take()
        {
            tokio::time::timeout(std::time::Duration::from_secs(5), release)
                .await
                .expect("bounded pre-spawn release")
                .expect("pre-spawn release");
        }
        #[cfg(test)]
        let worker_probe = probe.clone();
        let worker = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let _exited = LispWorkerExit(worker_probe.clone());
            #[cfg(test)]
            if let Some(probe) = &worker_probe {
                probe
                    .events
                    .send(LispEvent::Entered(std::thread::current().id()))
                    .expect("entry observer");
                if let Some(release) = probe
                    .worker_release
                    .lock()
                    .expect("worker gate lock")
                    .take()
                {
                    release
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .expect("bounded blocking-worker release");
                }
            }
            let result = serde_json::from_value(args)
                .map_err(|e| format!("invalid lisp_eval input: {e}"))
                .and_then(|input| agent::evaluate_lisp_controlled(input, Some(cancellation)));
            #[cfg(test)]
            if let Some(probe) = &worker_probe {
                probe
                    .events
                    .send(LispEvent::Evaluated(result.clone()))
                    .expect("evaluation observer");
            }
            result
        });
        #[cfg(test)]
        if let Some(probe) = &probe {
            probe
                .events
                .send(LispEvent::Spawned)
                .expect("spawn observer");
        }
        worker
            .await
            .map_err(|error| format!("lisp_eval worker failed: {error}"))?
    }
}

#[cfg(test)]
struct LispWorkerExit(Option<Arc<LispProbe>>);

#[cfg(test)]
impl Drop for LispWorkerExit {
    fn drop(&mut self) {
        // The shared helper has returned (or unwound), including its core
        // invocation teardown. This is completion, not notification.
        if let Some(probe) = &self.0
            && probe.events.send(LispEvent::Exited).is_err()
        {
            tracing::debug!("Lisp cleanup observer dropped");
        }
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
        if tool == "lisp_eval" {
            let worker = run_lisp(
                args,
                #[cfg(test)]
                self.lisp_probe.clone(),
            );
            return Box::pin(async move {
                worker
                    .await
                    .map_err(|detail| ToolPortError::InvocationFailed { detail, kind: None })
            });
        }
        Box::pin(async move {
            let result = if tool == "skill" {
                self.activate(args).await
            } else {
                run_pure(tool, args)
            };
            result.map_err(|detail| ToolPortError::InvocationFailed { detail, kind: None })
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
            "onto_anchor" => Some(info::<agent::OntoAnchorTool>(tool)),
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
                lisp_probe: None,
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
        for tool in ["skill", "render_template", "lisp_eval", "onto_anchor"] {
            let info = port.get_tool_info(SERVER, tool).await.expect("host tool");
            assert!(info.input_schema.is_object(), "{tool}");
            assert!(!info.description.is_empty(), "{tool}");
            let json_field = match tool {
                "render_template" => Some("context"),
                "lisp_eval" => Some("env"),
                _ => None,
            };
            if let Some(field) = json_field {
                assert_eq!(info.input_schema["properties"][field]["type"], "string");
            }
        }
        assert!(port.get_tool_info(SERVER, "terminal").await.is_none());
    }

    #[tokio::test]
    async fn onto_anchor_runs_the_editor_resolver() {
        let (port, _rx) = port();
        let value = port
            .invoke(
                SERVER,
                "onto_anchor",
                serde_json::json!({
                    "term": "sustainable growth rate",
                    "relation_query": {"to": "net margin", "max_hops": 2}
                }),
                webid(),
            )
            .await
            .expect("onto_anchor evaluated");
        assert_eq!(value["concept"], "sustainable_growth_rate");
        assert_eq!(value["tier"], "derived");
        assert_eq!(value["traversal"]["status"], "path_found");
    }

    /// expect: "Dropping an unpolled host invocation signals its already-owned flag without spawning" [H7]
    #[test]
    fn h7_unpolled_host_invocation_drop_signals_without_spawn() {
        let (mut port, _rx) = port();
        let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
        port.lisp_probe = Some(Arc::new(LispProbe {
            events,
            before_spawn: tokio::sync::Mutex::new(None),
            worker_release: std::sync::Mutex::new(None),
        }));
        let invocation = port.invoke(
            SERVER,
            "lisp_eval",
            serde_json::json!({"form":"42"}),
            webid(),
        );
        let LispEvent::GuardReady(flag) = observed.try_recv().expect("guard before first poll")
        else {
            panic!("invocation guard")
        };
        assert!(!flag.load(Ordering::Acquire));
        drop(invocation);
        assert!(flag.load(Ordering::Acquire));
        assert!(matches!(
            observed.try_recv().expect("guard drop"),
            LispEvent::Cancelled
        ));
        assert!(
            observed.try_recv().is_err(),
            "unpolled invocation must not spawn"
        );
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
    async fn lisp_eval_conforms_to_the_editor_evaluator_across_the_canonical_forms() {
        // lisp-repair L4: the host/lisp_eval dispatch surface must return
        // byte-identical results to the editor tool path. The full canonical
        // set (lib-vs-tool) lives in the agent crate's conformance module;
        // this pins the third surface with representatives covering the
        // L2 lie-fix behaviors, the L3 walker, assoc, infix, and errors.
        let (port, _rx) = port();
        let cases: Vec<(&str, serde_json::Value, &str)> = vec![
            (
                "exact comparison above 2^53",
                serde_json::json!({"form": "(= 9007199254740993 9007199254740992)"}),
                "false",
            ),
            (
                "walker",
                serde_json::json!({
                    "form": "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)",
                    "env": serde_json::json!({"items": [1, 2, 3, 4, 5]}).to_string()
                }),
                "5",
            ),
            (
                "assoc over object",
                serde_json::json!({
                    "form": "(assoc \"b\" data)",
                    "env": serde_json::json!({"data": {"a": 1, "b": "x"}}).to_string()
                }),
                "\"x\"",
            ),
            (
                "bare infix",
                serde_json::json!({"form": "z + 1", "env": serde_json::json!({"z": 41}).to_string()}),
                "42",
            ),
            (
                "division is always Float",
                serde_json::json!({"form": "(/ 6 3)"}),
                "2.0",
            ),
        ];
        for (name, args, expected) in cases {
            let host = port
                .invoke(SERVER, "lisp_eval", args.clone(), webid())
                .await
                .unwrap_or_else(|e| panic!("{name}: host surface errored: {e}"));
            let input: agent::LispEvalToolInput =
                serde_json::from_value(args).expect("args deserialize");
            let tool = agent::evaluate_lisp(input)
                .unwrap_or_else(|e| panic!("{name}: tool surface errored: {e}"));
            assert_eq!(
                serde_json::to_string(&host).expect("serialize"),
                serde_json::to_string(&tool).expect("serialize"),
                "{name}: host and tool surfaces disagree"
            );
            assert_eq!(
                serde_json::to_string(&host).expect("serialize"),
                expected,
                "{name}"
            );
        }
        // The error path: host and tool agree on the teaching message byte
        // for byte (the host dispatch maps evaluate_lisp's Err into the
        // port error, which must carry the same text).
        let overflow = serde_json::json!({"form": "(+ 9223372036854775807 1)"});
        let host_err = port
            .invoke(SERVER, "lisp_eval", overflow.clone(), webid())
            .await
            .expect_err("host surface should error");
        let input: agent::LispEvalToolInput =
            serde_json::from_value(overflow).expect("args deserialize");
        let tool_err = agent::evaluate_lisp(input).expect_err("tool surface should error");
        assert!(
            host_err.to_string().contains(&tool_err),
            "host error must carry the tool message: host={host_err} tool={tool_err}"
        );
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
