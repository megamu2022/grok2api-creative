use super::{ClientError, ClientResult, GrokClient};
use crate::domain::{ChatMessage, ReasoningEffort, Role, ToolActivity};
use futures::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub prompt_cache_key: Option<String>,
    pub reasoning_effort: ReasoningEffort,
    pub web_search: bool,
    pub x_search: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StreamSnapshot {
    pub text: String,
    pub reasoning: String,
    pub tools: Vec<ToolActivity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Delta { snapshot: StreamSnapshot },
    /// Emitted by the server after history is persisted (may include full message list).
    Done {
        snapshot: StreamSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        messages: Option<Vec<ChatMessage>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        assistant_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
    },
    Error { message: String },
}

pub async fn stream_chat(
    client: &GrokClient,
    req: ChatRequest,
    tx: mpsc::Sender<ChatStreamEvent>,
) -> ClientResult<StreamSnapshot> {
    let body = build_body(&req);
    let resp = client.post_stream("responses", &body).await?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("text/event-stream") {
        let text = resp.text().await?;
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let snap = snapshot_from_envelope(&payload);
        // Final Done is emitted by the server after history is saved.
        let _ = tx
            .send(ChatStreamEvent::Delta {
                snapshot: snap.clone(),
            })
            .await;
        return Ok(snap);
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tools: HashMap<String, ToolActivity> = HashMap::new();

    let emit = |tx: &mpsc::Sender<ChatStreamEvent>,
                text: &str,
                reasoning: &str,
                tools: &HashMap<String, ToolActivity>| {
        let snapshot = StreamSnapshot {
            text: text.to_string(),
            reasoning: reasoning.to_string(),
            tools: tools.values().cloned().collect(),
        };
        let _ = tx.try_send(ChatStreamEvent::Delta { snapshot });
    };

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        buffer = buffer.replace("\r\n", "\n");
        while let Some(idx) = buffer.find("\n\n") {
            let block = buffer[..idx].to_string();
            buffer = buffer[idx + 2..].to_string();
            if let Err(e) = consume_sse_block(
                &block,
                &mut text,
                &mut reasoning,
                &mut tools,
            ) {
                let _ = tx
                    .send(ChatStreamEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
                return Err(e);
            }
            emit(&tx, &text, &reasoning, &tools);
        }
    }
    if !buffer.trim().is_empty() {
        let _ = consume_sse_block(&buffer, &mut text, &mut reasoning, &mut tools);
    }

    let snapshot = StreamSnapshot {
        text,
        reasoning,
        tools: tools.into_values().collect(),
    };
    if snapshot.text.trim().is_empty()
        && snapshot.reasoning.trim().is_empty()
        && snapshot.tools.is_empty()
    {
        return Err(ClientError::InvalidResponse(
            "empty responses stream".into(),
        ));
    }
    // Final Done is emitted by the server after history is persisted.
    let _ = tx
        .send(ChatStreamEvent::Delta {
            snapshot: snapshot.clone(),
        })
        .await;
    Ok(snapshot)
}

fn build_body(req: &ChatRequest) -> Value {
    let input: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| !m.content.trim().is_empty() || matches!(m.role, Role::Assistant))
        .map(|m| {
            json!({
                "role": match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                "content": m.content,
            })
        })
        .collect();

    let mut body = json!({
        "model": req.model,
        "input": input,
        "stream": true,
        "store": false,
    });

    if let Some(key) = &req.prompt_cache_key {
        if !key.is_empty() {
            body["prompt_cache_key"] = json!(key);
        }
    }

    body["reasoning"] = match &req.reasoning_effort {
        ReasoningEffort::Auto => json!({ "summary": "auto" }),
        ReasoningEffort::None => json!({ "effort": "none" }),
        other => json!({ "effort": other.as_str(), "summary": "auto" }),
    };

    let mut tools = Vec::new();
    if req.web_search {
        tools.push(json!({ "type": "web_search" }));
    }
    if req.x_search {
        tools.push(json!({ "type": "x_search" }));
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }

    body
}

