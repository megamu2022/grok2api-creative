mod error;
mod images;
mod models;
mod responses;
mod videos;

pub use error::{ClientError, ClientResult};
pub use images::{edit_image, generate_image};
pub use models::list_models;
pub use responses::{stream_chat, ChatRequest, ChatStreamEvent};
pub use videos::{create_video, get_video};

use crate::config::Config;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;

#[derive(Clone)]
pub struct GrokClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl GrokClient {
    pub fn from_config(cfg: &Config) -> ClientResult<Self> {
        if !cfg.is_ready() {
            return Err(ClientError::NotConfigured);
        }
        Ok(Self::new(&cfg.base_url, &cfg.api_key))
    }

    pub fn new(base_url: &str, api_key: &str) -> Self {
        let base = crate::config::normalize_base_url(base_url);
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("http client"),
            base_url: base,
            api_key: api_key.trim().to_string(),
        }
    }

    fn auth_headers(&self, json: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", self.api_key);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("Bearer")),
        );
        if json {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        headers
    }

    pub fn url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }

    pub async fn get_json(&self, path: &str) -> ClientResult<serde_json::Value> {
        let resp = self
            .http
            .get(self.url(path))
            .headers(self.auth_headers(false))
            .send()
            .await?;
        Self::parse_json_response(resp).await
    }

    pub async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> ClientResult<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(path))
            .headers(self.auth_headers(true))
            .json(body)
            .send()
            .await?;
        Self::parse_json_response(resp).await
    }

    pub async fn post_stream(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> ClientResult<reqwest::Response> {
        let resp = self
            .http
            .post(self.url(path))
            .headers(self.auth_headers(true))
            .header("Accept", "text/event-stream")
            .json(body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let (message, code) = extract_error(&text);
            return Err(ClientError::Api {
                status,
                message: message.unwrap_or(text),
                code,
            });
        }
        Ok(resp)
    }

    async fn parse_json_response(resp: reqwest::Response) -> ClientResult<serde_json::Value> {
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        if !(200..300).contains(&status) {
            let (message, code) = extract_error(&text);
            return Err(ClientError::Api {
                status,
                message: message.unwrap_or_else(|| text.clone()),
                code,
            });
        }
        if text.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        Ok(serde_json::from_str(&text)?)
    }
}

fn extract_error(text: &str) -> (Option<String>, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return (None, None);
    };
    let err = v.get("error").unwrap_or(&v);
    let message = err
        .get("message")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let code = err
        .get("code")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    (message, code)
}
