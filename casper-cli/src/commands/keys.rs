use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum KeysCmd {
    /// Create a new API key.
    Create {
        /// Human-readable label for the key.
        #[arg(long)]
        label: Option<String>,
        /// Comma-separated list of scopes.
        #[arg(long)]
        scopes: Option<String>,
    },
    /// List all API keys.
    List,
    /// Revoke an API key.
    Revoke {
        /// API key ID to revoke.
        id: String,
    },
}

pub async fn handle(cmd: KeysCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let base = format!("{server}/api/v1/api-keys");

    match cmd {
        KeysCmd::Create { label, scopes } => {
            let mut body = serde_json::Map::new();
            if let Some(l) = label {
                body.insert("label".into(), serde_json::Value::String(l));
            }
            if let Some(s) = scopes {
                let scope_list: Vec<serde_json::Value> = s
                    .split(',')
                    .map(|s| serde_json::Value::String(s.trim().to_string()))
                    .collect();
                body.insert("scopes".into(), serde_json::Value::Array(scope_list));
            }

            let resp = client
                .post(&base)
                .json(&serde_json::Value::Object(body))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        KeysCmd::List => {
            let resp = client
                .get(&base)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        KeysCmd::Revoke { id } => {
            let resp = client
                .delete(format!("{base}/{id}"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            if resp.status().is_success() {
                println!("API key '{id}' revoked.");
                Ok(())
            } else {
                print_response(resp).await
            }
        }
    }
}
