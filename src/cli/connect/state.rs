use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConnectionState {
    /// Map of client id → connection info
    pub connections: HashMap<String, ClientConnection>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientConnection {
    pub client: String,
    pub gateway_url: String,
    pub connected_at: String, // ISO 8601
    pub config_files_modified: Vec<String>,
    pub backup_files: Vec<String>,
}

impl ConnectionState {
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".isartor/connections.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self).unwrap_or_default();
        let _ = std::fs::write(path, content);
    }
}
