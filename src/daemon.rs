use base64::{engine::general_purpose, Engine as _};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::VSCODE_KEYS;
use tokio::time::sleep;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct DaemonManager {
    storage_files: Vec<PathBuf>,
    original_values: HashMap<PathBuf, Map<String, Value>>,
}

impl DaemonManager {
    pub fn new() -> Self {
        Self {
            storage_files: Vec::new(),
            original_values: HashMap::new(),
        }
    }

    pub fn discover_storage_files(&mut self) -> Result<()> {
        if let Some(vscode_dirs) = crate::get_vscode_config_dirs() {
            for vscode_dir in vscode_dirs {
                let storage_path = vscode_dir.join("storage.json");
                if storage_path.exists() {
                    self.storage_files.push(storage_path);
                }
            }
        }
        
        // Found VSCode storage files to monitor
        Ok(())
    }

    pub fn capture_original_values(&mut self) -> Result<()> {
        for storage_path in &self.storage_files {
            if let Ok(content) = fs::read_to_string(storage_path) {
                if let Ok(data) = serde_json::from_str::<Map<String, Value>>(&content) {
                    self.original_values.insert(storage_path.clone(), data);
                }
            }
        }
        // Captured original values for files
        Ok(())
    }

    pub async fn start_daemon(self) -> Result<()> {
        // Set up file watcher
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default(),
        )?;

        // Watch all storage file directories (deduplicate directories)
        let mut watched_dirs = std::collections::HashSet::new();
        for storage_path in &self.storage_files {
            if let Some(parent) = storage_path.parent() {
                if watched_dirs.insert(parent.to_path_buf()) {
                    watcher.watch(parent, RecursiveMode::NonRecursive)?;
                }
            }
        }

        // Daemon is now monitoring storage files

        // Main daemon loop
        loop {
            // Check for file system events
            while let Ok(event) = rx.try_recv() {
                if let EventKind::Modify(_) = event.kind {
                    for path in &event.paths {
                        if path.file_name().and_then(|n| n.to_str()) == Some("storage.json") {
                            let _ = self.handle_storage_change(path).await;
                        }
                    }
                }
            }

            // Sleep briefly to avoid busy waiting
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn handle_storage_change(&self, storage_path: &Path) -> Result<()> {
        // Small delay to ensure file write is complete
        sleep(Duration::from_millis(50)).await;

        // Read current content
        let current_content = match fs::read_to_string(storage_path) {
            Ok(content) => content,
            Err(_) => return Ok(()), // File might be temporarily unavailable
        };

        let mut current_data: Map<String, Value> = match serde_json::from_str(&current_content) {
            Ok(data) => data,
            Err(_) => return Ok(()), // Invalid JSON, might be mid-write
        };

        // Get original values for this file
        let original_data = match self.original_values.get(storage_path) {
            Some(data) => data,
            None => return Ok(()), // No original data to restore
        };

        let mut needs_restore = false;

        // Check each monitored key
        for key_encoded in &VSCODE_KEYS {
            let key = String::from_utf8(general_purpose::STANDARD.decode(key_encoded)?)?;
            
            if let Some(original_value) = original_data.get(&key) {
                if let Some(current_value) = current_data.get(&key) {
                    if current_value != original_value {
                        current_data.insert(key, original_value.clone());
                        needs_restore = true;
                    }
                }
            }
        }

        // Restore the file if needed
        if needs_restore {
            let restored_content = serde_json::to_string_pretty(&current_data)?;
            fs::write(storage_path, restored_content)?;
        }

        Ok(())
    }
}


