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
use crate::request_types::{
    A2aBroadcastRequest, A2aSendRequest, ExecutePlanLocalRequest, GetLocalSwarmRequest,
};

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
                "expected messages".into(),
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
        let mut calls = self.0.lock().expect("recorder");
        calls.push(messages.to_vec());
        let text = format!("reply {}", calls.len());
        Box::pin(async move {
            tokio::task::yield_now().await;
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

fn server(
    dir: &std::path::Path,
    inference: Arc<RecordingInference>,
    passphrase: &str,
) -> SwarmServer {
    let agents = dir.join("agents").to_string_lossy().into_owned();
    let swarms = dir.join("swarms").to_string_lossy().into_owned();
    SwarmServer::new(
        hkask_types::WebID::new(),
        Arc::new(SwarmClient::new(
            reqwest::Client::new(),
            SwarmConfig::default(),
        )),
        Arc::new(ConsentStore::default()),
        Arc::new(LocalAgentRegistry::new(&agents)),
        Arc::new(LazyLocalSwarmRuntime::with_runtime(
            LocalSwarmRuntime::new_for_test(inference, Arc::new(NoTools), String::new()),
        )),
        Arc::new(LocalSwarmRegistry::new(swarms)),
        Arc::new(LazyLocalMemory::lazy(
            dir.join("semantic.db").to_string_lossy().into_owned(),
            "test-passphrase".into(),
            1024,
        )),
        Arc::new(crate::agent_stats::AgentStatsStore::load(&agents)),
        Arc::new(LazyEventStore::lazy(
            dir.join("events.db").to_string_lossy().into_owned(),
        )),
        Arc::new(crate::thread_store::SwarmThreadStore::new(
            dir.join("threads.db").to_string_lossy().into_owned(),
            passphrase.into(),
        )),
    )
}

fn content(output: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str::<Value>(output)?["content"].clone())
}

#[tokio::test]
async fn scoped_plan_broadcast_send_share_history_and_keep_verdict_and_board()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = server(dir.path(), recorder.clone(), "test-passphrase");
    for id in ["a", "b", "outsider"] {
        server
            .local_registry
            .write_card(&serde_json::from_value(json!({
                "agent_id": id, "agent_type": "test", "description": "fixture",
                "capabilities": {"system_prompt": "Answer tasks", "model": "fixture/model"}
            }))?)?;
    }
    let swarm = server
        .local_swarms
        .create("scoped", "", vec!["a".into(), "b".into()])?;
    let plan =
        |entries: Value, sid: Option<&str>| -> Result<ExecutePlanLocalRequest, serde_json::Error> {
            serde_json::from_value(json!({"delegations": entries, "swarm_id": sid}))
        };
    let denied = content(
        &server
            .swarm_execute_plan_local(Parameters(plan(
                json!([{"agent_name":"outsider", "task":"forbidden"}]),
                Some(&swarm.swarm_id),
            )?))
            .await?,
    )?;
    assert_eq!(denied["failed"], 1);
    assert!(
        server
            .swarm_a2a_send(Parameters(A2aSendRequest {
                swarm_id: Some(swarm.swarm_id.clone()),
                agent_name: "outsider".into(),
                message: "forbidden".into(),
                context_id: None,
            }))
            .await
            .is_err()
    );
    assert_eq!(
        recorder.0.lock().expect("calls").len(),
        0,
        "nonmember must not infer"
    );

    let scoped = content(&server.swarm_execute_plan_local(Parameters(plan(
        json!([{"agent_name":"a", "task":"one", "evaluator":{"evaluator":"contains", "spec":"reply"}},
               {"agent_name":"b", "task":"two"}]), Some(&swarm.swarm_id),
    )?)).await?)?;
    assert_eq!(scoped["succeeded"], 2);
    assert_eq!(scoped["results"][0]["task_success"]["pass"], true);
    assert_eq!(scoped["task_board"]["total"], 3);
    let broadcast = content(
        &server
            .swarm_a2a_broadcast(Parameters(A2aBroadcastRequest {
                swarm_id: swarm.swarm_id.clone(),
                message: "three".into(),
                context_id: None,
            }))
            .await?,
    )?;
    assert_eq!(broadcast["broadcast_count"], 2);
    let scoped_send = content(
        &server
            .swarm_a2a_send(Parameters(A2aSendRequest {
                swarm_id: Some(swarm.swarm_id.clone()),
                agent_name: "a".into(),
                message: "four".into(),
                context_id: None,
            }))
            .await?,
    )?;
    assert!(scoped_send["artifacts"].is_array());
    let readback = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: swarm.swarm_id.clone(),
            }))
            .await?,
    )?;
    assert_eq!(readback["turns"].as_array().map(Vec::len), Some(5));
    assert_eq!(readback["turns"][4]["sequence"], 5);
    assert_eq!(readback["turns"][4]["task"], "four");
    {
        let calls = recorder.0.lock().expect("calls");
        // The executor prepends a system message and appends the current user task.
        let prior: Vec<_> = calls[4]
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| (m.role.as_str(), m.content.as_str()))
            .collect();
        assert_eq!(
            prior,
            vec![
                ("user", "one"),
                ("assistant", "reply 1"),
                ("user", "two"),
                ("assistant", "reply 2"),
                ("user", "three"),
                ("assistant", "reply 3"),
                ("user", "three"),
                ("assistant", "reply 4"),
                ("user", "Answer tasks\n\n---\n\nTask: four")
            ]
        );
    }
    let unscoped = content(
        &server
            .swarm_execute_plan_local(Parameters(plan(
                json!([{"agent_name":"outsider", "task":"standalone"}]),
                None,
            )?))
            .await?,
    )?;
    assert_eq!(unscoped["succeeded"], 1);
    {
        let independent = recorder.0.lock().expect("calls");
        let independent_user: Vec<_> = independent[5]
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(independent_user.len(), 1);
        assert!(independent_user[0].ends_with("Task: standalone"));
    }
    content(
        &server
            .swarm_a2a_send(Parameters(A2aSendRequest {
                swarm_id: None,
                agent_name: "outsider".into(),
                message: "direct".into(),
                context_id: None,
            }))
            .await?,
    )?;
    let readback = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: swarm.swarm_id,
            }))
            .await?,
    )?;
    assert_eq!(readback["turns"].as_array().map(Vec::len), Some(5));
    Ok(())
}

