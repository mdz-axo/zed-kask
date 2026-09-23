use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

use crate::SwarmServer;
use crate::abw_client::SwarmClient;
use crate::config::SwarmConfig;
use crate::consent::ConsentStore;
use crate::local_knowledge::LazyLocalMemory;
use crate::local_registry::LocalAgentRegistry;
use crate::local_runtime::{LazyEventStore, LazyLocalSwarmRuntime, LocalSwarmRuntime};
use crate::local_swarms::LocalSwarmRegistry;
use crate::request_types::{DelegateInThreadLocalRequest, GetLocalSwarmRequest};

#[derive(Default)]
struct RecordingInference(Mutex<Vec<Vec<hkask_types::ChatMessage>>>);

impl hkask_types::InferencePort for RecordingInference {
    fn generate(
        &self,
        _: &str,
        _: &hkask_types::LLMParameters,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Model(
                "expected structured messages".into(),
            ))
        })
    }

    fn generate_with_messages(
        &self,
        messages: &[hkask_types::ChatMessage],
        _: &hkask_types::LLMParameters,
        _: Option<&str>,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        let mut calls = self.0.lock().expect("recording lock");
        calls.push(messages.to_vec());
        let text = format!("reply {}", calls.len());
        Box::pin(async move {
            Ok(hkask_types::InferenceResult {
                text,
                model: "fixture".into(),
                usage: Default::default(),
                finish_reason: "stop".into(),
                tool_calls: vec![],
                reasoning: None,
                cost_usd: None,
            })
        })
    }
}

struct NoTools;
impl hkask_types::ToolDispatchPort for NoTools {
    fn tool_definition<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::ChatToolDefinition, hkask_types::InferenceError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Model("no tools".into())) })
    }
    fn invoke_tool<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
        _: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Value, hkask_types::InferenceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Model("no tools".into())) })
    }
}

fn make_thread_server(dir: &std::path::Path, inference: Arc<RecordingInference>) -> SwarmServer {
    let agents = dir.join("agents").to_string_lossy().into_owned();
    let swarms = dir.join("swarms").to_string_lossy().into_owned();
    let stats = Arc::new(crate::agent_stats::AgentStatsStore::load(&agents));
    SwarmServer::new(
        hkask_types::WebID::new(),
        Arc::new(SwarmClient::new(
            reqwest::Client::new(),
            SwarmConfig::default(),
        )),
        Arc::new(ConsentStore::default()),
        Arc::new(LocalAgentRegistry::new(agents)),
        Arc::new(LazyLocalSwarmRuntime::with_runtime(
            LocalSwarmRuntime::new_for_test(inference, Arc::new(NoTools), String::new()),
        )),
        Arc::new(LocalSwarmRegistry::new(swarms)),
        Arc::new(LazyLocalMemory::lazy(
            dir.join("semantic.db").to_string_lossy().into_owned(),
            "test-passphrase".into(),
            1024,
        )),
        stats,
        Arc::new(LazyEventStore::lazy(
            dir.join("events.db").to_string_lossy().into_owned(),
        )),
        Arc::new(crate::thread_store::SwarmThreadStore::new(
            dir.join("threads.db").to_string_lossy().into_owned(),
            "test-passphrase".into(),
        )),
    )
}

fn content(output: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str::<Value>(output)?["content"].clone())
}

