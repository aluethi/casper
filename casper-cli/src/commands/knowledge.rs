use clap::Subcommand;

use crate::http::{authenticated_client, print_response};

#[derive(Subcommand)]
pub enum KnowledgeCmd {
    /// Upload a file to the knowledge base.
    Upload {
        /// Path to the file to upload.
        file: String,
    },
    /// List knowledge base entries.
    List,
    /// Search the knowledge base.
    Search {
        /// Search query.
        query: String,
    },
}

pub async fn handle(cmd: KnowledgeCmd) -> Result<(), String> {
    let (client, server) = authenticated_client()?;
    let base = format!("{server}/api/v1/knowledge");

    match cmd {
        KnowledgeCmd::Upload { file } => {
            let file_name = std::path::Path::new(&file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("upload")
                .to_string();

            let file_bytes =
                std::fs::read(&file).map_err(|e| format!("failed to read {file}: {e}"))?;

            let part = reqwest::multipart::Part::bytes(file_bytes)
                .file_name(file_name)
                .mime_str("application/octet-stream")
                .map_err(|e| format!("failed to set MIME type: {e}"))?;

            let form = reqwest::multipart::Form::new().part("file", part);

            let resp = client
                .post(&base)
                .multipart(form)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        KnowledgeCmd::List => {
            let resp = client
                .get(&base)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }

        KnowledgeCmd::Search { query } => {
            let body = serde_json::json!({ "query": query });
            let resp = client
                .post(format!("{base}/search"))
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;
            print_response(resp).await
        }
    }
}
