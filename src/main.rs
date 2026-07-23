mod client;
mod config;
mod domain;
mod history;
mod media_cache;
mod server;

use config::Config;
use history::HistoryStore;
use server::{start_server, AppState};
use std::sync::Arc;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use tokio::sync::RwLock;
use wry::WebViewBuilder;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let cfg = Config::load()?;
    let history = HistoryStore::load().unwrap_or_default();
    let state = AppState {
        config: Arc::new(RwLock::new(cfg)),
        history: Arc::new(RwLock::new(history)),
    };

    let (addr, _server_handle) = runtime.block_on(start_server(state))?;
    let url = format!("http://{addr}/");
    tracing::info!("grok2api-creative UI at {url}");

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Grok2API Creative")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 840.0))
        .build(&event_loop)?;

    let _webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
