use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub default_chat_model: String,
    #[serde(default)]
    pub default_image_model: String,
    #[serde(default)]
    pub default_video_model: String,
    #[serde(default)]
    pub default_image_edit_model: String,
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("no config dir")?
            .join("grok2api-creative");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("config.toml"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        let dir = dirs::data_local_dir()
            .context("no data dir")?
            .join("grok2api-creative");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn media_dir() -> Result<PathBuf> {
        let dir = Self::data_dir()?.join("media");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn history_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("history.json"))
    }

    pub fn load() -> Result<Self> {
        let mut cfg = Self::default();
        if let Ok(path) = Self::config_path() {
            if path.exists() {
                let text = fs::read_to_string(&path)?;
                cfg = toml::from_str(&text).context("parse config.toml")?;
            }
        }
        if let Ok(v) = std::env::var("GROK2API_BASE_URL") {
            if !v.trim().is_empty() {
                cfg.base_url = v;
            }
        }
        if let Ok(v) = std::env::var("GROK2API_API_KEY") {
            if !v.trim().is_empty() {
                cfg.api_key = v;
            }
        }
        cfg.base_url = normalize_base_url(&cfg.base_url);
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut c = self.clone();
        c.base_url = normalize_base_url(&c.base_url);
        fs::write(path, toml::to_string_pretty(&c)?)?;
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.api_key.trim().is_empty()
    }

    pub fn masked_key(&self) -> String {
        let k = self.api_key.trim();
        if k.len() <= 8 {
            return "***".into();
        }
        format!("{}…{}", &k[..4], &k[k.len().saturating_sub(4)..])
    }
}

pub fn normalize_base_url(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        return s;
    }
    if !s.ends_with("/v1") {
        s = format!("{s}/v1");
    }
    s
}
