use crate::converter::{
    convert_locally, copy_markdown, default_engine, is_enabled, is_markdown, is_supported,
    output_path, remove_generated_output, write_artifact,
};
use crate::mineru::MinerUClient;
use crate::models::{ConversionEngine, DeletePolicy, JobStatus, TaskRecord, WatchProfile};
use crate::state::AppState;
use crate::tag_runtime::TagRuntimeMessage;
use anyhow::{Context, Result};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore, mpsc};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub enum RuntimeMessage {
    Reload,
    ProcessQueued,
    Path { path: PathBuf, force: bool },
    Retry { task_id: String },
    RetryWaitingMineru,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleRequest {
    Normal,
    RetryWaitingMineru,
    Force,
}

impl ScheduleRequest {
    fn priority(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::RetryWaitingMineru => 1,
            Self::Force => 2,
        }
    }

    fn is_force(self) -> bool {
        self != Self::Normal
    }
}

#[derive(Debug, Default)]
struct ActivePaths {
    running: HashSet<PathBuf>,
    pending: HashMap<PathBuf, ScheduleRequest>,
}

impl ActivePaths {
    fn try_start(&mut self, path: &Path, request: ScheduleRequest) -> bool {
        if self.running.insert(path.to_path_buf()) {
            return true;
        }
        self.pending
            .entry(path.to_path_buf())
            .and_modify(|pending| {
                if request.priority() > pending.priority() {
                    *pending = request;
                }
            })
            .or_insert(request);
        false
    }

    fn finish(&mut self, path: &Path) -> Option<ScheduleRequest> {
        self.running.remove(path);
        self.pending.remove(path)
    }
}

type SharedActivePaths = Arc<Mutex<ActivePaths>>;

pub fn start(state: AppState) -> Result<()> {
    let (sender, receiver) = mpsc::unbounded_channel();
    state.set_runtime_sender(sender.clone());
    let health = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run(state, sender, receiver).await {
            let message = format!("目录监控后台已停止：{error:#}");
            tracing::error!("{message}");
            health.set_runtime_error(message);
        }
    });
    Ok(())
}