#[tokio::test]
async fn scoped_storage_failure_is_not_reported_as_success()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = server(dir.path(), recorder.clone(), "short");
    server
        .local_registry
        .write_card(&serde_json::from_value(json!({
            "agent_id":"a", "agent_type":"test", "description":"fixture",
            "capabilities":{"system_prompt":"Answer", "model":"fixture/model"}
        }))?)?;
    let swarm = server
        .local_swarms
        .create("blocked", "", vec!["a".into()])?;
    let result = content(
        &server
            .swarm_execute_plan_local(Parameters(serde_json::from_value(json!({
                "swarm_id":swarm.swarm_id, "delegations":[{"agent_name":"a", "task":"blocked"}]
            }))?))
            .await?,
    )?;
    assert_eq!(result["failed"], 1);
    assert_eq!(result["succeeded"], 0);
    assert_eq!(result["task_board"]["failed"], 1);
    assert!(
        server
            .swarm_a2a_broadcast(Parameters(A2aBroadcastRequest {
                swarm_id: swarm.swarm_id.clone(),
                message: "blocked".into(),
                context_id: None,
            }))
            .await
            .is_ok(),
        "broadcast reports per-recipient failures"
    );
    assert!(
        server
            .swarm_a2a_send(Parameters(A2aSendRequest {
                swarm_id: Some(swarm.swarm_id),
                agent_name: "a".into(),
                message: "blocked".into(),
                context_id: None,
            }))
            .await
            .is_err()
    );
    assert_eq!(
        recorder.0.lock().expect("calls").len(),
        0,
        "storage failure precedes inference"
    );
    Ok(())
}

