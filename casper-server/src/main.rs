mod config;

use std::sync::Arc;
use axum::{Json, Router, extract::State, routing::get};
use casper_observe::{AuditWriter, RuntimeMetrics, UsageRecorder};
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use config::ServerConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: PgPool,
    pub analytics_db: PgPool,
    pub audit: AuditWriter,
    pub usage: UsageRecorder,
    pub metrics: RuntimeMetrics,
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_status = match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => "ok",
        Err(_) => "error",
    };

    Json(json!({
        "status": "ok",
        "db": db_status,
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn metrics_handler(State(state): State<AppState>) -> String {
    state.metrics.render()
}

async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"
    )
    .execute(pool)
    .await?;

    let migration_files = [
        ("0001_platform", include_str!("../../migrations/0001_platform.sql")),
        ("0002_tenant", include_str!("../../migrations/0002_tenant.sql")),
    ];

    for (name, sql) in migration_files {
        let already_applied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM _migrations WHERE name = $1)"
        )
        .bind(name)
        .fetch_one(pool)
        .await?;

        if !already_applied {
            tracing::info!("Running migration: {name}");
            sqlx::raw_sql(sql).execute(pool).await?;
            sqlx::query("INSERT INTO _migrations (name) VALUES ($1)")
                .bind(name)
                .execute(pool)
                .await?;
            tracing::info!("Migration {name} applied");
        } else {
            tracing::debug!("Migration {name} already applied, skipping");
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".parse().unwrap()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config_path = std::env::var("CASPER_CONFIG")
        .unwrap_or_else(|_| "config/casper-server.yaml".to_string());
    let config = ServerConfig::load(&config_path)?;
    tracing::info!("Loaded config from {config_path}");

    let main_pool = PgPoolOptions::new()
        .max_connections(config.database.main_pool_size)
        .connect(&config.database.url)
        .await?;
    tracing::info!("Connected to database (main pool)");

    let analytics_pool = PgPoolOptions::new()
        .max_connections(config.database.analytics_pool_size)
        .connect(&config.database.url)
        .await?;

    run_migrations(&main_pool).await?;
    tracing::info!("Migrations complete");

    // Start observability
    let (audit, _audit_handle) = AuditWriter::start(main_pool.clone(), 10_000);
    let usage = UsageRecorder::new(main_pool.clone());
    let metrics = RuntimeMetrics::new();

    let cors = if config.listen.cors_origins.is_empty() {
        CorsLayer::new().allow_origin(Any)
    } else {
        let origins: Vec<_> = config
            .listen
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new().allow_origin(origins)
    }
    .allow_methods(Any)
    .allow_headers(Any);

    let state = AppState {
        config: Arc::new(config.clone()),
        db: main_pool,
        analytics_db: analytics_pool,
        audit,
        usage,
        metrics,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = config.listen.port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("Casper server listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received");
}
