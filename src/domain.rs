use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Chat,
    Image,
    ImageEdit,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Auto,
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolActivity {
    pub id: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatState {
    pub model: String,
    pub prompt_cache_key: String,
    pub reasoning_effort: ReasoningEffort,
    pub web_search: bool,
    pub x_search: bool,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAsset {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageState {
    pub model: String,
    pub prompt: String,
    pub count: u32,
    pub aspect_ratio: String,
    pub resolution: String,
    #[serde(default)]
    pub images: Vec<ImageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEditState {
    pub model: String,
    pub prompt: String,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local: Option<String>,
    pub count: u32,
    pub aspect_ratio: String,
    pub resolution: String,
    #[serde(default)]
    pub images: Vec<ImageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoState {
    pub model: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    pub duration: u32,
    pub aspect_ratio: String,
    pub resolution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub status: String,
    pub progress: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "payload_type", content = "data", rename_all = "snake_case")]
pub enum ItemPayload {
    Chat(ChatState),
    Image(ImageState),
    ImageEdit(ImageEditState),
    Video(VideoState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub title: String,
    pub kind: ItemKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: ItemPayload,
}

impl HistoryItem {
    pub fn new_chat(model: &str) -> Self {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        Self {
            id: id.clone(),
            title: "New chat".into(),
            kind: ItemKind::Chat,
            created_at: now,
            updated_at: now,
            payload: ItemPayload::Chat(ChatState {
                model: model.to_string(),
                prompt_cache_key: format!("g2a-creative-{}", Uuid::new_v4()),
                reasoning_effort: ReasoningEffort::Auto,
                web_search: false,
                x_search: false,
                messages: vec![],
            }),
        }
    }

    pub fn new_image(model: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: "Image".into(),
            kind: ItemKind::Image,
            created_at: now,
            updated_at: now,
            payload: ItemPayload::Image(ImageState {
                model: model.to_string(),
                prompt: String::new(),
                count: 1,
                aspect_ratio: "1:1".into(),
                resolution: "1k".into(),
                images: vec![],
            }),
        }
    }

    pub fn new_image_edit(model: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: "Image edit".into(),
            kind: ItemKind::ImageEdit,
            created_at: now,
            updated_at: now,
            payload: ItemPayload::ImageEdit(ImageEditState {
                model: model.to_string(),
                prompt: String::new(),
                source_url: String::new(),
                source_local: None,
                count: 1,
                aspect_ratio: "1:1".into(),
                resolution: "1k".into(),
                images: vec![],
            }),
        }
    }

    pub fn new_video(model: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: "Video".into(),
            kind: ItemKind::Video,
            created_at: now,
            updated_at: now,
            payload: ItemPayload::Video(VideoState {
                model: model.to_string(),
                prompt: String::new(),
                image_url: None,
                duration: 6,
                aspect_ratio: "16:9".into(),
                resolution: "720p".into(),
                request_id: None,
                status: "idle".into(),
                progress: 0.0,
                video_url: None,
                local_path: None,
                error: None,
            }),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

pub fn new_message_id() -> String {
    Uuid::new_v4().to_string()
}
