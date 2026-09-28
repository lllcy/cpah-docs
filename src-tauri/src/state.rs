use crate::index_runtime::IndexRuntimeMessage;
use crate::models::AppSettings;
use crate::runtime::RuntimeMessage;
use crate::storage::Storage;
use crate::tag_runtime::TagRuntimeMessage;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedRwLockReadGuard, RwLock, mpsc};
use tokio_util::sync::CancellationToken;

const KEYRING_SERVICE: &str = "CPAHDocs";
const LEGACY_KEYRING_SERVICE: &str = "CPAHelperDocumentConverter";
const LEGACY_APP_IDENTIFIER: &str = "com.cpahelper.document-converter";
const MINERU_KEYRING_USER: &str = "mineru-token";
const AGENT_KEYRING_USER: &str = "agent-api-key";

#[derive(Debug)]
pub struct ProfileCancelled;

impl std::fmt::Display for ProfileCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("监控目录已删除，任务已取消")
    }
}

impl std::error::Error for ProfileCancelled {}

#[derive(Clone)]
pub struct ProfileRuntimeControl {
    cancellation: CancellationToken,
    write_barrier: Arc<RwLock<()>>,
}

impl ProfileRuntimeControl {
    pub(crate) fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            write_barrier: Arc::new(RwLock::new(())),
        }
    }

    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub async fn write_permit(
        &self,
    ) -> std::result::Result<OwnedRwLockReadGuard<()>, ProfileCancelled> {
        let permit = self.write_barrier.clone().read_owned().await;
        if self.is_cancelled() {
            Err(ProfileCancelled)
        } else {
            Ok(permit)
        }
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub async fn wait_for_writers(&self) {
        let _barrier = self.write_barrier.write().await;
    }

    #[cfg(test)]
    async fn cancel_and_wait(&self) {
        self.cancel();
        self.wait_for_writers().await;
    }
}