async fn run(
    state: AppState,
    sender: mpsc::UnboundedSender<RuntimeMessage>,
    mut receiver: mpsc::UnboundedReceiver<RuntimeMessage>,
) -> Result<()> {
    let event_sender = sender.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) => {
                let should_reconcile = matches!(
                    event.kind,
                    EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
                );
                for path in event.paths {
                    let _ = event_sender.send(RuntimeMessage::Path { path, force: false });
                }
                if should_reconcile {
                    let _ = event_sender.send(RuntimeMessage::Reconcile);
                }
            }
            Err(error) => tracing::error!(error = %error, "folder watcher error"),
        })?;
    let mineru = MinerUClient::new()?;
    let active = Arc::new(Mutex::new(ActivePaths::default()));
    let semaphore = Arc::new(Semaphore::new(2));
    let mut watched_roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut watched_roots).await?;
    if let Err(error) = resume_interrupted(&state, &mineru, &active, &semaphore).await {
        tracing::error!(error = %format!("{error:#}"), "task resume failed");
    }
    if !state.is_paused()
        && AppState::read_mineru_token().is_ok()
        && let Err(error) = retry_waiting_mineru(&state, &mineru, &active, &semaphore).await
    {
        tracing::error!(error = %format!("{error:#}"), "initial MinerU queue retry failed");
    }
    if let Err(error) = scan_all(&state, &mineru, &active, &semaphore).await {
        tracing::error!(error = %format!("{error:#}"), "initial folder scan failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.tick().await;
    let mut mineru_retry_interval = tokio::time::interval(Duration::from_secs(5 * 60));
    mineru_retry_interval.tick().await;
    loop {
        tokio::select! {
            Some(message) = receiver.recv() => {
                match message {
                    RuntimeMessage::Reload => {
                        if let Err(error) = reload_watches(&state, &mut watcher, &mut watched_roots).await {
                            tracing::error!(error = %format!("{error:#}"), "watcher reload failed");
                        }
                        if let Err(error) = scan_all(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "folder scan failed");
                        }
                    }
                    RuntimeMessage::ProcessQueued => {
                        if let Err(error) = process_queued(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "queued conversion start failed");
                        }
                        if AppState::read_mineru_token().is_ok()
                            && let Err(error) = retry_waiting_mineru(&state, &mineru, &active, &semaphore).await
                        {
                            tracing::error!(error = %format!("{error:#}"), "waiting MinerU retry failed");
                        }
                    }
                    RuntimeMessage::Path { path, force } => {
                        let request = if force {
                            ScheduleRequest::Force
                        } else {
                            ScheduleRequest::Normal
                        };
                        if let Err(error) = handle_path(
                            &state,
                            &mineru,
                            &active,
                            &semaphore,
                            path,
                            request,
                        )
                        .await
                        {
                            tracing::error!(error = %format!("{error:#}"), "path synchronization failed");
                        }
                    }
                    RuntimeMessage::Retry { task_id } => {
                        if let Err(error) = retry_task(&state, &mineru, &active, &semaphore, &task_id).await {
                            tracing::error!(error = %format!("{error:#}"), "task retry failed");
                        }
                    }
                    RuntimeMessage::RetryWaitingMineru => {
                        if let Err(error) = retry_waiting_mineru(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "MinerU queue retry failed");
                        }
                    }
                    RuntimeMessage::Reconcile => {
                        if !state.is_monitoring_paused()
                            && let Err(error) = reconcile_missing_sources(&state).await
                        {
                            tracing::error!(error = %format!("{error:#}"), "source reconciliation failed");
                        }
                        if let Err(error) = scan_all(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "folder scan after reconciliation failed");
                        }
                    }
                }
            }
            _ = interval.tick() => {
                if !state.is_monitoring_paused()
                    && let Err(error) = reconcile_missing_sources(&state).await
                {
                    tracing::error!(error = %format!("{error:#}"), "source reconciliation failed");
                }
                if let Err(error) = scan_all(&state, &mineru, &active, &semaphore).await {
                    tracing::error!(error = %format!("{error:#}"), "folder scan failed");
                }
            }
            _ = mineru_retry_interval.tick() => {
                if !state.is_paused()
                    && AppState::read_mineru_token().is_ok()
                    && let Err(error) = retry_waiting_mineru(&state, &mineru, &active, &semaphore).await
                {
                    tracing::error!(error = %format!("{error:#}"), "scheduled MinerU queue retry failed");
                }
            }
            else => break,
        }
    }
    Ok(())
}

async fn reload_watches(
    state: &AppState,
    watcher: &mut RecommendedWatcher,
    watched_roots: &mut Vec<PathBuf>,
) -> Result<()> {
    for root in watched_roots.drain(..) {
        let _ = watcher.unwatch(&root);
    }
    let settings = state.settings.read().await.clone();
    if settings.monitoring_paused {
        return Ok(());
    }
    for profile in settings
        .profiles
        .into_iter()
        .filter(|profile| profile.enabled)
    {
        let root = PathBuf::from(profile.input_dir);
        if root.is_dir() {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            watched_roots.push(root);
        }
    }
    Ok(())
}

async fn scan_all(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    let settings = state.settings.read().await.clone();
    if settings.monitoring_paused {
        return Ok(());
    }
    let profiles: Vec<WatchProfile> = settings
        .profiles
        .into_iter()
        .filter(|profile| profile.enabled)
        .collect();
    let enabled_extensions = settings.enabled_extensions;
    let paths = tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for profile in profiles {
            paths.extend(scan_profile_tree(
                &profile,
                Path::new(&profile.input_dir),
                &enabled_extensions,
            )?);
        }
        Ok(paths)
    })
    .await??;
    for path in paths {
        schedule_path(
            state,
            mineru,
            active,
            semaphore,
            path,
            ScheduleRequest::Normal,
        )
        .await;
    }
    Ok(())
}

