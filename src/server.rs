use crate::client::{
    create_video, edit_image, generate_image, get_video, list_models, stream_chat, ChatRequest,
    GrokClient,
};
use base64::Engine;
use crate::config::Config;
use crate::domain::{new_message_id, ChatMessage, HistoryItem, ItemKind, ItemPayload, Role};
use crate::history::HistoryStore;
use crate::media_cache;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub history: Arc<RwLock<HistoryStore>>,
}

pub async fn start_server(state: AppState) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let ui_dir = ui_directory();
    let media_dir = Config::media_dir()?;

    let app = Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/models", get(api_models))
        .route("/api/history", get(list_history).post(create_history))
        .route("/api/history/{id}", get(get_history).put(update_history).delete(delete_history))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/chat/{id}/edit-message", post(edit_message))
        .route("/api/chat/{id}/delete-message", post(delete_message))
        .route("/api/chat/{id}/retry", post(retry_message))
        .route("/api/images/generate", post(api_generate_image))
        .route("/api/images/edit", post(api_edit_image))
        .route("/api/videos/create", post(api_create_video))
        .route("/api/videos/{request_id}", get(api_get_video))
        .route("/api/media/import-url", post(import_media_url))
        .route("/api/media/upload", post(upload_media))
        .nest_service("/local/media", ServeDir::new(media_dir))
        .fallback_service(ServeDir::new(ui_dir).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("server error: {e}");
        }
    });
    Ok((addr, handle))
}

fn ui_directory() -> PathBuf {
    if let Ok(p) = std::env::var("GROK2API_CREATIVE_UI") {
        return PathBuf::from(p);
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest.join("assets"),
        manifest.join("ui"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("assets")))
            .unwrap_or_default(),
    ];
    for c in candidates {
        if c.join("index.html").exists() {
            return c;
        }
    }
    manifest.join("assets")
}

async fn client_from_state(state: &AppState) -> Result<GrokClient, ApiError> {
    let cfg = state.config.read().await;
    GrokClient::from_config(&cfg).map_err(ApiError::from)
}

#[derive(Serialize)]
struct ConfigView {
    base_url: String,
    api_key_set: bool,
    api_key_masked: String,
    default_chat_model: String,
    default_image_model: String,
    default_video_model: String,
    default_image_edit_model: String,
    ready: bool,
}

async fn get_config(State(state): State<AppState>) -> Json<ConfigView> {
    let cfg = state.config.read().await;
    Json(ConfigView {
        base_url: cfg.base_url.clone(),
        api_key_set: !cfg.api_key.trim().is_empty(),
        api_key_masked: cfg.masked_key(),
        default_chat_model: cfg.default_chat_model.clone(),
        default_image_model: cfg.default_image_model.clone(),
        default_video_model: cfg.default_video_model.clone(),
        default_image_edit_model: cfg.default_image_edit_model.clone(),
        ready: cfg.is_ready(),
    })
}

#[derive(Deserialize)]
struct PutConfig {
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    default_chat_model: Option<String>,
    #[serde(default)]
    default_image_model: Option<String>,
    #[serde(default)]
    default_video_model: Option<String>,
    #[serde(default)]
    default_image_edit_model: Option<String>,
}

async fn put_config(
    State(state): State<AppState>,
    Json(body): Json<PutConfig>,
) -> Result<Json<ConfigView>, ApiError> {
    let mut cfg = state.config.write().await;
    cfg.base_url = crate::config::normalize_base_url(&body.base_url);
    if let Some(key) = body.api_key {
        if !key.trim().is_empty() {
            cfg.api_key = key.trim().to_string();
        }
    }
    if let Some(v) = body.default_chat_model {
        cfg.default_chat_model = v;
    }
    if let Some(v) = body.default_image_model {
        cfg.default_image_model = v;
    }
    if let Some(v) = body.default_video_model {
        cfg.default_video_model = v;
    }
    if let Some(v) = body.default_image_edit_model {
        cfg.default_image_edit_model = v;
    }
    cfg.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(ConfigView {
        base_url: cfg.base_url.clone(),
        api_key_set: !cfg.api_key.trim().is_empty(),
        api_key_masked: cfg.masked_key(),
        default_chat_model: cfg.default_chat_model.clone(),
        default_image_model: cfg.default_image_model.clone(),
        default_video_model: cfg.default_video_model.clone(),
        default_image_edit_model: cfg.default_image_edit_model.clone(),
        ready: cfg.is_ready(),
    }))
}

