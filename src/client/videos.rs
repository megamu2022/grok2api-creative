use super::{ClientError, ClientResult, GrokClient};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStatus {
    pub status: String,
    #[serde(default)]
    pub model: Option<String>,
    pub progress: f64,
    #[serde(default)]
    pub video_url: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
}

pub async fn create_video(
    client: &GrokClient,
    model: &str,
    prompt: &str,
    image_url: Option<&str>,
    duration: u32,
    aspect_ratio: &str,
    resolution: &str,
) -> ClientResult<String> {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "duration": duration,
        "aspect_ratio": aspect_ratio,
        "resolution": resolution,
    });
    if let Some(url) = image_url {
        if !url.trim().is_empty() {
            body["image"] = json!({ "url": url });
        }
    }
    let payload = client.post_json("videos/generations", &body).await?;
    let id = payload
        .get("request_id")
        .or_else(|| payload.get("id"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(ClientError::InvalidResponse(
            "video response missing request_id".into(),
        ));
    }
    Ok(id)
}

pub async fn get_video(client: &GrokClient, request_id: &str) -> ClientResult<VideoStatus> {
    let payload = client
        .get_json(&format!("videos/{}", urlencoding_path(request_id)))
        .await?;
    let status = payload
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("pending")
        .to_string();
    let progress = payload
        .get("progress")
        .and_then(|x| x.as_f64())
        .unwrap_or(if status == "done" { 100.0 } else { 0.0 })
        .clamp(0.0, 100.0);
    let video_url = payload
        .pointer("/video/url")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let duration = payload
        .pointer("/video/duration")
        .and_then(|x| x.as_f64());
    let error = payload
        .pointer("/error/message")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    Ok(VideoStatus {
        status,
        model: payload
            .get("model")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        progress,
        video_url,
        duration,
        error,
        request_id: Some(request_id.to_string()),
    })
}

fn urlencoding_path(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
