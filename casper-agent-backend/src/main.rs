mod ws;

use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

/// Casper Agent Backend sidecar — connects a local inference server to Casper.
///
/// The sidecar can serve multiple backends from a single machine. Each key
/// corresponds to one backend (and one model). All keys share the same local
/// inference server.
///
/// Minimal config:
/// ```yaml
/// casper_url: ws://casper.ventoo.ai/agent/connect
/// keys:
///   - csa-abc...    # Llama 3 backend
///   - csa-def...    # Gemma 2 backend
/// ```
#[derive(Parser)]
#[command(name = "casper-agent-backend", version, about)]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SidecarConfig {
    /// WebSocket URL: ws://casper.ventoo.ai/agent/connect
    pub casper_url: String,

    /// Agent backend keys. Each key maps to one backend (one model).
    /// All share the same local inference server.
    #[serde(default)]
    pub keys: Vec<String>,

    /// Single key (convenience for single-backend setups).
    /// If both `key` and `keys` are set, they are merged.
    #[serde(default, alias = "agent_key")]
    pub key: Option<String>,

    /// Optional: override the inference server URL from the server config.
    #[serde(default)]
    pub inference_base_url: Option<String>,
}

impl SidecarConfig {
    /// Get all keys (merges `key` and `keys` fields, deduplicates).
    pub fn all_keys(&self) -> Vec<String> {
        let mut all = self.keys.clone();
        if let Some(k) = &self.key {
            if !all.contains(k) {
                all.insert(0, k.clone());
            }
        }
        all
    }
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

    let keys = config.all_keys();
    if keys.is_empty() {
        return Err("no agent keys configured (set `key` or `keys` in config)".into());
    }

    tracing::info!(
        casper_url = %config.casper_url,
        backends = keys.len(),
        "starting casper-agent-backend sidecar"
    );

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    // Spawn one connection per key — each reconnects independently
    let mut handles = Vec::new();
    for agent_key in keys {
        let cfg = config.clone();
        let client = http_client.clone();
        let prefix = if agent_key.len() >= 12 { &agent_key[..12] } else { &agent_key };
        tracing::info!(key_prefix = %prefix, "spawning connection");

        handles.push(tokio::spawn(async move {
            let mut backoff_secs = 1u64;
            let max_backoff = 30u64;
            loop {
                match ws::run_connection(&cfg.casper_url, &agent_key, cfg.inference_base_url.as_deref(), &client).await {
                    Ok(()) => {
                        tracing::info!(key_prefix = %&agent_key[..12.min(agent_key.len())], "connection closed gracefully");
                        backoff_secs = 1;
                    }
                    Err(e) => {
                        tracing::error!(key_prefix = %&agent_key[..12.min(agent_key.len())], error = %e, "connection failed");
                    }
                }
                tracing::info!(backoff_secs, "reconnecting...");
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(max_backoff);
            }
        }));
    }

    // Wait for all connections (they run forever unless killed)
    futures::future::join_all(handles).await;
    Ok(())
}
