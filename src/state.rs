use crate::{CACHE_DIR, PusherError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
type Result<T> = std::result::Result<T, PusherError>;

const CREDENTIALS_FILE: &str = "credentials.json";
const HISTORY_FILE: &str = "push_history.json";
const MAX_HISTORY: usize = 5;

#[derive(Debug, Clone)]
pub struct StoredCredential {
    pub registry: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CredentialRecord {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct CredentialStore {
    entries: HashMap<String, CredentialRecord>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct PushHistory {
    entries: Vec<String>,
}

pub async fn store_credentials(registry: &str, username: &str, password: &str) -> Result<()> {
    let mut store = load_credentials_store().await?;
    store.entries.insert(
        registry.to_string(),
        CredentialRecord {
            username: username.to_string(),
            password: password.to_string(),
        },
    );
    save_credentials_store(&store).await
}

pub async fn load_credentials(registry: &str) -> Result<Option<(String, String)>> {
    let store = load_credentials_store().await?;
    Ok(store
        .entries
        .get(registry)
        .map(|entry| (entry.username.clone(), entry.password.clone())))
}

pub async fn all_credentials() -> Result<Vec<StoredCredential>> {
    let store = load_credentials_store().await?;
    Ok(store
        .entries
        .iter()
        .map(|(registry, record)| StoredCredential {
            registry: registry.clone(),
            username: record.username.clone(),
            password: record.password.clone(),
        })
        .collect())
}

pub async fn record_push_target(target: &str) -> Result<()> {
    let mut history = load_history().await?;
    history.entries.retain(|entry| entry != target);
    history.entries.insert(0, target.to_string());
    if history.entries.len() > MAX_HISTORY {
        history.entries.truncate(MAX_HISTORY);
    }
    save_history(&history).await
}

pub async fn recent_targets() -> Result<Vec<String>> {
    let history = load_history().await?;
    Ok(history.entries)
}

async fn load_credentials_store() -> Result<CredentialStore> {
    load_json(CREDENTIALS_FILE).await
}

async fn save_credentials_store(store: &CredentialStore) -> Result<()> {
    save_json(CREDENTIALS_FILE, store).await
}

async fn load_history() -> Result<PushHistory> {
    load_json(HISTORY_FILE).await
}

async fn save_history(history: &PushHistory) -> Result<()> {
    save_json(HISTORY_FILE, history).await
}

async fn load_json<T>(file_name: &str) -> Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    let path = state_file_path(file_name).await?;
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            PusherError::CacheError(format!("Failed to deserialize {}: {}", path.display(), e))
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(PusherError::CacheError(format!(
            "Failed to read {}: {}",
            path.display(),
            err
        ))),
    }
}

async fn save_json<T>(file_name: &str, value: &T) -> Result<()>
where
    T: Serialize,
{
    let path = state_file_path(file_name).await?;
    let data = serde_json::to_vec_pretty(value)
        .map_err(|e| PusherError::CacheError(format!("Failed to serialize state: {}", e)))?;
    tokio::fs::write(&path, data)
        .await
        .map_err(|e| PusherError::CacheError(format!("Failed to write {}: {}", path.display(), e)))
}

async fn state_file_path(file_name: &str) -> Result<PathBuf> {
    let cache_dir = PathBuf::from(CACHE_DIR);
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
        PusherError::CacheError(format!(
            "Failed to create cache directory {}: {}",
            cache_dir.display(),
            e
        ))
    })?;
    Ok(cache_dir.join(file_name))
}
