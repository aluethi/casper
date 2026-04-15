use crate::http::{authenticated_client, print_response};

/// Query audit log.
pub async fn handle_query() -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let resp = client
        .get(format!("{server}/api/v1/audit"))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    print_response(resp).await
}
