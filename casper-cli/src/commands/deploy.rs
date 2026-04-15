use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum DeployCmd {
    /// List all deployments.
    List,
    /// Create a new deployment (reads JSON from --file or stdin).
    Create {
        /// Path to deployment JSON file (reads from stdin if not provided).
        #[arg(short, long)]
        file: Option<String>,
    },
    /// Test a deployment.
    Test {
        /// Deployment ID.
        id: String,
    },
}

pub async fn handle(cmd: DeployCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let base = format!("{server}/api/v1/deployments");

    match cmd {
        DeployCmd::List => {
            let resp = client
                .get(&base)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        DeployCmd::Create { file } => {
            let json_str = match file {
                Some(path) => std::fs::read_to_string(&path)
                    .map_err(|e| format!("failed to read {path}: {e}"))?,
                None => {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| format!("failed to read stdin: {e}"))?;
                    buf
                }
            };
            let value: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| format!("failed to parse JSON: {e}"))?;

            let resp = client
                .post(&base)
                .json(&value)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        DeployCmd::Test { id } => {
            let resp = client
                .post(format!("{base}/{id}/test"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }
    }
}
