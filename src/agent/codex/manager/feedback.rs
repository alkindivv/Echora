//! Explicit user feedback uploads, with an acknowledged receipt identity.
use super::CodexAppServerManager;
use crate::agent::{AgentFeedbackRequest, ThreadId, WorkspaceResult};
use anyhow::Context as _;
use async_channel::Receiver;
use serde_json::{Value, json};
impl CodexAppServerManager {
    pub(in crate::agent::codex) fn upload_feedback(
        &self,
        request: AgentFeedbackRequest,
    ) -> Receiver<WorkspaceResult<ThreadId>> {
        self.workspace_call(move |manager| {
            let connection=manager.inner.ensure_connection()?;
            let mut params=json!({"classification":request.classification,"reason":request.reason,"includeLogs":request.include_logs,"threadId":request.thread_id});
            if !request.extra_log_files.is_empty() {params["extraLogFiles"]=json!(request.extra_log_files);}
            let response=connection.request("feedback/upload",params)?;
            super::workspace::validate_workspace_response(&connection,"feedback/upload",(|| {
                let id=response.pointer("/result/threadId").and_then(Value::as_str).filter(|id|!id.is_empty()).context("feedback/upload missing threadId")?;
                if let Some(hash)=response.pointer("/result/promptHash") {anyhow::ensure!(hash.is_null()||hash.is_string(),"feedback/upload invalid promptHash");}
                Ok(id.to_owned())
            })())
        })
    }
}
