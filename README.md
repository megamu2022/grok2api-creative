# Grok2API Creative

Desktop creative console for [grok2api](https://github.com/chenyme/grok2api): Chat (streaming, reasoning, web/X search), Image generation/edit, Video, unified sidebar history, local media preview.

## Requirements

- Rust 1.75+
- macOS (WKWebView) / Linux (WebKitGTK) / Windows (WebView2)

## Run

```bash
cd grok2api-creative
cargo run --release
```

## Build / Release (macOS Apple Silicon)

Local:

```bash
cargo build --release --target aarch64-apple-darwin
```

GitHub Actions:

- Push to `main` runs CI build on `macos-14` (arm64).
- Tag a version to publish artifacts:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Release assets:

- `Grok2API-Creative-macos-arm64.app.zip` — `.app` bundle
- `Grok2API-Creative-macos-arm64.tar.gz` — binary + `assets` + `run.sh`

Unsigned local builds may need: **System Settings → Privacy & Security → Open Anyway**.

On first launch, open **Settings** and set:

- **Base URL**: e.g. `http://127.0.0.1:8000/v1` (trailing `/v1` optional)
- **API Key**: client key `g2a_...`

Config: `~/.config/grok2api-creative/config.toml`  
History: `~/.local/share/grok2api-creative/history.json`  
Media cache: `~/.local/share/grok2api-creative/media/`

Env overrides:

```bash
export GROK2API_BASE_URL=http://127.0.0.1:8000/v1
export GROK2API_API_KEY=g2a_xxx
```

Optional UI path override:

```bash
export GROK2API_CREATIVE_UI=/path/to/assets
```

## Features

| Area | Details |
|------|---------|
| Chat | `POST /v1/responses` SSE, reasoning effort, web_search / x_search, prompt_cache_key |
| Messages | Edit, delete, retry user turns |
| Image | `POST /v1/images/generations` + local preview |
| Image edit | `POST /v1/images/edits` + upload / URL source |
| Video | Create + 3s poll + download + `<video>` preview |
| History | Single sidebar for chat / image / edit / video |

## Architecture

Rust hosts a local `axum` server (random localhost port) and a `wry` WebView that loads the UI. The server proxies grok2api with your key and serves cached media under `/local/media/`.
