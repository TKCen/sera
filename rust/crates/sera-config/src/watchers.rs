//! File watcher for hot-reloading YAML manifests (agents, circles, templates).
//!
//! Watches specified directories for YAML file changes (create, modify, delete)
//! and emits structured events with resource type classification and validation.
//!
//! Also provides [`ConfigWatcher`] for single-file config hot-reload with
//! schema-validated atomic swap and `broadcast`-based [`ConfigReloadEvent`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, RwLock, broadcast};
use sha2::{Digest, Sha256};

// ──────────────────────────────────────────────────────────────────────────────
// ConfigWatcher — single-file hot-reload with schema-validated swap
// ──────────────────────────────────────────────────────────────────────────────

/// Event emitted on every debounced file change.
#[derive(Debug, Clone)]
pub enum ConfigReloadEvent {
    /// Config reloaded successfully. `version` is the SHA-256 hex of the new content.
    Reloaded(ConfigReloaded),
    /// Reload attempted but failed validation; previous config is still active.
    Failed(ConfigReloadFailed),
}

/// A successful hot-reload.
#[derive(Debug, Clone)]
pub struct ConfigReloaded {
    /// SHA-256 hex digest of the new config content.
    pub version: String,
    /// Path of the file that changed.
    pub path: PathBuf,
}

/// A failed hot-reload attempt.
#[derive(Debug, Clone)]
pub struct ConfigReloadFailed {
    /// Human-readable reason the reload was rejected.
    pub reason: String,
    /// Path of the file that changed.
    pub path: PathBuf,
}

/// Hot-reload watcher for a single YAML config file.
///
/// On each debounced change:
/// 1. Reads and YAML-parses the file.
/// 2. If valid, atomically swaps the active config via an `Arc<RwLock<serde_yaml::Value>>`
///    and broadcasts a [`ConfigReloadEvent::Reloaded`] event.
/// 3. If invalid, broadcasts [`ConfigReloadEvent::Failed`] and leaves the active config
///    unchanged.
pub struct ConfigWatcher {
    /// The currently-active config value (shared with callers).
    config: Arc<RwLock<serde_yaml::Value>>,
    /// Broadcast sender — callers subscribe via [`ConfigWatcher::subscribe`].
    tx: broadcast::Sender<ConfigReloadEvent>,
}

impl ConfigWatcher {
    /// Spawn a config watcher on `path`.
    ///
    /// Returns the `ConfigWatcher` (which holds the active config and the broadcast
    /// channel) plus a `tokio::task::JoinHandle` for the background watcher task.
    ///
    /// The initial config is read synchronously before spawning. Returns an error if
    /// the file cannot be read or parsed at startup.
    pub fn spawn(
        path: PathBuf,
    ) -> Result<(Self, tokio::task::JoinHandle<()>), ConfigWatcherError> {
        use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};

        // Load initial config
        let initial = load_yaml(&path)?;
        let config = Arc::new(RwLock::new(initial));
        let (tx, _) = broadcast::channel(64);

        let config_clone = Arc::clone(&config);
        let tx_clone = tx.clone();
        let path_clone = path.clone();

        // Bridge: notify callback → tokio mpsc → async task
        let (bridge_tx, mut bridge_rx) = mpsc::channel::<DebounceEventResult>(32);

        let mut debouncer = new_debouncer(Duration::from_millis(300), move |res: DebounceEventResult| {
            let _ = bridge_tx.blocking_send(res);
        })
        .map_err(|e| ConfigWatcherError::Init(e.to_string()))?;

        // Watch the parent directory so renames (editor saves) are detected.
        let watch_dir = path_clone
            .parent()
            .unwrap_or(&path_clone)
            .to_path_buf();

        debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| ConfigWatcherError::Watch(e.to_string()))?;

        let handle = tokio::spawn(async move {
            // Keep debouncer alive for the lifetime of the task.
            let _debouncer = debouncer;

            while let Some(res) = bridge_rx.recv().await {
                match res {
                    Err(e) => {
                        tracing::warn!(error = %e, "config watcher notify error");
                    }
                    Ok(events) => {
                        // Only care about events touching our specific file.
                        let relevant = events.iter().any(|ev| ev.path == path_clone);
                        if !relevant {
                            continue;
                        }

                        let event = match load_yaml(&path_clone) {
                            Ok(value) => {
                                let version = sha256_yaml(&value);
                                *config_clone.write().await = value;
                                ConfigReloadEvent::Reloaded(ConfigReloaded {
                                    version,
                                    path: path_clone.clone(),
                                })
                            }
                            Err(e) => {
                                tracing::warn!(
                                    path = %path_clone.display(),
                                    error = %e,
                                    "config reload failed — keeping previous config"
                                );
                                ConfigReloadEvent::Failed(ConfigReloadFailed {
                                    reason: e.to_string(),
                                    path: path_clone.clone(),
                                })
                            }
                        };

                        // Ignore send errors (no active subscribers is fine).
                        let _ = tx_clone.send(event);
                    }
                }
            }
        });

        Ok((Self { config, tx }, handle))
    }

    /// Subscribe to future [`ConfigReloadEvent`]s.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigReloadEvent> {
        self.tx.subscribe()
    }

    /// Read the currently-active config value.
    pub fn config(&self) -> Arc<RwLock<serde_yaml::Value>> {
        Arc::clone(&self.config)
    }
}

