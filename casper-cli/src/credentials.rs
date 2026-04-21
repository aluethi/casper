use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Stored credentials for the CLI.
#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub server_url: String,
}

/// Return the path to ~/.casper/credentials.json.
pub fn credentials_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("could not determine home directory")?;
    Ok(home.join(".casper").join("credentials.json"))
}

/// Load credentials from disk.
pub fn load_credentials() -> Result<Credentials, String> {
    let path = credentials_path()?;
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "failed to read {}: {e} (have you run `casper auth login`?)",
            path.display()
        )
    })?;
    serde_json::from_str(&contents).map_err(|e| format!("failed to parse credentials: {e}"))
}

/// Save credentials to disk, creating the directory if needed.
pub fn save_credentials(creds: &Credentials) -> Result<(), String> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(creds)
        .map_err(|e| format!("failed to serialize credentials: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Remove the credentials file (logout).
pub fn remove_credentials() -> Result<(), String> {
    let path = credentials_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
    }
    Ok(())
}
