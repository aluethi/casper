use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use casper_base::CasperError;
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
        .route(
            "/api/v1/agents/{name}/tasks/{task_id}",
            get(get_task_status),
        )
}