async fn handle_path(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
    path: PathBuf,
    request: ScheduleRequest,
) -> Result<()> {
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        let path = dunce::canonicalize(path)?;
        let settings = state.settings.read().await.clone();
        if settings.monitoring_paused {
            return Ok(());
        }
        let Some(profile) = matching_profile(&settings.profiles, &path) else {
            return Ok(());
        };
        let profile_for_scan = profile.clone();
        let enabled_extensions = settings.enabled_extensions;
        let root = path.clone();
        let paths = tokio::task::spawn_blocking(move || {
            scan_profile_tree(&profile_for_scan, &root, &enabled_extensions)
        })
        .await??;
        for source in paths {
            schedule_path(state, mineru, active, semaphore, source, request).await;
        }
        return Ok(());
    }
    schedule_path(state, mineru, active, semaphore, path, request).await;
    Ok(())
}

fn scan_profile_tree(
    profile: &WatchProfile,
    root: &Path,
    enabled_extensions: &[String],
) -> Result<Vec<PathBuf>> {
    let metadata = std::fs::symlink_metadata(root)
        .with_context(|| format!("无法读取监控目录：{}", root.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(Vec::new());
    }
    let root = dunce::canonicalize(root)?;
    let input_root = Path::new(&profile.input_dir);
    if !root.starts_with(input_root) {
        anyhow::bail!("目录不在监控范围内：{}", root.display());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            mirror_source_directory(profile, entry.path())?;
        } else if entry.file_type().is_file()
            && is_supported(entry.path())
            && is_enabled(entry.path(), enabled_extensions)
        {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}

fn mirror_source_directory(profile: &WatchProfile, source: &Path) -> Result<PathBuf> {
    let relative = source
        .strip_prefix(&profile.input_dir)
        .with_context(|| format!("目录不在监控范围内：{}", source.display()))?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        anyhow::bail!("源目录相对路径不安全：{}", relative.display());
    }
    let output = Path::new(&profile.output_dir).join(relative);
    std::fs::create_dir_all(&output)
        .with_context(|| format!("无法创建镜像目录：{}", output.display()))?;
    Ok(output)
}

async fn schedule_path(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
    path: PathBuf,
    request: ScheduleRequest,
) {
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return;
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return;
    }
    let Ok(path) = dunce::canonicalize(path) else {
        return;
    };
    let settings = state.settings.read().await.clone();
    if settings.monitoring_paused
        || !is_supported(&path)
        || !is_enabled(&path, &settings.enabled_extensions)
    {
        return;
    }
    let Some(profile) = matching_profile(&settings.profiles, &path) else {
        return;
    };
    let key = path.clone();
    if !active.lock().await.try_start(&key, request) {
        return;
    }
    let relative = match path.strip_prefix(&profile.input_dir) {
        Ok(relative) => relative,
        Err(error) => {
            let pending = active.lock().await.finish(&key);
            dispatch_pending(state, key, pending);
            tracing::error!(file = %path.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), error = %error, "failed to queue file");
            return;
        }
    };
    let Some(engine) = default_engine(&path) else {
        let pending = active.lock().await.finish(&key);
        dispatch_pending(state, key, pending);
        return;
    };
    let output = match output_path(&profile, &path) {
        Ok(output) => output,
        Err(error) => {
            let pending = active.lock().await.finish(&key);
            dispatch_pending(state, key, pending);
            tracing::error!(file = %path.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), error = %format!("{error:#}"), "failed to resolve output path");
            return;
        }
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    let queued_task = match state.storage.queue_task(
        &profile,
        &path,
        relative,
        metadata.len(),
        modified_ms,
        engine,
        &output,
        request.is_force(),
    ) {
        Ok(Some(task)) => task,
        Ok(None) => {
            let pending = active.lock().await.finish(&key);
            dispatch_pending(state, key, pending);
            return;
        }
        Err(error) => {
            let pending = active.lock().await.finish(&key);
            dispatch_pending(state, key, pending);
            tracing::error!(file = %path.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), error = %format!("{error:#}"), "failed to persist queued task");
            return;
        }
    };
    let queued_task_id = queued_task.id;
    if state.is_paused() {
        let pending = active.lock().await.finish(&key);
        dispatch_pending(state, key, pending);
        return;
    }
    let state = state.clone();
    let mineru = mineru.clone();
    let active = active.clone();
    let semaphore = semaphore.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let _permit = semaphore.acquire_owned().await?;
            if state.is_paused() {
                return Ok(());
            }
            let settings = state.settings.read().await.clone();
            if !is_enabled(&path, &settings.enabled_extensions) {
                state.storage.delete_task(&queued_task_id)?;
                return Ok(());
            }
            let current_profile = settings.profiles.into_iter().find(|candidate| {
                candidate.id == profile.id
                    && candidate.enabled
                    && path.starts_with(&candidate.input_dir)
            });
            let Some(current_profile) = current_profile else {
                state.storage.delete_task(&queued_task_id)?;
                return Ok(());
            };
            process_path(&state, &mineru, &current_profile, &path, request.is_force()).await
        }
        .await;
        if let Err(error) = result {
            let _ = state.storage.set_status(
                &queued_task_id,
                JobStatus::Failed,
                Some(&format!("{error:#}")),
            );
            tracing::error!(file = %path.file_name().and_then(|name| name.to_str()).unwrap_or("<unknown>"), error = %format!("{error:#}"), "conversion failed");
        }
        let pending = active.lock().await.finish(&key);
        dispatch_pending(&state, key, pending);
    });
}