async fn api_models(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let client = client_from_state(&state).await?;
    let models = list_models(&client).await.map_err(ApiError::from)?;
    Ok(Json(json!({ "data": models })))
}

async fn list_history(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.history.read().await;
    Json(json!({ "items": store.items }))
}

#[derive(Deserialize)]
struct CreateHistory {
    kind: String,
    #[serde(default)]
    model: Option<String>,
}

async fn create_history(
    State(state): State<AppState>,
    Json(body): Json<CreateHistory>,
) -> Result<Json<HistoryItem>, ApiError> {
    let cfg = state.config.read().await.clone();
    let item = match body.kind.as_str() {
        "chat" => HistoryItem::new_chat(
            body.model
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&cfg.default_chat_model),
        ),
        "image" => HistoryItem::new_image(
            body.model
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&cfg.default_image_model),
        ),
        "image_edit" => HistoryItem::new_image_edit(
            body.model
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(if cfg.default_image_edit_model.is_empty() {
                    "grok-imagine-image-edit"
                } else {
                    &cfg.default_image_edit_model
                }),
        ),
        "video" => HistoryItem::new_video(
            body.model
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&cfg.default_video_model),
        ),
        other => return Err(ApiError::bad(format!("unknown kind {other}"))),
    };
    let mut store = state.history.write().await;
    store.upsert(item.clone());
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(item))
}

async fn get_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<HistoryItem>, ApiError> {
    let store = state.history.read().await;
    store
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("history item"))
}

async fn update_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut item): Json<HistoryItem>,
) -> Result<Json<HistoryItem>, ApiError> {
    if item.id != id {
        item.id = id;
    }
    item.touch();
    let mut store = state.history.write().await;
    store.upsert(item.clone());
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(item))
}

async fn delete_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut store = state.history.write().await;
    if !store.remove(&id) {
        return Err(ApiError::not_found("history item"));
    }
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ChatStreamBody {
    history_id: String,
    content: String,
}

