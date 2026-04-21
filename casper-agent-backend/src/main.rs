mod ws;

use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

/// Casper Agent Backend sidecar — connects local inference servers to Casper.
///
/// Each backend entry maps a Casper agent key to a local inference URL.
/// The sidecar opens one WebSocket per key and dispatches inference requests
/// to the configured local endpoint.
///
/// ```yaml
/// casper_url: ws://casper.ventoo.ai/agent/connect
/// backends:
///   - key: csa-abc123...
///     inference_url: http://localhost:8000    # vLLM with Llama 3
///   - key: csa-def456...
///     inference_url: http://localhost:8001    # Ollama with Gemma 2
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
    /// Casper WebSocket URL.
    pub casper_url: String,

    /// Backend entries. Each maps a key to a local inference URL.
    #[serde(default)]
    pub backends: Vec<BackendEntry>,

    /// Shorthand for a single backend (for simple setups).
    #[serde(default, alias = "agent_key")]
    pub key: Option<String>,
    /// Inference URL for the single-key shorthand.
    #[serde(default)]
    pub inference_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendEntry {
    /// Agent backend key: csa-...
    pub key: String,
    /// Local inference server URL (e.g. http://localhost:8000).
    #[serde(default = "default_inference_url")]
    pub inference_url: String,
}

fn default_inference_url() -> String {
    "http://localhost:11434".to_string()
}

impl SidecarConfig {
    /// Resolve all backend entries (merges single-key shorthand with backends list).
    pub fn all_backends(&self) -> Vec<BackendEntry> {
        let mut all = self.backends.clone();
        if let Some(k) = &self.key {
            if !all.iter().any(|b| b.key == *k) {
                all.insert(
                    0,
                    BackendEntry {
                        key: k.clone(),
                        inference_url: self
                            .inference_url
                            .clone()
                            .unwrap_or_else(default_inference_url),
                    },
                );
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
    let config_str = std::fs::read_to_string(&cli.config)
        .map_err(|e| format!("failed to read config {:?}: {e}", cli.config))?;
    let config: SidecarConfig = serde_yaml::from_str(&config_str)?;

    let backends = config.all_backends();
    if backends.is_empty() {
        return Err(
            "no backends configured (set `key` + `inference_url` or `backends` list)".into(),
        );
    }

    tracing::info!(
        casper_url = %config.casper_url,
        backends = backends.len(),
        "starting casper-agent-backend sidecar"
    );

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    // Spawn one connection per backend — each reconnects independently
    let mut handles = Vec::new();
    for entry in backends {
        let casper_url = config.casper_url.clone();
        let client = http_client.clone();
        let prefix = if entry.key.len() >= 12 {
            &entry.key[..12]
        } else {
            &entry.key
        };
        tracing::info!(key_prefix = %prefix, inference_url = %entry.inference_url, "spawning backend connection");

        handles.push(tokio::spawn(async move {
            let mut backoff_secs = 1u64;
            loop {
                match ws::run_connection(&casper_url, &entry.key, &entry.inference_url, &client).await {
                    Ok(()) => {
                        tracing::info!(key_prefix = %&entry.key[..12.min(entry.key.len())], "connection closed");
                        backoff_secs = 1;
                    }
                    Err(e) => {
                        tracing::error!(key_prefix = %&entry.key[..12.min(entry.key.len())], error = %e, "connection failed");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
            }
        }));
    }

    futures::future::join_all(handles).await;
    Ok(())
}
