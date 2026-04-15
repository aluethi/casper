use crate::http::{authenticated_client, print_response};

/// Run an agent with a message.
pub async fn handle(agent: &str, message: &str) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let url = format!("{server}/api/v1/agents/{agent}/run");

    let body = serde_json::json!({
        "message": message,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    print_response(resp).await
}