#[tokio::test]
async fn append_failure_after_inference_is_reported_and_not_read_back()
-> Result<(), Box<dyn std::error::Error>> {
    use hkask_storage::DatabaseDriver;
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = server(dir.path(), recorder.clone(), "test-passphrase");
    server
        .local_registry
        .write_card(&serde_json::from_value(json!({
            "agent_id":"a", "agent_type":"test", "description":"fixture",
            "capabilities":{"system_prompt":"Answer", "model":"fixture/model"}
        }))?)?;
    let swarm = server.local_swarms.create("append", "", vec!["a".into()])?;
    let invoke = |task: &str| -> Result<ExecutePlanLocalRequest, serde_json::Error> {
        serde_json::from_value(json!({"swarm_id":swarm.swarm_id,
            "delegations":[{"agent_name":"a", "task":task}]}))
    };
    content(
        &server
            .swarm_execute_plan_local(Parameters(invoke("first")?))
            .await?,
    )?;
    let db = hkask_storage::Database::open(
        dir.path().join("threads.db").to_string_lossy().as_ref(),
        "test-passphrase",
    )?;
    let driver = hkask_storage::SqliteDriver::new(db.sqlite_pool()?);
    driver.execute_batch(
        "CREATE TRIGGER fail_thread_append BEFORE INSERT ON swarm_thread_turns
        BEGIN SELECT RAISE(ABORT, 'injected append failure'); END;",
    )?;
    let result = content(
        &server
            .swarm_execute_plan_local(Parameters(invoke("second")?))
            .await?,
    )?;
    assert_eq!(result["failed"], 1);
    assert_eq!(result["succeeded"], 0);
    assert!(result["results"][0].to_string().contains("thread append"));
    assert_eq!(
        recorder.0.lock().expect("calls").len(),
        2,
        "failure occurs after inference"
    );
    let read = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: swarm.swarm_id,
            }))
            .await?,
    )?;
    assert_eq!(read["turns"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[tokio::test]
async fn independent_store_instances_serialize_history_and_sequences()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let first = server(dir.path(), recorder.clone(), "test-passphrase");
    let second = server(dir.path(), recorder.clone(), "test-passphrase");
    first
        .local_registry
        .write_card(&serde_json::from_value(json!({
            "agent_id":"a", "agent_type":"test", "description":"fixture",
            "capabilities":{"system_prompt":"Answer", "model":"fixture/model"}
        }))?)?;
    let swarm = first.local_swarms.create("overlap", "", vec!["a".into()])?;
    let send = |message: &str| A2aSendRequest {
        swarm_id: Some(swarm.swarm_id.clone()),
        agent_name: "a".into(),
        message: message.into(),
        context_id: None,
    };
    let (one, two) = tokio::join!(
        first.swarm_a2a_send(Parameters(send("first"))),
        second.swarm_a2a_send(Parameters(send("second"))),
    );
    content(&one?)?;
    content(&two?)?;
    let read = content(
        &first
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: swarm.swarm_id,
            }))
            .await?,
    )?;
    assert_eq!(read["turns"].as_array().map(Vec::len), Some(2));
    assert_eq!(read["turns"][0]["sequence"], 1);
    assert_eq!(read["turns"][1]["sequence"], 2);
    let calls = recorder.0.lock().expect("calls");
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1]
            .iter()
            .any(|m| m.role == "assistant" && m.content == "reply 1"),
        "second inference must see first committed response"
    );
    Ok(())
}