pub fn is_profile_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ProfileCancelled>().is_some()
}

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
    pub settings: Arc<RwLock<AppSettings>>,
    monitoring_paused: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    classification_paused: Arc<AtomicBool>,
    profile_controls: Arc<Mutex<HashMap<String, ProfileRuntimeControl>>>,
    runtime_sender: Arc<Mutex<Option<mpsc::UnboundedSender<RuntimeMessage>>>>,
    tag_runtime_sender: Arc<Mutex<Option<mpsc::UnboundedSender<TagRuntimeMessage>>>>,
    index_runtime_sender: Arc<Mutex<Option<mpsc::UnboundedSender<IndexRuntimeMessage>>>>,
    runtime_error: Arc<Mutex<Option<String>>>,
    tag_runtime_error: Arc<Mutex<Option<String>>>,
    index_runtime_error: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        let storage = Storage::new(data_dir)?;
        let mut settings = storage.load_settings()?;
        settings.mineru_configured = Self::read_mineru_token().is_ok_and(|token| !token.is_empty());
        settings.agent.configured = Self::read_agent_api_key().is_ok_and(|token| !token.is_empty());
        let monitoring_paused = settings.monitoring_paused;
        let paused = settings.paused;
        let classification_paused = settings.classification_paused;
        let profile_controls = settings
            .profiles
            .iter()
            .map(|profile| (profile.id.clone(), ProfileRuntimeControl::new()))
            .collect();
        Ok(Self {
            storage,
            settings: Arc::new(RwLock::new(settings)),
            monitoring_paused: Arc::new(AtomicBool::new(monitoring_paused)),
            paused: Arc::new(AtomicBool::new(paused)),
            classification_paused: Arc::new(AtomicBool::new(classification_paused)),
            profile_controls: Arc::new(Mutex::new(profile_controls)),
            runtime_sender: Arc::new(Mutex::new(None)),
            tag_runtime_sender: Arc::new(Mutex::new(None)),
            index_runtime_sender: Arc::new(Mutex::new(None)),
            runtime_error: Arc::new(Mutex::new(None)),
            tag_runtime_error: Arc::new(Mutex::new(None)),
            index_runtime_error: Arc::new(Mutex::new(None)),
        })
    }

    pub fn set_runtime_sender(&self, sender: mpsc::UnboundedSender<RuntimeMessage>) {
        *self
            .runtime_sender
            .lock()
            .expect("runtime sender lock poisoned") = Some(sender);
    }

    pub fn send_runtime(&self, message: RuntimeMessage) -> Result<()> {
        self.runtime_sender
            .lock()
            .expect("runtime sender lock poisoned")
            .as_ref()
            .context("后台监控尚未启动")?
            .send(message)
            .context("后台监控已停止")
    }

    pub fn set_tag_runtime_sender(&self, sender: mpsc::UnboundedSender<TagRuntimeMessage>) {
        *self
            .tag_runtime_sender
            .lock()
            .expect("tag runtime sender lock poisoned") = Some(sender);
    }

    pub fn send_tag_runtime(&self, message: TagRuntimeMessage) -> Result<()> {
        self.tag_runtime_sender
            .lock()
            .expect("tag runtime sender lock poisoned")
            .as_ref()
            .context("Agent 分类后台尚未启动")?
            .send(message)
            .context("Agent 分类后台已停止")
    }

    pub fn set_index_runtime_sender(&self, sender: mpsc::UnboundedSender<IndexRuntimeMessage>) {
        *self
            .index_runtime_sender
            .lock()
            .expect("index runtime sender lock poisoned") = Some(sender);
    }

    pub fn send_index_runtime(&self, message: IndexRuntimeMessage) -> Result<()> {
        self.index_runtime_sender
            .lock()
            .expect("index runtime sender lock poisoned")
            .as_ref()
            .context("知识库索引后台尚未启动")?
            .send(message)
            .context("知识库索引后台已停止")
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn is_monitoring_paused(&self) -> bool {
        self.monitoring_paused.load(Ordering::Relaxed)
    }

    pub fn set_monitoring_paused_flag(&self, paused: bool) {
        self.monitoring_paused.store(paused, Ordering::Relaxed);
    }

    pub fn set_paused_flag(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_classification_paused(&self) -> bool {
        self.classification_paused.load(Ordering::Relaxed)
    }

    pub fn set_classification_paused_flag(&self, paused: bool) {
        self.classification_paused.store(paused, Ordering::Relaxed);
    }

    pub fn profile_control(&self, profile_id: &str) -> Option<ProfileRuntimeControl> {
        self.profile_controls
            .lock()
            .expect("profile controls lock poisoned")
            .get(profile_id)
            .cloned()
    }

    pub fn ensure_profile_controls<'a>(&self, profile_ids: impl IntoIterator<Item = &'a str>) {
        let mut controls = self
            .profile_controls
            .lock()
            .expect("profile controls lock poisoned");
        for profile_id in profile_ids {
            controls
                .entry(profile_id.to_string())
                .or_insert_with(ProfileRuntimeControl::new);
        }
    }

    pub fn begin_profile_removal(&self, profile_id: &str) -> Option<ProfileRuntimeControl> {
        let control = self.profile_control(profile_id)?;
        control.cancel();
        Some(control)
    }

    pub fn forget_cancelled_profile(&self, profile_id: &str) {
        let mut controls = self
            .profile_controls
            .lock()
            .expect("profile controls lock poisoned");
        if controls
            .get(profile_id)
            .is_some_and(ProfileRuntimeControl::is_cancelled)
        {
            controls.remove(profile_id);
        }
    }

    pub fn set_runtime_error(&self, error: String) {
        *self
            .runtime_error
            .lock()
            .expect("runtime health lock poisoned") = Some(error);
    }

    pub fn runtime_error(&self) -> Option<String> {
        self.runtime_error
            .lock()
            .expect("runtime health lock poisoned")
            .clone()
    }

    pub fn set_tag_runtime_error(&self, error: String) {
        *self
            .tag_runtime_error
            .lock()
            .expect("tag runtime health lock poisoned") = Some(error);
    }

    pub fn tag_runtime_error(&self) -> Option<String> {
        self.tag_runtime_error
            .lock()
            .expect("tag runtime health lock poisoned")
            .clone()
    }

    pub fn set_index_runtime_error(&self, error: String) {
        *self
            .index_runtime_error
            .lock()
            .expect("index runtime health lock poisoned") = Some(error);
    }

    pub fn index_runtime_error(&self) -> Option<String> {
        self.index_runtime_error
            .lock()
            .expect("index runtime health lock poisoned")
            .clone()
    }

    pub fn clear_index_runtime_error(&self) {
        *self
            .index_runtime_error
            .lock()
            .expect("index runtime health lock poisoned") = None;
    }

    pub fn read_mineru_token() -> Result<String> {
        read_or_migrate_secret(MINERU_KEYRING_USER).context("未配置 MinerU Token")
    }

    pub fn write_mineru_token(token: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, MINERU_KEYRING_USER)?;
        entry.set_password(token).context("无法写入系统凭据库")
    }

    pub fn read_agent_api_key() -> Result<String> {
        read_or_migrate_secret(AGENT_KEYRING_USER).context("未配置 Agent API Key")
    }

    pub fn write_agent_api_key(token: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, AGENT_KEYRING_USER)?;
        entry.set_password(token).context("无法写入系统凭据库")
    }
}