fn consume_sse_block(
    block: &str,
    text: &mut String,
    reasoning: &mut String,
    tools: &mut HashMap<String, ToolActivity>,
) -> ClientResult<()> {
    let mut data = String::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let payload: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let ty = payload.get("type").and_then(|x| x.as_str()).unwrap_or("");

    match ty {
        "response.output_text.delta" => {
            if let Some(d) = payload.get("delta").and_then(|x| x.as_str()) {
                text.push_str(d);
            }
        }
        "response.output_text.done" => {
            if let Some(t) = payload.get("text").and_then(|x| x.as_str()) {
                *text = t.to_string();
            }
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            if let Some(d) = payload.get("delta").and_then(|x| x.as_str()) {
                reasoning.push_str(d);
            }
        }
        "response.reasoning_summary_text.done" | "response.reasoning_text.done" => {
            if let Some(t) = payload.get("text").and_then(|x| x.as_str()) {
                *reasoning = t.to_string();
            }
        }
        "response.output_item.added" | "response.output_item.done" => {
            if let Some(item) = payload.get("item").and_then(|x| x.as_object()) {
                let item_type = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if item_type == "message" {
                    if ty.ends_with("done") {
                        if let Some(t) = read_content_text(item.get("content")) {
                            if !t.is_empty() {
                                *text = t;
                            }
                        }
                    }
                } else if item_type == "reasoning" {
                    if let Some(r) = read_reasoning_item(item) {
                        if !r.is_empty() {
                            *reasoning = r;
                        }
                    }
                } else if let Some(tool) = read_tool_item(
                    item,
                    if ty.ends_with("done") {
                        "completed"
                    } else {
                        "in_progress"
                    },
                ) {
                    tools.insert(tool.id.clone(), tool);
                }
            }
        }
        "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
            update_tool_detail(
                tools,
                &payload,
                payload.get("delta").and_then(|x| x.as_str()).unwrap_or(""),
                true,
            );
        }
        "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
            let detail = payload
                .get("arguments")
                .and_then(|x| x.as_str())
                .or_else(|| payload.get("input").and_then(|x| x.as_str()))
                .unwrap_or("");
            update_tool_detail(tools, &payload, detail, false);
        }
        "response.completed" | "response.incomplete" => {
            if let Some(resp) = payload.get("response") {
                let snap = snapshot_from_envelope(resp);
                if !snap.text.is_empty() {
                    *text = snap.text;
                }
                if !snap.reasoning.is_empty() {
                    *reasoning = snap.reasoning;
                }
                for t in snap.tools {
                    tools.insert(t.id.clone(), t);
                }
            }
            if ty == "response.incomplete" {
                return Err(ClientError::InvalidResponse(
                    "response incomplete".into(),
                ));
            }
        }
        "response.failed" | "error" => {
            let msg = payload
                .pointer("/error/message")
                .or_else(|| payload.pointer("/response/error/message"))
                .and_then(|x| x.as_str())
                .unwrap_or("stream failed")
                .to_string();
            return Err(ClientError::InvalidResponse(msg));
        }
        _ => {}
    }
    Ok(())
}

fn snapshot_from_envelope(payload: &Value) -> StreamSnapshot {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tools = Vec::new();
    if let Some(t) = payload.get("output_text").and_then(|x| x.as_str()) {
        text = t.trim().to_string();
    }
    if let Some(arr) = payload.get("output").and_then(|x| x.as_array()) {
        for item in arr {
            let Some(obj) = item.as_object() else { continue };
            let ty = obj.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match ty {
                "message" => {
                    if let Some(t) = read_content_text(obj.get("content")) {
                        if !t.is_empty() {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(&t);
                        }
                    }
                }
                "reasoning" => {
                    if let Some(r) = read_reasoning_item(obj) {
                        if !r.is_empty() {
                            if !reasoning.is_empty() {
                                reasoning.push('\n');
                            }
                            reasoning.push_str(&r);
                        }
                    }
                }
                _ => {
                    if let Some(tool) = read_tool_item(obj, "completed") {
                        tools.push(tool);
                    }
                }
            }
        }
    }
    StreamSnapshot {
        text,
        reasoning,
        tools,
    }
}

fn read_content_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    if let Some(s) = content.as_str() {
        return Some(s.trim().to_string());
    }
    let arr = content.as_array()?;
    let mut out = String::new();
    for item in arr {
        if let Some(s) = item.as_str() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(s);
        } else if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    Some(out.trim().to_string())
}

fn read_reasoning_item(item: &serde_json::Map<String, Value>) -> Option<String> {
    read_content_text(item.get("summary"))
        .filter(|s| !s.is_empty())
        .or_else(|| read_content_text(item.get("content")))
}

fn read_tool_item(
    item: &serde_json::Map<String, Value>,
    fallback_status: &str,
) -> Option<ToolActivity> {
    let type_name = item.get("type").and_then(|x| x.as_str())?.to_string();
    let id = first_str(item, &["id", "call_id"])
        .unwrap_or_else(|| format!("{type_name}-tool"));
    let name = first_str(item, &["name"]).unwrap_or_else(|| tool_name_from_type(&type_name));
    let action = item.get("action").and_then(|x| x.as_object());
    let detail = first_str(item, &["arguments", "input", "query"])
        .or_else(|| {
            action.and_then(|a| a.get("query").and_then(|x| x.as_str()).map(|s| s.to_string()))
        })
        .unwrap_or_default();
    let status = item
        .get("status")
        .and_then(|x| x.as_str())
        .map(|s| match s {
            "completed" => "completed",
            "failed" | "incomplete" => "failed",
            "in_progress" | "searching" => "in_progress",
            _ => fallback_status,
        })
        .unwrap_or(fallback_status)
        .to_string();
    Some(ToolActivity {
        id,
        type_name,
        name,
        status,
        detail,
    })
}

fn update_tool_detail(
    tools: &mut HashMap<String, ToolActivity>,
    payload: &Value,
    detail: &str,
    append: bool,
) {
    let id = payload
        .get("item_id")
        .or_else(|| payload.get("call_id"))
        .and_then(|x| x.as_str());
    let Some(id) = id else { return };
    let entry = tools.entry(id.to_string()).or_insert(ToolActivity {
        id: id.to_string(),
        type_name: "function_call".into(),
        name: "tool".into(),
        status: "in_progress".into(),
        detail: String::new(),
    });
    if append {
        entry.detail.push_str(detail);
    } else if !detail.is_empty() {
        entry.detail = detail.to_string();
    }
}

fn tool_name_from_type(ty: &str) -> String {
    match ty {
        "web_search_call" | "web_search" => "web_search".into(),
        "x_search_call" | "x_search" => "x_search".into(),
        other => other.trim_end_matches("_call").to_string(),
    }
}

fn first_str(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = map.get(*k).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
    }
    None
}
