use crate::domain::HistoryItem;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryStore {
    pub items: Vec<HistoryItem>,
}

impl HistoryStore {
    pub fn load() -> Result<Self> {
        let path = crate::config::Config::history_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).context("read history")?;
        let mut store: Self = serde_json::from_str(&text).context("parse history")?;
        store
            .items
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        let path = crate::config::Config::history_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut items = self.items.clone();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if items.len() > MAX_ITEMS {
            items.truncate(MAX_ITEMS);
        }
        let store = Self { items };
        fs::write(path, serde_json::to_string_pretty(&store)?)?;
        Ok(())
    }

    pub fn upsert(&mut self, item: HistoryItem) {
        if let Some(pos) = self.items.iter().position(|x| x.id == item.id) {
            self.items[pos] = item;
        } else {
            self.items.insert(0, item);
        }
        self.items
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if self.items.len() > MAX_ITEMS {
            self.items.truncate(MAX_ITEMS);
        }
    }

    pub fn get(&self, id: &str) -> Option<&HistoryItem> {
        self.items.iter().find(|x| x.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut HistoryItem> {
        self.items.iter_mut().find(|x| x.id == id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|x| x.id != id);
        before != self.items.len()
    }
}
