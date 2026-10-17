//! Durable conversation forks use the shared lifecycle registry.
use super::{
    CodexAppServerManager,
    connection::{PendingThreadLifecycle, ThreadLifecycleKind},
};
use crate::agent::{ThreadId, WorkspaceResult};
use anyhow::{Context as _, anyhow, bail};
use async_channel::Receiver;
use serde_json::{Value, json};
impl CodexAppServerManager {
    pub(in crate::agent::codex) fn fork_conversation(
        &self,
        thread_id: ThreadId,
        cwd: std::path::PathBuf,
    ) -> Receiver<WorkspaceResult<ThreadId>> {
        self.workspace_call(move |manager| {
            let connection = manager.inner.ensure_connection()?;
            let _lifecycle = connection.lifecycle_lock.lock().map_err(|_| anyhow!("Codex thread lifecycle lock poisoned"))?;
            {
                let mut state = connection.state.lock().map_err(|_| anyhow!("Codex connection state lock poisoned"))?;
                if state.pending_thread_lifecycle.is_some() { bail!("Codex thread lifecycle registry occupied"); }
                state.pending_thread_lifecycle = Some(PendingThreadLifecycle { kind: ThreadLifecycleKind::Fork, observed_thread_id: None });
            }
            let response = match connection.request("thread/fork", json!({"threadId": thread_id, "cwd": cwd, "ephemeral": false, "excludeTurns": true, "threadSource": "user"})) {
                Ok(response) => response,
                Err(error) => {
                    if let Ok(mut state) = connection.state.lock() { state.pending_thread_lifecycle = None; }
                    return Err(error);
                }
            };
            let parsed = (|| {
                let id = response.pointer("/result/thread/id").and_then(Value::as_str).filter(|id| !id.is_empty() && *id != thread_id).context("thread/fork missing a new thread id")?;
                if response.pointer("/result/thread/ephemeral").and_then(Value::as_bool) != Some(false) { bail!("thread/fork did not create a durable chat"); }
                let mut state = connection.state.lock().map_err(|_| anyhow!("Codex connection state lock poisoned"))?;
                let pending = state.pending_thread_lifecycle.take().context("thread/fork lifecycle registry empty")?;
                if pending.observed_thread_id.as_deref().is_some_and(|observed| observed != id) { bail!("thread/fork response disagrees with thread/started"); }
                if let Some(settings) = super::super::notifications::lifecycle_settings(&response)? { state.thread_settings.insert(id.to_owned(), settings); }
                state.loaded_threads.insert(id.to_owned());
                Ok(id.to_owned())
            })();
            super::workspace::validate_workspace_response(&connection, "thread/fork", parsed)
        })
    }
}
