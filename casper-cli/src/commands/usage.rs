use crate::http::{authenticated_client, print_response};

/// Show usage summary.
pub async fn handle_summary() -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let resp = client
        .get(format!("{server}/api/v1/usage"))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    print_response(resp).await
}
