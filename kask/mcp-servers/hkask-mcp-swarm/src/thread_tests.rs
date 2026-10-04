use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;

use crate::request_types::{DelegateInThreadLocalRequest, GetLocalSwarmRequest};
use crate::test_support::{RecordingInference, content, make_thread_server};

#[tokio::test]
async fn scoped_thread_is_structured_durable_isolated_and_archived()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = make_thread_server(dir.path(), recorder.clone(), "test-passphrase");
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
    let restarted = make_thread_server(dir.path(), recorder.clone(), "test-passphrase");
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
    let restarted_again = make_thread_server(dir.path(), recorder, "test-passphrase");
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

#[tokio::test]
async fn malformed_roster_refuses_scoped_inference_instead_of_using_cached_members()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let recorder = Arc::new(RecordingInference::default());
    let server = make_thread_server(dir.path(), recorder.clone(), "test-passphrase");
    server
        .local_registry
        .write_card(&serde_json::from_value(json!({
            "agent_id": "a", "agent_type": "test", "description": "fixture",
            "capabilities": {"system_prompt": "Answer tasks", "model": "fixture/model"}
        }))?)?;
    let swarm = server
        .local_swarms
        .create("protected", "", vec!["a".into()])?;
    assert!(server.local_swarms.get(&swarm.swarm_id).is_some());
    std::fs::write(
        dir.path()
            .join("swarms")
            .join(&swarm.swarm_id)
            .join("swarm.json"),
        "{malformed",
    )?;
    assert!(
        server
            .swarm_delegate_in_thread_local(Parameters(DelegateInThreadLocalRequest {
                swarm_id: swarm.swarm_id,
                agent_name: "a".into(),
                task: "must not run".into(),
            }))
            .await
            .is_err()
    );
    assert_eq!(recorder.0.lock().expect("calls").len(), 0);
    Ok(())
}

#[tokio::test]
async fn independent_swarms_do_not_wait_for_each_others_inference()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let store = crate::thread_store::SwarmThreadStore::new(
        dir.path().join("threads.db").to_string_lossy().into_owned(),
        "test-passphrase".into(),
    );
    let first = store.lock("first").await?;
    let second =
        tokio::time::timeout(std::time::Duration::from_secs(2), store.lock("second")).await??;
    drop(second);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), store.lock("first"),)
            .await
            .is_err(),
        "same swarm must wait for the first turn"
    );
    drop(first);
    assert!(store.lock("../outside").await.is_err());
    Ok(())
}
