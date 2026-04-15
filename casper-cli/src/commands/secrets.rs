use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum SecretsCmd {
    /// Set a secret (reads value from stdin).
    Set {
        /// Secret key name.
        key: String,
    },
    /// List all secret keys (values are not shown).
    List,
    /// Delete a secret.
    Delete {
        /// Secret key name.
        key: String,
    },
}

pub async fn handle(cmd: SecretsCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let base = format!("{server}/api/v1/secrets");

    match cmd {
        SecretsCmd::Set { key } => {
            let mut value = String::new();
            use std::io::Read;
            std::io::stdin()
                .read_to_string(&mut value)
                .map_err(|e| format!("failed to read stdin: {e}"))?;

            let body = serde_json::json!({
                "key": key,
                "value": value.trim_end(),
            });

            let resp = client
                .post(&base)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            if resp.status().is_success() {
                println!("Secret '{key}' set.");
                Ok(())
            } else {
                print_response(resp).await
            }
        }

        SecretsCmd::List => {
            let resp = client
                .get(&base)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        SecretsCmd::Delete { key } => {
            let resp = client
                .delete(format!("{base}/{key}"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            if resp.status().is_success() {
                println!("Secret '{key}' deleted.");
                Ok(())
            } else {
                print_response(resp).await
            }
        }
    }
}
