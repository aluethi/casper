use std::convert::Infallible;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
};
use casper_base::CasperError;
use casper_proxy::StreamEvent;
use futures::{stream::Stream, StreamExt};
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::run_service::{
    self, AsyncAccepted, RunRequest, TaskStatusResponse,
};

// ── Handlers ────────────────────────────────────────────────────

/// POST /api/v1/agents/:name/run
async fn run_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<RunRequest>,
) -> Result<axum::response::Response, CasperError> {
    guard.require(&format!("agents:{name}:run"))?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let actor = guard.0.actor();

    // Prepare (validate agent, create/verify conversation)
    let conversation_id = run_service::prepare_conversation(
        &state.db,
        tenant_id,
        &name,
        body.conversation_id,
        &body.message,
    )
    .await?;

    // If async mode, spawn a background task
    if body.r#async {
        let task_id = Uuid::now_v7();
        state.async_tasks.insert(task_id, None);

        let state_clone = state.clone();
        let name_clone = name.clone();
        let message = body.message.clone();
        let metadata = body.metadata.clone();
        let actor_clone = actor.clone();

        tokio::spawn(async move {
            let result = run_service::execute_run(
                &state_clone,
                tenant_id.0,
                &name_clone,
                conversation_id,
                &message,
                &actor_clone,
                &metadata,
            )
            .await;

            match result {
                Ok(run_resp) => {
                    let value = serde_json::to_value(&run_resp).unwrap_or_default();
                    state_clone.async_tasks.insert(task_id, Some(value));
                }
                Err(e) => {
                    let error_value = serde_json::json!({
                        "error": e.to_string()
                    });
                    state_clone.async_tasks.insert(task_id, Some(error_value));
                }
            }
        });

        let accepted = AsyncAccepted {
            task_id,
            status: "accepted",
            poll_url: format!("/api/v1/agents/{name}/tasks/{task_id}"),
        };

        Ok((StatusCode::ACCEPTED, Json(accepted)).into_response())
    } else {
        // Sync mode: run directly
        let run_resp = run_service::execute_run(
            &state,
            tenant_id.0,
            &name,
            conversation_id,
            &body.message,
            &actor,
            &body.metadata,
        )
        .await?;

        Ok(Json(run_resp).into_response())
    }
}

/// POST /api/v1/agents/:name/run/stream -- Streaming agent run via SSE.
async fn run_agent_stream(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<RunRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, CasperError> {
    guard.require(&format!("agents:{name}:run"))?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let actor = guard.0.actor();

    let conversation_id = run_service::prepare_conversation(
        &state.db, tenant_id, &name, body.conversation_id, &body.message,
    ).await?;

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

    // Spawn the agent engine in the background
    let state_clone = state.clone();
    let name_clone = name.clone();
    let message = body.message.clone();
    let metadata = body.metadata.clone();
    let actor_clone = actor.clone();

    tokio::spawn(async move {
        let engine = casper_agent::engine::AgentEngine::new(
            state_clone.db_owner.clone(),
            state_clone.http_client.clone(),
            casper_agent::tools::ToolDispatcher::new(),
            Some(state_clone.audit.clone()),
            Some(state_clone.usage.clone()),
        );

        if let Err(e) = engine.run_stream(
            tenant_id.0,
            &name_clone,
            conversation_id,
            &message,
            &actor_clone,
            &metadata,
            tx.clone(),
        ).await {
            let _ = tx.send(StreamEvent::Error { message: e.to_string() }).await;
        }
        // tx is dropped here, closing the stream
    });

    // Convert the mpsc receiver to an SSE stream
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        let event_name = match &event {
            StreamEvent::Thinking { .. } => "thinking",
            StreamEvent::ContentDelta { .. } => "content_delta",
            StreamEvent::ToolCallStart { .. } => "tool_call_start",
            StreamEvent::ToolResult { .. } => "tool_result",
            StreamEvent::Done { .. } => "done",
            StreamEvent::Error { .. } => "error",
        };
        let data = serde_json::to_string(&event).unwrap_or_default();
        Ok(Event::default().event(event_name).data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// GET /api/v1/agents/:name/tasks/:task_id -- Poll async task result.
async fn get_task_status(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((name, task_id)): Path<(String, Uuid)>,
) -> Result<Json<TaskStatusResponse>, CasperError> {
    guard.require(&format!("agents:{name}:run"))?;
    let result = run_service::get_task_status(&state, task_id)?;
    Ok(Json(result))
}

// ── Router ──────────────────────────────────────────────────────

pub fn run_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents/{name}/run", post(run_agent))
        .route("/api/v1/agents/{name}/run/stream", post(run_agent_stream))
        .route(
            "/api/v1/agents/{name}/tasks/{task_id}",
            get(get_task_status),
        )
}