#[tokio::test]
async fn scoped_fanout_and_pipeline_commit_ordered_history_and_deny_nonmembers()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = server(dir.path(), recorder.clone(), "test-passphrase");
    for id in ["a", "b", "outsider"] {
        server
            .local_registry
            .write_card(&serde_json::from_value(json!({
                "agent_id": id, "agent_type": "test", "description": "fixture",
                "capabilities": {"system_prompt": "Answer tasks", "model": "fixture/model"}
            }))?)?;
    }
    let swarm = server
        .local_swarms
        .create("scoped", "", vec!["a".into(), "b".into()])?;
    let sid = &swarm.swarm_id;
    let fanout = content(
        &server
            .swarm_fanout_local(Parameters(serde_json::from_value(json!({
                "swarm_id": sid, "delegations": [
                    {"agent_name":"a", "task":"one"},
                    {"agent_name":"outsider", "task":"forbidden"},
                    {"agent_name":"b", "task":"two"}
                ]
            }))?))
            .await?,
    )?;
    assert_eq!(fanout["succeeded"], 2);
    assert_eq!(fanout["failed"], 1);
    assert!(fanout["results"][1].to_string().contains("not a member"));
    assert_eq!(recorder.0.lock().expect("calls").len(), 2);
    assert!(
        recorder.0.lock().expect("calls")[1]
            .iter()
            .any(|m| m.role == "assistant" && m.content == "reply 1")
    );
    assert!(
        server
            .swarm_fanout_local(Parameters(serde_json::from_value(json!({
                "swarm_id": sid, "parallel": true,
                "delegations": [{"agent_name":"a", "task":"must not run"}]
            }))?))
            .await
            .is_err()
    );
    assert_eq!(recorder.0.lock().expect("calls").len(), 2);

    let pipeline = content(
        &server
            .swarm_pipeline_local(Parameters(serde_json::from_value(json!({
                "swarm_id": sid, "steps": [
                    {"agent_name":"a", "task":"three"},
                    {"agent_name":"b", "task":"after {prev_output}"}
                ]
            }))?))
            .await?,
    )?;
    assert_eq!(pipeline["steps_completed"], 2);
    assert_eq!(pipeline["final_output"], "reply 4");
    let denied = content(
        &server
            .swarm_pipeline_local(Parameters(serde_json::from_value(json!({
                "swarm_id": sid, "steps": [{"agent_name":"outsider", "task":"forbidden"}]
            }))?))
            .await?,
    )?;
    assert!(denied["results"][0].to_string().contains("not a member"));
    let read = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: sid.clone(),
            }))
            .await?,
    )?;
    assert_eq!(read["turns"].as_array().map(Vec::len), Some(4));
    assert_eq!(read["turns"][3]["sequence"], 4);
    assert_eq!(read["turns"][3]["task"], "after reply 3");
    let calls = recorder.0.lock().expect("calls");
    assert_eq!(
        calls.len(),
        4,
        "nonmember and rejected parallel calls cannot infer"
    );
    assert!(
        calls[3]
            .iter()
            .any(|m| m.role == "assistant" && m.content == "reply 3")
    );
    Ok(())
}

#[tokio::test]
async fn scoped_workflow_dispatches_each_stage_and_refuses_outsider()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = server(dir.path(), recorder.clone(), "test-passphrase");
    server
        .local_registry
        .write_card(&serde_json::from_value(json!({
            "agent_id":"lead", "agent_type":"test", "description":"fixture",
            "capabilities":{"system_prompt":"Answer tasks", "model":"fixture/model"},
            "workflow_template":{"stages":[{"name":"first", "agent":"a"},
                                            {"name":"second", "agent":"b"}]}
        }))?)?;
    for id in ["a", "b"] {
        server
            .local_registry
            .write_card(&serde_json::from_value(json!({
                "agent_id":id, "agent_type":"test", "description":"fixture",
                "capabilities":{"system_prompt":"Answer tasks", "model":"fixture/model"}
            }))?)?;
    }
    let swarm = server
        .local_swarms
        .create("workflow", "", vec!["a".into()])?;
    let result = content(
        &server
            .swarm_run_workflow_local(Parameters(serde_json::from_value(json!({
                "agent_name":"lead", "task":"start", "swarm_id":swarm.swarm_id
            }))?))
            .await?,
    )?;
    assert_eq!(result["stages_completed"], 1);
    assert_eq!(result["stages"][0]["response"], "reply 1");
    assert!(
        result["stages"][1]["error"]
            .as_str()
            .is_some_and(|e| e.contains("not a member"))
    );
    assert_eq!(recorder.0.lock().expect("calls").len(), 1);
    let read = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: swarm.swarm_id,
            }))
            .await?,
    )?;
    assert_eq!(read["turns"].as_array().map(Vec::len), Some(1));

    let members = server
        .local_swarms
        .create("both", "", vec!["a".into(), "b".into()])?;
    let result = content(
        &server
            .swarm_run_workflow_local(Parameters(serde_json::from_value(json!({
                "agent_name":"lead", "task":"again", "swarm_id":members.swarm_id
            }))?))
            .await?,
    )?;
    assert_eq!(result["stages_completed"], 2);
    assert_eq!(result["final_output"], "reply 3");
    let read = content(
        &server
            .swarm_thread_local(Parameters(GetLocalSwarmRequest {
                swarm_id: members.swarm_id,
            }))
            .await?,
    )?;
    assert_eq!(read["turns"].as_array().map(Vec::len), Some(2));
    assert_eq!(read["turns"][1]["task"], "reply 2");
    let calls = recorder.0.lock().expect("calls");
    assert_eq!(calls.len(), 3);
    assert!(
        calls[2]
            .iter()
            .any(|m| m.role == "assistant" && m.content == "reply 2")
    );
    Ok(())
}