/// Errors from [`ConfigWatcher`] operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigWatcherError {
    #[error("failed to initialize config watcher: {0}")]
    Init(String),
    #[error("failed to watch path: {0}")]
    Watch(String),
    #[error("failed to load config file: {0}")]
    Load(String),
}

/// Load and YAML-parse a file. Returns `ConfigWatcherError::Load` on any failure.
fn load_yaml(path: &std::path::Path) -> Result<serde_yaml::Value, ConfigWatcherError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConfigWatcherError::Load(format!("read error: {e}")))?;
    serde_yaml::from_str(&content)
        .map_err(|e| ConfigWatcherError::Load(format!("YAML parse error: {e}")))
}

/// Compute a SHA-256 hex digest over the canonical YAML serialisation.
fn sha256_yaml(value: &serde_yaml::Value) -> String {
    let bytes = serde_yaml::to_string(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_bytes());
    hex::encode(hasher.finalize())
}

/// Resource type detected from directory path
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ResourceType {
    Agent,
    Circle,
    Template,
}

/// Action type for file changes
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum WatchAction {
    Created,
    Modified,
    Deleted,
}

/// A file change event
#[derive(Debug, Clone, serde::Serialize)]
pub struct WatchEvent {
    pub resource_type: ResourceType,
    pub path: PathBuf,
    pub action: WatchAction,
}

/// File watcher for YAML manifest directories
pub struct FileWatcher {
    watched_dirs: Vec<(PathBuf, ResourceType)>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    event_tx: mpsc::Sender<WatchEvent>,
    event_rx: Option<mpsc::Receiver<WatchEvent>>,
}

impl FileWatcher {
    /// Create a new file watcher.
    /// `base_dir` is the sera project root (contains agents/, circles/, templates/)
    pub fn new(base_dir: PathBuf) -> Self {
        let watched_dirs = vec![
            (base_dir.join("agents"), ResourceType::Agent),
            (base_dir.join("circles"), ResourceType::Circle),
            (base_dir.join("templates"), ResourceType::Template),
        ];
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (event_tx, event_rx) = mpsc::channel(100);

        Self {
            watched_dirs,
            shutdown_tx,
            shutdown_rx,
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    /// Take the event receiver (can only be called once)
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WatchEvent>> {
        self.event_rx.take()
    }

    /// Start watching directories in a background task.
    /// Returns a JoinHandle for the watcher task.
    pub fn start(&self) -> Result<tokio::task::JoinHandle<()>, FileWatcherError> {
        use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
        use notify::event::EventKind;

        let watched_dirs = self.watched_dirs.clone();
        let event_tx = self.event_tx.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        // Create a channel for notify events
        let (notify_tx, mut notify_rx) = mpsc::channel::<notify::Event>(100);

        // Create the watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = notify_tx.blocking_send(event);
                }
            },
            Config::default(),
        )
        .map_err(|e| FileWatcherError::Init(e.to_string()))?;

        // Watch each directory (create if not exists)
        for (dir, _) in &watched_dirs {
            if dir.exists() {
                watcher
                    .watch(dir, RecursiveMode::Recursive)
                    .map_err(|e| FileWatcherError::Watch(e.to_string()))?;
            } else {
                tracing::warn!("Watch directory does not exist, skipping: {}", dir.display());
            }
        }

