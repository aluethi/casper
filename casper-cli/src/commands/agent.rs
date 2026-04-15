use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum AgentCmd {
    /// List all agents.
    List,
    /// Get details of a specific agent.
    Get {
        /// Agent name.
        name: String,
    },
    /// Create an agent from a YAML file.
    Create {
        /// Path to YAML definition file.
        #[arg(short, long)]
        file: String,
    },
    /// Delete an agent.
    Delete {
        /// Agent name.
        name: String,
    },
    /// Export an agent definition.
    Export {
        /// Agent name.
        name: String,
    },
    /// Import an agent from a file.
    Import {
        /// Path to agent definition file.
        file: String,
    },
}

pub async fn handle(cmd: AgentCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let base = format!("{server}/api/v1/agents");

    match cmd {
        AgentCmd::List => {
            let resp = client
                .get(&base)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        AgentCmd::Get { name } => {
            let resp = client
                .get(format!("{base}/{name}"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        AgentCmd::Create { file } => {
            let contents = std::fs::read_to_string(&file)
                .map_err(|e| format!("failed to read {file}: {e}"))?;
            let yaml_value: serde_json::Value = serde_yaml::from_str(&contents)
                .map_err(|e| format!("failed to parse YAML: {e}"))?;

            let resp = client
                .post(&base)
                .json(&yaml_value)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        AgentCmd::Delete { name } => {
            let resp = client
                .delete(format!("{base}/{name}"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            if resp.status().is_success() {
                println!("Agent '{name}' deleted.");
                Ok(())
            } else {
                print_response(resp).await
            }
        }

        AgentCmd::Export { name } => {
            let resp = client
                .get(format!("{base}/{name}/export"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        AgentCmd::Import { file } => {
            let contents = std::fs::read_to_string(&file)
                .map_err(|e| format!("failed to read {file}: {e}"))?;
            let value: serde_json::Value = serde_json::from_str(&contents)
                .map_err(|e| format!("failed to parse JSON: {e}"))?;

            let resp = client
                .post(format!("{base}/import"))
                .json(&value)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }
    }
}
