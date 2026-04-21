use clap::Subcommand;

use crate::credentials::{Credentials, remove_credentials, save_credentials};
use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum AuthCmd {
    /// Log in to the Casper server.
    Login {
        /// Server URL (default: http://localhost:3000).
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Email address.
        #[arg(long)]
        email: String,
        /// Password.
        #[arg(long)]
        password: String,
    },
    /// Show current authentication status.
    Status,
    /// Log out and remove stored credentials.
    Logout,
}

pub async fn handle(cmd: AuthCmd) -> Result<(), String> {
    match cmd {
        AuthCmd::Login {
            server,
            email,
            password,
        } => {
            let client = reqwest::Client::new();
            let url = format!("{server}/auth/login");
            let body = serde_json::json!({
                "email": email,
                "password": password,
            });

            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("login request failed: {e}"))?;

            let status = resp.status();
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("failed to read response: {e}"))?;

            if !status.is_success() {
                eprintln!("Login failed (HTTP {status}):");
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string())
                );
                return Err("login failed".into());
            }

            let access_token = json["access_token"]
                .as_str()
                .ok_or("response missing access_token")?
                .to_string();
            let refresh_token = json["refresh_token"].as_str().map(|s| s.to_string());

            save_credentials(&Credentials {
                access_token,
                refresh_token,
                server_url: server,
            })?;

            println!("Logged in successfully. Credentials saved.");
            Ok(())
        }

        AuthCmd::Status => {
            let (client, server) = authenticated_client()?;
            let resp = client
                .get(format!("{server}/auth/status"))
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        AuthCmd::Logout => {
            // Best-effort server-side logout.
            if let Ok((client, server)) = authenticated_client() {
                let _ = client.post(format!("{server}/auth/logout")).send().await;
            }
            remove_credentials()?;
            println!("Logged out. Credentials removed.");
            Ok(())
        }
    }
}