async fn chat_stream(
    State(state): State<AppState>,
    Json(body): Json<ChatStreamBody>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let client = client_from_state(&state).await?;
    let (req, assistant_id, user_id) = {
        let mut store = state.history.write().await;
        let item = store
            .get_mut(&body.history_id)
            .ok_or_else(|| ApiError::not_found("history"))?;
        let chat = match &mut item.payload {
            ItemPayload::Chat(chat) => chat,
            _ => return Err(ApiError::bad("not a chat item")),
        };
        let user_id = new_message_id();
        chat.messages.push(ChatMessage {
            id: user_id.clone(),
            role: Role::User,
            content: body.content.clone(),
            reasoning: None,
            tools: vec![],
        });
        let assistant_id = new_message_id();
        chat.messages.push(ChatMessage {
            id: assistant_id.clone(),
            role: Role::Assistant,
            content: String::new(),
            reasoning: Some(String::new()),
            tools: vec![],
        });
        if item.title == "New chat" || item.title.is_empty() {
            item.title = truncate_title(&body.content);
        }
        item.touch();
        let chat = match &item.payload {
            ItemPayload::Chat(chat) => chat,
            _ => unreachable!(),
        };
        let mut send_msgs = chat.messages.clone();
        if matches!(
            send_msgs.last().map(|m| &m.role),
            Some(Role::Assistant)
        ) {
            if send_msgs
                .last()
                .map(|m| m.content.is_empty())
                .unwrap_or(false)
            {
                send_msgs.pop();
            }
        }
        let req = ChatRequest {
            model: chat.model.clone(),
            messages: send_msgs,
            prompt_cache_key: Some(chat.prompt_cache_key.clone()),
            reasoning_effort: chat.reasoning_effort,
            web_search: chat.web_search,
            x_search: chat.x_search,
        };
        store.save().map_err(|e| ApiError::internal(e.to_string()))?;
        (req, assistant_id, user_id)
    };

    let (tx, rx) = mpsc::channel::<crate::client::ChatStreamEvent>(64);
    let state2 = state.clone();
    let history_id = body.history_id.clone();
    let assistant_id2 = assistant_id.clone();
    let user_id2 = user_id.clone();

    tokio::spawn(async move {
        let result = stream_chat(&client, req, tx.clone()).await;
        match result {
            Ok(snap) => {
                let messages = {
                    let mut store = state2.history.write().await;
                    if let Some(item) = store.get_mut(&history_id) {
                        if let ItemPayload::Chat(chat) = &mut item.payload {
                            if let Some(msg) =
                                chat.messages.iter_mut().find(|m| m.id == assistant_id2)
                            {
                                msg.content = snap.text.clone();
                                msg.reasoning = Some(snap.reasoning.clone());
                                msg.tools = snap.tools.clone();
                            }
                            let msgs = chat.messages.clone();
                            item.touch();
                            let _ = store.save();
                            Some(msgs)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                let _ = tx
                    .send(crate::client::ChatStreamEvent::Done {
                        snapshot: snap,
                        messages,
                        assistant_id: Some(assistant_id2),
                        user_id: Some(user_id2),
                    })
                    .await;
            }
            Err(e) => {
                {
                    let mut store = state2.history.write().await;
                    if let Some(item) = store.get_mut(&history_id) {
                        if let ItemPayload::Chat(chat) = &mut item.payload {
                            if let Some(msg) =
                                chat.messages.iter_mut().find(|m| m.id == assistant_id2)
                            {
                                if msg.content.is_empty() {
                                    msg.content = format!("Error: {e}");
                                }
                            }
                            item.touch();
                        }
                    }
                    let _ = store.save();
                }
                let _ = tx
                    .send(crate::client::ChatStreamEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        }
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(ev) => {
                let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                Some((Ok::<Event, Infallible>(Event::default().data(data)), rx))
            }
            None => None,
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
struct EditMessageBody {
    message_id: String,
    content: String,
    #[serde(default)]
    resend: bool,
}

async fn edit_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<EditMessageBody>,
) -> Result<Json<HistoryItem>, ApiError> {
    let mut store = state.history.write().await;
    let item = store
        .get_mut(&id)
        .ok_or_else(|| ApiError::not_found("history"))?;
    let ItemPayload::Chat(chat) = &mut item.payload else {
        return Err(ApiError::bad("not a chat"));
    };
    let pos = chat
        .messages
        .iter()
        .position(|m| m.id == body.message_id)
        .ok_or_else(|| ApiError::not_found("message"))?;
    chat.messages[pos].content = body.content.clone();
    // Truncate after this message
    chat.messages.truncate(pos + 1);
    // If assistant was edited, keep as is; if user and resend, will stream separately
    if matches!(chat.messages[pos].role, Role::User) && body.resend {
        // leave only up to user; caller should call stream with empty? 
        // We mark by returning item; frontend calls stream with special flag
    }
    item.touch();
    let out = item.clone();
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(out))
}

#[derive(Deserialize)]
struct DeleteMessageBody {
    message_id: String,
}

async fn delete_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DeleteMessageBody>,
) -> Result<Json<HistoryItem>, ApiError> {
    let mut store = state.history.write().await;
    let item = store
        .get_mut(&id)
        .ok_or_else(|| ApiError::not_found("history"))?;
    let ItemPayload::Chat(chat) = &mut item.payload else {
        return Err(ApiError::bad("not a chat"));
    };
    let pos = chat
        .messages
        .iter()
        .position(|m| m.id == body.message_id)
        .ok_or_else(|| ApiError::not_found("message"))?;
    // If deleting user, also remove following assistant
    if matches!(chat.messages[pos].role, Role::User) {
        chat.messages.remove(pos);
        if pos < chat.messages.len() && matches!(chat.messages[pos].role, Role::Assistant) {
            chat.messages.remove(pos);
        }
    } else {
        chat.messages.remove(pos);
    }
    item.touch();
    let out = item.clone();
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(out))
}

#[derive(Deserialize)]
struct RetryBody {
    /// Retry from this user message id (re-generate assistant after it)
    message_id: String,
}

async fn retry_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RetryBody>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let client = client_from_state(&state).await?;
    let (req, assistant_id) = {
        let mut store = state.history.write().await;
        let item = store
            .get_mut(&id)
            .ok_or_else(|| ApiError::not_found("history"))?;
        {
            let chat = match &mut item.payload {
                ItemPayload::Chat(chat) => chat,
                _ => return Err(ApiError::bad("not a chat")),
            };
            let pos = chat
                .messages
                .iter()
                .position(|m| m.id == body.message_id)
                .ok_or_else(|| ApiError::not_found("message"))?;
            if !matches!(chat.messages[pos].role, Role::User) {
                return Err(ApiError::bad("retry must target a user message"));
            }
            chat.messages.truncate(pos + 1);
            chat.messages.push(ChatMessage {
                id: new_message_id(),
                role: Role::Assistant,
                content: String::new(),
                reasoning: Some(String::new()),
                tools: vec![],
            });
        }
        item.touch();
        let chat = match &item.payload {
            ItemPayload::Chat(chat) => chat,
            _ => unreachable!(),
        };
        let assistant_id = chat
            .messages
            .last()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let mut send_msgs = chat.messages.clone();
        send_msgs.pop();
        let req = ChatRequest {
            model: chat.model.clone(),
            messages: send_msgs,
            prompt_cache_key: Some(chat.prompt_cache_key.clone()),
            reasoning_effort: chat.reasoning_effort,
            web_search: chat.web_search,
            x_search: chat.x_search,
        };
        store.save().map_err(|e| ApiError::internal(e.to_string()))?;
        (req, assistant_id)
    };

    let (tx, rx) = mpsc::channel::<crate::client::ChatStreamEvent>(64);
    let state2 = state.clone();
    let history_id = id.clone();
    let assistant_id2 = assistant_id.clone();
    let user_id2 = body.message_id.clone();
    tokio::spawn(async move {
        match stream_chat(&client, req, tx.clone()).await {
            Ok(snap) => {
                let messages = {
                    let mut store = state2.history.write().await;
                    if let Some(item) = store.get_mut(&history_id) {
                        if let ItemPayload::Chat(chat) = &mut item.payload {
                            if let Some(msg) =
                                chat.messages.iter_mut().find(|m| m.id == assistant_id2)
                            {
                                msg.content = snap.text.clone();
                                msg.reasoning = Some(snap.reasoning.clone());
                                msg.tools = snap.tools.clone();
                            }
                            let msgs = chat.messages.clone();
                            item.touch();
                            let _ = store.save();
                            Some(msgs)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                let _ = tx
                    .send(crate::client::ChatStreamEvent::Done {
                        snapshot: snap,
                        messages,
                        assistant_id: Some(assistant_id2),
                        user_id: Some(user_id2),
                    })
                    .await;
            }
            Err(e) => {
                {
                    let mut store = state2.history.write().await;
                    if let Some(item) = store.get_mut(&history_id) {
                        if let ItemPayload::Chat(chat) = &mut item.payload {
                            if let Some(msg) =
                                chat.messages.iter_mut().find(|m| m.id == assistant_id2)
                            {
                                if msg.content.is_empty() {
                                    msg.content = format!("Error: {e}");
                                }
                            }
                            item.touch();
                        }
                    }
                    let _ = store.save();
                }
                let _ = tx
                    .send(crate::client::ChatStreamEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        }
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(ev) => {
                let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                Some((Ok::<Event, Infallible>(Event::default().data(data)), rx))
            }
            None => None,
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
struct GenImageBody {
    history_id: String,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    #[serde(default = "default_one")]
    count: u32,
    #[serde(default = "default_aspect")]
    aspect_ratio: String,
    #[serde(default = "default_res")]
    resolution: String,
}

fn default_one() -> u32 {
    1
}
fn default_aspect() -> String {
    "1:1".into()
}
fn default_res() -> String {
    "1k".into()
}

async fn api_generate_image(
    State(state): State<AppState>,
    Json(body): Json<GenImageBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = client_from_state(&state).await?;
    let model = {
        let store = state.history.read().await;
        let item = store
            .get(&body.history_id)
            .ok_or_else(|| ApiError::not_found("history"))?;
        match &item.payload {
            ItemPayload::Image(s) => body.model.clone().unwrap_or_else(|| s.model.clone()),
            _ => body.model.clone().unwrap_or_default(),
        }
    };
    if model.is_empty() {
        return Err(ApiError::bad("model required"));
    }
    let images = generate_image(
        &client,
        &model,
        &body.prompt,
        body.count,
        &body.aspect_ratio,
        &body.resolution,
    )
    .await
    .map_err(ApiError::from)?;

    let mut assets = Vec::new();
    for img in images {
        let local = if img.url.starts_with("data:") {
            media_cache::save_data_url(&img.url).ok()
        } else {
            media_cache::download_to_cache(&img.url, "png").await.ok()
        };
        assets.push(crate::domain::ImageAsset {
            url: img.url,
            local_path: local.map(|p| p.display().to_string()),
            revised_prompt: img.revised_prompt,
        });
    }

    let mut store = state.history.write().await;
    let item = store
        .get_mut(&body.history_id)
        .ok_or_else(|| ApiError::not_found("history"))?;
    if let ItemPayload::Image(s) = &mut item.payload {
        s.model = model;
        s.prompt = body.prompt.clone();
        s.count = body.count;
        s.aspect_ratio = body.aspect_ratio.clone();
        s.resolution = body.resolution.clone();
        s.images = assets.clone();
        item.title = truncate_title(&body.prompt);
        item.kind = ItemKind::Image;
        item.touch();
    }
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "images": assets })))
}

#[derive(Deserialize)]
struct EditImageBody {
    history_id: String,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    image_url: String,
    #[serde(default = "default_one")]
    count: u32,
    #[serde(default = "default_aspect")]
    aspect_ratio: String,
    #[serde(default = "default_res")]
    resolution: String,
}

async fn api_edit_image(
    State(state): State<AppState>,
    Json(body): Json<EditImageBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = client_from_state(&state).await?;
    let model = body
        .model
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "grok-imagine-image-edit".into());
    let images = edit_image(
        &client,
        &model,
        &body.prompt,
        &body.image_url,
        body.count,
        &body.aspect_ratio,
        &body.resolution,
    )
    .await
    .map_err(ApiError::from)?;

    let mut assets = Vec::new();
    for img in images {
        let local = if img.url.starts_with("data:") {
            media_cache::save_data_url(&img.url).ok()
        } else {
            media_cache::download_to_cache(&img.url, "png").await.ok()
        };
        assets.push(crate::domain::ImageAsset {
            url: img.url,
            local_path: local.map(|p| p.display().to_string()),
            revised_prompt: img.revised_prompt,
        });
    }

    let mut store = state.history.write().await;
    let item = store
        .get_mut(&body.history_id)
        .ok_or_else(|| ApiError::not_found("history"))?;
    if let ItemPayload::ImageEdit(s) = &mut item.payload {
        s.model = model;
        s.prompt = body.prompt.clone();
        s.source_url = body.image_url.clone();
        s.count = body.count;
        s.aspect_ratio = body.aspect_ratio.clone();
        s.resolution = body.resolution.clone();
        s.images = assets.clone();
        item.title = truncate_title(&body.prompt);
        item.kind = ItemKind::ImageEdit;
        item.touch();
    }
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "images": assets })))
}

#[derive(Deserialize)]
struct CreateVideoBody {
    history_id: String,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default = "default_dur")]
    duration: u32,
    #[serde(default = "default_video_aspect")]
    aspect_ratio: String,
    #[serde(default = "default_video_res")]
    resolution: String,
}

