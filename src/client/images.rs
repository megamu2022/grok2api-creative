use super::{ClientError, ClientResult, GrokClient};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
    pub url: String,
    #[serde(default)]
    pub revised_prompt: Option<String>,
}

pub async fn generate_image(
    client: &GrokClient,
    model: &str,
    prompt: &str,
    count: u32,
    aspect_ratio: &str,
    resolution: &str,
) -> ClientResult<Vec<ImageResult>> {
    let body = json!({
        "model": model,
        "prompt": prompt,
        "n": count.max(1),
        "aspect_ratio": aspect_ratio,
        "resolution": resolution,
        "response_format": "url",
        "stream": false,
    });
    let payload = client.post_json("images/generations", &body).await?;
    let images = read_images(&payload);
    if images.is_empty() {
        return Err(ClientError::InvalidResponse(
            "image response empty".into(),
        ));
    }
    Ok(images)
}

pub async fn edit_image(
    client: &GrokClient,
    model: &str,
    prompt: &str,
    image_url: &str,
    count: u32,
    aspect_ratio: &str,
    resolution: &str,
) -> ClientResult<Vec<ImageResult>> {
    let body = json!({
        "model": model,
        "prompt": prompt,
        "image": { "url": image_url },
        "n": count.max(1),
        "aspect_ratio": aspect_ratio,
        "resolution": resolution,
        "response_format": "url",
        "stream": false,
    });
    let payload = client.post_json("images/edits", &body).await?;
    let images = read_images(&payload);
    if images.is_empty() {
        return Err(ClientError::InvalidResponse(
            "image edit response empty".into(),
        ));
    }
    Ok(images)
}

fn read_images(payload: &serde_json::Value) -> Vec<ImageResult> {
    let Some(arr) = payload.get("data").and_then(|d| d.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|item| {
            let url = item
                .get("url")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    item.get("b64_json")
                        .and_then(|x| x.as_str())
                        .map(|b| format!("data:image/png;base64,{b}"))
                })?;
            if url.is_empty() {
                return None;
            }
            Some(ImageResult {
                url,
                revised_prompt: item
                    .get("revised_prompt")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}
