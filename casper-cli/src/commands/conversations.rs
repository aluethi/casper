use crate::http::{authenticated_client, print_response};

/// List conversations.
pub async fn handle_list() -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let resp = client
        .get(format!("{server}/api/v1/conversations"))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    print_response(resp).await
}