fn default_dur() -> u32 {
    6
}
fn default_video_aspect() -> String {
    "16:9".into()
}
fn default_video_res() -> String {
    "720p".into()
}

async fn api_create_video(
    State(state): State<AppState>,
    Json(body): Json<CreateVideoBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = client_from_state(&state).await?;
    let model = body
        .model
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "grok-imagine-video".into());
    let request_id = create_video(
        &client,
        &model,
        &body.prompt,
        body.image_url.as_deref(),
        body.duration,
        &body.aspect_ratio,
        &body.resolution,
    )
    .await
    .map_err(ApiError::from)?;

    let mut store = state.history.write().await;
    let item = store
        .get_mut(&body.history_id)
        .ok_or_else(|| ApiError::not_found("history"))?;
    if let ItemPayload::Video(s) = &mut item.payload {
        s.model = model;
        s.prompt = body.prompt.clone();
        s.image_url = body.image_url.clone();
        s.duration = body.duration;
        s.aspect_ratio = body.aspect_ratio.clone();
        s.resolution = body.resolution.clone();
        s.request_id = Some(request_id.clone());
        s.status = "pending".into();
        s.progress = 0.0;
        s.error = None;
        item.title = truncate_title(&body.prompt);
        item.kind = ItemKind::Video;
        item.touch();
    }
    store.save().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "request_id": request_id })))
}