fn dispatch_pending(state: &AppState, path: PathBuf, pending: Option<ScheduleRequest>) {
    let message = match pending {
        Some(ScheduleRequest::Normal) => RuntimeMessage::Path { path, force: false },
        Some(ScheduleRequest::Force) => RuntimeMessage::Path { path, force: true },
        Some(ScheduleRequest::RetryWaitingMineru) => RuntimeMessage::RetryWaitingMineru,
        None => return,
    };
    if let Err(error) = state.send_runtime(message) {
        tracing::error!(error = %format!("{error:#}"), "failed to enqueue deferred conversion");
    }
}

async fn process_path(
    state: &AppState,
    mineru: &MinerUClient,
    profile: &WatchProfile,
    path: &Path,
    force: bool,
) -> Result<()> {
    let metadata = wait_until_stable(path).await?;
    let source = path.to_path_buf();
    let hash = tokio::task::spawn_blocking(move || sha256_file(&source)).await??;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    let relative = path
        .strip_prefix(&profile.input_dir)
        .with_context(|| format!("文件不在监控目录内：{}", path.display()))?;
    let engine = default_engine(path).context("不支持的文档格式")?;
    let output = output_path(profile, path)?;
    let previous_output = state
        .storage
        .find_by_source(&path.to_string_lossy())?
        .and_then(|task| task.output_path)
        .map(PathBuf::from)
        .filter(|previous| previous != &output && previous.starts_with(&profile.output_dir));
    let Some(task) = state.storage.prepare_task(
        profile,
        path,
        relative,
        &hash,
        metadata.len(),
        modified_ms,
        engine.clone(),
        &output,
        force,
    )?
    else {
        return Ok(());
    };

    let result: Result<()> = match engine {
        ConversionEngine::Anytomd => {
            async {
                state
                    .storage
                    .set_status(&task.id, JobStatus::Converting, None)?;
                let source = path.to_path_buf();
                if is_markdown(&source) {
                    let output = task
                        .output_path
                        .as_deref()
                        .map(PathBuf::from)
                        .context("Markdown 任务缺少输出路径")?;
                    let profile = profile.clone();
                    tokio::task::spawn_blocking(move || copy_markdown(&profile, &source, &output))
                        .await??;
                } else {
                    let artifact =
                        tokio::task::spawn_blocking(move || convert_locally(&source)).await??;
                    let profile = profile.clone();
                    let task_for_write = task.clone();
                    tokio::task::spawn_blocking(move || {
                        write_artifact(&profile, &task_for_write, artifact)
                    })
                    .await??;
                }
                Ok(())
            }
            .await
        }
        ConversionEngine::Mineru => run_mineru(state, mineru, profile, &task).await,
    };

    match result {
        Ok(()) => {
            if let Some(previous) = previous_output {
                let profile = profile.clone();
                tokio::task::spawn_blocking(move || {
                    remove_generated_output(&profile, &previous, false)
                })
                .await??;
            }
            state
                .storage
                .set_status(&task.id, JobStatus::Completed, None)?;
            if profile.tagging.enabled && !profile.tagging.labels.is_empty() {
                let _ = state.send_tag_runtime(TagRuntimeMessage::Path {
                    path: output,
                    force: true,
                });
            }
        }
        Err(error) => {
            let status = if engine == ConversionEngine::Mineru {
                mineru_failure_status(&error)
            } else {
                JobStatus::Failed
            };
            state
                .storage
                .set_status(&task.id, status, Some(&format!("{error:#}")))?;
        }
    }
    Ok(())
}

