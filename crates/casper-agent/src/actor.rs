//! Actor registry: manages per-agent/conversation actor tasks.
//!
//! Each unique (tenant, agent, conversation) triple maps to a single actor task
//! running as a tokio::spawn. The actor receives messages via a bounded mpsc
//! channel and processes them sequentially. Responses are sent back via oneshot.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, oneshot};
use uuid::Uuid;

use casper_base::CasperError;

use crate::engine::AgentEngine;

// ── Keys and handles ─────────────────────────────────────────────

/// Unique key for an actor: (tenant, agent, conversation).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActorKey {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub conversation_id: Uuid,
}

impl fmt::Display for ActorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.tenant_id, self.agent_name, self.conversation_id
        )
    }
}

/// Handle to a running actor task.
pub struct ActorHandle {
    /// Send messages to the actor's mailbox.
    pub tx: mpsc::Sender<ActorMessage>,
    /// Tracks when the actor last processed a message.
    pub last_activity: Arc<RwLock<Instant>>,
}

// ── Messages ─────────────────────────────────────────────────────

/// A message sent to an actor for processing.
pub struct ActorMessage {
    /// The user/system content.
    pub content: String,
    /// Who sent this message (e.g., "user:alice@ventoo.ch").
    pub author: String,
    /// Additional metadata (e.g., source, attachments).
    pub metadata: serde_json::Value,
    /// Channel to send the response back on.
    pub response_tx: oneshot::Sender<Result<AgentResponse, CasperError>>,
}

/// The response from processing a message through the agent engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// The agent's reply text.
    pub message: String,
    /// The conversation this reply belongs to.
    pub conversation_id: Uuid,
    /// Token/call usage for this invocation.
    pub usage: AgentUsage,
    /// Correlation ID for tracing.
    pub correlation_id: Uuid,
    /// Intermediate ReAct steps (thinking + tool calls per turn).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<AgentStep>,
}

/// A single ReAct loop iteration: optional thinking + optional tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallStep>>,
}

/// One tool call with its input and result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStep {
    pub name: String,
    pub input: serde_json::Value,
    pub result: String,
    pub is_error: bool,
}

/// Token and call usage for a single agent invocation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: i32,
    pub cache_write_tokens: i32,
    pub llm_calls: i32,
    pub tool_calls: i32,
    pub duration_ms: u64,
}

// ── Actor registry ───────────────────────────────────────────────

/// Concurrent registry of running actor tasks.
pub struct ActorRegistry {
    actors: Arc<DashMap<ActorKey, ActorHandle>>,
    /// Size of the bounded mpsc mailbox per actor.
    mailbox_size: usize,
}

impl ActorRegistry {
    /// Create a new registry.
    pub fn new(mailbox_size: usize) -> Self {
        Self {
            actors: Arc::new(DashMap::new()),
            mailbox_size,
        }
    }

    /// Get or activate an actor for the given key. If no actor exists,
    /// spawns a new tokio task with a bounded mailbox.
    ///
    /// Returns a sender that can be used to send messages to the actor.
    pub fn get_or_activate(
        &self,
        key: ActorKey,
        engine: Arc<AgentEngine>,
    ) -> mpsc::Sender<ActorMessage> {
        // Fast path: actor already exists
        if let Some(handle) = self.actors.get(&key) {
            return handle.tx.clone();
        }

        // Slow path: spawn a new actor
        let (tx, rx) = mpsc::channel(self.mailbox_size);
        let last_activity = Arc::new(RwLock::new(Instant::now()));

        let handle = ActorHandle {
            tx: tx.clone(),
            last_activity: last_activity.clone(),
        };

        // If another thread raced us, use theirs.
        let entry = self.actors.entry(key.clone());
        match entry {
            dashmap::mapref::entry::Entry::Occupied(existing) => {
                return existing.get().tx.clone();
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(handle);
            }
        }

        // Spawn the actor task
        let actor_key = key.clone();
        let actors_map = Arc::clone(&self.actors);

        tokio::spawn(async move {
            Self::actor_loop(actor_key.clone(), rx, engine, last_activity).await;
            // Remove ourselves from the registry when the mailbox closes
            actors_map.remove(&actor_key);
            tracing::debug!(actor = %actor_key, "actor task exited");
        });

        tx
    }

    /// The main loop for an actor task.
    async fn actor_loop(
        key: ActorKey,
        mut rx: mpsc::Receiver<ActorMessage>,
        engine: Arc<AgentEngine>,
        last_activity: Arc<RwLock<Instant>>,
    ) {
        tracing::info!(actor = %key, "actor activated");

        while let Some(msg) = rx.recv().await {
            // Update activity timestamp
            {
                let mut ts = last_activity.write().await;
                *ts = Instant::now();
            }

            let start = Instant::now();

            // Process the message through the agent engine
            let result = engine
                .run(
                    key.tenant_id,
                    &key.agent_name,
                    key.conversation_id,
                    &msg.content,
                    &msg.author,
                    &msg.metadata,
                )
                .await;

            let duration = start.elapsed();
            tracing::debug!(
                actor = %key,
                duration_ms = duration.as_millis() as u64,
                success = result.is_ok(),
                "actor message processed"
            );

            // Send response (ignore error if caller dropped the receiver)
            let _ = msg.response_tx.send(result);

            // Update activity again after processing
            {
                let mut ts = last_activity.write().await;
                *ts = Instant::now();
            }
        }

        tracing::info!(actor = %key, "actor mailbox closed");
    }

    /// Return the number of active actors.
    pub fn active_count(&self) -> usize {
        self.actors.len()
    }

    /// Remove an actor by key, closing its mailbox (which causes the task to exit).
    pub fn remove(&self, key: &ActorKey) -> bool {
        self.actors.remove(key).is_some()
    }

    /// Get all actor keys (for reaper scanning).
    pub fn keys(&self) -> Vec<ActorKey> {
        self.actors.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get the last activity time for an actor.
    pub async fn last_activity(&self, key: &ActorKey) -> Option<Instant> {
        if let Some(handle) = self.actors.get(key) {
            Some(*handle.last_activity.read().await)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_key_display() {
        let key = ActorKey {
            tenant_id: Uuid::nil(),
            agent_name: "triage".to_string(),
            conversation_id: Uuid::nil(),
        };
        let s = key.to_string();
        assert!(s.contains("triage"));
    }

    #[test]
    fn actor_key_equality() {
        let a = ActorKey {
            tenant_id: Uuid::nil(),
            agent_name: "triage".to_string(),
            conversation_id: Uuid::nil(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn registry_new() {
        let registry = ActorRegistry::new(32);
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn agent_usage_default() {
        let usage = AgentUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.llm_calls, 0);
    }
}
