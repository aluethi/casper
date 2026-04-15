mod ws;

use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

/// Casper Agent Backend sidecar — connects a local inference server to Casper.
#[derive(Parser)]
#[command(name = "casper-agent-backend", version, about)]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SidecarConfig {
    pub casper_url: String,
    pub agent_key: String,
    pub backend_id: Uuid,
    pub inference_server: InferenceServerConfig,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
}

fn default_max_concurrent() -> u32 {
    8
}

#[derive(Debug, Clone, Deserialize)]
pub struct InferenceServerConfig {
    #[serde(rename = "type")]
    pub server_type: String,
    pub base_url: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    let config_str = std::fs::read_to_string(&cli.config).map_err(|e| {
        format!("failed to read config file {:?}: {e}", cli.config)
    })?;
    let config: SidecarConfig = serde_yaml::from_str(&config_str)?;

    tracing::info!(
        casper_url = %config.casper_url,
        server_type = %config.inference_server.server_type,
        base_url = %config.inference_server.base_url,
        max_concurrent = config.max_concurrent,
        "starting casper-agent-backend sidecar"
    );

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    // Reconnect loop with exponential backoff
    let mut backoff_secs = 1u64;
    let max_backoff = 30u64;

    loop {
        match ws::run_connection(&config, &http_client).await {
            Ok(()) => {
                tracing::info!("WebSocket connection closed gracefully");
                backoff_secs = 1;
            }
            Err(e) => {
                tracing::error!(error = %e, "WebSocket connection failed");
            }
        }

        tracing::info!(backoff_secs, "reconnecting in {backoff_secs}s...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(max_backoff);
    }
}