async fn run_mineru(
    state: &AppState,
    mineru: &MinerUClient,
    profile: &WatchProfile,
    task: &TaskRecord,
) -> Result<()> {
    let token = AppState::read_mineru_token()?;
    let base_url = state.settings.read().await.mineru_base_url.clone();
    let source = Path::new(&task.source_path);
    let submission = mineru.submit(source, &base_url, &token).await?;
    state.storage.set_mineru_submission(
        &task.id,
        &submission.batch_id,
        &submission.data_id,
        JobStatus::Uploading,
    )?;
    mineru.upload(source, &submission.upload_url).await?;
    state
        .storage
        .set_status(&task.id, JobStatus::Processing, None)?;
    let progress_storage = state.storage.clone();
    let progress_task_id = task.id.clone();
    let result = mineru
        .poll(
            &submission.batch_id,
            Some(&submission.data_id),
            &base_url,
            &token,
            move |item| {
                let progress = item.extract_progress.as_ref();
                progress_storage.set_mineru_progress(
                    &progress_task_id,
                    item.state.as_deref(),
                    progress.and_then(|value| value.extracted_pages),
                    progress.and_then(|value| value.total_pages),
                    progress.and_then(|value| value.start_time.as_deref()),
                )
            },
        )
        .await?;
    state
        .storage
        .set_status(&task.id, JobStatus::Downloading, None)?;
    let artifact = mineru.download(&result).await?;
    let profile = profile.clone();
    let task = task.clone();
    tokio::task::spawn_blocking(move || write_artifact(&profile, &task, artifact)).await??;
    Ok(())
}

async fn resume_interrupted(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    let tasks = state.storage.list_tasks_with_statuses(&[
        JobStatus::WaitingStable,
        JobStatus::Uploading,
        JobStatus::Processing,
        JobStatus::Downloading,
        JobStatus::Converting,
        JobStatus::Queued,
    ])?;
    for task in tasks {
        if task.engine == ConversionEngine::Mineru
            && matches!(task.status, JobStatus::Processing | JobStatus::Downloading)
            && task.mineru_batch_id.is_some()
        {
            resume_mineru(
                state,
                mineru,
                active,
                semaphore,
                task,
                ScheduleRequest::Force,
            )
            .await;
        } else if !state.is_paused() {
            schedule_path(
                state,
                mineru,
                active,
                semaphore,
                PathBuf::from(task.source_path),
                ScheduleRequest::Force,
            )
            .await;
        } else {
            state
                .storage
                .set_status(&task.id, JobStatus::Queued, None)?;
        }
    }
    Ok(())
}