async fn api_get_video(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = client_from_state(&state).await?;
    let status = get_video(&client, &request_id).await.map_err(ApiError::from)?;
    let mut local_path: Option<String> = None;
    let mut local_url: Option<String> = None;

    if status.status == "done" {
        if let Some(url) = status.video_url.as_deref() {
            if let Ok(path) = media_cache::download_to_cache(url, "mp4").await {
                local_path = Some(path.display().to_string());
                local_url = Some(format!(
                    "/local/media/{}",
                    media_cache::filename(&path)
                ));
            }
        }
    }

    {
        let mut store = state.history.write().await;
        for item in store.items.iter_mut() {
            if let ItemPayload::Video(v) = &mut item.payload {
                if v.request_id.as_deref() == Some(&request_id) {
                    v.status = status.status.clone();
                    v.progress = status.progress;
                    v.video_url = status.video_url.clone();
                    if let Some(lp) = &local_path {
                        v.local_path = Some(lp.clone());
                    }
                    v.error = status.error.clone();
                    item.touch();
                }
            }
        }
        let _ = store.save();
    }

    Ok(Json(json!({
        "status": status.status,
        "progress": status.progress,
        "video_url": status.video_url,
        "local_path": local_path,
        "local_url": local_url,
        "error": status.error,
    })))
}

