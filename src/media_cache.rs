use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub async fn download_to_cache(url: &str, preferred_ext: &str) -> Result<PathBuf> {
    let dir = crate::config::Config::media_dir()?;
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .context("download media")?
        .error_for_status()
        .context("media http status")?;
    let bytes = resp.bytes().await?;
    let ext = extension_from_url(url).unwrap_or_else(|| preferred_ext.to_string());
    let name = format!("{}.{}", Uuid::new_v4(), ext);
    let path = dir.join(name);
    fs::write(&path, &bytes)?;
    Ok(path)
}

pub fn save_bytes(bytes: &[u8], ext: &str) -> Result<PathBuf> {
    let dir = crate::config::Config::media_dir()?;
    let path = dir.join(format!("{}.{}", Uuid::new_v4(), ext));
    fs::write(&path, bytes)?;
    Ok(path)
}

pub fn save_data_url(data_url: &str) -> Result<PathBuf> {
    let (meta, b64) = data_url
        .split_once(',')
        .context("invalid data url")?;
    let ext = if meta.contains("image/jpeg") || meta.contains("image/jpg") {
        "jpg"
    } else if meta.contains("image/webp") {
        "webp"
    } else if meta.contains("image/gif") {
        "gif"
    } else if meta.contains("video/mp4") {
        "mp4"
    } else {
        "png"
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(b64.trim()))
        .context("decode data url")?;
    save_bytes(&bytes, ext)
}

pub fn filename(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into())
}

fn extension_from_url(url: &str) -> Option<String> {
    let path = url::Url::parse(url).ok()?.path().to_string();
    let ext = Path::new(&path).extension()?.to_string_lossy().to_lowercase();
    if ext.is_empty() || ext.len() > 5 {
        None
    } else {
        Some(ext)
    }
}