async fn resume_mineru(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
    task: TaskRecord,
    request: ScheduleRequest,
) {
    let path = PathBuf::from(&task.source_path);
    if !path.is_file() || !active.lock().await.try_start(&path, request) {
        return;
    }
    let settings = state.settings.read().await.clone();
    let Some(profile) = settings
        .profiles
        .into_iter()
        .find(|item| item.id == task.profile_id)
    else {
        let pending = active.lock().await.finish(&path);
        dispatch_pending(state, path, pending);
        return;
    };
    let state = state.clone();
    let mineru = mineru.clone();
    let active = active.clone();
    let semaphore = semaphore.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let _permit = semaphore.acquire_owned().await?;
            let token = AppState::read_mineru_token()?;
            let base_url = state.settings.read().await.mineru_base_url.clone();
            let batch_id = task
                .mineru_batch_id
                .as_deref()
                .context("任务缺少 MinerU batch_id")?;
            let progress_storage = state.storage.clone();
            let progress_task_id = task.id.clone();
            let result = mineru
                .poll(
                    batch_id,
                    task.mineru_data_id.as_deref(),
                    &base_url,
                    &token,
                    move |item| {
                        let progress = item.extract_progress.as_ref();
                        progress_storage.set_mineru_progress(
                            &progress_task_id,
                            item.state.as_deref(),
                            progress.and_then(|value| value.extracted_pages),
                            progress.and_then(|value| value.total_pages),
                            progress.and_then(|value| value.start_time.as_deref()),
                        )
                    },
                )
                .await?;
            state
                .storage
                .set_status(&task.id, JobStatus::Downloading, None)?;
            let artifact = mineru.download(&result).await?;
            let profile_for_write = profile.clone();
            let task_for_write = task.clone();
            tokio::task::spawn_blocking(move || {
                write_artifact(&profile_for_write, &task_for_write, artifact)
            })
            .await??;
            state
                .storage
                .set_status(&task.id, JobStatus::Completed, None)?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            let _ = state.storage.set_status(
                &task.id,
                mineru_failure_status(&error),
                Some(&format!("{error:#}")),
            );
        }
        let pending = active.lock().await.finish(&path);
        dispatch_pending(&state, path, pending);
    });
}

async fn retry_task(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
    task_id: &str,
) -> Result<()> {
    let task = state.storage.get_task(task_id)?.context("任务不存在")?;
    if state.is_paused() {
        state
            .storage
            .set_status(&task.id, JobStatus::Queued, None)?;
        return Ok(());
    }
    if task.status == JobStatus::WaitingMineru
        && task.engine == ConversionEngine::Mineru
        && task.mineru_batch_id.is_some()
    {
        resume_mineru(
            state,
            mineru,
            active,
            semaphore,
            task,
            ScheduleRequest::Force,
        )
        .await;
        return Ok(());
    }
    schedule_path(
        state,
        mineru,
        active,
        semaphore,
        PathBuf::from(task.source_path),
        ScheduleRequest::Force,
    )
    .await;
    Ok(())
}

async fn process_queued(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    if state.is_paused() {
        return Ok(());
    }
    let tasks = state.storage.list_tasks_with_statuses(&[
        JobStatus::WaitingStable,
        JobStatus::Queued,
        JobStatus::Converting,
        JobStatus::Uploading,
    ])?;
    for task in tasks {
        let source = PathBuf::from(task.source_path);
        if source.is_file() {
            schedule_path(
                state,
                mineru,
                active,
                semaphore,
                source,
                ScheduleRequest::Force,
            )
            .await;
        }
    }
    Ok(())
}

async fn retry_waiting_mineru(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    if state.is_paused() {
        return Ok(());
    }
    for task in state
        .storage
        .list_tasks_with_statuses(&[JobStatus::WaitingMineru])?
    {
        if task.engine == ConversionEngine::Mineru && task.mineru_batch_id.is_some() {
            resume_mineru(
                state,
                mineru,
                active,
                semaphore,
                task,
                ScheduleRequest::RetryWaitingMineru,
            )
            .await;
        } else {
            schedule_path(
                state,
                mineru,
                active,
                semaphore,
                PathBuf::from(task.source_path),
                ScheduleRequest::RetryWaitingMineru,
            )
            .await;
        }
    }
    Ok(())
}