#[derive(Deserialize)]
struct ImportUrlBody {
    url: String,
}

async fn import_media_url(
    Json(body): Json<ImportUrlBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.url.starts_with("data:") {
        let path = media_cache::save_data_url(&body.url)
            .map_err(|e| ApiError::bad(e.to_string()))?;
        return Ok(Json(json!({
            "local_path": path.display().to_string(),
            "local_url": format!("/local/media/{}", media_cache::filename(&path)),
            "data_url": body.url,
        })));
    }
    let path = media_cache::download_to_cache(&body.url, "bin")
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(Json(json!({
        "local_path": path.display().to_string(),
        "local_url": format!("/local/media/{}", media_cache::filename(&path)),
        "source_url": body.url,
    })))
}

#[derive(Deserialize)]
struct UploadBody {
    /// base64 payload without data: prefix, or full data url
    data: String,
    #[serde(default)]
    filename: Option<String>,
}

async fn upload_media(Json(body): Json<UploadBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let data = body.data.trim();
    let path = if data.starts_with("data:") {
        media_cache::save_data_url(data).map_err(|e| ApiError::bad(e.to_string()))?
    } else {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| ApiError::bad(e.to_string()))?;
        let ext = body
            .filename
            .as_deref()
            .and_then(|f| std::path::Path::new(f).extension())
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "png".into());
        media_cache::save_bytes(&bytes, &ext).map_err(|e| ApiError::bad(e.to_string()))?
    };
    // Build data url for gateway if needed
    let bytes = std::fs::read(&path).map_err(|e| ApiError::internal(e.to_string()))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("png") {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        _ => "image/png",
    };
    Ok(Json(json!({
        "local_path": path.display().to_string(),
        "local_url": format!("/local/media/{}", media_cache::filename(&path)),
        "data_url": format!("data:{mime};base64,{b64}"),
    })))
}

fn truncate_title(s: &str) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= 40 {
        if t.is_empty() {
            "Untitled".into()
        } else {
            t
        }
    } else {
        format!("{}…", t.chars().take(40).collect::<String>())
    }
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl From<crate::client::ClientError> for ApiError {
    fn from(e: crate::client::ClientError) -> Self {
        match e {
            crate::client::ClientError::NotConfigured => Self {
                status: StatusCode::BAD_REQUEST,
                message: e.to_string(),
            },
            crate::client::ClientError::Api { status, message, .. } => Self {
                status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                message,
            },
            other => Self {
                status: StatusCode::BAD_GATEWAY,
                message: other.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "message": self.message } });
        (self.status, Json(body)).into_response()
    }
}