#[tokio::test]
async fn scoped_thread_is_structured_durable_isolated_and_archived()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = make_thread_server(dir.path(), recorder.clone());
    for id in ["a", "b", "outsider"] {
        server
            .local_registry
            .write_card(&serde_json::from_value(json!({
                "agent_id":id,"agent_type":"test","description":"fixture",
                "capabilities":{"system_prompt":"Answer tasks","model":"fixture/model"}
            }))?)?;
    }
    let first = server
        .local_swarms
        .create("first", "", vec!["a".into(), "b".into()])?;
    let other = server.local_swarms.create("other", "", vec!["b".into()])?;
    let request = |swarm_id: &str, agent_name: &str, task: &str| DelegateInThreadLocalRequest {
        swarm_id: swarm_id.into(),
        agent_name: agent_name.into(),
        task: task.into(),
    };
    assert!(
        server
            .swarm_delegate_in_thread_local(Parameters(request(&first.swarm_id, "outsider", "bad")))
            .await
            .is_err()
    );
    assert_eq!(
        recorder.0.lock().expect("calls").len(),
        0,
        "nonmember must never infer"
    );
    content(
        &server
            .swarm_delegate_in_thread_local(Parameters(request(&first.swarm_id, "a", "first task")))
            .await?,
    )?;
    content(
        &server
            .swarm_delegate_in_thread_local(Parameters(request(
                &first.swarm_id,
                "b",
                "second task",
            )))
            .await?,
    )?;
    {
        let calls = recorder.0.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        let second = &calls[1];
        assert_eq!(
            second.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            vec!["user", "assistant", "user"]
        );
        assert!(second[0].content.contains("first task"));
        assert_eq!(second[1].content, "reply 1");
        assert!(second[2].content.contains("second task"));
    }
    let read = |swarm_id: &str| GetLocalSwarmRequest {
        swarm_id: swarm_id.into(),
    };
    let thread = content(
        &server
            .swarm_thread_local(Parameters(read(&first.swarm_id)))
            .await?,
    )?;
    assert_eq!(thread["swarm_id"], first.swarm_id);
    assert_eq!(thread["turns"].as_array().map(Vec::len), Some(2));
    assert_eq!(thread["turns"][0]["sequence"], 1);
    assert_eq!(thread["turns"][1]["agent_id"], "b");
    assert!(thread["turns"][0]["created_at"].is_string());
    assert_eq!(thread["archived"], false);
    let encrypted_file = std::fs::read(dir.path().join("threads.db"))?;
    assert!(
        !encrypted_file.starts_with(b"SQLite format 3\0"),
        "thread database must be SQLCipher-encrypted"
    );
    assert!(
        !dir.path().join("semantic.db").exists(),
        "thread turns must not write semantic memory"
    );
    assert_eq!(
        content(
            &server
                .swarm_thread_local(Parameters(read(&other.swarm_id)))
                .await?
        )?["turns"],
        json!([])
    );
    content(
        &server
            .swarm_delegate_in_thread_local(Parameters(request(
                &other.swarm_id,
                "b",
                "isolated task",
            )))
            .await?,
    )?;
    {
        let calls = recorder.0.lock().expect("calls");
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[2].iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            vec!["user"]
        );
    }
    assert_eq!(
        content(
            &server
                .swarm_thread_local(Parameters(read(&other.swarm_id)))
                .await?
        )?["turns"][0]["sequence"],
        1,
    );
    let clone = server.local_swarms.clone_swarm(&first.swarm_id)?;
    assert_eq!(
        content(
            &server
                .swarm_thread_local(Parameters(read(&clone.swarm_id)))
                .await?
        )?["turns"],
        json!([])
    );
    drop(server);
    let restarted = make_thread_server(dir.path(), recorder.clone());
    assert_eq!(
        content(
            &restarted
                .swarm_thread_local(Parameters(read(&first.swarm_id)))
                .await?
        )?["turns"],
        thread["turns"]
    );
    restarted.local_swarms.delete(&first.swarm_id)?;
    let archived = content(
        &restarted
            .swarm_thread_local(Parameters(read(&first.swarm_id)))
            .await?,
    )?;
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["turns"], thread["turns"]);
    assert!(
        restarted
            .swarm_delegate_in_thread_local(Parameters(request(
                &first.swarm_id,
                "b",
                "after delete"
            )))
            .await
            .is_err()
    );
    assert_eq!(recorder.0.lock().expect("calls").len(), 3);
    drop(restarted);
    let restarted_again = make_thread_server(dir.path(), recorder);
    assert_eq!(
        content(
            &restarted_again
                .swarm_thread_local(Parameters(read(&first.swarm_id)))
                .await?
        )?["archived"],
        true,
    );
    Ok(())
}
