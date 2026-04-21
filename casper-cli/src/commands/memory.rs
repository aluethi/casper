use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum MemoryCmd {
    /// Show agent memory.
    Show {
        /// Agent name.
        agent: String,
    },
    /// Update agent memory (reads JSON from stdin).
    Update {
        /// Agent name.
        agent: String,
    },
}

#[derive(Subcommand)]
pub enum TenantMemoryCmd {
    /// Show tenant-level memory.
    Show,
    /// Update tenant-level memory (reads JSON from stdin).
    Update,
}

pub async fn handle(cmd: MemoryCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;

    match cmd {
        MemoryCmd::Show { agent } => {
            let resp = client
                .get(format!("{server}/api/v1/agents/{agent}/memory"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        MemoryCmd::Update { agent } => {
            let mut buf = String::new();
            use std::io::Read;
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("failed to read stdin: {e}"))?;
            let value: serde_json::Value =
                serde_json::from_str(&buf).map_err(|e| format!("failed to parse JSON: {e}"))?;

            let resp = client
                .put(format!("{server}/api/v1/agents/{agent}/memory"))
                .json(&value)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }
    }
}

pub async fn handle_tenant(cmd: TenantMemoryCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let url = format!("{server}/api/v1/tenant-memory");

    match cmd {
        TenantMemoryCmd::Show => {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        TenantMemoryCmd::Update => {
            let mut buf = String::new();
            use std::io::Read;
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("failed to read stdin: {e}"))?;
            let value: serde_json::Value =
                serde_json::from_str(&buf).map_err(|e| format!("failed to parse JSON: {e}"))?;

            let resp = client
                .put(&url)
                .json(&value)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }
    }
}
