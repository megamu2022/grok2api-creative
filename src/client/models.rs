use super::{ClientResult, GrokClient};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub owned_by: String,
    #[serde(default)]
    pub capability: String,
}

pub async fn list_models(client: &GrokClient) -> ClientResult<Vec<ModelInfo>> {
    let payload = client.get_json("models").await?;
    let mut out = Vec::new();
    if let Some(arr) = payload.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            let id = item
                .get("id")
                .or_else(|| item.get("public_id"))
                .or_else(|| item.get("publicId"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let capability = item
                .get("capability")
                .and_then(|x| x.as_str())
                .unwrap_or_else(|| infer_capability(&id))
                .to_string();
            let owned_by = item
                .get("owned_by")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            out.push(ModelInfo {
                id,
                owned_by,
                capability,
            });
        }
    }
    Ok(out)
}

fn infer_capability(id: &str) -> &'static str {
    let lower = id.to_lowercase();
    if lower.contains("imagine-video") || lower.contains("video") {
        "video"
    } else if lower.contains("image-edit") || lower.contains("imagine-image-edit") {
        "image"
    } else if lower.contains("imagine-image") || lower.contains("image") {
        "image"
    } else {
        "responses"
    }
}
