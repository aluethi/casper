use crate::credentials::load_credentials;

/// Build a reqwest client with the stored auth token.
/// Returns (client, server_url).
pub fn authenticated_client() -> Result<(reqwest::Client, String), String> {
    let creds = load_credentials()?;

    let mut headers = reqwest::header::HeaderMap::new();
    let auth_value = format!("Bearer {}", creds.access_token);
    headers.insert(
        reqwest::header::AUTHORIZATION,
        auth_value.parse().map_err(|e| format!("invalid token: {e}"))?,
    );

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    Ok((client, creds.server_url))
}

/// Pretty-print a JSON response body to stdout.
pub async fn print_response(resp: reqwest::Response) -> Result<(), String> {
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;

    if status.is_success() {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
        );
    } else {
        eprintln!("Error (HTTP {status}):");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
        );
    }

    Ok(())
}