async fn reconcile_missing_sources(state: &AppState) -> Result<()> {
    let settings = state.settings.read().await.clone();
    for profile in settings.profiles {
        for task in state.storage.list_profile_tasks(&profile.id)? {
            if Path::new(&task.source_path).exists() {
                continue;
            }
            if let Some(output) = task.output_path.as_deref().map(Path::new) {
                match profile.delete_policy {
                    DeletePolicy::Keep => {}
                    DeletePolicy::Trash => remove_generated_output(&profile, output, true)?,
                    DeletePolicy::Delete => remove_generated_output(&profile, output, false)?,
                }
            }
            state.storage.delete_task(&task.id)?;
        }
        if profile.enabled {
            tokio::task::spawn_blocking(move || prune_empty_output_directories(&profile)).await??;
        }
    }
    Ok(())
}

fn prune_empty_output_directories(profile: &WatchProfile) -> Result<()> {
    let output_root = Path::new(&profile.output_dir);
    if !output_root.is_dir() {
        return Ok(());
    }
    let input_root = Path::new(&profile.input_dir);
    let mut directories = WalkDir::new(output_root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));

    for directory in directories {
        let relative = directory.strip_prefix(output_root)?;
        if relative
            .components()
            .any(|component| component.as_os_str() == ".trash")
            || input_root.join(relative).is_dir()
        {
            continue;
        }
        if std::fs::read_dir(&directory)?.next().is_none() {
            std::fs::remove_dir(&directory)
                .with_context(|| format!("无法清理空镜像目录：{}", directory.display()))?;
        }
    }
    Ok(())
}

fn matching_profile(profiles: &[WatchProfile], path: &Path) -> Option<WatchProfile> {
    profiles
        .iter()
        .filter(|profile| profile.enabled && path.starts_with(&profile.input_dir))
        .max_by_key(|profile| Path::new(&profile.input_dir).components().count())
        .cloned()
}

