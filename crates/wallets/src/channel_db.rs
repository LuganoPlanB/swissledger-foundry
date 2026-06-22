use crate::error::StoreError;
use std::{
    collections::HashMap,
    path::Path,
    sync::Mutex,
};

/// A payment channel persisted in memory + JSON file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub channel_id: String,
    pub version: i64,
    pub origin: String,
    pub request_url: String,
    pub chain_id: i64,
    pub escrow_contract: String,
    pub token: String,
    pub payee: String,
    pub payer: String,
    pub authorized_signer: String,
    pub salt: String,
    pub deposit: String,
    pub cumulative_amount: String,
    pub challenge_echo: String,
    pub state: String,
    pub close_requested_at: i64,
    pub grace_ready_at: i64,
    pub created_at: i64,
    pub last_used_at: i64,
}

/// In-memory channel store backed by a JSON file.
pub struct ChannelDb {
    channels: Mutex<HashMap<String, Channel>>,
    file_path: Option<std::path::PathBuf>,
}

impl ChannelDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let file_path = path.as_ref().to_path_buf();
        let channels = if file_path.exists() {
            let data = std::fs::read_to_string(&file_path)
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self { channels: Mutex::new(channels), file_path: Some(file_path) })
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Ok(Self { channels: Mutex::new(HashMap::new()), file_path: None })
    }

    fn save(&self) -> Result<(), StoreError> {
        if let Some(ref path) = self.file_path {
            let guard = self.channels.lock().unwrap();
            let data = serde_json::to_string_pretty(&*guard)
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            std::fs::write(path, data).map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    pub fn find(&self, channel_id: &str) -> Result<Option<Channel>, StoreError> {
        let guard = self.channels.lock().unwrap();
        Ok(guard.get(channel_id).cloned())
    }

    pub fn find_by_origin(&self, origin: &str) -> Result<Vec<Channel>, StoreError> {
        let guard = self.channels.lock().unwrap();
        Ok(guard.values().filter(|ch| ch.origin == origin).cloned().collect())
    }

    pub fn load(&self) -> Result<Vec<Channel>, StoreError> {
        let guard = self.channels.lock().unwrap();
        Ok(guard.values().cloned().collect())
    }

    pub fn upsert(&self, ch: &Channel) -> Result<(), StoreError> {
        let mut guard = self.channels.lock().unwrap();
        let created_at = guard.get(&ch.channel_id).map(|c| c.created_at).unwrap_or(ch.created_at);
        let mut ch = ch.clone();
        ch.created_at = created_at;
        guard.insert(ch.channel_id.clone(), ch);
        drop(guard);
        self.save()
    }

    pub fn delete(&self, channel_id: &str) -> Result<bool, StoreError> {
        let mut guard = self.channels.lock().unwrap();
        let existed = guard.remove(channel_id).is_some();
        drop(guard);
        if existed {
            self.save()?;
        }
        Ok(existed)
    }
}