fn read_or_migrate_secret(user: &str) -> Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, user)?;
    if let Ok(secret) = entry.get_password()
        && !secret.is_empty()
    {
        return Ok(secret);
    }

    let legacy_entry = keyring::Entry::new(LEGACY_KEYRING_SERVICE, user)?;
    let secret = legacy_entry.get_password()?;
    if secret.is_empty() {
        anyhow::bail!("凭据为空");
    }
    entry.set_password(&secret).context("无法迁移系统凭据")?;
    Ok(secret)
}

pub fn migrate_legacy_data_dir(data_dir: &Path) -> Result<()> {
    let Some(parent) = data_dir.parent() else {
        return Ok(());
    };
    let legacy_dir = parent.join(LEGACY_APP_IDENTIFIER);
    if !legacy_dir.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(data_dir).context("无法创建新的应用数据目录")?;
    for file_name in [
        "settings.json",
        "settings.backup.json",
        "converter.db",
        "converter.db-wal",
        "converter.db-shm",
    ] {
        let source = legacy_dir.join(file_name);
        let destination = data_dir.join(file_name);
        if source.is_file() && !destination.exists() {
            fs::copy(&source, &destination)
                .with_context(|| format!("无法迁移旧应用数据：{}", source.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ProfileRuntimeControl, migrate_legacy_data_dir};
    use std::fs;

    #[test]
    fn migrates_legacy_settings_and_database_once() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("com.cpahelper.document-converter");
        let current = root.path().join("com.cpah.docs");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings.json"), b"legacy settings").unwrap();
        fs::write(legacy.join("converter.db"), b"legacy database").unwrap();

        migrate_legacy_data_dir(&current).unwrap();
        assert_eq!(
            fs::read(current.join("settings.json")).unwrap(),
            b"legacy settings"
        );
        assert_eq!(
            fs::read(current.join("converter.db")).unwrap(),
            b"legacy database"
        );

        fs::write(current.join("settings.json"), b"current settings").unwrap();
        migrate_legacy_data_dir(&current).unwrap();
        assert_eq!(
            fs::read(current.join("settings.json")).unwrap(),
            b"current settings"
        );
    }

    #[tokio::test]
    async fn cancelling_profile_waits_for_active_writer_and_blocks_future_writes() {
        let control = ProfileRuntimeControl::new();
        let write_permit = control.write_permit().await.unwrap();
        let cancelling = control.clone();
        let cancellation = tokio::spawn(async move {
            cancelling.cancel_and_wait().await;
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !control.is_cancelled() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(control.is_cancelled());
        assert!(!cancellation.is_finished());

        drop(write_permit);
        cancellation.await.unwrap();
        assert!(control.write_permit().await.is_err());
    }
}