async fn wait_until_stable(path: &Path) -> Result<std::fs::Metadata> {
    let mut previous = None;
    let mut stable_observations = 0;
    for _ in 0..60 {
        let metadata = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("无法读取文件属性：{}", path.display()))?;
        let signature = (
            metadata.len(),
            metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok()),
        );
        if previous.as_ref() == Some(&signature) {
            stable_observations += 1;
            if stable_observations >= 2 {
                return Ok(metadata);
            }
        } else {
            stable_observations = 0;
            previous = Some(signature);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    anyhow::bail!("等待文件写入完成超时：{}", path.display())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn mineru_failure_status(error: &anyhow::Error) -> JobStatus {
    let network_error = error
        .chain()
        .any(|cause| cause.downcast_ref::<reqwest::Error>().is_some());
    if network_error || format!("{error:#}").contains("未配置 MinerU Token") {
        JobStatus::WaitingMineru
    } else {
        JobStatus::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_profile(input: &Path, output: &Path) -> WatchProfile {
        std::fs::create_dir_all(output).unwrap();
        let input = dunce::canonicalize(input).unwrap();
        let output = dunce::canonicalize(output).unwrap();
        WatchProfile {
            id: "mirror-test".to_string(),
            name: "mirror-test".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: DeletePolicy::Delete,
            tagging: Default::default(),
        }
    }

    #[test]
    fn waiting_mineru_retry_is_deferred_while_scan_is_active() {
        let path = PathBuf::from("report.pdf");
        let mut active = ActivePaths::default();

        assert!(active.try_start(&path, ScheduleRequest::Normal));
        assert!(!active.try_start(&path, ScheduleRequest::RetryWaitingMineru));
        assert_eq!(
            active.finish(&path),
            Some(ScheduleRequest::RetryWaitingMineru)
        );
    }

    #[test]
    fn stronger_pending_request_wins_and_requests_are_coalesced() {
        let path = PathBuf::from("report.pdf");
        let mut active = ActivePaths::default();

        assert!(active.try_start(&path, ScheduleRequest::Normal));
        assert!(!active.try_start(&path, ScheduleRequest::Normal));
        assert!(!active.try_start(&path, ScheduleRequest::RetryWaitingMineru));
        assert!(!active.try_start(&path, ScheduleRequest::Force));
        assert_eq!(active.finish(&path), Some(ScheduleRequest::Force));
        assert!(active.try_start(&path, ScheduleRequest::Normal));
    }

    #[tokio::test]
    async fn stopped_conversion_keeps_new_files_queued_until_started() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        let data = temporary.path().join("data");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let source = input.join("notes.md");
        std::fs::write(&source, b"# paused test").unwrap();
        let canonical_source = dunce::canonicalize(&source).unwrap();
        let profile = temporary_profile(&input, &output);
        let state = AppState::new(data).unwrap();
        {
            let mut settings = state.settings.write().await;
            settings.profiles = vec![profile];
            settings.paused = true;
        }
        state.set_paused_flag(true);
        let mineru = MinerUClient::new().unwrap();
        let active = Arc::new(Mutex::new(ActivePaths::default()));
        let semaphore = Arc::new(Semaphore::new(2));

        schedule_path(
            &state,
            &mineru,
            &active,
            &semaphore,
            source.clone(),
            ScheduleRequest::Normal,
        )
        .await;
        let queued = state
            .storage
            .find_by_source(&canonical_source.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(queued.status, JobStatus::Queued);
        assert!(!output.join("notes.md").exists());

        {
            state.settings.write().await.paused = false;
        }
        state.set_paused_flag(false);
        schedule_path(
            &state,
            &mineru,
            &active,
            &semaphore,
            source,
            ScheduleRequest::Normal,
        )
        .await;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let task = state
                    .storage
                    .find_by_source(&canonical_source.to_string_lossy())
                    .unwrap();
                if task.is_some_and(|task| task.status == JobStatus::Completed) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(output.join("notes.md")).unwrap(),
            b"# paused test"
        );
    }

    #[tokio::test]
    async fn stopped_monitoring_does_not_queue_new_files() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let source = input.join("notes.md");
        std::fs::write(&source, b"# monitoring stopped").unwrap();
        let canonical_source = dunce::canonicalize(&source).unwrap();
        let state = AppState::new(temporary.path().join("data")).unwrap();
        {
            let mut settings = state.settings.write().await;
            settings.profiles = vec![temporary_profile(&input, &output)];
            settings.monitoring_paused = true;
            settings.paused = false;
        }
        state.set_monitoring_paused_flag(true);
        state.set_paused_flag(false);
        schedule_path(
            &state,
            &MinerUClient::new().unwrap(),
            &Arc::new(Mutex::new(ActivePaths::default())),
            &Arc::new(Semaphore::new(2)),
            source,
            ScheduleRequest::Normal,
        )
        .await;
        assert!(
            state
                .storage
                .find_by_source(&canonical_source.to_string_lossy())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn scan_mirrors_nested_and_empty_directories() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(input.join("客户甲").join("空目录")).unwrap();
        std::fs::create_dir_all(input.join("客户乙")).unwrap();
        std::fs::write(input.join("客户乙").join("报告.pdf"), b"pdf").unwrap();
        std::fs::write(input.join("客户乙").join("忽略.tmp"), b"tmp").unwrap();
        let profile = temporary_profile(&input, &output);

        let files = scan_profile_tree(
            &profile,
            &input,
            &crate::models::default_enabled_extensions(),
        )
        .unwrap();

        assert!(output.join("客户甲").join("空目录").is_dir());
        assert!(output.join("客户乙").is_dir());
        assert_eq!(files, vec![input.join("客户乙").join("报告.pdf")]);
    }

    #[test]
    fn prune_removes_only_empty_directories_missing_from_source() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(input.join("仍存在")).unwrap();
        std::fs::create_dir_all(output.join("仍存在")).unwrap();
        std::fs::create_dir_all(output.join("已删除").join("子目录")).unwrap();
        std::fs::create_dir_all(output.join("保留非空")).unwrap();
        std::fs::write(output.join("保留非空").join("结果.md"), b"result").unwrap();
        std::fs::create_dir_all(output.join(".trash").join("保留")).unwrap();
        let profile = temporary_profile(&input, &output);

        prune_empty_output_directories(&profile).unwrap();

        assert!(output.join("仍存在").is_dir());
        assert!(!output.join("已删除").exists());
        assert!(output.join("保留非空").is_dir());
        assert!(output.join(".trash").join("保留").is_dir());
    }
}
