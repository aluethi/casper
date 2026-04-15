pub mod auth;
mod config;
mod routes;

use std::sync::Arc;
use axum::{Json, Router, extract::State, middleware, routing::get};
use casper_auth::{JwtSigner, RevocationCache};
use casper_base::JwtVerifier;
use casper_observe::{AuditWriter, RuntimeMetrics, UsageRecorder};
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use auth::AuthState;
use config::ServerConfig;
use routes::auth_routes::auth_router;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: PgPool,
    pub db_owner: PgPool,  // Bypasses RLS (table owner)
    pub analytics_db: PgPool,
    pub audit: AuditWriter,
    pub usage: UsageRecorder,
    pub metrics: RuntimeMetrics,
    pub jwt_signer: Option<Arc<JwtSigner>>,
    pub jwt_verifier: Option<Arc<JwtVerifier>>,
    pub revocation_cache: RevocationCache,
    pub http_client: reqwest::Client,
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

fn setup_signing_keys(config: &ServerConfig) -> (Option<Arc<JwtSigner>>, Option<Arc<JwtVerifier>>) {
    if config.auth.dev_auth {
        // In dev mode, generate ephemeral keys
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng)
            .expect("failed to generate dev signing key");
        let pkcs8_bytes = pkcs8.as_ref();

        let signer = JwtSigner::from_pkcs8_der(pkcs8_bytes)
            .expect("failed to create JWT signer");

        use ring::signature::KeyPair;
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes)
            .expect("failed to parse keypair");
        let pub_bytes: [u8; 32] = key_pair.public_key().as_ref().try_into().unwrap();
        let verifier = JwtVerifier::from_public_key(&pub_bytes)
            .expect("failed to create JWT verifier");

        tracing::info!("Dev mode: using ephemeral signing keys");
        (Some(Arc::new(signer)), Some(Arc::new(verifier)))
    } else {
        // Production: load from file
        match &config.auth.signing_key_file {
            Some(path) => {
                let key_bytes = std::fs::read(path)
                    .unwrap_or_else(|e| panic!("failed to read signing key from {path}: {e}"));
                let signer = JwtSigner::from_pkcs8_der(&key_bytes)
                    .expect("failed to create JWT signer from file");

                use ring::signature::KeyPair;
                let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(&key_bytes)
                    .expect("failed to parse keypair from file");
                let pub_bytes: [u8; 32] = key_pair.public_key().as_ref().try_into().unwrap();
                let verifier = JwtVerifier::from_public_key(&pub_bytes)
                    .expect("failed to create JWT verifier from file");

                (Some(Arc::new(signer)), Some(Arc::new(verifier)))
            }
            None => {
                tracing::warn!("No signing key configured, JWT auth disabled");
                (None, None)
            }
        }
    }
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

    // Owner pool — bypasses RLS (for auth lookups, platform admin)
    let owner_url = config.database.owner_url.as_deref().unwrap_or(&config.database.url);
    let owner_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(owner_url)
        .await?;
    tracing::info!("Connected to database (owner pool)");

    run_migrations(&owner_pool).await?;
    tracing::info!("Migrations complete");

    // Start observability
    let (audit, _audit_handle) = AuditWriter::start(main_pool.clone(), 10_000);
    let usage = UsageRecorder::new(main_pool.clone());
    let metrics = RuntimeMetrics::new();

    // Setup signing keys
    let (jwt_signer, jwt_verifier) = setup_signing_keys(&config);

    // Setup revocation cache
    let revocation_cache = RevocationCache::new();

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

    // HTTP client for LLM proxy calls
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("failed to build HTTP client");

    let state = AppState {
        config: Arc::new(config.clone()),
        db: main_pool.clone(),
        db_owner: owner_pool,
        analytics_db: analytics_pool,
        audit,
        usage,
        metrics,
        jwt_signer,
        jwt_verifier: jwt_verifier.clone(),
        revocation_cache: revocation_cache.clone(),
        http_client,
    };

    // Auth middleware state
    let auth_state = AuthState {
        jwt_verifier: jwt_verifier.unwrap_or_else(|| {
            // Dummy verifier for when JWT is not configured
            Arc::new(JwtVerifier::from_public_key(&[0u8; 32]).unwrap())
        }),
        revocation_cache,
        db: main_pool,
    };

    // Routes that require authentication
    let authenticated = Router::new()
        .route("/auth/status", get(routes::auth_routes::auth_status))
        .merge(routes::tenant_routes::tenant_router())
        .merge(routes::sso_routes::sso_router())
        .merge(routes::domain_routes::domain_router())
        .merge(routes::user_routes::user_router())
        .merge(routes::apikey_routes::apikey_router())
        .merge(routes::secret_routes::secret_router())
        .merge(routes::model_routes::model_router())
        .merge(routes::backend_routes::backend_router())
        .merge(routes::quota_routes::quota_router())
        .merge(routes::catalog_routes::catalog_router())
        .merge(routes::deployment_routes::deployment_router())
        .merge(routes::inference_routes::inference_router())
        .merge(routes::knowledge_routes::knowledge_router())
        .merge(routes::memory_routes::memory_router())
        .merge(routes::snippet_routes::snippet_router())
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth::auth_middleware,
        ));

    // Public routes (no auth required)
    let public = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .merge(auth_router());

    let app = Router::new()
        .merge(authenticated)
        .merge(public)
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
