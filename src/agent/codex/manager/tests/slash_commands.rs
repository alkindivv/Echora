use super::*;
use crate::agent::AgentFeedbackRequest;

#[test]
fn durable_fork_correlates_notification_and_never_becomes_a_temporary_side_chat() {
    let (manager, spawner) = manager_with_fake();
    let receiver = manager.fork_conversation("parent".into(), "/tmp/worktree".into());
    let mut endpoint = spawner.next_endpoint();
    handshake(&mut endpoint);
    let fork = endpoint.recv();
    assert_eq!(fork["method"], "thread/fork");
    assert_eq!(fork["params"]["ephemeral"], false);
    assert_eq!(fork["params"]["cwd"], "/tmp/worktree");
    assert!(fork["params"].get("developerInstructions").is_none());
    let mut thread = workspace_thread("durable", None);
    thread["ephemeral"] = json!(false);
    endpoint.send(json!({"method":"thread/started","params":{"thread":thread}}));
    endpoint.respond(&fork, json!({"thread":thread}));
    assert_eq!(wait_value(&receiver).unwrap(), "durable");
    assert!(!manager.is_temporary_thread("durable"));
}
#[test]
fn durable_fork_rejects_a_different_started_identity() {
    let (manager, spawner) = manager_with_fake();
    let receiver = manager.fork_conversation("parent".into(), "/tmp/repo".into());
    let mut endpoint = spawner.next_endpoint();
    handshake(&mut endpoint);
    let fork = endpoint.recv();
    let mut started = workspace_thread("started", None);
    started["ephemeral"] = json!(false);
    endpoint.send(json!({"method":"thread/started","params":{"thread":started}}));
    let mut returned = workspace_thread("different", None);
    returned["ephemeral"] = json!(false);
    endpoint.respond(&fork, json!({"thread":returned}));
    assert!(wait_value(&receiver).is_err());
}
#[test]
fn feedback_upload_transmits_explicit_log_choices_and_requires_a_receipt() {
    let (manager, spawner) = manager_with_fake();
    let receiver = manager.upload_feedback(AgentFeedbackRequest {
        classification: "bug".into(),
        reason: "中文 input".into(),
        include_logs: false,
        thread_id: Some("parent".into()),
        extra_log_files: vec!["/tmp/browser logs.json".into()],
    });
    let mut endpoint = spawner.next_endpoint();
    handshake(&mut endpoint);
    let request = endpoint.recv();
    assert_eq!(request["method"], "feedback/upload");
    assert_eq!(
        request["params"],
        json!({"classification":"bug","reason":"中文 input","includeLogs":false,"threadId":"parent","extraLogFiles":["/tmp/browser logs.json"]})
    );
    endpoint.respond(&request, json!({"threadId":"receipt","promptHash":"hash"}));
    assert_eq!(wait_value(&receiver).unwrap(), "receipt");
    let receiver = manager.upload_feedback(AgentFeedbackRequest {
        classification: "other".into(),
        reason: "no logs".into(),
        ..Default::default()
    });
    let request = endpoint.recv();
    assert!(request["params"].get("extraLogFiles").is_none());
    endpoint.respond(&request, json!({}));
    assert!(wait_value(&receiver).is_err());
}