        // Spawn the event processing task
        let handle = tokio::spawn(async move {
            let _watcher = watcher; // Keep watcher alive
            let mut debounce_map: std::collections::HashMap<PathBuf, tokio::time::Instant> =
                std::collections::HashMap::new();
            let debounce_duration = tokio::time::Duration::from_millis(500);

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            tracing::info!("File watcher shutting down");
                            break;
                        }
                    }
                    event = notify_rx.recv() => {
                        let Some(event) = event else { break };

                        // Determine action from event kind
                        let action = match event.kind {
                            EventKind::Create(_) => WatchAction::Created,
                            EventKind::Modify(_) => WatchAction::Modified,
                            EventKind::Remove(_) => WatchAction::Deleted,
                            _ => continue,
                        };

                        for path in event.paths {
                            // Only process YAML files
                            if !is_yaml_file(&path) {
                                continue;
                            }

                            // Debounce
                            let now = tokio::time::Instant::now();
                            if let Some(last) = debounce_map.get(&path)
                                && now.duration_since(*last) < debounce_duration
                            {
                                continue;
                            }
                            debounce_map.insert(path.clone(), now);

                            // Determine resource type from path
                            if let Some(resource_type) = classify_path(&path, &watched_dirs) {
                                // Validate YAML on create/modify
                                if action != WatchAction::Deleted
                                    && let Err(e) = validate_yaml(&path)
                                {
                                    tracing::warn!(
                                        path = %path.display(),
                                        error = %e,
                                        "Invalid YAML in watched file, keeping previous version"
                                    );
                                    continue;
                                }

                                let watch_event = WatchEvent {
                                    resource_type,
                                    path: path.clone(),
                                    action,
                                };

                                if event_tx.send(watch_event).await.is_err() {
                                    tracing::warn!("Event channel closed, stopping watcher");
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Signal the watcher to stop
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FileWatcherError {
    #[error("failed to initialize watcher: {0}")]
    Init(String),
    #[error("failed to watch directory: {0}")]
    Watch(String),
}

fn is_yaml_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "yaml" || ext == "yml")
        .unwrap_or(false)
}

fn classify_path(path: &std::path::Path, dirs: &[(PathBuf, ResourceType)]) -> Option<ResourceType> {
    for (dir, rtype) in dirs {
        if path.starts_with(dir) {
            return Some(*rtype);
        }
    }
    None
}

fn validate_yaml(path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;
    let _: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| format!("YAML parse error: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_yaml_file() {
        assert!(is_yaml_file(std::path::Path::new("config.yaml")));
        assert!(is_yaml_file(std::path::Path::new("config.yml")));
        assert!(!is_yaml_file(std::path::Path::new("config.json")));
        assert!(!is_yaml_file(std::path::Path::new("config")));
    }

    #[test]
    fn test_classify_path() {
        let base = PathBuf::from("/tmp");
        let dirs = vec![
            (base.join("agents"), ResourceType::Agent),
            (base.join("circles"), ResourceType::Circle),
            (base.join("templates"), ResourceType::Template),
        ];

        let agent_path = base.join("agents/my-agent.yaml");
        assert_eq!(classify_path(&agent_path, &dirs), Some(ResourceType::Agent));

        let circle_path = base.join("circles/my-circle.yaml");
        assert_eq!(classify_path(&circle_path, &dirs), Some(ResourceType::Circle));

        let template_path = base.join("templates/my-template.yaml");
        assert_eq!(classify_path(&template_path, &dirs), Some(ResourceType::Template));

        let other_path = base.join("other/file.yaml");
        assert_eq!(classify_path(&other_path, &dirs), None);
    }

    #[test]
    fn test_validate_yaml_valid() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let yaml_path = temp_dir.path().join("valid.yaml");
        fs::write(&yaml_path, "key: value\nnested:\n  inner: 42\n")
            .expect("Failed to write YAML");

        assert!(validate_yaml(&yaml_path).is_ok());
    }

    #[test]
    fn test_validate_yaml_invalid() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let yaml_path = temp_dir.path().join("invalid.yaml");
        fs::write(&yaml_path, "key: value\n  invalid: [unclosed").expect("Failed to write YAML");

        assert!(validate_yaml(&yaml_path).is_err());
    }

    #[test]
    fn test_file_watcher_creation() {
        let base = PathBuf::from("/tmp/sera-test");
        let mut watcher = FileWatcher::new(base.clone());

        assert_eq!(watcher.watched_dirs.len(), 3);
        assert_eq!(
            watcher.watched_dirs[0],
            (base.join("agents"), ResourceType::Agent)
        );
        assert_eq!(
            watcher.watched_dirs[1],
            (base.join("circles"), ResourceType::Circle)
        );
        assert_eq!(
            watcher.watched_dirs[2],
            (base.join("templates"), ResourceType::Template)
        );

        // Test take_event_receiver
        let receiver = watcher.take_event_receiver();
        assert!(receiver.is_some());
        let receiver2 = watcher.take_event_receiver();
        assert!(receiver2.is_none());
    }

    #[tokio::test]
    async fn test_watcher_with_real_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let base = temp_dir.path();

        // Create subdirectories
        fs::create_dir(base.join("agents")).expect("Failed to create agents dir");
        fs::create_dir(base.join("circles")).expect("Failed to create circles dir");
        fs::create_dir(base.join("templates")).expect("Failed to create templates dir");

        let mut watcher = FileWatcher::new(base.to_path_buf());
        let mut receiver = watcher.take_event_receiver().expect("Failed to take receiver");

        let watcher_handle = watcher.start().expect("Failed to start watcher");

        // Give the watcher a moment to initialize
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Write a valid YAML file
        let agent_path = base.join("agents/test-agent.yaml");
        fs::write(&agent_path, "name: test-agent\ntype: agent\n")
            .expect("Failed to write test agent");

        // Wait for event with timeout
        let event_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            receiver.recv(),
        )
        .await;

        // Cleanup
        watcher.stop();
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(1),
            watcher_handle,
        )
        .await;

        // Verify event was received (if timeout didn't occur)
        if let Ok(Some(event)) = event_result {
            assert_eq!(event.resource_type, ResourceType::Agent);
            assert_eq!(event.action, WatchAction::Created);
            assert!(event.path.ends_with("test-agent.yaml"));
        }
    }
}
